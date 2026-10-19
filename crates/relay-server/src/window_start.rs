//! Sends the opt-in tiny Claude message that starts the next five-hour window.
//!
//! One scheduler per Relay server acts on the Claude account that server's
//! Claude Code is signed in to. It plans from the shared snapshot
//! (`plan_window_start`), takes a durable claim in SQLite so a second process
//! on the same database or a restart cannot repeat an attempt, and sends one
//! fresh Claude Code turn with a canonical Haiku model: a standalone prompt,
//! no tools, no hooks, no MCP servers, no resumed transcript and an empty
//! working directory.
//!
//! The sign-in check, the model-list request and the message all launch the
//! server's Claude Code the same way: the same sanitized environment, an empty
//! temporary working directory, its own process group, and the same hardened
//! options. Nothing is sent unless the sign-in is verified as a first-party
//! subscription, so a stored API key cannot turn it into API billing, and
//! nothing is launched where administrator-managed hooks could run.
//!
//! Dedup is per Relay database. Two Relay servers signed in to the same Claude
//! account do not share claims or activity, so run this on one server per
//! account.
//!
//! The setting is the owner's explicit authorization for these background
//! model calls. It is unrelated to any agent's Implement or approval
//! settings: the message has no tools, so it can neither change files nor ask
//! for approval.
use super::*;
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt, BufReader};

const PROMPT: &str = "Reply OK.";
const SYSTEM_PROMPT: &str = "Reply with the single word OK.";
/// Upper bound on the reply, through Claude Code's output token setting.
const MAX_OUTPUT_TOKENS: &str = "64";
/// Longer than the subprocess limits together, so a live attempt is never taken over.
const LEASE_SECS: u64 = 150;
const CLAIM_RETENTION_SECS: i64 = 30 * 24 * 60 * 60;
/// Settings that would send the message through API billing or another provider.
const BILLING_ENV: [&str; 7] = [
    "ANTHROPIC_API_KEY",
    "ANTHROPIC_AUTH_TOKEN",
    "ANTHROPIC_BASE_URL",
    "AWS_BEARER_TOKEN_BEDROCK",
    "CLAUDE_CODE_USE_BEDROCK",
    "CLAUDE_CODE_USE_VERTEX",
    "CLAUDE_CODE_USE_FOUNDRY",
];
const RELAY_ENV: [&str; 4] = [
    "RELAY_TOKEN",
    "RELAY_TOKEN_FILE",
    "RELAY_SETUP_TOKEN_FILE",
    "CREDENTIALS_DIRECTORY",
];

#[derive(Clone, Copy)]
struct Limits {
    /// Where Claude Code reads administrator policy on Linux. `None` where
    /// Relay does not know how to check policy, which is refused.
    managed: Option<&'static str>,
    auth: Duration,
    catalog: Duration,
    message: Duration,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            managed: cfg!(target_os = "linux").then_some("/etc/claude-code"),
            auth: Duration::from_secs(10),
            catalog: Duration::from_secs(30),
            message: Duration::from_secs(90),
        }
    }
}

/// What one attempt learned. Limits are reported even when the message failed.
struct Attempt {
    limits: Vec<Value>,
    /// The model that answered.
    result: Result<String, WindowStartFailure>,
    /// The message was handed to Claude Code and nothing proves it was not
    /// delivered.
    maybe_delivered: bool,
}

impl Attempt {
    /// An attempt that ended before anything was handed to Claude Code.
    fn before_dispatch(reason: WindowStartFailure) -> Self {
        Self {
            limits: Vec::new(),
            result: Err(reason),
            maybe_delivered: false,
        }
    }
}

fn read_claim(connection: &Connection, key: &str) -> Result<Option<WindowClaim>, Error> {
    connection
        .query_row(
            "SELECT attempts, settled, dispatched, retry_at, lease_until FROM window_start_claims WHERE window_key=?1",
            [key],
            |r| {
                Ok(WindowClaim {
                    attempts: r.get(0)?,
                    settled: r.get::<_, i64>(1)? != 0,
                    dispatched: r.get::<_, i64>(2)? != 0,
                    retry_at: r.get::<_, i64>(3)?.max(0) as u64,
                    lease_until: r.get::<_, i64>(4)?.max(0) as u64,
                })
            },
        )
        .optional()
        .map_err(Error::internal)
}

impl Store {
    fn window_claim(&self, key: &str) -> Result<Option<WindowClaim>, Error> {
        read_claim(&self.connection, key)
    }

    /// Takes the attempt for `key` and returns its number, unless the stored
    /// snapshot no longer says it is due, another holder has it, it is
    /// settled, out of attempts, backing off, or an earlier message for it may
    /// have been delivered. The checks and the write share one immediate
    /// transaction over the stored state, so concurrent processes on the same
    /// database cannot both win and a stale in-memory view cannot claim. The
    /// retry time is set at the claim, so a crash mid-attempt backs off like
    /// any failure.
    fn claim_window_start(
        &mut self,
        key: &str,
        owner: &str,
        now: u64,
    ) -> Result<Option<u32>, Error> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(Error::internal)?;
        let json: String = transaction
            .query_row("SELECT snapshot FROM workspace WHERE id = 1", [], |r| {
                r.get(0)
            })
            .map_err(Error::internal)?;
        let snapshot: Snapshot = serde_json::from_str(&json).map_err(Error::internal)?;
        if !window_start_due(&snapshot, now, key) {
            return Ok(None);
        }
        let held = read_claim(&transaction, key)?.unwrap_or_default();
        if held.settled
            || held.dispatched
            || held.attempts >= WINDOW_START_MAX_ATTEMPTS
            || held.retry_at > now
            || held.lease_until > now
        {
            return Ok(None);
        }
        let attempt = held.attempts + 1;
        transaction
            .execute(
                "DELETE FROM window_start_claims WHERE claimed_at < ?1",
                params![now as i64 - CLAIM_RETENTION_SECS],
            )
            .map_err(Error::internal)?;
        transaction
            .execute(
                "INSERT INTO window_start_claims(window_key, owner, claimed_at, lease_until, retry_at, attempts, settled, dispatched)
                 VALUES(?1, ?2, ?3, ?4, ?5, ?6, 0, 0)
                 ON CONFLICT(window_key) DO UPDATE SET owner=?2, claimed_at=?3, lease_until=?4, retry_at=?5, attempts=?6",
                params![
                    key,
                    owner,
                    now as i64,
                    (now + LEASE_SECS) as i64,
                    (now + window_start_backoff(attempt)) as i64,
                    attempt
                ],
            )
            .map_err(Error::internal)?;
        transaction.commit().map_err(Error::internal)?;
        Ok(Some(attempt))
    }

    /// The last step before anything is handed to Claude Code: confirms from the
    /// stored snapshot that the message is still due and records, durably, that
    /// the holder's message may now be delivered. Both happen in one immediate
    /// transaction, so a change committed by another process cannot fall between
    /// the check and the record: it is either seen here or happens after the
    /// message is already recorded as dispatched. A crash from here on leaves
    /// the claim dispatched and unsettled, which blocks any resend for the key.
    fn dispatch_window_start(&mut self, key: &str, owner: &str, now: u64) -> Result<bool, Error> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(Error::internal)?;
        let json: String = transaction
            .query_row("SELECT snapshot FROM workspace WHERE id = 1", [], |r| {
                r.get(0)
            })
            .map_err(Error::internal)?;
        let snapshot: Snapshot = serde_json::from_str(&json).map_err(Error::internal)?;
        if !window_start_due(&snapshot, now, key) {
            return Ok(false);
        }
        let changed = transaction
            .execute(
                "UPDATE window_start_claims SET dispatched=1 WHERE window_key=?1 AND owner=?2 AND settled=0",
                params![key, owner],
            )
            .map_err(Error::internal)?;
        transaction.commit().map_err(Error::internal)?;
        Ok(changed == 1)
    }

    /// Gives back an attempt that never reached dispatch, so it neither
    /// counts nor delays the next one.
    fn release_window_start(&mut self, key: &str, owner: &str) -> Result<(), Error> {
        self.connection
            .execute(
                "UPDATE window_start_claims SET lease_until=0, retry_at=0, attempts=MAX(attempts-1, 0)
                 WHERE window_key=?1 AND owner=?2 AND settled=0 AND dispatched=0",
                params![key, owner],
            )
            .map_err(Error::internal)?;
        Ok(())
    }

    /// Ends the holder's attempt: releases the lease and sets the next retry,
    /// or settles the key so nothing more is sent for it. `dispatched` stays
    /// set when the message may have been delivered without an answer.
    fn finish_window_start(
        &mut self,
        key: &str,
        owner: &str,
        settled: bool,
        retry_at: u64,
        dispatched: bool,
    ) -> Result<(), Error> {
        self.connection
            .execute(
                "UPDATE window_start_claims SET settled=?3, retry_at=?4, dispatched=?5, lease_until=0 WHERE window_key=?1 AND owner=?2",
                params![key, owner, settled as i64, retry_at as i64, dispatched as i64],
            )
            .map_err(Error::internal)?;
        Ok(())
    }
}

impl Shutdown {
    /// Starts the window-start scheduler. It does nothing until the owner
    /// turns the setting on, and ends when the server shuts down.
    pub fn start_window_start(&self) {
        let workspace = self.workspace.clone();
        tokio::spawn(async move { workspace.window_start_loop(Limits::default()).await });
    }
}

impl Workspace {
    async fn window_start_loop(self, limits: Limits) {
        let mut shutdown = self.transport_shutdown.subscribe();
        let mut changes = self.snapshots.subscribe();
        loop {
            if *shutdown.borrow() {
                return;
            }
            let wake = self.window_start_tick(limits).await;
            // Without a known time, only a setting or usage change matters.
            let sleep = Duration::from_secs(match wake {
                Some(at) => at.saturating_sub(now()).max(1),
                None => 3600,
            });
            tokio::select! {
                _ = shutdown.changed() => {}
                _ = changes.changed() => {}
                _ = tokio::time::sleep(sleep) => {}
            }
        }
    }

    /// Plans from the current snapshot and, when due, makes one attempt.
    /// Returns when to look again, in Unix seconds.
    async fn window_start_tick(&self, limits: Limits) -> Option<u64> {
        let at = now();
        // Never hold the snapshot channel's lock while taking the store's.
        let key = match window_start_eligibility(&self.snapshots.borrow(), at) {
            WindowStartEligibility::Due { key } => Some(key),
            _ => None,
        };
        let claim = key.as_deref().and_then(|key| {
            self.store
                .lock()
                .ok()
                .and_then(|store| store.window_claim(key).ok().flatten())
        });
        let plan = plan_window_start(&self.snapshots.borrow(), at, claim);
        match plan {
            WindowStartPlan::Off => {
                let _ = self.update_window_start(|s| s.claude_window_start.next_due_at = None);
                None
            }
            WindowStartPlan::Wait { until } => {
                let _ = self.update_window_start(|s| s.claude_window_start.next_due_at = until);
                until
            }
            WindowStartPlan::NotNeeded => {
                let _ = self.update_window_start(|s| {
                    s.claude_window_start.next_due_at = None;
                    s.claude_window_start.last_outcome = Some(WindowStartOutcome::NotNeeded);
                });
                None
            }
            WindowStartPlan::Unconfirmed => {
                // An earlier message for this window may have been delivered,
                // for example when the server stopped mid-attempt.
                let _ = self.update_window_start(|s| {
                    s.claude_window_start.next_due_at = None;
                    if !matches!(
                        s.claude_window_start.last_outcome,
                        Some(WindowStartOutcome::Unconfirmed { .. })
                    ) {
                        s.claude_window_start.last_outcome =
                            Some(WindowStartOutcome::Unconfirmed {
                                reason: WindowStartFailure::Interrupted,
                            });
                    }
                });
                None
            }
            WindowStartPlan::Send { key } => {
                self.attempt_window_start(key, limits).await;
                Some(now())
            }
        }
    }

    fn release_window_claim(&self, key: &str, owner: &str) {
        if let Ok(mut store) = self.store.lock() {
            let _ = store.release_window_start(key, owner);
        }
    }

    /// Claims `key` and sends the message. It is abandoned before dispatch if
    /// the window is no longer due and ended if the setting is turned off or
    /// the server stops. Records the outcome.
    async fn attempt_window_start(&self, key: String, limits: Limits) {
        let started = now();
        let owner = uuid::Uuid::new_v4().to_string();
        let claimed = match self.store.lock() {
            Ok(mut store) => store.claim_window_start(&key, &owner, started),
            Err(_) => return,
        };
        let Ok(Some(attempt)) = claimed else {
            return;
        };
        let binary =
            harness::selected_binary(&self.snapshots.borrow(), Harness::ClaudeCode, &self.config);
        let mut shutdown = self.transport_shutdown.subscribe();
        let mut changes = self.snapshots.subscribe();
        // Preparing takes seconds; real work or a newly reported window can
        // arrive meanwhile, so stop as soon as the message is no longer due.
        let prepare = prepare(binary.clone(), limits);
        tokio::pin!(prepare);
        let prepared = loop {
            if *shutdown.borrow() {
                return;
            }
            if !window_start_due(&self.snapshots.borrow(), now(), &key) {
                self.release_window_claim(&key, &owner);
                return;
            }
            tokio::select! {
                result = &mut prepare => break result,
                _ = shutdown.changed() => {}
                _ = changes.changed() => {}
            }
        };
        let prepared = match prepared {
            Ok(prepared) => prepared,
            Err(reason) => {
                self.finish_window_attempt(&key, &owner, attempt, Attempt::before_dispatch(reason));
                return;
            }
        };
        // Another process may have changed the stored state without this
        // process hearing of it. Check the store and record that the message
        // may be delivered in one transaction, under the store lock, before
        // handing it over.
        let dispatched = match self.store.lock() {
            Ok(mut store) => store.dispatch_window_start(&key, &owner, now()),
            Err(_) => Ok(false),
        };
        if !matches!(dispatched, Ok(true)) {
            self.release_window_claim(&key, &owner);
            return;
        }
        let run = dispatch(binary, prepared, limits);
        tokio::pin!(run);
        let finished = loop {
            if *shutdown.borrow() || !self.snapshots.borrow().claude_window_start.enabled {
                break None;
            }
            tokio::select! {
                result = &mut run => break Some(result),
                _ = shutdown.changed() => {}
                _ = changes.changed() => {}
            }
        };
        let Some(done) = finished else {
            // Dropping the pinned future ends the message's process group. A
            // stopped server leaves the claim dispatched, so the message is
            // not resent; turning the setting off settles it.
            if !*shutdown.borrow()
                && let Ok(mut store) = self.store.lock()
            {
                let _ = store.finish_window_start(&key, &owner, true, 0, true);
            }
            return;
        };
        self.finish_window_attempt(&key, &owner, attempt, done);
    }

    /// Records an attempt's outcome and usage, and settles, delays or blocks
    /// the key accordingly.
    fn finish_window_attempt(&self, key: &str, owner: &str, attempt: u32, done: Attempt) {
        let at = now();
        let Attempt {
            limits: reported,
            result,
            maybe_delivered,
        } = done;
        let outcome = match &result {
            Ok(model) => WindowStartOutcome::Sent {
                model: model.clone(),
            },
            Err(reason) if maybe_delivered => WindowStartOutcome::Unconfirmed { reason: *reason },
            Err(reason) => WindowStartOutcome::Failed {
                reason: *reason,
                attempts: attempt,
            },
        };
        // The reply's own rate-limit report, even a partial one, is the
        // refreshed usage: the next attempt follows the reset time Claude
        // reports, not a fixed period.
        let _ = self.update_window_start(|snapshot| {
            for info in &reported {
                claude::rate_limits(snapshot, info, at);
            }
            snapshot.claude_window_start.last_attempt_at = Some(at);
            snapshot.claude_window_start.last_outcome = Some(outcome);
        });
        let (settled, retry_at) = match result {
            Ok(_) => (true, 0),
            // No answer was read and nothing proves it was not delivered:
            // block the key until a new window is reported.
            Err(_) if maybe_delivered => (false, 0),
            Err(WindowStartFailure::WeeklyLimit) => {
                let reset = self
                    .snapshots
                    .borrow()
                    .usage_limits
                    .iter()
                    .find(|l| l.harness == Harness::ClaudeCode && l.window == UsageWindow::Weekly)
                    .and_then(|l| l.resets_at)
                    .filter(|reset| *reset > at);
                (reset.is_none(), reset.unwrap_or(0))
            }
            Err(reason) if reason.retryable() && attempt < WINDOW_START_MAX_ATTEMPTS => {
                (false, at + window_start_backoff(attempt))
            }
            Err(_) => (true, 0),
        };
        if let Ok(mut store) = self.store.lock() {
            let _ = store.finish_window_start(key, owner, settled, retry_at, maybe_delivered);
        }
    }

    /// Applies `update` to the stored snapshot and publishes it if the
    /// window-start setting or the usage windows changed.
    fn update_window_start(&self, update: impl FnOnce(&mut Snapshot)) -> Result<(), Error> {
        let mut store = self.store.lock().map_err(Error::internal)?;
        let mut snapshot = store.snapshot()?;
        let before = (
            snapshot.claude_window_start.clone(),
            snapshot.usage_limits.clone(),
        );
        update(&mut snapshot);
        if before.0 == snapshot.claude_window_start && before.1 == snapshot.usage_limits {
            return Ok(());
        }
        snapshot.revision = snapshot
            .revision
            .checked_add(1)
            .ok_or_else(|| Error::invalid("Revision exhausted"))?;
        store.save(&snapshot)?;
        self.snapshots.send_replace(snapshot);
        Ok(())
    }
}

/// An empty working directory and the Haiku model to pin, once the sign-in
/// and the model list have been verified.
struct Prepared {
    directory: tempfile::TempDir,
    /// The canonical model id the message is pinned to.
    model: String,
    /// The name to show.
    name: String,
}

/// Verifies the sign-in and chooses the model. Nothing here sends a prompt.
async fn prepare(binary: PathBuf, limits: Limits) -> Result<Prepared, WindowStartFailure> {
    let directory = tempfile::tempdir().map_err(|_| WindowStartFailure::Failed)?;
    let subscription = verify_subscription(&binary, directory.path(), limits.auth).await?;
    // Other platforms keep policy elsewhere, so their policy is refused
    // rather than assumed to define no hooks.
    let managed = limits.managed.ok_or(WindowStartFailure::ManagedHooks)?;
    managed_hooks(Path::new(managed), &subscription)?;
    let (model, name) = haiku(&binary, directory.path(), limits.catalog).await?;
    Ok(Prepared {
        directory,
        model,
        name,
    })
}

async fn dispatch(binary: PathBuf, prepared: Prepared, limits: Limits) -> Attempt {
    message(
        &binary,
        prepared.directory.path(),
        &prepared.model,
        &prepared.name,
        limits.message,
    )
    .await
}

/// A Claude Code launch in an empty directory with the billing and Relay
/// environment removed and its own process group. The sign-in check, the
/// model list and the message all start from this.
fn claude_command(binary: &Path, directory: &Path) -> tokio::process::Command {
    let mut command = tokio::process::Command::new(binary);
    command.current_dir(directory).kill_on_drop(true);
    for name in BILLING_ENV.iter().chain(RELAY_ENV.iter()) {
        command.env_remove(name);
    }
    #[cfg(unix)]
    {
        command.process_group(0);
    }
    command
}

/// The options both the model-list request and the message add: customizations
/// off (safe mode and hooks disabled), user settings and slash commands off,
/// no MCP servers, nothing saved. Hooks defined in administrator-managed
/// settings still run under these options, which is why `managed_hooks`
/// refuses to launch where they could exist. `--tools ""` follows them, last, because
/// the option takes a list.
fn harden(command: &mut tokio::process::Command) {
    command.args([
        "--output-format",
        "stream-json",
        "--verbose",
        "--safe-mode",
        "--no-session-persistence",
        "--strict-mcp-config",
        "--disable-slash-commands",
        "--setting-sources",
        "project,local",
        "--settings",
        r#"{"disableAllHooks":true}"#,
        "--tools",
        "",
    ]);
}

/// Whether Claude Code's `auth status --json` shows a first-party
/// subscription sign-in. Anything else, or anything unreadable, is refused:
/// an API key or another provider could bill API usage. The fields are those
/// Claude Code 2.1.302 reports; if a version names them differently the check
/// fails closed rather than guessing.
fn subscription_status(success: bool, bytes: &[u8]) -> Result<String, WindowStartFailure> {
    let value: Option<Value> = serde_json::from_slice(bytes).ok();
    if value.as_ref().is_some_and(|v| v["loggedIn"] == false) {
        return Err(WindowStartFailure::SignedOut);
    }
    let Some(value) = value.filter(|_| success) else {
        return Err(WindowStartFailure::NotSubscription);
    };
    let lower = |name: &str| value[name].as_str().map(str::to_ascii_lowercase);
    let first_party = lower("apiProvider").as_deref() == Some("firstparty");
    let subscription = value["subscriptionType"]
        .as_str()
        .is_some_and(|kind| !kind.trim().is_empty());
    let stored_key = match &value["apiKeySource"] {
        Value::Null => false,
        Value::String(source) => !source.is_empty() && !source.eq_ignore_ascii_case("none"),
        _ => true,
    };
    let key_method = lower("authMethod").is_none_or(|m| m.contains("key") || m.contains("api"));
    if value["loggedIn"] == true && first_party && subscription && !stored_key && !key_method {
        Ok(value["subscriptionType"]
            .as_str()
            .unwrap_or_default()
            .to_owned())
    } else {
        Err(WindowStartFailure::NotSubscription)
    }
}

/// Refuses to run where administrator policy can run hooks. `--safe-mode` and
/// `disableAllHooks` in `--settings` do not stop hooks defined in managed
/// (policy) settings, and Relay must not override policy, so no launch option
/// can give the promised hook-free turn there. Reads only the managed settings
/// files Claude Code reads on Linux: `managed-settings.json`, then the sorted
/// `managed-settings.d/*.json`. The owner's prerequisite is a policy without
/// hooks or with `"disableAllHooks": true`.
///
/// Server-managed settings (Teams and Enterprise plans) cannot be read from
/// here, so those plans need `disableAllHooks` set explicitly in the file
/// policy. An unreadable or unparseable file is refused too.
fn managed_hooks(directory: &Path, subscription: &str) -> Result<(), WindowStartFailure> {
    let refuse = WindowStartFailure::ManagedHooks;
    let mut files = Vec::new();
    let base = directory.join("managed-settings.json");
    if base.try_exists().map_err(|_| refuse)? {
        files.push(base);
    }
    match std::fs::read_dir(directory.join("managed-settings.d")) {
        Ok(entries) => {
            let mut dropins = Vec::new();
            for entry in entries {
                let path = entry.map_err(|_| refuse)?.path();
                let hidden = path
                    .file_name()
                    .is_some_and(|name| name.to_string_lossy().starts_with('.'));
                if path.extension().is_some_and(|ext| ext == "json") && !hidden {
                    dropins.push(path);
                }
            }
            dropins.sort();
            files.extend(dropins);
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return Err(refuse),
    }
    let mut has_hooks = false;
    let mut hooks_disabled = None;
    for file in files {
        let text = std::fs::read_to_string(&file).map_err(|_| refuse)?;
        let value: Value = serde_json::from_str(&text).map_err(|_| refuse)?;
        has_hooks |= match &value["hooks"] {
            Value::Object(hooks) => !hooks.is_empty(),
            Value::Array(hooks) => !hooks.is_empty(),
            Value::Null => false,
            _ => true,
        };
        if let Some(disabled) = value["disableAllHooks"].as_bool() {
            hooks_disabled = Some(disabled);
        }
    }
    let plan = subscription.to_ascii_lowercase();
    let server_managed = plan.contains("team") || plan.contains("enterprise");
    if hooks_disabled == Some(true) || (!has_hooks && !server_managed) {
        Ok(())
    } else {
        Err(refuse)
    }
}

async fn verify_subscription(
    binary: &Path,
    directory: &Path,
    limit: Duration,
) -> Result<String, WindowStartFailure> {
    let mut check = claude_command(binary, directory);
    check
        .args(["auth", "status", "--json"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut child = check.spawn().map_err(|_| WindowStartFailure::Unsupported)?;
    let _guard = runtime::ProcessGuard {
        owned: child
            .id()
            .and_then(|pid| runtime::process_identity(pid).map(|identity| (pid, identity))),
    };
    let mut stdout = child.stdout.take().ok_or(WindowStartFailure::Failed)?;
    let read = async {
        let mut bytes = Vec::new();
        (&mut stdout)
            .take(16 * 1024)
            .read_to_end(&mut bytes)
            .await
            .map_err(|_| WindowStartFailure::Failed)?;
        let status = child.wait().await.map_err(|_| WindowStartFailure::Failed)?;
        Ok::<(bool, Vec<u8>), WindowStartFailure>((status.success(), bytes))
    };
    let result = tokio::time::timeout(limit, read).await;
    runtime::terminate(&mut child).await;
    let (success, bytes) = result.map_err(|_| WindowStartFailure::TimedOut)??;
    subscription_status(success, &bytes)
}

/// The canonical Haiku model in the account's catalog, as (id to pin, name to
/// show). An entry counts only if the model it runs, as the catalog itself
/// resolves it, is a concrete Haiku id: an alias remapped to another model
/// (for example `ANTHROPIC_DEFAULT_HAIKU_MODEL`) or one the catalog does not
/// resolve is refused. Relay does not rank prices and never substitutes
/// another model.
fn canonical_haiku(catalog: &HarnessCatalog) -> Result<(String, String), WindowStartFailure> {
    let mentions = |text: &str| text.to_ascii_lowercase().contains("haiku");
    catalog
        .models
        .iter()
        .find_map(|model| {
            let concrete = model.resolved.as_deref().unwrap_or(&model.id);
            let claims = mentions(&model.id) || mentions(&model.name) || mentions(concrete);
            (claims && concrete.starts_with("claude-") && mentions(concrete)).then(|| {
                let name = if mentions(&model.name) {
                    model.name.clone()
                } else {
                    concrete.to_owned()
                };
                (concrete.to_owned(), name)
            })
        })
        .ok_or(WindowStartFailure::ModelUnavailable)
}

async fn haiku(
    binary: &Path,
    directory: &Path,
    limit: Duration,
) -> Result<(String, String), WindowStartFailure> {
    let mut discovery = claude_command(binary, directory);
    discovery.args(["--print", "--input-format", "stream-json"]);
    harden(&mut discovery);
    discovery
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut child = discovery
        .spawn()
        .map_err(|_| WindowStartFailure::Unsupported)?;
    let _guard = runtime::ProcessGuard {
        owned: child
            .id()
            .and_then(|pid| runtime::process_identity(pid).map(|identity| (pid, identity))),
    };
    let mut stdin = child.stdin.take().ok_or(WindowStartFailure::Failed)?;
    let mut output = BufReader::new(child.stdout.take().ok_or(WindowStartFailure::Failed)?);
    let request = json!({"type":"control_request","request_id":"relay-window-start","request":{"subtype":"initialize","hooks":null}});
    let probe = async {
        let mut bytes = serde_json::to_vec(&request).map_err(|_| WindowStartFailure::Failed)?;
        bytes.push(b'\n');
        stdin
            .write_all(&bytes)
            .await
            .map_err(|_| WindowStartFailure::Failed)?;
        loop {
            let line = runtime::line(&mut output)
                .await
                .map_err(|_| WindowStartFailure::Failed)?
                .ok_or(WindowStartFailure::Failed)?;
            let Ok(value) = serde_json::from_slice::<Value>(&line) else {
                continue;
            };
            if value["type"] == "control_response"
                && value["response"]["request_id"] == "relay-window-start"
            {
                if value["response"]["subtype"] != "success" {
                    return Err(WindowStartFailure::Failed);
                }
                return Ok::<Value, WindowStartFailure>(value["response"]["response"].clone());
            }
        }
    };
    let data = tokio::time::timeout(limit, probe).await;
    drop(stdin);
    runtime::terminate(&mut child).await;
    let data = data.map_err(|_| WindowStartFailure::TimedOut)??;
    let mut catalog = HarnessCatalog {
        harness: Harness::ClaudeCode,
        ..Default::default()
    };
    commands::add_models(&mut catalog, &data, Harness::ClaudeCode);
    canonical_haiku(&catalog)
}

/// What the stream said so far. It outlives the read, so reports already
/// received survive a timeout.
struct Reply {
    limits: Vec<Value>,
    /// The window type of a rejected rate-limit report.
    rejected: Option<String>,
    /// Whether the turn failed, and its text.
    result: Option<(bool, String)>,
}

/// One turn: a fresh session with no tools, hooks, MCP servers, slash
/// commands, user settings or saved transcript, pinned to the canonical model.
async fn message(
    binary: &Path,
    directory: &Path,
    model: &str,
    name: &str,
    limit: Duration,
) -> Attempt {
    let mut turn = claude_command(binary, directory);
    turn.args([
        "--print",
        "--model",
        model,
        "--system-prompt",
        SYSTEM_PROMPT,
    ]);
    harden(&mut turn);
    turn.env("CLAUDE_CODE_MAX_OUTPUT_TOKENS", MAX_OUTPUT_TOKENS)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let Ok(mut child) = turn.spawn() else {
        return Attempt::before_dispatch(WindowStartFailure::Unsupported);
    };
    let _guard = runtime::ProcessGuard {
        owned: child
            .id()
            .and_then(|pid| runtime::process_identity(pid).map(|identity| (pid, identity))),
    };
    let (Some(mut stdin), Some(stdout), Some(mut stderr)) =
        (child.stdin.take(), child.stdout.take(), child.stderr.take())
    else {
        runtime::terminate(&mut child).await;
        return Attempt::before_dispatch(WindowStartFailure::Failed);
    };
    let _ = stdin.write_all(PROMPT.as_bytes()).await;
    drop(stdin);
    // Diagnostics only classify the failure; they are never stored or logged.
    let diagnostics = tokio::spawn(async move {
        let mut text = Vec::new();
        let _ = (&mut stderr).take(8 * 1024).read_to_end(&mut text).await;
        let _ = tokio::io::copy(&mut stderr, &mut tokio::io::sink()).await;
        text
    });
    let mut output = BufReader::new(stdout);
    let mut reply = Reply {
        limits: Vec::new(),
        rejected: None,
        result: None,
    };
    // A model other than Haiku in the startup report is a consistency check
    // only: the request may already be on its way, so it cannot prevent
    // billing. The model is pinned to a verified id for that.
    let mut unexpected_model = false;
    let read = async {
        while let Some(bytes) = runtime::line(&mut output)
            .await
            .map_err(|_| WindowStartFailure::Failed)?
        {
            let Ok(value) = serde_json::from_slice::<Value>(&bytes) else {
                continue;
            };
            match value["type"].as_str().unwrap_or("") {
                "system" if value["subtype"] == "init" => {
                    let seen = value["model"].as_str().unwrap_or("");
                    if !seen.to_ascii_lowercase().contains("haiku") {
                        unexpected_model = true;
                        return Ok(());
                    }
                }
                "rate_limit_event" => {
                    let info = value["rate_limit_info"].clone();
                    if info["status"] == "rejected" {
                        reply.rejected =
                            Some(info["rateLimitType"].as_str().unwrap_or("").to_owned());
                    }
                    reply.limits.push(info);
                }
                "result" => {
                    let failed = value["is_error"] == true || value["subtype"] != "success";
                    let text = value["result"].as_str().unwrap_or("").to_owned();
                    reply.result = Some((failed, text));
                    break;
                }
                _ => {}
            }
        }
        Ok::<(), WindowStartFailure>(())
    };
    let stopped = match tokio::time::timeout(limit, read).await {
        Ok(Ok(())) => None,
        Ok(Err(reason)) => Some(reason),
        Err(_) => Some(WindowStartFailure::TimedOut),
    };
    runtime::terminate(&mut child).await;
    let diagnostics = diagnostics.await.unwrap_or_default();
    let stderr = String::from_utf8_lossy(&diagnostics).into_owned();
    let result = if unexpected_model {
        Err(WindowStartFailure::ModelUnavailable)
    } else {
        match (stopped, reply.result.take()) {
            (_, Some((false, _))) => Ok(name.to_owned()),
            (_, Some((true, text))) => Err(failure(
                &format!("{text} {stderr}"),
                reply.rejected.as_deref(),
            )),
            (Some(reason), None) => Err(reason),
            (None, None) => Err(failure(&stderr, reply.rejected.as_deref())),
        }
    };
    // Only the service itself refusing the request, as a structured rate-limit
    // rejection, shows nothing was delivered. No error text, connection error
    // or timeout does: they can follow a request the service already took.
    let maybe_delivered = result.is_err() && reply.rejected.is_none();
    Attempt {
        limits: reply.limits,
        result,
        maybe_delivered,
    }
}

fn failure(text: &str, rejected: Option<&str>) -> WindowStartFailure {
    if rejected == Some("seven_day") {
        WindowStartFailure::WeeklyLimit
    } else {
        classify(text)
    }
}

/// Maps Claude Code's error text to a failure kind. The text is not kept.
fn classify(text: &str) -> WindowStartFailure {
    let text = text.to_ascii_lowercase();
    let has = |words: &[&str]| words.iter().any(|word| text.contains(word));
    if has(&[
        "not logged in",
        "/login",
        "invalid api key",
        "invalid_api_key",
        "authentication_error",
        "authentication failed",
        "unauthorized",
        "token has expired",
        "oauth token",
    ]) {
        WindowStartFailure::SignedOut
    } else if text.contains("week") && has(&["limit", "usage"]) {
        WindowStartFailure::WeeklyLimit
    } else if has(&[
        "unknown option",
        "unrecognized option",
        "unknown argument",
        "unexpected argument",
    ]) {
        WindowStartFailure::Unsupported
    } else if text.contains("model")
        && has(&[
            "not found",
            "not available",
            "does not exist",
            "unknown model",
            "issue with the selected model",
            "may not exist",
        ])
    {
        WindowStartFailure::ModelUnavailable
    } else if has(&[
        "enotfound",
        "econnrefused",
        "econnreset",
        "etimedout",
        "unable to connect",
        "fetch failed",
        "could not resolve",
        "network error",
        "connection error",
        "connection refused",
    ]) {
        WindowStartFailure::Offline
    } else {
        WindowStartFailure::Failed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::workspace;
    use std::os::unix::fs::PermissionsExt;

    const SHORT: Limits = Limits {
        managed: Some("/nonexistent/relay-test-managed-settings"),
        auth: Duration::from_secs(10),
        catalog: Duration::from_secs(10),
        message: Duration::from_secs(10),
    };
    const SUBSCRIPTION: &str = r#"{"loggedIn":true,"authMethod":"claude.ai","apiProvider":"firstParty","subscriptionType":"max"}"#;
    const HAIKU: &str = "claude-haiku-4-5-20251001";
    const MODELS: &str = r#"[{"value":"default","model":"default","displayName":"Default","resolvedModel":"claude-opus-4-5","isDefault":true},{"value":"opus","displayName":"Opus","resolvedModel":"claude-opus-4-5"},{"value":"haiku","displayName":"Haiku 4.5","resolvedModel":"claude-haiku-4-5-20251001"}]"#;
    /// The Haiku alias is remapped (ANTHROPIC_DEFAULT_HAIKU_MODEL) to Sonnet.
    const REMAPPED: &str = r#"[{"value":"haiku","displayName":"Haiku 4.5","resolvedModel":"claude-sonnet-4-5"},{"value":"sonnet","displayName":"Sonnet","resolvedModel":"claude-sonnet-4-5"}]"#;
    const UNRESOLVED: &str = r#"[{"value":"haiku","displayName":"Haiku 4.5"}]"#;
    const WITHOUT_HAIKU: &str =
        r#"[{"value":"opus","displayName":"Opus"},{"value":"sonnet","displayName":"Sonnet"}]"#;

    /// A stand-in `claude` that logs how it was started and answers the
    /// sign-in check, the model catalog and the message.
    struct Fake {
        _directory: tempfile::TempDir,
        binary: PathBuf,
        log: PathBuf,
        gate: PathBuf,
    }

    /// A turn that reports windows in the shape Claude Code 2.1.302 uses.
    fn reply(reset: Option<u64>) -> String {
        let limit = reset.map_or_else(String::new, |reset| {
            let weekly = reset + 6 * 86_400;
            format!(
                r#"printf '%s\n' '{{"type":"rate_limit_event","rate_limit_info":{{"status":"allowed","unifiedWindows":{{"five_hour":{{"utilization":0.01,"resetsAt":{reset}}},"seven_day":{{"utilization":0.2,"resetsAt":{weekly}}}}}}}}}'"#
            )
        });
        format!(
            r#"printf '%s\n' '{{"type":"system","subtype":"init","model":"{HAIKU}"}}'
{limit}
printf '%s\n' '{{"type":"result","subtype":"success","is_error":false,"result":"OK"}}'"#
        )
    }

    fn error(text: &str) -> String {
        format!(
            r#"printf '%s\n' '{{"type":"system","subtype":"init","model":"{HAIKU}"}}'
printf '%s\n' '{{"type":"result","subtype":"error_during_execution","is_error":true,"result":"{text}"}}'"#
        )
    }

    /// The service itself refuses the request: a structured rejection report.
    fn rejected(kind: &str, text: &str) -> String {
        format!(
            r#"printf '%s\n' '{{"type":"system","subtype":"init","model":"{HAIKU}"}}'
printf '%s\n' '{{"type":"rate_limit_event","rate_limit_info":{{"status":"rejected","rateLimitType":"{kind}"}}}}'
printf '%s\n' '{{"type":"result","subtype":"error_during_execution","is_error":true,"result":"{text}"}}'"#
        )
    }

    /// `gated` makes the model-list request wait for the gate file to exist.
    fn fake_with(auth: &str, auth_exit: u8, models: &str, message: &str, gated: bool) -> Fake {
        let directory = tempfile::tempdir().unwrap();
        let binary = directory.path().join("claude");
        let log = directory.path().join("log");
        let gate = directory.path().join("gate");
        let wait = if gated {
            format!("while [ ! -e '{}' ]; do sleep 0.05; done", gate.display())
        } else {
            String::new()
        };
        let script = format!(
            r#"#!/bin/sh
case "$1" in
  auth)
    echo "auth $*" >> '{log}'
    echo "auth-env ${{ANTHROPIC_API_KEY:-unset}}" >> '{log}'
    echo '{auth}'
    exit {auth_exit} ;;
esac
case "$*" in
  *--input-format*)
    echo "catalog $*" >> '{log}'
    {wait}
    read -r line
    printf '%s\n' '{{"type":"control_response","response":{{"subtype":"success","request_id":"relay-window-start","response":{{"models":{models}}}}}}}'
    exit 0 ;;
esac
echo "message $*" >> '{log}'
echo "cwd $(pwd)" >> '{log}'
echo "api-key ${{ANTHROPIC_API_KEY:-unset}}" >> '{log}'
echo "max-output ${{CLAUDE_CODE_MAX_OUTPUT_TOKENS:-unset}}" >> '{log}'
read -r prompt
echo "prompt $prompt" >> '{log}'
{message}
"#,
            log = log.display()
        );
        std::fs::write(&binary, script).unwrap();
        std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o700)).unwrap();
        Fake {
            _directory: directory,
            binary,
            log,
            gate,
        }
    }

    fn fake(models: &str, message: &str) -> Fake {
        fake_with(SUBSCRIPTION, 0, models, message, false)
    }

    impl Fake {
        fn lines(&self) -> Vec<String> {
            std::fs::read_to_string(&self.log)
                .unwrap_or_default()
                .lines()
                .map(str::to_owned)
                .collect()
        }
        fn count(&self, prefix: &str) -> usize {
            self.lines()
                .iter()
                .filter(|l| l.starts_with(prefix))
                .count()
        }
        fn messages(&self) -> usize {
            self.count("message ")
        }
        fn config(&self) -> RuntimeConfig {
            RuntimeConfig {
                claude: self.binary.clone(),
                ..Default::default()
            }
        }
        fn open_gate(&self) {
            std::fs::write(&self.gate, "").unwrap();
        }
        async fn wait_for(&self, prefix: &str) {
            for _ in 0..200 {
                if self.count(prefix) > 0 {
                    return;
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
            panic!("{prefix} never started: {:?}", self.lines());
        }
    }

    /// An enabled snapshot whose reported five-hour window ended at `reset`.
    fn expired(reset: Option<u64>) -> Snapshot {
        let mut snapshot = Snapshot::default();
        snapshot.claude_window_start.set_enabled(true, 1);
        if let Some(reset) = reset {
            snapshot.record_usage_limit(UsageLimit::new(
                Harness::ClaudeCode,
                UsageWindow::FiveHour,
                40.0,
                300,
                Some(reset),
                reset - 100,
            ));
        }
        snapshot
    }

    fn fresh_window(reset: u64) -> impl FnOnce(&mut Snapshot) {
        move |snapshot| {
            snapshot.record_usage_limit(UsageLimit::new(
                Harness::ClaudeCode,
                UsageWindow::FiveHour,
                1.0,
                300,
                Some(reset),
                now(),
            ));
        }
    }

    fn work_entry(completed_at: u64) -> UsageEntry {
        UsageEntry {
            id: "run".into(),
            session_id: "session".into(),
            harness: Harness::ClaudeCode,
            model: None,
            input_tokens: 1,
            cached_input_tokens: 0,
            output_tokens: 1,
            completed_at,
        }
    }

    fn open(dir: &Path, snapshot: &Snapshot, fake: &Fake) -> Workspace {
        workspace(&dir.join("relay.sqlite3"), snapshot, fake.config())
    }

    fn status(workspace: &Workspace) -> WindowStart {
        workspace
            .store
            .lock()
            .unwrap()
            .snapshot()
            .unwrap()
            .claude_window_start
    }

    fn claim(workspace: &Workspace, key: &str) -> Option<WindowClaim> {
        workspace.store.lock().unwrap().window_claim(key).unwrap()
    }

    fn alive(pid: &str) -> bool {
        let Ok(stat) = std::fs::read_to_string(format!("/proc/{pid}/stat")) else {
            return false;
        };
        let state = stat
            .rsplit_once(')')
            .and_then(|(_, rest)| rest.trim_start().chars().next());
        state.is_some_and(|state| state != 'Z')
    }

    #[test]
    fn classification_separates_what_waiting_can_fix_from_what_it_cannot() {
        for (text, expected) in [
            (
                "Invalid API key · Please run /login",
                WindowStartFailure::SignedOut,
            ),
            ("OAuth token has expired", WindowStartFailure::SignedOut),
            (
                "Weekly limit reached ∙ resets Mon 9am",
                WindowStartFailure::WeeklyLimit,
            ),
            (
                "There's an issue with the selected model (x). It may not exist",
                WindowStartFailure::ModelUnavailable,
            ),
            (
                "error: unknown option '--safe-mode'",
                WindowStartFailure::Unsupported,
            ),
            (
                "fetch failed: ENOTFOUND api.anthropic.com",
                WindowStartFailure::Offline,
            ),
            ("Overloaded", WindowStartFailure::Failed),
        ] {
            assert_eq!(classify(text), expected, "{text}");
        }
        assert_eq!(
            failure("rate limit", Some("seven_day")),
            WindowStartFailure::WeeklyLimit
        );
    }

    #[test]
    fn only_a_verified_first_party_subscription_passes_the_sign_in_check() {
        let check = |success: bool, text: &str| subscription_status(success, text.as_bytes());
        assert_eq!(check(true, SUBSCRIPTION), Ok("max".to_string()));
        for (text, expected) in [
            // A stored API key, including one set up through /login.
            (
                r#"{"loggedIn":true,"authMethod":"api_key","apiProvider":"firstParty","apiKeySource":"/login managed key"}"#,
                WindowStartFailure::NotSubscription,
            ),
            (
                r#"{"loggedIn":true,"authMethod":"/login managed key","apiProvider":"firstParty","subscriptionType":"max"}"#,
                WindowStartFailure::NotSubscription,
            ),
            (
                r#"{"loggedIn":true,"authMethod":"claude.ai","apiProvider":"firstParty","subscriptionType":"max","apiKeySource":"ANTHROPIC_API_KEY"}"#,
                WindowStartFailure::NotSubscription,
            ),
            // Another provider.
            (
                r#"{"loggedIn":true,"authMethod":"claude.ai","apiProvider":"bedrock","subscriptionType":"max"}"#,
                WindowStartFailure::NotSubscription,
            ),
            // Not enough to prove a subscription.
            (
                r#"{"loggedIn":true,"authMethod":"claude.ai","apiProvider":"firstParty"}"#,
                WindowStartFailure::NotSubscription,
            ),
            (r#"{"loggedIn":true}"#, WindowStartFailure::NotSubscription),
            ("not json", WindowStartFailure::NotSubscription),
            ("", WindowStartFailure::NotSubscription),
            (r#"{"loggedIn":false}"#, WindowStartFailure::SignedOut),
        ] {
            assert_eq!(check(true, text), Err(expected), "{text}");
        }
        // A failing status command is refused even when its output looks fine.
        assert_eq!(
            check(false, SUBSCRIPTION),
            Err(WindowStartFailure::NotSubscription)
        );
        assert_eq!(
            check(false, r#"{"loggedIn":false}"#),
            Err(WindowStartFailure::SignedOut)
        );
    }

    #[test]
    fn only_a_concrete_haiku_the_catalog_resolves_is_chosen() {
        let catalog = |models: &str| {
            let mut catalog = HarnessCatalog::default();
            let data: Value = serde_json::from_str(&format!(r#"{{"models":{models}}}"#)).unwrap();
            commands::add_models(&mut catalog, &data, Harness::ClaudeCode);
            catalog
        };
        assert_eq!(
            canonical_haiku(&catalog(MODELS)),
            Ok((HAIKU.to_string(), "Haiku 4.5".to_string()))
        );
        // A full model id needs no alias.
        assert_eq!(
            canonical_haiku(&catalog(
                r#"[{"value":"claude-haiku-4-5","displayName":"Haiku"}]"#
            )),
            Ok(("claude-haiku-4-5".to_string(), "Haiku".to_string()))
        );
        // A remapped alias is never accepted, nor is one the catalog leaves
        // unresolved.
        for models in [REMAPPED, UNRESOLVED, WITHOUT_HAIKU] {
            assert_eq!(
                canonical_haiku(&catalog(models)),
                Err(WindowStartFailure::ModelUnavailable),
                "{models}"
            );
        }
        // A later canonical entry is still found past a remapped alias.
        let mixed = r#"[{"value":"haiku","displayName":"Haiku","resolvedModel":"claude-sonnet-4-5"},{"value":"claude-haiku-4-5","displayName":"Claude Haiku"}]"#;
        assert_eq!(
            canonical_haiku(&catalog(mixed)),
            Ok(("claude-haiku-4-5".to_string(), "Claude Haiku".to_string()))
        );
    }

    #[tokio::test]
    async fn an_expired_window_gets_one_hardened_haiku_message_and_the_reported_windows_are_adopted()
     {
        let dir = tempfile::tempdir().unwrap();
        let reset = now() - 30;
        let next = now() + 5 * 3600;
        let fake = fake(MODELS, &reply(Some(next)));
        let workspace = open(dir.path(), &expired(Some(reset)), &fake);
        let wake = workspace.window_start_tick(SHORT).await;
        let lines = fake.lines();
        assert_eq!(fake.messages(), 1, "{lines:?}");
        assert_eq!(fake.count("catalog "), 1, "{lines:?}");
        // The model-list request and the message are launched alike: no
        // customizations, user settings, MCP servers or saved transcript, no
        // tools, with an empty last `--tools` value.
        for prefix in ["catalog ", "message "] {
            let line = lines.iter().find(|l| l.starts_with(prefix)).unwrap();
            for flag in [
                "--safe-mode",
                "--no-session-persistence",
                "--strict-mcp-config",
                "--disable-slash-commands",
                "--setting-sources project,local",
                r#"{"disableAllHooks":true}"#,
            ] {
                assert!(line.contains(flag), "{prefix}{flag}: {line}");
            }
            assert!(line.ends_with("--tools "), "{line}");
            assert!(
                !line.contains("--resume") && !line.contains("--add-dir"),
                "{line}"
            );
        }
        // The message is pinned to the canonical id, not the alias.
        let message = lines.iter().find(|l| l.starts_with("message ")).unwrap();
        assert!(message.contains(&format!("--model {HAIKU}")), "{message}");
        assert!(
            lines
                .iter()
                .any(|l| l.starts_with("cwd ") && l.contains("/.tmp")),
            "{lines:?}"
        );
        assert!(lines.contains(&"api-key unset".to_string()), "{lines:?}");
        assert!(lines.contains(&"auth-env unset".to_string()), "{lines:?}");
        assert!(lines.contains(&"max-output 64".to_string()), "{lines:?}");
        assert!(lines.contains(&"prompt Reply OK.".to_string()), "{lines:?}");
        // The sign-in check ran first, in the same context.
        assert_eq!(lines[0], "auth auth status --json");
        // Both reported windows replaced the expired one, the next due time
        // follows the five-hour reset, and the outcome is visible.
        assert!(wake.is_some());
        let saved = workspace.store.lock().unwrap().snapshot().unwrap();
        assert!(saved.usage_limits.iter().any(|l| {
            l.window == UsageWindow::Weekly && l.resets_at == Some(next + 6 * 86_400)
        }));
        assert!(matches!(
            status(&workspace).last_outcome,
            Some(WindowStartOutcome::Sent { ref model }) if model == "Haiku 4.5"
        ));
        let again = workspace.window_start_tick(SHORT).await;
        assert_eq!(fake.messages(), 1);
        assert_eq!(again, Some(next + WINDOW_START_GRACE_SECS));
        assert_eq!(
            status(&workspace).next_due_at,
            Some(next + WINDOW_START_GRACE_SECS)
        );
    }

    #[tokio::test]
    async fn processes_sharing_a_database_send_one_message_even_after_a_restart() {
        let dir = tempfile::tempdir().unwrap();
        let reset = now() - 30;
        // The reply reports no new window, so the key stays the same.
        let fake = fake(MODELS, &reply(None));
        let snapshot = expired(Some(reset));
        let first = open(dir.path(), &snapshot, &fake);
        let second = open(dir.path(), &snapshot, &fake);
        let key = window_start_key(&snapshot, now()).unwrap();
        tokio::join!(
            first.attempt_window_start(key.clone(), SHORT),
            second.attempt_window_start(key.clone(), SHORT)
        );
        assert_eq!(fake.messages(), 1);
        // A restarted server finds the settled claim.
        let restarted = open(dir.path(), &snapshot, &fake);
        restarted.window_start_tick(SHORT).await;
        restarted.attempt_window_start(key.clone(), SHORT).await;
        assert_eq!(fake.messages(), 1);
        let held = claim(&restarted, &key).unwrap();
        assert!(held.settled && held.attempts == 1);
        // With no new window reported, scheduling stalls rather than guessing.
        assert_eq!(restarted.window_start_tick(SHORT).await, None);
    }

    #[tokio::test]
    async fn disabled_activity_and_running_turns_send_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let reset = now() - 30;
        let fake = fake(MODELS, &reply(Some(now() + 5 * 3600)));
        // Off by default.
        let mut off = expired(Some(reset));
        off.claude_window_start.set_enabled(false, 2);
        let workspace = open(dir.path(), &off, &fake);
        assert_eq!(workspace.window_start_tick(SHORT).await, None);
        // Real work after the window ended.
        let mut worked = expired(Some(reset));
        worked.record_usage(work_entry(reset + 10), now());
        let workspace = open(dir.path(), &worked, &fake);
        workspace.window_start_tick(SHORT).await;
        assert!(matches!(
            status(&workspace).last_outcome,
            Some(WindowStartOutcome::NotNeeded)
        ));
        // A genuine Claude turn is running.
        let mut running = crate::tests::live();
        demo_orchestration(&mut running);
        for session in &mut running.sessions {
            session.fixture = false;
        }
        running.claude_window_start = expired(Some(reset)).claude_window_start;
        running.usage_limits = expired(Some(reset)).usage_limits;
        let workspace = open(dir.path(), &running, &fake);
        assert_eq!(workspace.window_start_tick(SHORT).await, None);
        assert!(fake.lines().is_empty(), "{:?}", fake.lines());
    }

    #[tokio::test]
    async fn no_reported_reset_means_nothing_is_started_by_enabling_the_setting() {
        let dir = tempfile::tempdir().unwrap();
        let fake = fake(MODELS, &reply(Some(now() + 5 * 3600)));
        let workspace = open(dir.path(), &expired(None), &fake);
        assert_eq!(workspace.window_start_tick(SHORT).await, None);
        // Not even the sign-in check or the model list runs.
        assert!(fake.lines().is_empty(), "{:?}", fake.lines());
    }

    #[tokio::test]
    async fn only_a_structured_service_rejection_proves_nothing_was_delivered() {
        for (kind, text, reason, retryable) in [
            (
                "seven_day",
                "Weekly limit reached",
                WindowStartFailure::WeeklyLimit,
                false,
            ),
            (
                "five_hour",
                "5-hour limit reached",
                WindowStartFailure::Failed,
                true,
            ),
        ] {
            let dir = tempfile::tempdir().unwrap();
            let fake = fake(MODELS, &rejected(kind, text));
            let snapshot = expired(Some(now() - 30));
            let key = window_start_key(&snapshot, now()).unwrap();
            let workspace = open(dir.path(), &snapshot, &fake);
            workspace.window_start_tick(SHORT).await;
            assert_eq!(fake.messages(), 1, "{kind}");
            assert_eq!(
                status(&workspace).last_outcome,
                Some(WindowStartOutcome::Failed {
                    reason,
                    attempts: 1
                }),
                "{kind}"
            );
            // Nothing more goes out for this window right away.
            workspace.window_start_tick(SHORT).await;
            workspace.attempt_window_start(key.clone(), SHORT).await;
            assert_eq!(fake.messages(), 1, "{kind}");
            let held = claim(&workspace, &key).unwrap();
            assert_eq!(held.settled, !retryable, "{kind}");
            assert!(!held.dispatched, "{kind}");
            if retryable {
                assert!(held.retry_at >= now() + window_start_backoff(1) - 5);
            }
        }
    }

    #[tokio::test]
    async fn every_other_failure_after_hand_off_is_unconfirmed_and_never_resent() {
        // A partial reply, as if the request had reached the service.
        let partial = r#"printf '%s\n' '{"type":"stream_event","event":{"type":"message_start"}}'"#;
        let cases = [
            // Error text that looks like a refusal or a connection problem
            // proves nothing: it can follow a request the service took.
            (
                error("Invalid API key · Please run /login"),
                WindowStartFailure::SignedOut,
            ),
            (
                format!("{partial}\n{}", error("fetch failed: ECONNRESET")),
                WindowStartFailure::Offline,
            ),
            (
                format!("{partial}\n{}", error("read ETIMEDOUT")),
                WindowStartFailure::Offline,
            ),
            (
                error("Unable to connect to API (ENOTFOUND)"),
                WindowStartFailure::Offline,
            ),
            (
                error("Weekly limit reached"),
                WindowStartFailure::WeeklyLimit,
            ),
            // The connection drops mid-stream with no answer at all.
            (
                format!("{partial}\necho 'read ECONNRESET' >&2\nexit 1"),
                WindowStartFailure::Offline,
            ),
        ];
        for (script, reason) in cases {
            let dir = tempfile::tempdir().unwrap();
            let fake = fake(MODELS, &script);
            let snapshot = expired(Some(now() - 30));
            let key = window_start_key(&snapshot, now()).unwrap();
            let workspace = open(dir.path(), &snapshot, &fake);
            workspace.window_start_tick(SHORT).await;
            assert_eq!(fake.messages(), 1, "{script}");
            assert_eq!(
                status(&workspace).last_outcome,
                Some(WindowStartOutcome::Unconfirmed { reason }),
                "{script}"
            );
            let held = claim(&workspace, &key).unwrap();
            assert!(held.dispatched && !held.settled, "{script}");
            // After the lease and every backoff, nothing is sent again.
            workspace
                .store
                .lock()
                .unwrap()
                .connection
                .execute(
                    "UPDATE window_start_claims SET retry_at=0, lease_until=0",
                    [],
                )
                .unwrap();
            workspace.window_start_tick(SHORT).await;
            workspace.attempt_window_start(key, SHORT).await;
            assert_eq!(fake.messages(), 1, "{script}");
        }
    }

    #[tokio::test]
    async fn an_unexplained_failure_after_hand_off_is_not_resent_until_a_new_window_is_reported() {
        let dir = tempfile::tempdir().unwrap();
        let fake = fake(MODELS, &error("Overloaded"));
        let snapshot = expired(Some(now() - 30));
        let key = window_start_key(&snapshot, now()).unwrap();
        let workspace = open(dir.path(), &snapshot, &fake);
        workspace.window_start_tick(SHORT).await;
        assert_eq!(fake.messages(), 1);
        assert_eq!(
            status(&workspace).last_outcome,
            Some(WindowStartOutcome::Unconfirmed {
                reason: WindowStartFailure::Failed
            })
        );
        let held = claim(&workspace, &key).unwrap();
        assert!(held.dispatched && !held.settled);
        // Even after the lease and every backoff, it stays blocked.
        workspace
            .store
            .lock()
            .unwrap()
            .connection
            .execute(
                "UPDATE window_start_claims SET retry_at=0, lease_until=0",
                [],
            )
            .unwrap();
        assert_eq!(workspace.window_start_tick(SHORT).await, None);
        workspace.attempt_window_start(key.clone(), SHORT).await;
        assert_eq!(fake.messages(), 1);
        // A newly reported window reconciles it: the old key is dropped and
        // the next window is scheduled normally.
        let next = now() + 5 * 3600;
        workspace.update_window_start(fresh_window(next)).unwrap();
        assert_eq!(
            workspace.window_start_tick(SHORT).await,
            Some(next + WINDOW_START_GRACE_SECS)
        );
        assert_eq!(fake.messages(), 1);
    }

    #[tokio::test]
    async fn a_crash_after_hand_off_leaves_the_message_unconfirmed_and_unsent() {
        let dir = tempfile::tempdir().unwrap();
        let fake = fake(MODELS, &reply(Some(now() + 5 * 3600)));
        let snapshot = expired(Some(now() - 30));
        let key = window_start_key(&snapshot, now()).unwrap();
        let workspace = open(dir.path(), &snapshot, &fake);
        {
            // A previous server took the claim, marked the message dispatched
            // and died; its lease and backoff have long passed.
            let mut store = workspace.store.lock().unwrap();
            assert_eq!(
                store.claim_window_start(&key, "dead", now()).unwrap(),
                Some(1)
            );
            assert!(store.dispatch_window_start(&key, "dead", now()).unwrap());
            store
                .connection
                .execute(
                    "UPDATE window_start_claims SET retry_at=0, lease_until=0",
                    [],
                )
                .unwrap();
        }
        assert_eq!(workspace.window_start_tick(SHORT).await, None);
        assert!(fake.lines().is_empty(), "{:?}", fake.lines());
        assert!(matches!(
            status(&workspace).last_outcome,
            Some(WindowStartOutcome::Unconfirmed { .. })
        ));
    }

    #[tokio::test]
    async fn refused_sign_ins_never_reach_the_model_list_or_the_message() {
        for (auth, exit, reason) in [
            (
                r#"{"loggedIn":true,"authMethod":"api_key","apiProvider":"firstParty","apiKeySource":"/login managed key"}"#,
                0,
                WindowStartFailure::NotSubscription,
            ),
            (
                r#"{"loggedIn":true,"authMethod":"claude.ai","apiProvider":"vertex","subscriptionType":"max"}"#,
                0,
                WindowStartFailure::NotSubscription,
            ),
            ("not json", 0, WindowStartFailure::NotSubscription),
            (SUBSCRIPTION, 1, WindowStartFailure::NotSubscription),
            (r#"{"loggedIn":false}"#, 1, WindowStartFailure::SignedOut),
        ] {
            let dir = tempfile::tempdir().unwrap();
            let fake = fake_with(auth, exit, MODELS, &reply(Some(now() + 3600)), false);
            let workspace = open(dir.path(), &expired(Some(now() - 30)), &fake);
            workspace.window_start_tick(SHORT).await;
            assert_eq!(fake.count("catalog "), 0, "{auth}");
            assert_eq!(fake.messages(), 0, "{auth}");
            assert_eq!(
                status(&workspace).last_outcome,
                Some(WindowStartOutcome::Failed {
                    reason,
                    attempts: 1
                }),
                "{auth}"
            );
        }
    }

    #[tokio::test]
    async fn a_missing_or_remapped_haiku_never_sends_a_message() {
        for models in [WITHOUT_HAIKU, REMAPPED, UNRESOLVED] {
            let dir = tempfile::tempdir().unwrap();
            let fake = fake(models, &reply(Some(now() + 3600)));
            let workspace = open(dir.path(), &expired(Some(now() - 30)), &fake);
            workspace.window_start_tick(SHORT).await;
            assert_eq!(fake.count("catalog "), 1, "{models}");
            assert_eq!(fake.messages(), 0, "{models}");
            assert!(matches!(
                status(&workspace).last_outcome,
                Some(WindowStartOutcome::Failed {
                    reason: WindowStartFailure::ModelUnavailable,
                    ..
                })
            ));
        }
    }

    #[tokio::test]
    async fn real_work_or_a_new_window_during_preparation_stops_the_message() {
        // A newly reported window, published to this process.
        let dir = tempfile::tempdir().unwrap();
        let fake = fake_with(SUBSCRIPTION, 0, MODELS, &reply(Some(now() + 3600)), true);
        let snapshot = expired(Some(now() - 30));
        let key = window_start_key(&snapshot, now()).unwrap();
        let workspace = open(dir.path(), &snapshot, &fake);
        let running = workspace.clone();
        let attempt_key = key.clone();
        let task =
            tokio::spawn(async move { running.attempt_window_start(attempt_key, SHORT).await });
        fake.wait_for("catalog ").await;
        workspace
            .update_window_start(fresh_window(now() + 5 * 3600))
            .unwrap();
        tokio::time::timeout(Duration::from_secs(5), task)
            .await
            .unwrap()
            .unwrap();
        fake.open_gate();
        assert_eq!(fake.messages(), 0, "{:?}", fake.lines());
        // The attempt is given back, as if it had not happened.
        let held = claim(&workspace, &key).unwrap();
        assert!(!held.dispatched && !held.settled);
        assert_eq!(held.attempts, 0);

        // Real work recorded by another process, which this one never hears of.
        let dir = tempfile::tempdir().unwrap();
        let fake = fake_with(SUBSCRIPTION, 0, MODELS, &reply(Some(now() + 3600)), true);
        let workspace = open(dir.path(), &snapshot, &fake);
        let running = workspace.clone();
        let attempt_key = key.clone();
        let task =
            tokio::spawn(async move { running.attempt_window_start(attempt_key, SHORT).await });
        fake.wait_for("catalog ").await;
        let mut stored = workspace.store.lock().unwrap().snapshot().unwrap();
        stored.record_usage(work_entry(now() - 5), now());
        workspace.store.lock().unwrap().save(&stored).unwrap();
        fake.open_gate();
        tokio::time::timeout(Duration::from_secs(5), task)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(fake.messages(), 0, "{:?}", fake.lines());
        let held = claim(&workspace, &key).unwrap();
        assert!(!held.dispatched && !held.settled);
        assert_eq!(held.attempts, 0);
    }

    #[tokio::test]
    async fn turning_the_setting_off_during_preparation_stops_it_before_any_message() {
        let dir = tempfile::tempdir().unwrap();
        let fake = fake_with(SUBSCRIPTION, 0, MODELS, &reply(Some(now() + 3600)), true);
        let snapshot = expired(Some(now() - 30));
        let key = window_start_key(&snapshot, now()).unwrap();
        let workspace = open(dir.path(), &snapshot, &fake);
        let running = workspace.clone();
        let task = tokio::spawn(async move { running.attempt_window_start(key, SHORT).await });
        fake.wait_for("catalog ").await;
        workspace
            .update_window_start(|s| s.claude_window_start.set_enabled(false, now()))
            .unwrap();
        tokio::time::timeout(Duration::from_secs(5), task)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(fake.messages(), 0);
    }

    #[tokio::test]
    async fn a_hung_message_times_out_keeps_its_reports_and_is_not_resent() {
        let dir = tempfile::tempdir().unwrap();
        let pids = dir.path().join("pid");
        let next = now() + 5 * 3600;
        // It reports the new window, then never answers.
        let hang = format!(
            "{}\necho $$ > '{}'\nsleep 60",
            reply(Some(next))
                .lines()
                .take(2)
                .collect::<Vec<_>>()
                .join("\n"),
            pids.display()
        );
        let fake = fake(MODELS, &hang);
        let snapshot = expired(Some(now() - 30));
        let key = window_start_key(&snapshot, now()).unwrap();
        let workspace = open(dir.path(), &snapshot, &fake);
        let limits = Limits {
            message: Duration::from_secs(1),
            ..SHORT
        };
        workspace.window_start_tick(limits).await;
        assert!(matches!(
            status(&workspace).last_outcome,
            Some(WindowStartOutcome::Unconfirmed {
                reason: WindowStartFailure::TimedOut
            })
        ));
        // The report read before the timeout was kept.
        let saved = workspace.store.lock().unwrap().snapshot().unwrap();
        assert!(
            saved
                .usage_limits
                .iter()
                .any(|l| l.window == UsageWindow::FiveHour && l.resets_at == Some(next))
        );
        let held = claim(&workspace, &key).unwrap();
        assert!(held.dispatched && !held.settled);
        let pid = std::fs::read_to_string(&pids).unwrap().trim().to_owned();
        for _ in 0..50 {
            if !alive(&pid) {
                assert_eq!(fake.messages(), 1);
                return;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        panic!("the message process outlived its timeout");
    }

    #[tokio::test]
    async fn turning_the_setting_off_ends_the_message_in_flight() {
        let dir = tempfile::tempdir().unwrap();
        let pids = dir.path().join("pid");
        let fake = fake(MODELS, &format!("echo $$ > '{}'\nsleep 60", pids.display()));
        let snapshot = expired(Some(now() - 30));
        let key = window_start_key(&snapshot, now()).unwrap();
        let workspace = open(dir.path(), &snapshot, &fake);
        let running = workspace.clone();
        let task = tokio::spawn(async move { running.attempt_window_start(key, SHORT).await });
        for _ in 0..100 {
            if pids.exists() && !std::fs::read_to_string(&pids).unwrap().trim().is_empty() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        let pid = std::fs::read_to_string(&pids).unwrap().trim().to_owned();
        assert!(alive(&pid));
        workspace
            .update_window_start(|s| s.claude_window_start.set_enabled(false, now()))
            .unwrap();
        tokio::time::timeout(Duration::from_secs(5), task)
            .await
            .unwrap()
            .unwrap();
        for _ in 0..50 {
            if !alive(&pid) {
                return;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        panic!("the message process survived turning the setting off");
    }

    #[test]
    fn claims_are_exclusive_due_checked_and_block_resends_after_hand_off() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("claims.sqlite3");
        let snapshot = expired(Some(1_000));
        let mut first = Store::open(&path, AgentProfile::default()).unwrap();
        first.save(&snapshot).unwrap();
        let mut second = Store::open(&path, AgentProfile::default()).unwrap();
        let key = window_start_key(&snapshot, 2_000).unwrap();
        assert_eq!(first.claim_window_start(&key, "a", 2_000).unwrap(), Some(1));
        // Held by the first, even from another connection.
        assert_eq!(second.claim_window_start(&key, "b", 2_001).unwrap(), None);
        // After a crash the lease lapses, but the retry time still gates it.
        assert_eq!(
            second
                .claim_window_start(&key, "b", 2_000 + LEASE_SECS + 1)
                .unwrap(),
            None
        );
        // A claim the stored state no longer supports is refused.
        assert_eq!(
            second.claim_window_start("reset:1:1", "b", 99_999).unwrap(),
            None
        );
        // Released, the attempt does not count and can be taken at once.
        first.release_window_start(&key, "a").unwrap();
        assert_eq!(
            second.claim_window_start(&key, "b", 2_002).unwrap(),
            Some(1)
        );
        // After hand-off no later claim succeeds, whatever the time.
        assert!(second.dispatch_window_start(&key, "b", 2_003).unwrap());
        assert!(!second.dispatch_window_start(&key, "other", 2_003).unwrap());
        assert_eq!(first.claim_window_start(&key, "a", 99_999).unwrap(), None);
        // Only the holder can finish a claim; proof of non-delivery frees it.
        first
            .finish_window_start(&key, "intruder", true, 0, false)
            .unwrap();
        assert!(first.window_claim(&key).unwrap().unwrap().dispatched);
        second
            .finish_window_start(&key, "b", false, 2_100, false)
            .unwrap();
        assert_eq!(first.claim_window_start(&key, "a", 2_100).unwrap(), Some(2));
        // Settled, nothing more is claimed.
        second
            .finish_window_start(&key, "a", true, 0, false)
            .unwrap();
        assert_eq!(first.claim_window_start(&key, "a", 99_999).unwrap(), None);
    }

    #[test]
    fn a_change_committed_while_dispatch_waits_for_the_store_stops_the_message() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("race.sqlite3");
        let snapshot = expired(Some(1_000));
        let mut other = Store::open(&path, AgentProfile::default()).unwrap();
        other.save(&snapshot).unwrap();
        let mut holder = Store::open(&path, AgentProfile::default()).unwrap();
        let key = window_start_key(&snapshot, 2_000).unwrap();
        assert_eq!(
            holder.claim_window_start(&key, "a", 2_000).unwrap(),
            Some(1)
        );
        // Another process opens a write transaction that turns the setting off
        // and has not committed it yet.
        let mut off = snapshot.clone();
        off.claude_window_start.set_enabled(false, 5);
        let writer = other
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .unwrap();
        writer
            .execute(
                "UPDATE workspace SET snapshot=?1 WHERE id=1",
                [serde_json::to_string(&off).unwrap()],
            )
            .unwrap();
        let (sent, received) = std::sync::mpsc::channel();
        let racing_key = key.clone();
        let racer = std::thread::spawn(move || {
            let dispatched = holder
                .dispatch_window_start(&racing_key, "a", 2_001)
                .unwrap();
            sent.send(dispatched).unwrap();
            holder
        });
        // The check and the record need the store, so they wait for the other
        // write instead of reading the state from before it.
        assert!(received.recv_timeout(Duration::from_millis(500)).is_err());
        writer.commit().unwrap();
        assert!(!received.recv_timeout(Duration::from_secs(10)).unwrap());
        let holder = racer.join().unwrap();
        let held = holder.window_claim(&key).unwrap().unwrap();
        assert!(!held.dispatched && !held.settled);
        // Had nothing changed, the same transition succeeds.
        let mut holder = holder;
        let mut on = snapshot;
        on.claude_window_start.set_enabled(true, 1);
        holder.save(&on).unwrap();
        assert!(holder.dispatch_window_start(&key, "a", 2_002).unwrap());
    }

    #[test]
    fn administrator_hooks_refuse_the_message_unless_policy_disables_hooks() {
        let hooks = r#"{"hooks":{"SessionStart":[{"hooks":[{"type":"command","command":"x"}]}]}}"#;
        let policy = |files: &[(&str, &str)]| {
            let dir = tempfile::tempdir().unwrap();
            for (name, text) in files {
                let path = dir.path().join(name);
                std::fs::create_dir_all(path.parent().unwrap()).unwrap();
                std::fs::write(path, text).unwrap();
            }
            dir
        };
        let refuse = Err(WindowStartFailure::ManagedHooks);
        // No managed settings at all, or none that define hooks.
        assert_eq!(managed_hooks(policy(&[]).path(), "max"), Ok(()));
        assert_eq!(
            managed_hooks(&policy(&[]).path().join("missing"), "max"),
            Ok(())
        );
        let empty = policy(&[("managed-settings.json", r#"{"hooks":{},"model":"x"}"#)]);
        assert_eq!(managed_hooks(empty.path(), "max"), Ok(()));
        // Hooks in the policy are refused, whatever file they come from.
        let base = policy(&[("managed-settings.json", hooks)]);
        assert_eq!(managed_hooks(base.path(), "max"), refuse);
        let dropin = policy(&[("managed-settings.d/10-hooks.json", hooks)]);
        assert_eq!(managed_hooks(dropin.path(), "max"), refuse);
        // The administrator can switch hooks off in the policy itself; the
        // last file that sets it wins.
        let disabled = policy(&[
            ("managed-settings.json", hooks),
            ("managed-settings.d/10.json", r#"{"disableAllHooks":true}"#),
        ]);
        assert_eq!(managed_hooks(disabled.path(), "max"), Ok(()));
        let reenabled = policy(&[
            ("managed-settings.json", hooks),
            ("managed-settings.d/10.json", r#"{"disableAllHooks":true}"#),
            ("managed-settings.d/20.json", r#"{"disableAllHooks":false}"#),
        ]);
        assert_eq!(managed_hooks(reenabled.path(), "max"), refuse);
        // Hidden and non-JSON drop-ins are not policy.
        let ignored = policy(&[
            ("managed-settings.d/.hidden.json", hooks),
            ("managed-settings.d/notes.txt", hooks),
        ]);
        assert_eq!(managed_hooks(ignored.path(), "max"), Ok(()));
        // Unreadable policy is not assumed to be harmless.
        let broken = policy(&[("managed-settings.json", "not json")]);
        assert_eq!(managed_hooks(broken.path(), "max"), refuse);
        // Server-managed settings on Teams and Enterprise plans cannot be read
        // from the host, so they need the explicit switch-off.
        for plan in ["team", "Enterprise"] {
            assert_eq!(managed_hooks(policy(&[]).path(), plan), refuse, "{plan}");
            let off = policy(&[("managed-settings.json", r#"{"disableAllHooks":true}"#)]);
            assert_eq!(managed_hooks(off.path(), plan), Ok(()), "{plan}");
        }
    }

    #[tokio::test]
    async fn administrator_hooks_stop_the_attempt_before_the_model_list_and_message() {
        let policy = tempfile::tempdir().unwrap();
        let file = policy.path().join("managed-settings.json");
        std::fs::write(
            &file,
            r#"{"hooks":{"SessionStart":[{"hooks":[{"type":"command","command":"x"}]}]}}"#,
        )
        .unwrap();
        let managed: &'static str = Box::leak(policy.path().display().to_string().into_boxed_str());
        let limits = Limits {
            managed: Some(managed),
            ..SHORT
        };
        let dir = tempfile::tempdir().unwrap();
        let refused = fake(MODELS, &reply(Some(now() + 3600)));
        let workspace = open(dir.path(), &expired(Some(now() - 30)), &refused);
        workspace.window_start_tick(limits).await;
        assert_eq!(refused.count("auth "), 1, "{:?}", refused.lines());
        assert_eq!(refused.count("catalog "), 0, "{:?}", refused.lines());
        assert_eq!(refused.messages(), 0, "{:?}", refused.lines());
        assert_eq!(
            status(&workspace).last_outcome,
            Some(WindowStartOutcome::Failed {
                reason: WindowStartFailure::ManagedHooks,
                attempts: 1
            })
        );
        // With the administrator's switch-off in place, it proceeds.
        std::fs::write(
            &file,
            r#"{"hooks":{"SessionStart":[]},"disableAllHooks":true}"#,
        )
        .unwrap();
        let dir = tempfile::tempdir().unwrap();
        let fake = fake(MODELS, &reply(Some(now() + 3600)));
        let workspace = open(dir.path(), &expired(Some(now() - 30)), &fake);
        workspace.window_start_tick(limits).await;
        assert_eq!(fake.messages(), 1, "{:?}", fake.lines());
    }

    #[tokio::test]
    async fn policy_that_cannot_be_checked_stops_the_attempt() {
        assert_eq!(
            Limits::default().managed.is_some(),
            cfg!(target_os = "linux")
        );
        // Off Linux, Claude Code's policy lives elsewhere; it is refused, not
        // assumed absent.
        let limits = Limits {
            managed: None,
            ..SHORT
        };
        let dir = tempfile::tempdir().unwrap();
        let fake = fake(MODELS, &reply(Some(now() + 3600)));
        let workspace = open(dir.path(), &expired(Some(now() - 30)), &fake);
        workspace.window_start_tick(limits).await;
        assert_eq!(fake.count("catalog "), 0, "{:?}", fake.lines());
        assert_eq!(fake.messages(), 0, "{:?}", fake.lines());
        assert_eq!(
            status(&workspace).last_outcome,
            Some(WindowStartOutcome::Failed {
                reason: WindowStartFailure::ManagedHooks,
                attempts: 1
            })
        );
    }
}

# Claude window start

Status: **source implementation, not enabled, not deployed.** The setting is off by default. Nothing in this change has been run against a live Claude account, so no live message has been sent. The Rust checks could not run in the authoring environment; see [validation](#validation).

Claude subscriptions have a five-hour usage limit and a weekly limit ([how usage and length limits work](https://support.claude.com/en/articles/11647753-how-do-usage-and-length-limits-work), [managing usage credits](https://support.claude.com/en/articles/12429409-manage-usage-credits-for-paid-claude-plans)). When this setting is on, Relay sends one tiny message after a five-hour window it saw Claude report has ended, so the next window can start while the workspace is idle.

This reduces idle gaps on a best-effort basis. It adds no usage and does not change weekly limits. Anthropic's documentation does not state exactly when a window begins, so Relay does not rely on a first-message rule: it only acts on a reset time Claude has reported, and takes the one Claude reports after each message. There is no guarantee that Claude keeps a window active.

## Setting

Settings → Claude Window Start, Off or On, saved on the server. It is a workspace setting for the Claude account the server's Claude Code is signed in to, not a per-project one, so projects and sessions never send their own messages. The page states that each message uses a little Claude usage and shows the last attempt, its outcome and the next message due.

Turning it on is the owner's explicit authorization for these background model calls, and only for messages after a reported window end. It does not authorize a message to start a first window: with no reported reset, nothing is sent until a real Claude turn reports one. It is separate from an agent's Implement permission and approval mode, which never apply to it: the message has no tools, so it cannot change files or ask for approval.

## Scope of deduplication

Deduplication is per Relay database, not per Claude account. Two Relay servers (separate databases) signed in to the same Claude account share no claims and no activity, so each could send its own message and neither sees the other's real work. Run this on one Relay server per Claude account. Relay cannot detect or enforce that.

## Behavior

One scheduler per server reads the stored usage windows (`usage_limits`, kept current by every Claude turn's rate-limit report) and the usage ledger.

- **Reset known.** Nothing is sent while the reported window is open. Five seconds after its reset, if no Claude turn is running and no Claude turn completed since the reset, Relay prepares and sends one message. If Claude did have work after the reset, nothing is sent and the status says so.
- **No reset known.** Nothing is sent, including not asking Claude Code anything. Scheduling waits for a real turn to report a reset. Turning the setting on does not start a window.
- **After a message.** The reply's own rate-limit report replaces the expired window (the five-hour and weekly `unifiedWindows` that Claude Code 2.1.302 emits), and the next message follows the new reset. If the reply reports no window, nothing more is sent until a real turn reports one. A report received before a timeout is kept.
- **Running turns.** While any real Claude turn is queued or running, nothing is sent.
- **Weekly limit.** While the stored weekly window is used up and its reset is in the future, nothing is sent until that reset. A weekly-limit failure waits for the reset when it is known and otherwise stops.

The usage refresh at expiry is the stored state, which real turns keep current. Claude Code has an experimental `get_usage` control request, but it is marked unstable, so Relay does not use it. Relay also does not read Anthropic's undocumented usage endpoints or use an API key.

### Eligibility is checked again before sending

Preparing takes seconds, and real work or a newly reported window can arrive meanwhile, possibly through another process on the same database.

1. **Claim.** The claim transaction reads the stored snapshot and refuses unless the message for that reset is still due.
2. **Preparation.** While the sign-in check and model list run, any change to this process's snapshot that makes the message no longer due (real work, a running turn, a new reported window, the setting turned off) ends the preparation and gives the attempt back without counting it.
3. **Dispatch.** The last check and the durable "dispatched" record are one `BEGIN IMMEDIATE` transaction over the stored snapshot, taken under the store lock. A change committed by another process, which this process never hears of, either lands before it and stops the message, or waits for it and finds the message already recorded as dispatched. There is no gap between the check and the record.

## Message

The sign-in check, the model-list request and the message are launched the same way, as a fresh Claude Code process in an empty temporary directory (deleted afterwards) with its own process group:

- The environment drops `ANTHROPIC_API_KEY`, `ANTHROPIC_AUTH_TOKEN`, `ANTHROPIC_BASE_URL`, the Bedrock, Vertex and Foundry switches and Relay's own credentials. A stored API key is not in the environment, so it is handled by the sign-in check below.
- The model-list request and the message add `--safe-mode`, `--no-session-persistence`, `--strict-mcp-config`, `--disable-slash-commands`, `--setting-sources project,local`, `--settings {"disableAllHooks":true}` and `--tools ""`: no tools, MCP servers, skills, plugins, hooks, user settings or saved transcript, so enabling this cannot run a user, project or plugin hook or start a configured MCP server. Hooks defined by administrator policy are the exception, handled next.
- The message sends `Reply OK.` on standard input with the system prompt `Reply with the single word OK.`, and output capped by `CLAUDE_CODE_MAX_OUTPUT_TOKENS=64`.
- Limits are 10 seconds for the sign-in check, 30 for the model list and 90 for the message. All three are cancelled with the attempt: when the setting is turned off or the window is no longer due, the process group is ended; the server stopping ends them too.
- Claude output is used only to classify failures and is not stored or logged.

### Administrator hooks

Claude Code runs hooks from managed (policy) settings even under `--safe-mode` and with `disableAllHooks` in `--settings`: only `disableAllHooks` set in the policy itself suppresses them (read from the shipped 2.1.302 CLI by review, not exercised here). Relay does not override administrator policy, and no launch option can give a hook-free turn where policy defines hooks, so it refuses to launch there rather than weaken the guarantee. After the sign-in check and before the model list or the message, it reads only the managed settings files Claude Code uses on Linux, `/etc/claude-code/managed-settings.json` and the sorted `managed-settings.d/*.json`, and refuses with "Managed settings may run hooks" when they define hooks and the last `disableAllHooks` they set is not true, or when a file cannot be read or parsed.

Owner prerequisite: administrator policy on the server must define no hooks, or set `"disableAllHooks": true`. Server-managed settings (Teams and Enterprise plans) cannot be read from the host, so on those plans the file policy must set `"disableAllHooks": true` explicitly. Relay checks policy files only on Linux. On other operating systems Claude Code keeps policy elsewhere, so Relay refuses with the same message instead of assuming there is no policy. This reads configuration, never credentials.

### Subscription sign-in

Nothing is sent unless `claude auth status --json`, run in that same context, reports a first-party subscription: `loggedIn` true, `apiProvider` `firstParty`, a non-empty `subscriptionType`, no `apiKeySource`, and an `authMethod` that is not an API key. A stored API key (including one set up through `/login`), another provider, a nonzero exit, unreadable output or missing fields are refused ("Claude Code is not signed in with a subscription"), and signed-out is reported as such. Relay does not read credential files. The field names and values come from Claude Code 2.1.302's `auth status` implementation as described in review; the check was not run here, and if a version reports them differently it fails closed instead of guessing. This may refuse some subscription-style logins, such as a long-lived `CLAUDE_CODE_OAUTH_TOKEN`, which can lack a `subscriptionType`.

### Model

Relay asks the installed Claude Code for the account's model list and accepts only an entry whose resolved model is a concrete Haiku id (`claude-…haiku…`): the entry's `resolvedModel`, or its own id when the catalog gives no resolution. An alias the catalog resolves to another model, such as Haiku remapped to Sonnet through `ANTHROPIC_DEFAULT_HAIKU_MODEL`, and an alias it does not resolve are refused. The message then runs with `--model` set to that canonical id rather than the alias, so alias remapping cannot change it. Relay does not rank prices, and with no such entry it sends nothing and never substitutes another model.

The model Claude Code reports at startup is checked again, but that report arrives while the request may already be on its way, so it is only a consistency check and cannot prevent billing. The protection is the validated, pinned id.

## Deduplication, ambiguous sends and failures

Attempts are recorded in the `window_start_claims` table (schema 24), keyed by the expired reset and the opt-in. A claim is one `BEGIN IMMEDIATE` transaction, so two server processes on one database, several projects and restarts cannot start the same attempt. A claim holds a 150 second lease and a retry time set when it is taken, so a crash before dispatch backs off like a failure.

Before anything is handed to Claude Code the claim is marked **dispatched**, durably. From then on a message may have been delivered, so:

- Only the service refusing the request, as a structured `rate_limit_event` with status `rejected`, shows nothing was delivered. Such a failure leaves the claim retryable or settled as below.
- Every other failure after hand-off is **unconfirmed**, whatever its text: a timeout, the stream ending without an answer, a connection reset or `ETIMEDOUT` (which can happen while a reply streams back), an authentication or limit message, a different model reported, the server stopping or crashing. Error text is keyword-classified and proves nothing about delivery. The claim stays dispatched and unsettled, and no message is sent again for that reset, even after the lease and backoff. It is reconciled when Claude reports a new reset, which makes the old key irrelevant, or when the owner turns the setting off and on (a new opt-in key). The status shows "may have been delivered". If Claude never reports a new window, scheduling stays stopped; this fails closed.

So for a given reported reset, at most one message is handed over unless every earlier attempt proved nothing was delivered. Before hand-off, or after a structured rejection, at most three attempts are made, 5 then 15 minutes apart, for timeouts and transient errors; being signed out, not on a subscription, administrator hooks, a missing canonical Haiku, an incompatible Claude Code, or a weekly limit with no known reset stops attempts for that window. Turning the setting off and on re-arms it.

## Schema 24

The setting and status are stored in the snapshot (`claude_window_start`, omitted while untouched) and the claim table (with its `dispatched` column) is created at startup. Older servers would drop both, so the schema is 24. Existing databases load with the setting off. The [schema 21 to 25 plan](schema-25-migration.md) covers this step for a release from schema 21.

## Validation

Authoring environment: no Rust toolchain was available. Nix could not reach its daemon socket in the sandbox, the worktree's `.gitmodules` was masked, and a system Rust toolchain is not an accepted substitute. No `cargo build`, `cargo test`, Clippy or formatter ran, so the new code has not been compiled and none of the tests below have run. Review it with `just check` before relying on it.

Tests written but not run:

- `relay-core`: synthetic-clock tests of the schedule for expiry, new windows, real work, running turns, no reported reset, retry limits, possibly delivered messages and reconciliation, weekly exhaustion and disabling.
- `relay-server`, with a fake `claude`:
  - the success path, including the identical hardened arguments of the model-list request and the message, the pinned canonical model, an empty directory, no API key and the real `unifiedWindows` shape;
  - no reset known (nothing, not even the sign-in check, runs);
  - sign-in refusals (stored API key, another provider, malformed output, nonzero exit, signed out) never reaching the model list or message;
  - a missing, remapped or unresolved Haiku;
  - a gated model list during which a new window is published, real work is recorded by another process, or the setting is turned off;
  - a structured service rejection (retryable or settled) versus every other failure after hand-off, including partial replies followed by `ECONNRESET`, `ETIMEDOUT`, `ENOTFOUND`, a dropped stream, an authentication message or a limit message, none of which is resent after the lease and backoff are cleared; a crash after hand-off and a timeout, also not resent, with a timeout keeping its reports;
  - administrator policy: a table of managed settings files (hooks, disabled, overridden, drop-ins, hidden files, broken files, Teams and Enterprise plans) and an attempt that stops before the model list and message, then proceeds once policy disables hooks;
  - a host whose policy Relay cannot check (not Linux), which stops before the model list and message;
  - a deterministic interleaving: another connection holds an uncommitted write that turns the setting off while the dispatch transition runs in a thread; it blocks until the commit and then refuses;
  - two stores on one database plus a restart;
  - turning the setting off mid-message;
  - exclusive, due-checked claims.
- `relay-desktop`: the status text and the control sending the setting.
- The 23→24 upgrade test in `tests.rs`. Schema 25 ([worker execution approval](architecture.md#worker-execution-approval)) followed, so the schema assertions in every place that opens a database with `Store::open` now expect 25.

Not verified:

- That hooks are in fact suppressed or refused in execution: the tests check the policy decision from files in a temporary directory, not Claude Code's behavior. Whether server-managed settings or other platforms' policy locations define hooks is not checked.
- Claude Code's real output for `auth status --json`, `rate_limit_event`, `--safe-mode`, `--tools ""`, `--setting-sources project,local` and `CLAUDE_CODE_MAX_OUTPUT_TOKENS=64`.
- Whether the account lists a canonical Haiku model, and the service's behavior for a real tiny message.
- That processes are ended if the Relay server dies abruptly; shutdown and cancellation paths are covered, abrupt parent death is not, and process-group cleanup on drop relies on Linux process identity.
- Rustfmt, Clippy, and whether the new Settings module affects existing desktop layout tests.

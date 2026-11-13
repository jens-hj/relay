# Deployment

Deployment runs independently of the Relay service being replaced. Stopping that service can stop the deploying agent, so an agent's last message or a successful `systemctl restart` is not deployment confirmation. The deployment's durable status is authoritative.

## Health and Verification

Check the candidate locally before treating a public-network failure as an application failure. Public HTTPS reaches Relay through additional DNS, TLS, and proxy infrastructure; an EOF or timeout there does not establish that the candidate server is broken.

| Result | Deployment Decision |
| --- | --- |
| Local startup, revision, artifact, or data-preservation check fails | Report failure and attempt rollback only when ownership and schema checks permit it. |
| Local checks pass, but public DNS, TLS, connection, rate-limit, or gateway checks fail | Keep the healthy candidate active and report public verification pending. Retry verification without restarting. |
| Public revision, asset hash, or authentication checks fail | Report a verification failure; never claim deployment success. |
| Local and public checks pass | Record the deployment as verified, including the running revision. |

Public checks retain certificate verification and compare the served browser assets with the candidate package. Retrying a TLS connection must not silently disable authentication, accept a mismatched asset, or treat an HTTP authorization failure as success.

Public HTTP `408`, `429`, `502`, `503`, and `504` responses are retryable transport or availability failures. Authorization failures and application errors such as `401`, `403`, and `500` remain hard failures. A public failure is never reported as verified deployment success.

A truncated HTTP response body is a transport failure. Discard the partial body and use the same certificate-verified curl fallback for public requests. If that also fails in transport, retain the healthy candidate with public verification pending; never accept partial asset bytes or restart it to retry verification. A truncated local response fails local readiness.

## Preparation and Recovery

This helper handles releases that preserve the installed protocol and schema, and releases whose candidate migrates the database schema, the stored protocol, or both at startup. A schema- or protocol-changing release needs a separately reviewed migration and recovery plan; the helper refuses mismatched pre-deployment versions. The schema 21 to 25 release, which includes [Claude window start](claude-window-start.md) and [worker execution approval](architecture.md#worker-execution-approval), has a [proposed plan](schema-25-migration.md).

Build the server and browser packages from the same immutable commit, and preserve the previous package bundle as a Nix GC root. Prepare a private deployment directory outside Git with mode `0700`; keep its manifest, status, logs, and backups owner-readable. Manifests contain credential file paths, never credential values. Credentials are read only inside authenticated requests and never placed in command-line arguments or logs.

Use SQLite's backup API for a consistent backup. Preserve the existing database, credential paths, origin, port, and harness environment. Activate only the Relay units; do not apply unrelated host configuration changes. Keep the previous bundle and backup until acceptance.

Use the same helper for managed deployments so its activation lock can serialize them. The status command does not take that exclusive lock and remains readable while slow probes are in progress. Repeating activation for an existing attempt is rejected without restarting or rolling it back.

A deployment must not roll back another deployment's active bundle. It also must not start an older server against a database whose schema is incompatible with that server. Such cases require explicit recovery, not an automatic database restore that could discard later user data. Restarted agent turns remain interrupted; deployment does not replay them.

## Run the Workflow

### Lifecycle Outbox Migration: Schema 29 to 30

The lifecycle notice outbox tracked by [#156](https://github.com/jens-hj/relay/issues/156) and [#151](https://github.com/jens-hj/relay/issues/151) adds `lifecycle_events` and `lifecycle_holds`. The migration preserves existing snapshots, runs, drafts, receipts, and spawn outcomes. It starts an empty lifecycle ledger: historical terminal sessions are not scanned to recreate old notifications. Active runs interrupted by this startup get terminal events as part of their interruption transaction. The wire protocol stays at 17; server and clients retain the run-bound controls.

Terminal events and stop suppression commit with their corresponding snapshot changes. Unbound events recovered at startup become paused notices for explicit review. A bound notice keeps its submission identity across restarts, even if launch or native output was not acknowledged. Deployment never releases a human hold, replays an interrupted notice, or retries its tool effects. A stopped parent also retains future notices for review when its queue was empty at the time of the stop.

Treat this as a separate migration release from the schema 29 catalog deadline fix. After review, build matching immutable server/browser packages and a unit bundle, retain the previous bundle as a GC root, and take a consistent SQLite backup before activation. In the protected helper manifest set `previous_schema_version` to `29` and `schema_version` to `30`; use the candidate's actual unchanged protocol version rather than example numbers. Verify schema, running revision, executable/package identity, history/drafts, and recovered notice identities locally before public verification. Keep all credentials, snapshots, and transcript contents out of logs.

Schema 29 servers reject schema 30 databases. Once migration commits, switching only the old binary back is not a valid rollback. The helper's schema fence must refuse that downgrade and must not automatically restore a backup over newer user data. If recovery is needed, prefer a compatible schema 30 fix. Restoring the pre-migration database and old bundle requires an explicit, separately reviewed recovery decision covering all writes since the backup; preserve the current database and WAL before any approved restore. Do not lower `user_version` or drop the ledger to make an older binary start. Public transport failure after successful local migration retries verification only, without another activation.

The helper takes a protected JSON manifest and provides three commands:

```sh
just deploy activate --manifest /protected/release/manifest.json
just deploy verify --manifest /protected/release/manifest.json
just deploy status --manifest /protected/release/manifest.json
```

`activate` is a service-changing operation requiring explicit authorization. Run it from an independent user systemd job, using a copy of the helper and an absolute Python executable outside the service's control group:

```sh
systemd-run --user --unit=relay-deploy-RELEASE --property=Type=exec \
  /absolute/python3 /protected/release/deploy-relay.py activate \
  --manifest /protected/release/manifest.json
```

Do not use `--scope`, `--wait`, `PartOf=relay.service`, or `BindsTo=relay.service`. Keep the helper and its manifest available until the job and any verification retries have finished. The direct `just deploy activate` form is suitable only when its caller will survive the restart.

Use `verify` to resume verification after a public transport failure. It does not reactivate the bundle or restart Relay. A separate verification job or timer can invoke this command again while public verification is pending. A process that automatically retries must retry **verification**, not **activation**.

Data preservation is checked immediately after activation, before the candidate is marked locally healthy. Later verification checks current health and package identity; it does not compare live user data against an old baseline and mistake subsequent user deletions for deployment damage.

Exit codes distinguish outcomes: `0` means verified success, `2` means public verification is pending with the candidate retained, and `1` means a hard failure. The `status` command reports durable state; its successful exit only means that the status was read. Check the recorded phase before announcing deployment success.

The manifest includes absolute paths and package identities, for example:

```json
{
  "active_link": "/home/alex/.local/share/relay/deployment",
  "candidate_bundle": "/nix/store/CANDIDATE-relay-units",
  "previous_bundle": "/nix/store/PREVIOUS-relay-units",
  "database": "/home/alex/.local/share/relay/workspace.sqlite3",
  "credential_file": "/home/alex/.config/relay/admin-token",
  "server_executable": "/nix/store/CANDIDATE-relay-server/bin/relay-server",
  "web_root": "/nix/store/CANDIDATE-relay-web",
  "revision": "FULL_GIT_COMMIT",
  "protocol_version": 11,
  "schema_version": 18,
  "local_url": "http://127.0.0.1:7331",
  "public_url": "https://relay.example.org",
  "state_dir": "/home/alex/.local/share/relay/deployments/RELEASE"
}
```

Replace the example values with the built packages and existing installation. Protocol and schema versions must match the candidate; the example numbers are not a migration policy. For a release that migrates the database, set `previous_schema_version` to the live schema and `schema_version` to the candidate's schema. `activate` checks the live database against `previous_schema_version`, the checks after activation use `schema_version`, and the previous bundle is restored automatically only while the database is still at `previous_schema_version`. Without the field, both checks use `schema_version`. Likewise, for a release that moves the protocol, set `previous_protocol_version` to the live protocol and `protocol_version` to the candidate's: `activate` checks the running server against the previous protocol and, after activation, the candidate against `protocol_version`. The helper defaults to the `relay.service` and `relay-funnel.service` user units; specify `service` and `public_service` when the installation uses different names. The manifest and its parent must have modes `0600` and `0700`, and live deployment state must stay outside the checkout. Do not include the credential itself in the manifest.

## Regression Checks

Run `nix develop --command just deploy-check` to exercise deployment decisions with isolated fixtures. These checks belong to `just check`; they do not restart production services or contact the public deployment.

The regression that matters for the reported incident is: activate a locally healthy candidate, fail public TLS verification, retain that candidate with a pending status, then complete verification successfully without another restart.

## Usage Reset Notifications: Schema 33 to 34

[Issue #173](https://github.com/jens-hj/relay/issues/173) adds opt-in profile
booleans `notify_session_usage_reset` and `notify_weekly_usage_reset`.
Both default to false and support profile inheritance. Codex only uses the
session option; Claude Code supports both. Session means the reported five-hour
usage window, not the conversation's context window.

The scheduler observes future reset timestamps reported to Relay and queues one
notice per enabled agent and window when the timestamp is reached. It does not
poll provider accounts or claim that a new quota has been confirmed. Unknown
and already-expired timestamps do not arm new notices. Existing running agents
receive a queued turn; stopped or paused agents retain it for review. Disabling
an option or archiving an agent cancels its pending notices. Overdue records
recovered at startup become paused turns, never automatic work replay.

Schema 34 adds an initially empty `usage_reset_notices` table. Delivery receipts
commit atomically with queued submissions, so restart cannot duplicate a notice.
Protocol 19 protects clients from profile fields older clients cannot deserialize.
The migration retains existing data and defaults all new settings off.

Deploy matching immutable server/browser builds using the helper with
`previous_schema_version: 33`, `schema_version: 34`,
`previous_protocol_version: 18`, and `protocol_version: 19`.
Keep a protected SQLite backup and previous bundle GC root before activation.
After migration, schema 33 binaries cannot safely open the database; recover with
a schema 34 fix. Do not restore the pre-migration backup over later writes without
an explicit recovery decision. Verify local and public revision and asset checks
through the helper; a public transport failure leaves the healthy candidate active
and retries verification only.

## Cross-Harness Reset Subscriptions: Schema 34 to 35

The follow-up to #173 makes subscriptions independent of the agent's harness.
Every profile can select Codex session, Claude session, and Claude weekly
notifications under Usage Reset Notifications. A Codex delegator can subscribe
to Claude resets while retaining Codex resets. Delivery goes to the subscribed
agent; it decides whether to continue a worker using its existing permissions.

The optional `usage_reset_notifications` profile object contains booleans
`codex_session`, `claude_session`, and `claude_weekly`. An explicit object
takes precedence over the legacy two booleans, including when all choices are
off. Missing objects retain the old own-harness behavior, preserving existing
settings and pending notices. Editing subscriptions stores an explicit object;
subsequent harness changes leave those subscriptions unchanged. Overrides
inherit the subscription group, and reverting to inheritance restores the
parent group's choices.

Schema 35 protects the new stored profile field; protocol 20 keeps old clients
from rejecting it. Existing notice receipts and pending windows remain intact.
Use deployment manifest versions 34 to 35 and protocol versions 19 to 20,
retain the old bundle and a protected database backup, and verify local health,
data preservation, public revision, and asset hashes. After migration, recover
with a schema 35 fix; do not start schema 34 binaries against the migrated data
or restore a backup over later writes without a separate recovery decision.

## Reset Notice Attribution and Launch Authorization

The #173 follow-up fixes the reset-specific notice prefix being omitted from
Relay prompt attribution and the no-client-receipt launch path. A reset may
launch only with a matching durable submission and a delivered reset record
for the same session. Current implementation permissions, stop/pause holds,
workspace contention, and run identity checks still apply. Tests run real
Codex and Claude harness transports after a simulated five-hour idle wait.

Schema 35 and protocol 20 stay unchanged. At startup, historical prompt rows
backed by delivered reset records have their author corrected from You to
Relay. Message IDs, order, payloads, submissions, and session states are
preserved; this repair does not retry already-failed turns. Future eligible
reset notices continue automatically, and an existing failed turn can be
continued explicitly. Deploy matching immutable packages with the usual
protected backup and local/public verification.

## Fable Weekly Allowance: Schema 35 to 36

Issue #174 preserves Claude's seven_day_overage_included window as the distinct
FableWeekly allowance. The installed Claude CLI labels this field Fable limit.
Percentages are relative to that allowance, not total weekly usage. The detail
shows the reported percentage and reset time with the user-supplied allowance
explanation; missing or expired measurements are hidden, never estimated from
token mix or the overall weekly percentage. Existing reset subscriptions retain
their meaning; this feature adds no automatic Fable reset notifications.

Schema 36 and protocol 21 protect the additional stored enum variant. The
migration adds no tables, removes no data, and preserves old usage windows,
profiles, receipts, and history. Older servers reject the new schema before
reading the new variant. Use a protected SQLite backup and retain the previous
bundle before deployment. Use manifest versions 35 to 36 and 20 to 21, with
matching immutable server/browser artifacts. After migration, use a schema 36
fix for recovery; do not downgrade the binary or restore over later writes
without an explicit recovery decision.

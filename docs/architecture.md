# Architecture

## Product direction

Relay is a native, task-centered agent workspace. A named project owns a server root and independent repository, board, and directory connections. Work can start on a local task board, then be published to GitHub/GitLab while preserving task and session identity. Connected remote boards remain authoritative. Conversations, delegation, verification, and results preserve provenance.

Directors coordinate workers, follow work to its configured completion criteria, and surface decisions that need human attention. A project may have multiple specialized directors. The workflow is opt-out; direct interaction with agents remains part of the product. Declarative profiles guide behavior and supported action boundaries will be enforced outside the agent's prose instructions.

Execution belongs on the server. Clients can disconnect, reconnect, and share sessions from different machines. The interface treats conversations as interactive documents, with keyboard navigation and contextual review. Efficiency controls remain explicit: compact and continue, reset context while retaining history, and archive/start a linked session. Cache and usage claims must distinguish measured state from estimates.

## Foundation boundaries

`relay-core` defines the domain, profile resolution/validation, fixture seed, and protocol. `relay-server` owns storage, authorization, mutations, and event publication. `relay-desktop` contains the Mosaic interface and a background network runtime. Reactive UI objects never cross worker threads; Mosaic's state channel delivers network updates to the UI thread.

The demo board and transcripts are immutable fixtures. Named projects coexist in the sidebar; changing the selected project or board does not stop agents elsewhere. Boards own their columns and task memberships, separately from stable task identities. Local tasks have no fabricated provider reference. A source issue can belong to several boards within one project. Explicit synchronization retains the last confirmed board on failure and preserves removed tasks for history; work requires active membership. GitHub imports issue items across all repositories, excluding draft issues and pull requests. GitLab supports project/group Open/Closed and label lists; unsupported filters/list types produce explicit errors rather than a misleading mirror.

Repositories clone under `ROOT/repos/HOST/NAMESPACE/REPO` using the server's existing Git credentials. `ROOT/workspace` is the default directly writable directory. Each new repository session receives isolated worktrees beneath `ROOT/.relay/sessions/SESSION`, with a recorded base and review per repository. Connected ordinary directories are edited directly under the execution policy. Sessions capture their resource IDs at launch; later connections never silently widen a resumed thread's writable roots. Existing sessions retain their original paths and exact harness thread.

Agent sessions always reference a local task or remote issue and a director. A director's first conversation creates a local planning task. Each turn checks current scope, action permission, board membership, and worker limit; Ask requires approval and Deny is not overridable. Directors do not consume worker slots. Codex uses app-server stdio; Claude Code uses streaming JSON/control protocol. Both report transcript, thread identity, lifecycle and measured usage. Completed means the turn ended, not independently accepted work. Agent completion never commits, integrates, publishes a board, or changes remote status automatically.

Explicit task edits and moves on a remote board go through its provider. Publishing a local board selects a new or existing destination, destination repositories, and column mappings. Each write persists an in-flight journal key before contacting the provider and its result before the next step. Provider APIs do not guarantee exactly-once creation: uncertain outcomes require a validated remote result before continuing. A board becomes remote only after publication and authoritative readback succeed. Publication redirects duplicate board/task identities to the preserved local IDs; historical records, sessions, scopes and receipts retain their original IDs. Session transcripts are retained locally, not exported as issue comments.

Reconciliation commands accept only the exact unresolved journal step. Board and issue results must decode to scoped provider references; membership results require a nonempty item ID; status and edit results require the literal `confirmed`. Steps must belong to the operation's task and kind. Pending markers, column mappings and known results cannot be overwritten. Rejected input leaves the operation unchanged and schedules no continuation.

Execution settings inherit project → director → worker and are captured when each turn launches. Workers retain their original harness across profile changes. Automatic configures Codex workspace-write/never approval or Claude native auto/required sandbox. Ask uses native permission callbacks, recorded with exact run identity and answered inline once. Pending requests expire on terminal state/restart; receipts make answers idempotent. Unrestricted Access explicitly selects native full access/bypass. Server-enforced scope, board membership, action permission, and worker limits apply in every mode. Other profile responsibilities and completion criteria remain workflow configuration; Relay does not yet turn every configured action into a separately enforceable harness tool permission. Context compaction/reset and predictive cache warnings require a later harness integration. Cached token counts are measurements from completed turns, not a cache-expiry estimate.

One server process owns one SQLite database. Its schema is versioned with `PRAGMA user_version`; schema versions newer than the server are rejected. Schema 8 retains the latest measured usage while subsequent turns are queued. Cache retention and future re-cache size remain unknown. Schema 7 adds persisted harness-reported model and context metadata (issue #19), defaulting missing fields in older snapshots to unavailable. Schema 6 adds browser authentication tables. Schema 5 migrates schema 4 and earlier workspaces into project connections and separate board memberships, preserving project/task/director/session IDs, receipts, comments, drafts, assets and exact harness thread/worktree identities. Existing checkouts are neither moved nor cloned again. New servers reject newer schema versions. It updates only unchanged bundled implementation Ask defaults to Allow; custom defaults and explicit overrides remain intact. Shared drafts, draft receipts, immutable assets, and durable submissions remain preserved. Every mutation and receipt is committed in one transaction before publishing the snapshot. Run one server per database; horizontal scaling is not supported.

On shutdown, the server closes WebSocket handlers and terminates owned worker groups. On Linux, process identity includes boot/start information and harness children receive a parent-death signal; restart classifies unfinished runs as Interrupted without relaunching them. Processes that deliberately escape supervision are outside this guarantee. Non-Linux crash cleanup and native behavior still require platform validation. GitHub and Git subprocess captures have output limits and deadlines; transcript messages and review diffs explicitly report truncation.

## Protocol

All routes require `Authorization: Bearer <token>`, including WebSocket upgrade. The shared token defines a trusted workspace, not individual accounts or project roles. Legacy command bodies are bounded to 64 KiB, structured conversation/draft envelopes to 512 KiB, and individual asset uploads to 20 MiB.

| Endpoint | Behavior |
| --- | --- |
| `GET /v1/snapshot` | Current authoritative snapshot, including monotonic revision |
| `POST /v1/commands` | Typed command envelope; returns committed snapshot |
| `POST /v1/boards/discover` | Read destination metadata and named columns before publication |
| `POST /v1/operations/reconcile` | Validate a known remote result URL and preview recovery without writing |
| `GET /v1/harnesses` | Cached, bounded executable/version/authentication probes on the server |
| `POST /v1/harnesses/refresh` | Refresh read-only harness probes |
| `GET /v1/events` | WebSocket: initial snapshot followed by committed snapshots |
| `POST /v1/conversation/commands` | Structured turn/queue commands; returns committed snapshot |
| `GET /v1/drafts` | Shared draft documents with independent per-session revisions |
| `POST /v1/drafts/{session}` | Compare-and-swap draft save, with an immutable request receipt |
| `GET /v1/drafts/events` | WebSocket: full draft state, independent of transcript streaming |
| `POST /v1/assets/{uuid}` | Immutable binary upload and validated metadata acknowledgement |
| `GET /v1/assets/{uuid}` | Authenticated inline file/image content |

Command envelopes carry `request_id` (UUID), `expected_revision`, and a tagged command. Supported commands register GitHub project/checkout bindings, configure harness executables and execution modes, answer native tool requests, update profiles/comments, synchronize saved projects, and start/send/stop workers. Stale revisions return HTTP 409. Invalid references/configuration return 422; invalid credentials return 401. Unexpected storage failures return a sanitized 500 response. Reusing an ID for a different command is rejected. Execution and provider commands are server runtime operations, not pure domain mutations; durable command receipts prevent duplicate launch on deliberate retry.

Event delivery uses a watch channel: intermediate snapshots may coalesce, but each event is complete state. Client revision checks prevent late responses from replacing newer snapshots. Reconnection reads authoritative state and resubscribes. Writes are not retried automatically. Durable receipts make deliberate retries idempotent.

Legacy comments retain their message IDs and optional quotes. Buffer replies carry a message ID, exact UTF-8 byte range, and source quote, so repeated passages remain distinguishable. Recorded source text is immutable; typing there creates a reply in the shared next-message draft. Every inline and bottom-draft view edits the same ordered parts. Text, assets, and anchored replies survive serialization without flattening their positions.

Draft saves compare their own revision, so token streaming cannot invalidate a save. Submitting compares the saved draft revision and exact content, atomically clears that draft, and creates one durable submission. Exact receipt retries never create another turn. Queue edits retain submission identity and atomically consume a reviewed shared draft. Other queue actions compare the workspace revision; promotion also requires the exact observed active run ID. The desktop retains the original envelope for ambiguous outcomes.

A running agent finishes before queued turns launch, unless the user promotes a specific queued message. Promotion requests `turn/interrupt`, stops the owned process, and resumes the same thread with the promoted payload. Current issue membership, effective permissions, scope, harness, and capacity are checked again at launch. Failure, manual stop, shutdown, or restart pauses unsent work. Restart marks launching/running submissions Interrupted and never replays them; only unsent queued submissions can be explicitly resumed. A crash after delivery cannot guarantee a terminal outcome, and is never treated as permission to replay.

Asset metadata must match an acknowledged upload. Drafts are limited to 64 KiB of text/quotes, 1024 parts, and 64 MiB of referenced files; images are validated and limited to 32 megapixels. On each turn the server materializes assets in a private temporary directory outside Git, removes it after the turn, and sends ordered text/localImage Codex inputs or text/base64-image Claude content blocks. Other files are referenced at their exact position and require the agent to use a read tool. Replies identify quoted source explicitly as context. Local recovery stores unsent drafts and pending asset bytes next to client settings with owner-only permissions on Unix. It does not automatically submit recovered work.

## Profiles

Project defaults are complete typed profiles. Directors hold optional overrides. Omitted values follow current project defaults; explicitly set values replace the whole field. Changing defaults validates all affected directors. New directors inherit the current defaults. A reusable complete TOML template can seed a new database via `RELAY_DEFAULT_PROFILE`.

Profiles configure Codex or Claude Code, whole-project or selected-issue scope, responsibilities, completion requirements, a worker limit from 0 to 64, and deny/ask/allow permissions for each named action. Completion requirements are a checklist, not a fixed terminal stage or ordered pipeline. Editing/importing changes a draft; saving validates and persists it. The UI displays effective values and their origins. Harness status distinguishes installation/authentication from model access and managed execution policy. Project bindings are persisted on the server and validate the checkout’s GitHub origin; both current and previously registered boards remain usable.

## Next milestones

1. Extend GitLab support to additional board filters and list types while preserving provider semantics.
2. Add autonomous director delegation, richer harness capability reporting, and enforceable permissions for additional actions.
3. Add identities, membership, review permissions, and shared-session handoff.
4. Extend document selection across recorded messages, add richer file review, and implement the three context lifecycle actions with actual harness support.
5. Add reliable usage/cache reporting, then validate macOS/Windows and consider the browser client.

## Protocol compatibility

Snapshots declare `protocol_version: 2`. Every mutation requires `X-Relay-Protocol: 2`; older clients receive `protocol_mismatch` before changing state. Update server and desktop together. New commands cover project creation, connection management, board/task changes, publication, recovery and task-linked session launch. `POST /v1/boards/discover` reads provider destination metadata for named column choices. Remote credentials stay on the server; the desktop supplies project and provider identifiers only.

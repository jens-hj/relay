# Relay working on Relay

Tracked milestone: one authoritative GitHub Projects board, one issue-linked Codex worker, and a reviewable result in the native client. Relay itself is the first connected repository. The orchestrator owns integration and validation; two HAPI workers own the server and desktop slices in separate worktrees.

## Acceptance criteria

- Mirror GitHub Projects v2 Status columns and issue items for the configured repository. Paginate; exclude draft issues and pull requests explicitly. Preserve provider-qualified issue identity. Remote sync errors leave the last good board intact and visible. Sync is explicit initially. No remote status changes are implied by local execution.
- Configure the repository and board on the server, never accept an arbitrary filesystem path or executable from the client. Keep the fixture workspace usable when no remote is configured.
- Start a worker only from a live issue and a director whose effective profile allows implementation, includes the issue in scope, selects Codex, and has an available worker slot. Ask permission requires explicit approval for each turn; deny cannot be overridden by the client.
- Create an isolated Git worktree/branch before starting `codex exec --json`, using the server's existing CLI authentication. Use workspace-write sandboxing; never a permission bypass. Resume the exact recorded thread ID. Neither closing the desktop nor dropping a WebSocket stops a run.
- Persist session identity, immutable messages, thread ID, lifecycle state, measured token usage, and a bounded review diff including untracked files. No automatic merge, push, deploy, or claim that tests succeeded without evidence. Mark unfinished runs interrupted on server restart; continuation is explicit.
- Commands remain revision-checked and idempotent, including process launch. Process output cannot block other clients. Stop terminates the owned process and produces a truthful terminal state. Errors are actionable without exposing credentials.
- UI: remote board identity and sync action, issue-scoped start form, session status/transcript, send/continue/stop, measured usage, review diff and worktree provenance. Retain drafts on failure, retain keyboard access, and label fixtures honestly. Context/cache controls remain unavailable until supported; usage is not a cache-expiry prediction.
- Validate migration of existing databases, sync pagination/mapping/failure, permission/scope/concurrency rejection, duplicate start prevention, Codex event parsing and process failure/stop/restart, reconnect while a run is active, and native/headless UI interactions. Finish with a live Relay issue run and inspect its real result before pushing.

## Shared contract

Keep `/v1/snapshot`, `/v1/commands`, `/v1/events`. Add optional, serde-defaulted `Project.github: Option<GitHubProject>` and `Session.worker: Option<WorkerRun>` so existing snapshots migrate without losing comments/profiles. Existing fixture constructors set both to `None`.

`GitHubProject { owner: String, number: u64, url: String, last_synced_at: Option<u64>, sync_error: Option<String> }`.

`WorkerStatus`: queued, running, completed, failed, stopped, interrupted (snake_case). Completed means the harness turn ended, not that issue acceptance criteria were independently met.

`TokenUsage { input_tokens: u64, cached_input_tokens: u64, output_tokens: u64 }` holds the latest measured turn usage. Unknown usage is `None`.

`ChangeSet { files: Vec<String>, diff: String, truncated: bool }` is bounded review material, not permission to apply a patch.

`WorkerRun { status: WorkerStatus, thread_id: Option<String>, worktree: Option<String>, branch: Option<String>, base_commit: Option<String>, error: Option<String>, usage: Option<TokenUsage>, changes: Option<ChangeSet> }`.

New commands:

- `SyncProject { project_id }`
- `StartWorker { issue_id, director_id, prompt, approve_implementation: bool }`
- `SendWorker { session_id, prompt, approve_implementation: bool }`
- `StopWorker { session_id }`

Execution/sync commands are dispatched by the server, not `Snapshot::apply`. Session IDs for start are `session-{request_id}`. All output messages are immutable once published, with unique IDs; avoid updating an existing message body under a comment anchor. Runtime progress belongs in session state. Resume is possible only for a nonactive session with a recorded thread/worktree. UI can select that session immediately after start acknowledgement.

Server environment: `RELAY_GITHUB_REPO=owner/repo`, `RELAY_GITHUB_PROJECT_OWNER=owner`, `RELAY_GITHUB_PROJECT_NUMBER=number`, `RELAY_REPO_PATH=/absolute/local/repository`. Use `gh` for authenticated GitHub requests, argument arrays without a shell. `codex` comes from the server environment. Missing configuration/tools/auth must be explicit errors. Tokens never enter task files or command arguments.

## Ownership and order

1. Parent commits this contract and core types, creates the remote board/issues, then starts two Codex HAPI workers using their configured model and normal permissions.
2. Server worker owns `crates/relay-server`, server tests, and necessary workspace dependency/lockfile changes. It may adjust the shared core only after coordinating the change with the parent and desktop worker.
3. Desktop worker owns `crates/relay-desktop` and its tests. No server/environment changes.
4. Parent owns Nix/just ergonomics, docs, integration, independent review, live smoke test, final issue updates, and push. Workers commit to their own branches and do not push or merge into main.

Codex reference: [non-interactive execution and JSONL events](https://developers.openai.com/codex/noninteractive/). GitHub Projects remains the source of board status; worker status is displayed separately.

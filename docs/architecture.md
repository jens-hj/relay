# Architecture

## Product direction

Relay is a native, issue-centered agent workspace for small teams. GitHub/GitLab remains the authoritative project board. Exploration may begin without an issue; implementation must be issue-linked. Conversations, delegation, verification, and results preserve the work's provenance.

Directors coordinate workers, follow work to its configured completion criteria, and surface decisions that need human attention. A project may have multiple specialized directors. The workflow is opt-out; direct interaction with agents remains part of the product. Declarative profiles guide behavior and supported action boundaries will be enforced outside the agent's prose instructions.

Execution belongs on the server. Clients can disconnect, reconnect, and share sessions from different machines. The interface treats conversations as interactive documents, with keyboard navigation and contextual review. Efficiency controls remain explicit: compact and continue, reset context while retaining history, and archive/start a linked session. Cache and usage claims must distinguish measured state from estimates.

## Foundation boundaries

`relay-core` defines the domain, profile resolution/validation, fixture seed, and protocol. `relay-server` owns storage, authorization, mutations, and event publication. `relay-desktop` contains the Mosaic interface and a background network runtime. Reactive UI objects never cross worker threads; Mosaic's state channel delivers network updates to the UI thread.

The seed board and transcripts are immutable fixtures. Profiles and comments are real workspace data. Every fixture implementation session links to a provider-qualified issue reference. Issues can have multiple director and worker sessions. No subprocesses, LLM calls, remote board writes, merges, or deployment actions exist in this foundation. All execution is therefore unavailable regardless of a profile's configured permissions.

One server process owns one SQLite database. Its schema is versioned with `PRAGMA user_version`; schema versions newer than the server are rejected. The initial schema stores a serialized workspace snapshot plus durable command receipts. This keeps the first contract small; future growth can migrate entities to dedicated tables without changing their stable IDs. Every mutation and receipt is committed in one transaction before publishing the snapshot. Run one server per database; horizontal scaling is not supported.

## Protocol

All routes require `Authorization: Bearer <token>`, including WebSocket upgrade. The shared token defines a trusted workspace, not individual accounts or project roles. Request bodies are bounded to 64 KiB.

| Endpoint | Behavior |
| --- | --- |
| `GET /v1/snapshot` | Current authoritative snapshot, including monotonic revision |
| `POST /v1/commands` | Typed command envelope; returns committed snapshot |
| `GET /v1/events` | WebSocket: initial snapshot followed by committed snapshots |

Command envelopes carry `request_id` (UUID), `expected_revision`, and a tagged command. Supported commands update project defaults, create/update directors, and add comments. Stale revisions return HTTP 409. Invalid references/configuration return 422; invalid credentials return 401. Unexpected storage failures return a sanitized 500 response. Reusing an ID for a different command is rejected.

Event delivery uses a watch channel: intermediate snapshots may coalesce, but each event is complete state. Client revision checks prevent late responses from replacing newer snapshots. Reconnection reads authoritative state and resubscribes. Writes are not retried automatically. Durable receipts make deliberate retries idempotent.

Comments anchor to immutable message IDs, with an optional quote verified against the original body. Repeated text is identified by its message and quotation, not a unique text range. Fine-grained range anchors and a full transcript cursor are later work.

## Profiles

Project defaults are complete typed profiles. Directors hold optional overrides. Omitted values follow current project defaults; explicitly set values replace the whole field. Changing defaults validates all affected directors. New directors inherit the current defaults. A reusable complete TOML template can seed a new database via `RELAY_DEFAULT_PROFILE`.

Profiles configure Codex or Claude Code, whole-project or selected-issue scope, responsibilities, completion requirements, a worker limit from 0 to 64, and deny/ask/allow permissions for each named action. Completion requirements are a checklist, not a fixed terminal stage or ordered pipeline. Editing/importing changes a draft; saving validates and persists it. The UI displays effective values and their origins. The future harness layer must report which capabilities and boundaries it can reliably enforce.

## Next milestones

1. Connect an authoritative GitHub or GitLab board, preserving provider-specific board identity and semantics.
2. Integrate Codex and Claude Code with persistent server execution, capability reporting, issue-gated work, and enforceable director permissions.
3. Add identities, membership, review permissions, and shared-session handoff.
4. Implement full transcript cursor navigation, richer review anchors, and the three context lifecycle actions with actual harness support.
5. Add reliable usage/cache reporting, then validate macOS/Windows and consider the browser client.

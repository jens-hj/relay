# Orchestrator UI plan


Tracked in [#27](https://github.com/jens-hj/relay/issues/27). Workstreams: A [#28](https://github.com/jens-hj/relay/issues/28), B [#29](https://github.com/jens-hj/relay/issues/29), C [#30](https://github.com/jens-hj/relay/issues/30), D1 [#31](https://github.com/jens-hj/relay/issues/31), D2 [#32](https://github.com/jens-hj/relay/issues/32), E [#33](https://github.com/jens-hj/relay/issues/33).
## Context

The design pack (`relay-ui-handoff.zip`: guide, tokens.json, three HTML boards, screenshots, approval-forwarding diagram) defines a new design language for Relay:
- a dark warm-neutral ground, set in Geist Mono with Instrument Serif for named moments;
- status, model and tool colour families that are never mixed;
- square/cut-corner geometry, dither that means intensity, and micrographics that encode data;
- an agent tree of unbounded depth, where any agent that spawns becomes a manager.

It replaces the current Labelism look, which is implemented ad hoc inside `relay-desktop`: `theme.rs`, `styles.rs`, `labels.rs` and 92 `view!` blocks, nearly all coupled to `Model`.

Goals:
1. A new **`relay-ui`** crate: a pure, reusable Mosaic component library with no `relay-core` dependency, internal to Relay for now. It contains every component the design needs, plus new-language versions of the surfaces the pack doesn't draw: connection status, board, profile editor, settings, project pages, palette and notices.
2. Wire `relay-desktop` to it, replacing the Labelism views.
3. Extend the backend so the data is real (your choice: UI + backend). This covers:
   - sessions with parent sessions, spawned by agents through Relay MCP tools;
   - approvals that surface up the tree;
   - 5h/weekly usage windows;
   - compaction counts;
   - the 13 tool categories with micrographic data.

Decisions made:
- **Themes:** the design-pack tokens are the default; the token structure and mode switcher are kept.
- **Spawning:** through Relay MCP tools.
- **Permissions:** a parent **may widen** a child's permissions, but only with human approval. This answers the pack's open question.
- **Truthful readouts:** any field without measured data is hidden rather than faked. Estimates are labelled as estimates.

## Phase 0: setup (orchestrator)

- Create branch `orchestrator-ui`.
- Copy the pack into `design/orchestrator-ui/` (README, guide, tokens.json, boards, screenshots, diagrams). It becomes the in-repo spec workers read.
- Create the GitHub umbrella issue in `jens-hj/relay` with `gh`, plus one sub-issue per workstream below.
- Write `docs/orchestrator-ui-plan.md`, the accepted-plan summary (same role as `docs/labelism-plan.md`), and mark `docs/labelism-plan.md` as superseded.
- Update the `labelism-visual-direction` memory: it is superseded by this design, and the pastel/no-black rule no longer applies.

## Workstream A: `relay-ui` foundations (`crates/relay-ui`)

**Crate setup.** A workspace member that depends only on `mosaic` (the same pinned rev, with the dependency named `mosaic` so the macros resolve).
- It must build for native and wasm32.
- Public API is `relay_ui::prelude::*`, which re-exports components with their `*Props`/`*Handle` types (Mosaic component nodes are bare idents, so callers glob-import).
- Add `crates/relay-ui/src` to the `just fmt` / `fmt-check` mosaic-fmt paths.

**Tokens** (`tokens.rs`): a `scheme! UiTheme` with these groups:
- `ground` (page, panel, panel-raised, code, hairline, rule, rule-strong, ink, muted, faint)
- `status` (live, needs-you, failed, full-steam)
- `model` (sol, opus, sonnet, haiku)
- `tool` (13)
- `context` (5)
- `type` (sizes micro…display)
- `shape` (cut small/card/block, status-square, indent 14, fold depth 4)
- `motion`
- `ui-scale`

Themes:
- `theme_dark()` is canonical and uses the exact `tokens.json` values.
- `theme_light()` and `theme_dark_contrast()` / `theme_light_contrast()` are derived variants with the same roles. They keep the switcher meaningful, and every pair is contrast-tested.
- `px()` scale helper.
- A `thresholds` module: amber from 80, red from 95.

**Fonts** (`fonts.rs` and `assets/fonts/`):
- Bundle Geist Mono (400/500/600) and Instrument Serif (regular and italic) with OFL licence files, downloaded from the upstream releases.
- Provide `relay_ui::fonts::install(&mut FontContext)`.
- Monospace digits are already tabular, so no Mosaic font-feature support is needed.

**Primitives.** Each one maps to fields, per the guide's checklist:
- `StatusSquare`: working (breathing 2.4s), waiting (cut corner), failed (cross), queued (hollow), done (check).
- `HarnessGlyph`: Codex terminal, Claude Code braces, Copilot plane, Cursor pointer; square caps, mitred joins; as SVG icon tokens.
- `EffortTrack`: n growing squares, the current one framed, an optional max diamond, in the model colour, horizontal or vertical.
- `CutCorner` frame: an outer shape plus an inner panel inset 1px, built from `radius` + `exponent:1` per-corner chamfers.
- `Dither` / `Wash`:
  - CPU-generated thresholded fractal noise (`alpha = gradAlpha + 2·noise − 1`, discrete) as an `ImageData` mask over a single-colour fill.
  - Generated at physical size and cached per (size, seed, gradient).
  - Reach and opacity are props: row 0.3, header 0.14, approval 0.22, reach = 22% + 13% per effort level.
- `Breathe` driver: an `Element::animate` sine on opacity, for the live square (2.4s) and the model glow (3.6s).
- Cells and rules: `Cell`, `RuledRow`/`RuledCol` (one hairline between neighbours, never a gap plus a border), `CellButton` (fills its cell, with hover/pressed/focus), `PrimaryButton` (amber), `LinkButton`, `IconCell`, `Chip`, `Kbd`.
- Text:
  - `Label` (mono roles: micro, caption, label, body) and `SerifTitle`.
  - `Ellipsis`: single-line trimming with "…", measured through `FontContext`. Mosaic has no truncation; fall back to the existing fade-mask technique if measurement isn't available in a context.
  - `VerticalLabel` for the ledger spine. Mosaic can't rotate glyphs, so rasterise the text with cosmic-text/swash into `ImageData` and draw it with `Scene::image_rotated`.
- `Popover` (anchored overlay with click-outside and Escape dismissal) and `Tooltip` (non-hit-testing).
- `Gallery` example (`cargo run -p relay-ui --example gallery`): every component in every state, with the pack's example data. It is used for visual review against the boards.

## Workstream B: contract and harness data (`relay-core`, `relay-server`)

**`relay-core`**, protocol 5:
- `Session`: add `parent_session_id: Option<String>`, `spawn: Option<SpawnGrant { task, context, permissions: BTreeMap<Task, Permission>, requested_by_run }>` and `compactions: Vec<Compaction { at_tokens: Option<u64> }>`. All are `#[serde(default)]`.
- `ToolKind` becomes `Read, Edit, Create, Search, Run, Test, Fetch, Plan, Spawn, Message, Wait, Result, Other`. Serde aliases keep old snapshots loading: Terminal→Run, Web→Fetch, Agent→Spawn.
- `ToolCall`: add optional `started_ms`/`finished_ms` and `detail: Option<ToolDetail>`. The detail variants carry:
  - Read: range and total lines
  - Edit: hunks with +/−
  - Create: line count
  - Search: hits per file
  - Run/Test: exit code and pass/fail lists
  - Fetch: status and title
  - Plan: steps, done flags and owner session
  - Spawn/Message/Wait/Result: their references
- `Snapshot.usage_limits: Vec<UsageLimit { id, harness, window: FiveHour|Weekly, used_percent, window_minutes, resets_at, observed_at }>`.
- `Snapshot.usage_ledger`: weekly per-run records `{ id, session_id, harness, model, tokens, cached, completed_at }`, retained for 8 days. They feed model mix, heaviest agents and quick stats.
- `TaskWithChildren` helpers: tree/ancestor iteration and aggregate status counts. They are pure functions so desktop and tests share them.

**`relay-server`**, schema 12:
- Add an `if version < 12` block in `lib.rs` and bump the guards.
- Update the schema assertions (`tests.rs:1022`, `transport.rs:886`) and move the "newer DB rejected" test to 13.
- Update `docs/architecture.md` and `docs/development.md`.

Adapters:
- **Codex** (`app_server.rs`):
  - Count every `contextCompaction`.
  - Read `account/rateLimits/updated` into `usage_limits`; today this is dropped in `publish()`.
  - Classify `fileChange` add → Create.
  - Map `turn/plan/updated` → Plan.
  - Detect test commands → Test.
  - Record diff +/−.
- **Claude** (`claude.rs`):
  - Count each `compact_boundary`; today several collapse into one message.
  - Fill `context_tokens` from assistant message usage and `context_window` from `result.modelUsage`.
  - Read `rate_limit_event` into `usage_limits`.
  - Map Write-on-new-file → Create, TodoWrite → Plan.
  - Parse Read offset/limit and Grep hits.
  - Verify these event names against the installed harness versions first (`just doctor`). If an event is absent, leave the field unavailable and document it.
- Classify `mcp__relay__*` tools → Spawn/Message/Wait/Result.
- Extend the fake harnesses in `src/tests.rs` (`script()`, `CLAUDE_FAKE`) with these events, and add adapter tests.

## Workstream C: Relay MCP spawning (`relay-server`, after B)

**Transport.** A `relay-server mcp` stdio subcommand that speaks minimal MCP JSON-RPC: `initialize`, `tools/list`, `tools/call`.
- It is launched by each harness: Claude through `--mcp-config` in `claude::arguments`; Codex through `-c mcp_servers.relay.*` config overrides.
- It calls back to the server with a **per-run scoped capability**: a new random secret bound to `session_id` and `run_id`, revoked when the run ends.
- The workspace `RELAY_TOKEN` stays stripped from the harness environment.

**Tools:**
- `spawn_agent { title, task, context?, harness?, model?, effort?, permissions? }`:
  - Creates a child session with `parent_session_id`, the parent's `director_id` and issue.
  - The child gets a worktree forked from the parent's current state, reusing `commands::fork_workspaces`.
  - The first turn is the task.
  - The call respects `max_workers` across the director's whole tree, and the `Delegate` permission (Deny blocks, Ask asks the human).
- `message_agent { agent, text }`: delivers to a parent, child or sibling as a queued submission, and reports "read" once consumed.
- `wait_for_agents { agents, timeout_s }`: a bounded long-poll that returns statuses and results.
- `report_result { summary, attachments }`:
  - Child to parent. It records a Result message in the parent's transcript and stores it on the child.
  - It does not merge anything; the parent integrates by reading the child's worktree path.
- `list_agents`.

**Permissions.**
- The child's effective permissions are the parent's effective permissions, overridden by the grant.
- Any entry wider than the parent's creates a `ToolPermission`-style human approval ("Spawn widens permissions: …"). The spawn blocks until it is answered, and denial returns an error to the agent.
- Approvals need no duplication: each `ToolPermission` stays on its session. Ancestors, "Needs you" and manager transcripts are client-side projections of pending permissions in the subtree. One `RespondPermission` resolves the request everywhere.

**Lifecycle.**
- Stopping a session stops its descendants.
- Restart repair marks descendants Interrupted, as today.
- The worker prompt at `runtime.rs:1099` changes from "do not start other workers" to describing the Relay tools when `Delegate` ≠ Deny.

**Tests:** spawn, widening approval, deny, depth > 4, max_workers across the tree, message/wait/result round trip, capability revocation and stop cascade.

## Workstream D: `relay-ui` composites (after A)

All composites take plain view-model structs defined in `relay-ui`: `AgentNode`, `ToolCallView`, `UsageWindowView`, `ContextView`, `ModelIdentity { family, version, vendor, hue }`, and so on. They expose callbacks and never touch Relay types.

**Agent tree:**
- `AgentSidebar`: 60px serif "Agents" header plus count and a New cell.
- `StatusCounts`: five filter cells covering the whole tree, folded branches included.
- `NeedsYou` cards: cut corner, amber wash, path, Approve and Review.
- `AgentTreeRow`: expand, status, harness, name, child count, model and effort in the model colour; the wash reach equals effort; done rows soften; the selected row is highlighted.
- Collapsed managers show a `ChildStatusBar` and "n waiting".
- Square elbows, 14px per level.
- `FoldedBranch` past depth 4: "3 more levels · 6 agents" with a Focus link.
- `FocusCrumb`: back cell, path, serif name and depth chip.
- Rows render recursively over any depth.

**Usage:**
- `UsageFoot`: rows per harness with 5h/wk dithered bars and an elapsed tick.
- `UsageDetail`: forecast bars with the dashed projection, verdict, model mix, heaviest agents, quick stats, and a suggestion footer with Apply.
- Forecast math is a pure, unit-tested module: rate, projection, time to empty, verdict thresholds.

**Header and context:**
- `SessionHeader`: path plus serif title, live wash reaching to the effort level, `ContextGauge`, and a `ControlBlock` (Working with child squares and "n below", Pause P, Stop ⇧S).
- `ContextGauge`: a square ring with a gap, numeral, compaction squares (a number past 3), and ink → amber → red.
- `ContextPanel`: a greys breakdown shown only for parts the caller supplies, the compaction history, and a Compact now cell.
- `FingerprintStrip`: 4px segments; hover or focus raises a segment to 10px, dims the rest, and shows a label.

**Transcript:**
- `TranscriptColumn` with a 128px speaker gutter, `UserMessage` panel and agent text.
- `ActionGroup`: "4 actions · all done".
- `SpawnBlock`: tree rows plus "Open in tree".
- `ApprovalBlock`: amber cut corner with Approve / Reject / Review diff and what it blocks.
- `ThinkingLine`: violet wash.

**Composer:**
- `Composer` = `Ledger` + message field + actions column.
- `Ledger` has a vertical harness spine, a model cell (serif family, bold version, vendor caption, breathing model-colour glow), a collapse cell, and the effort label plus track with the max diamond.
- It collapses to a 66px strip.
- Send stays quiet until there is text.
- The message field accepts an injected editor element, so Relay's `BufferText` editor is preserved.

**Tool calls:**
- `ToolCallRow`: status, icon and verb in the tool colour, target, a micrographic, and three opening squares.
- Three openings: collapsed, summary, expanded.
- 13 micrographics and bodies: ruler, ± squares, new-file cut icon, hit columns, exit chip plus duration, test grid, status chip, plan n/m, spawn lockup, message read, wait squares plus time lanes, approval pending, result.
- A shared `CodeBlock` with line numbers, a diff with strike-through, and "n more lines".
- Working and waiting calls keep their wash and status border.

**Surfaces the pack doesn't draw** (built in the same language from the primitives):
- `ConnectionStatus`: a sidebar-foot cell with a ground-colour state glyph, red when disconnected, latency micro-bars, and a popover with endpoint, transport and protocol. Violet and amber are not used because they mean other things.
- `PageHeader` / `HeaderCells`
- `Notice` (retry/rebase)
- `CommandPalette`
- Form primitives: `Field`, `Segments` (replaces SlidingSegments), `Stepper`, `Toggle`, `Select`
- `Module` sections
- Board: `BoardColumn`, `IssueCard` (status square, identifier, title, labels), `IssueInspector`
- `PermissionMatrix` (allow/ask/deny)
- `HarnessCard`

## Workstream E: desktop wiring (after C and D)

- Add a `relay-desktop/src/adapt/` layer with pure functions from `relay-core` types to `relay-ui` view models:
  - the tree from `parent_session_id`. Legacy unparented sessions stay roots; no fabricated provenance.
  - status mapping and descendant aggregation
  - `ModelIdentity` from model id: claude-opus/sonnet/haiku → Opus/Sonnet/Haiku, gpt/codex → Sol hue; unknown models use the muted hue
  - effort track from `HarnessModel.efforts`, with the max diamond only when offered
  - fingerprint counts
  - usage forecast inputs
  - pending permissions in the subtree
- Rebuild the views on `relay-ui`. Model stays the state and command owner.
  - The sidebar keeps projects, boards and settings navigation as ruled header cells above the agent tree. The tree is scoped to the selected project.
  - Conversation becomes header, fingerprint, transcript (with `BufferText` preserved inside the composer) and ledger. Ledger edits issue `SetHarnessSelection`; Stop issues `StopWorker`; Pause holds the queue; Compact now issues `/compact`.
  - Board, profile, settings and project pages use the generic primitives.
- Delete `theme.rs`, `styles.rs`, `labels.rs` and the Reddit Sans / Zed Mono fonts once they are unused.
- Settings:
  - Keep the mode (System/Light/Dark) and high-contrast toggle.
  - Remove the four palette families.
  - Old settings files still load; obsolete fields are ignored and preserved, using the existing unknown-field suspension rules.
- Extend the demo fixture (`relay-core/src/fixture.rs`) with a deep tree, a pending approval, usage windows, compactions and every tool kind, so `just demo` shows every state.
- Update `src/tests.rs`:
  - Keep accessibility labels stable where the control still exists.
  - Change the Labelism geometry assertions (56/74/92/330px) to the new dimensions: 380px sidebar, 60px sidebar head, 88px session header, 128px gutter, 34px rows.
  - Replace the palette contrast test with one over the new themes.
  - Add tests for tree depth folding, focus re-rooting, Needs-you projection and ledger effort selection.

## Delegation (hapi-orchestrator)

**Workers.** Claude workers with the configured model, each in its own worktree, assigned to its sub-issue.
- Workers commit on their own worktree branches (local only).
- I review the diffs and merge them into `orchestrator-ui`.
- Nothing is pushed or turned into a PR without asking you.

**Waves:**
1. Wave 1, 2 workers in parallel: A, and B.
2. Wave 2, 3 workers in parallel: D split as D1 (tree, usage, header/context, fingerprint, non-designed surfaces) and D2 (tool calls, transcript, composer), plus C.
3. Wave 3, 1 worker: E. I do the integration review.

Every task file points to `design/orchestrator-ui/` and `docs/orchestrator-ui-plan.md`, and lists its acceptance criteria and validation commands.

## Verification

- Per worker, inside `nix develop`: `just lint`, `just test`, `just fmt-check`, plus `cargo build -p relay-ui --target wasm32-unknown-unknown` (wasm parity) and `just build-web`.
- `relay-ui`:
  - Run the gallery and screenshot it, then compare against `design/orchestrator-ui/screenshots/*` for tree, conversation and tool calls.
  - Unit tests: forecast math, thresholds, dither determinism, effort-track geometry, and fold depth.
- Backend:
  - Adapter tests with the extended fake harnesses, plus the spawn/approval/stop-cascade tests.
  - A real-boundary check: `just dev` with installed Claude Code and Codex, where an agent spawns a child through Relay MCP, the child asks for approval, it appears in Needs you and the manager transcript, and approving it unblocks the child. Confirm the rate-limit events are observed live.
  - A schema 11 → 12 migration test on a copy of `data/dogfood.sqlite3`.
- Desktop: `just test` (headless), `just demo` screenshots of every surface, and reconnect behaviour unchanged (network boundary).
- Final: `just check` on the merged branch. Report any harness events that turned out to be unavailable.

## Form colour

Form controls on the profile (New Director) page and in settings use the status family only, one meaning each; ground greys carry everything else. The mapping lives in `Segments` (`crates/relay-ui/src/surfaces/form.rs`).

| Colour | Meaning on forms | Where |
| --- | --- | --- |
| Green (`status.full-steam`) | Work runs without you: allowed or ok | Allow permission, Automatic approval, harness ready (installed and signed in), Connected, Valid profile fields |
| Amber (`status.needs-you`) | Your hand: you are asked, you must act, or you set it | Ask permission and approval, harness needs a sign-in or update, a failed (retryable) check or refresh, Override origin, the primary action (Save profile) |
| Red (`status.failed`) | Blocked, failed or a risk | Deny permission, Unrestricted Access approval, harness not installed, settings not being saved, Offline, Invalid profile fields |
| Ink / muted | Neutral choices, inherited values and pending checks | Theme choice, Default origin, unchosen options, Checking and Connecting (a static hollow square) |

Violet stays reserved for live work and never appears on forms or harness readouts. Status readouts colour their mark and word; the harness glyph and name stay ink. The chosen option of a choice sits on the selected ground with its text, icon and a 2px bar under it in its colour; the others are muted, so the choice also reads without colour.

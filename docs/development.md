# Development

Relay requires SSH access to the private Mosaic repository on GitLab. Both Cargo and Nix use `ssh://git@gitlab.com/unincorporated/mosaic/mosaic.git`, pinned to the same published commit. They use your normal SSH configuration and agent; Cargo is configured to fetch through Git's CLI. Verify access with `git ls-remote git@gitlab.com:unincorporated/mosaic/mosaic.git HEAD`. Credentials are never embedded in the manifests or lockfiles.

## Local startup

The project's [flake.nix](../flake.nix) declares Relay's development environment. Enter `nix develop`, or use `direnv allow` with the checked-in `.envrc` to load it automatically. The default shell supplies Rust 1.89 with rustfmt, Clippy, rust-analyzer and Rust sources; Bash, just, Git, GitHub CLI, GitLab CLI, Codex CLI, Claude Code, pkg-config, curl, jq, OpenSSL and SQLite; and Linux graphics/windowing libraries. `nix develop .#server` provides the server environment without desktop libraries. Inputs are pinned in `flake.lock`, with shared dependency pins following Mosaic.

Codex is pinned separately to the official 0.161.0 release in [nix/codex.nix](../nix/codex.nix), including its bundled sandbox resources. Release archives are hash-verified for Linux and macOS on x86_64 and ARM64. Claude Code is supplied by the locked nixpkgs package; Linux shells also include bubblewrap and socat for its native sandbox. Only the Claude Code package is allowed as an unfree dependency. Relay prefers executable installations in `~/.local/bin`, `~/.npm-global/bin`, and `~/.nix-profile/bin`, then PATH, so the Nix shell does not hide your existing harnesses. `RELAY_CODEX_BIN`, `RELAY_CLAUDE_BIN`, or Settings → Harnesses can override the executable.

`just doctor` checks each harness independently. Settings → Harnesses reports status from the server machine, including authentication without exposing credentials. Check again refreshes read-only probes; no model call is made. Installed/authenticated does not prove model access or account support for every execution mode. Relay retains each harness’s configured model/provider and login.

## Work on Relay through Relay

Authenticate on the server machine using `gh auth login`, then `codex login` and/or `claude auth login`. GitHub board reads require `read:project` access in addition to repository access; if an existing CLI login lacks that scope, use `gh auth refresh --scopes read:project`. Existing harness authentication and model/provider configuration are reused; Relay never copies credentials into its database. See [GitHub's Projects API authentication](https://docs.github.com/en/issues/planning-and-tracking-with-projects/automating-your-project/using-the-api-to-manage-projects#authentication).

```sh
nix develop
just doctor
just dogfood
```

Keep this server terminal running. In a second terminal, enter the same project's dev environment and run `just client`. `just dogfood` creates a random shared token in a Git-ignored, owner-readable `.env` when one does not exist; existing configuration is preserved. It connects `jens-hj/relay` to [GitHub Project 5](https://github.com/users/jens-hj/projects/5), uses the current repository as the server-owned source checkout, and defaults to `data/dogfood.sqlite3`. Supply another board with `just dogfood owner/repo owner NUMBER`; set `RELAY_REPO_PATH` to its matching local Git checkout.

Choose the live project, sync its board, select an issue, and start a worker under a director. The bundled implementation permission is Allow. Customized project defaults and explicit director overrides are preserved; existing unchanged bundled Ask defaults migrate to Allow. Ask workflow permission still requires approval when prompted. Scope, harness, implementation permission, and active-worker limits are checked by the server. Choose Codex or Claude Code and an explicit model in project defaults or the agent profile. Model choices come from the selected harness. New agents require a model; workers inherit it, and changing harness clears the previous model choice. Existing profiles without a model remain readable but must select one before creating a new agent. A worker remains bound to its original harness when its director changes.

Closing a desktop leaves the server and worker running. Reopen `just client` to reconnect, or use another machine with the same token and an SSH tunnel. Stopping the server interrupts active runs; they remain in history and must be continued explicitly after restart. `just dev` saves its token in `.env` and workspace in `data/local.sqlite3`, with first-run New Project setup in the main pane. Saved projects survive restart; no environment board configuration is needed afterward. It stops its server when its desktop closes. Use separate server/client processes for workers that should continue after closing the client. `just demo` uses an isolated temporary fixture database seeded with an example orchestration — an agent tree more than four levels deep, an approval pending two levels down, one call of every tool kind and recorded compactions — and example usage windows. Live workspaces never receive this example data.

Worker turns run in isolated Git worktrees. Execution settings inherit from project defaults through director overrides to optional worker overrides (conversation menu). Automatic is the default: Codex uses workspace-write with approvalPolicy never; Claude uses native auto mode with its sandbox required. Ask uses Codex on-request or Claude default mode and displays supported native tool requests inline with Allow once/Deny. Unrestricted Access explicitly selects Codex danger-full-access or Claude bypassPermissions. Managed harness policies and account/version support still apply. These modes configure the harness boundary; responsibilities/completion and Merge/Deploy permissions remain workflow intent. Changing a running worker’s mode affects its next turn. Use Stop to interrupt current execution. Session details show the thread, branch, base commit, worktree, latest measured token usage, and a bounded diff including untracked files. A completed turn still needs review against its source issue. Review changes and validation evidence before integrating; Relay does not merge, push, deploy, or change GitHub board status automatically. Cached input token counts describe the completed turn; they do not predict cache expiry or the cost of the next message. Open Commands beside Send or type `/` for command completion. The composer shows loading and discovery errors, with Retry when discovery fails. `/model` chooses a session model followed by effort or fast mode; changes apply to the next turn. `/plan` controls native planning separately from execution permissions. Type `$` to discover and select skills in either harness. `/compact` uses native compaction; `/clear` retains the old conversation, `/resume` selects retained history, and `/fork` creates a sibling session with independent repository worktrees including staged, unstaged, and untracked files. Ordinary directory connections remain shared and active turns cannot use the same shared workspace concurrently. Commands that require unsupported terminal controls show a reason. See [command validation](commands-validation.md).

Use the [worker review checklist](review-checklist.md) before integrating a completed turn.

Mosaic tools are available separately through `nix run .#mosaic-fmt` and `nix run .#mosaic-cli`, so entering the shell does not build editor or packaging tools. Use `nix fmt` to format the flake.

For native frame-rate diagnostics, enable Mosaic's CPU renderer timings before starting the desktop. This command also works in fish:

```fish
env MOSAIC_RENDER_TIMINGS=1 RUST_LOG=mosaic_runtime::hud=info relay-desktop
```

The additional `perf: renderer` line splits flattening, surface acquisition, uploads, compositing planning, command encoding, queue submission, asynchronous readback scheduling, and surface presentation. `redraw_wait` measures the earliest coalesced redraw request to event dispatch; missing requests print `n/a`. Samples cover completed frames over a one-second window. The original summary's `present=` still measures the whole renderer call. GPU timings are asynchronous and must not be added to CPU totals. Profiling is disabled by default.

Capture ten seconds of continuous hovering or animation with the HUD hidden at fullscreen and small-window sizes. Repeat without `MOSAIC_RENDER_TIMINGS`, keeping the workspace and display scale fixed, to check profiling overhead. Share both the original summary and `perf: renderer` lines when investigating frame pacing.

For Wayland/Niri comparisons, Mosaic also exposes presentation controls and per-pass GPU diagnostics:

```fish
env MOSAIC_RENDER_TIMINGS=1 MOSAIC_GPU_PASS_TIMINGS=1 MOSAIC_HUD=0 RUST_LOG=mosaic_runtime=info,mosaic_render_wgpu=info relay-desktop
```

Startup logs identify the actual window backend, GPU, physical dimensions, supported presentation modes, and selected settings. The default is `MOSAIC_PRESENT_MODE=fifo MOSAIC_FRAME_LATENCY=2`, with Winit notified immediately before each presentation. Compare against `MOSAIC_PRE_PRESENT_NOTIFY=0`, then FIFO with `MOSAIC_FRAME_LATENCY=1`, and `MOSAIC_PRESENT_MODE=mailbox` if supported. Test `immediate` separately because it can tear. Invalid or unsupported settings warn and fall back to defaults.

Record the active refresh rate and scale with `niri msg outputs`. Keep the scene, activity, and window dimensions fixed; warm up for ten seconds, then alternate baseline and experiment for three 20-second captures each. Repeat the winner without detailed profiling. GPU pass samples identify an earlier encoded frame and report dimensions, actual draws, and freshness. A `composite-and-content` pass may also draw ordinary scene content, so its time is not solely composite-shader cost. Preserve transparent-group and backdrop-filter behavior when choosing further optimizations. See [issue #24](https://github.com/jens-hj/relay/issues/24).

`nix develop .#ui-test` also supplies Linux native-window verification tools: Xvfb, xdotool, ImageMagick, xclip, and Mesa. These stay out of the default shell. The desktop still needs a display; use Xvfb and Mesa software Vulkan when testing without a physical screen.

For visual review without a display, render offscreen with Mesa: `nix develop .#ui-test --command cargo test -p relay-desktop shell_screenshots -- --ignored` writes the shell over the demo orchestration (conversation, manager, board, settings, profile, connections, command palette) to `target/relay-desktop-shots/`; set `RELAY_SHOT` to render one. The component gallery renders the same way from `relay-ui` (see `crates/relay-ui/src/tests.rs`). Compare both against `design/orchestrator-ui/screenshots/`.

```sh
nix develop --command just dev
```

`just dev` builds the workspace, creates `.env` with a random token if neither configuration nor `RELAY_TOKEN` exists, starts a loopback server, waits for it to be ready, and opens the desktop with the same token. Closing the desktop or pressing Ctrl+C stops the processes it started. It leaves the database intact. Use `just dev 7440` to choose another local port, or `just dev --release` to build and run release binaries; both options can be combined as `just dev --release 7440`. This recipe sets `RELAY_BIND` and `RELAY_ENDPOINT` for the local pair; use `just server` and `just client` for separate or remote processes.

For separate terminals, enter the development shell in each and supply the same token:

```sh
# First terminal:
export RELAY_TOKEN="$(openssl rand -hex 32)"
just server

# Second terminal, using the first terminal's token:
export RELAY_TOKEN="<same token>"
just client
```

The default server address is `127.0.0.1:7331`. Both processes require the token. Tokens must contain at least 16 printable ASCII characters with no spaces; use a randomly generated token. The token grants access to the entire workspace. Comment author names are display labels, not authenticated identities.

| Variable | Process | Default / behavior |
| --- | --- | --- |
| `RELAY_TOKEN` | Both | Required; never logged |
| `RELAY_BIND` | Server | `127.0.0.1:7331` |
| `RELAY_DATABASE` | Server | `data/relay.sqlite3` |
| `RELAY_DEFAULT_PROFILE` | Server | Optional path to a complete TOML profile; used to seed a new database only |
| `RELAY_GITHUB_REPO` | Server | Live repository, `owner/repo`; optional bootstrap; saved bindings remain usable |
| `RELAY_GITHUB_PROJECT_OWNER` | Server | GitHub user or organization that owns the board |
| `RELAY_GITHUB_PROJECT_NUMBER` | Server | Projects v2 number from the board URL |
| `RELAY_REPO_PATH` | Server | Legacy bootstrap checkout on the server; New Project manages named roots and connections |
| `RELAY_CODEX_BIN` / `RELAY_CLAUDE_BIN` | Server | Optional executable overrides; saved Harnesses configuration takes precedence |
| `RELAY_SHARED_BUILD_CACHE` | Server | Agents share `ROOT/.relay/cache/cargo-target` as `CARGO_TARGET_DIR`; `0` gives each worktree its own `target/`. An existing `CARGO_TARGET_DIR` takes precedence |
| `RELAY_BACKGROUND_TASK_LIMIT_SECS` | Server | Longest wait, in seconds, for Claude Code background tasks after a turn's result; default 3600 |
| `RELAY_WORKER_SCOPE` | Server | `auto` runs each harness in a memory-limited systemd user scope when one can be created; `off` launches harnesses directly |
| `RELAY_WORKER_MEMORY_HIGH` / `RELAY_WORKER_MEMORY_MAX` | Server | Per-worker scope `MemoryHigh` (throttle) and `MemoryMax` (OOM kill) limits; bytes with `K`/`M`/`G`/`T`, a percentage of RAM, or `infinity`. Defaults `30%` and `40%` |
| `RELAY_SYSTEMD_RUN_BIN` | Server | Optional `systemd-run` executable override |
| `RELAY_ENDPOINT` | Client | `http://127.0.0.1:7331/` |
| `RELAY_THEME` | Client | Optional startup override: `dark`, `light`, or `system`; defaults to saved settings, initially dark |
| `RELAY_SETTINGS_PATH` | Client | Optional settings file; otherwise the platform's local configuration directory, `relay/settings.toml` |

Keep tokens in your shell/session environment or an external secret manager. `just` also loads an optional `.env` file, allowing separate terminals to share your local configuration. `.env` and database files are ignored by Git; the binaries do not load `.env` themselves. The bundled reusable template is [profiles/default.toml](../profiles/default.toml); set `RELAY_DEFAULT_PROFILE` to your own complete template when starting a new workspace.

For a remote native client, transfer the administrator credentials only as a private file over an authenticated secure file-transfer channel. Never paste tokens into chat, command examples, issue text, or logs. Save the supplied `relay-desktop.env` outside the repository, for example at `~/.config/relay/relay-desktop.env`, with directory permissions `0700` and file permissions `0600`. The file contains `RELAY_ENDPOINT` and `RELAY_TOKEN`; keep its contents private.

From the Relay checkout, this command loads the supplied file inside Bash and runs the release desktop with performance diagnostics. It also works when the calling shell is fish:

```sh
nix develop --command env MOSAIC_RENDER_TIMINGS=1 MOSAIC_GPU_PASS_TIMINGS=1 MOSAIC_HUD=0 RUST_LOG=mosaic_runtime=info,mosaic_render_wgpu=info bash -c 'set +x; set -a; source "$1" || exit; set +a; exec cargo run --locked --release -p relay-desktop' _ ~/.config/relay/relay-desktop.env
```

After a credential exposure, rotate the server's administrator token, restart the service to load it, and verify that the old credential returns `401` while the replacement authenticates. Existing clients must load the replacement. Do not print either token during verification. See [issue #25](https://github.com/jens-hj/relay/issues/25).

The 380px sidebar starts with ruled cells for the selected project: its name opens the project menu (every project, the project's directors with their profiles, new director, project defaults, connections and **New Project**), then the board and **Settings**. Below them is the agent tree of the selected project. Sessions started by another agent sit under the session that spawned them, at any depth; sessions without a recorded parent, including those from earlier versions, are roots. Each row shows its status square, harness, name, child count, model and effort. Collapsing a row replaces its model with a status bar of everything below it and "n waiting" when an approval is pending there. Branches deeper than four levels fold into one line with **Focus**, which re-roots the tree on that branch with a breadcrumb back. The status counts filter the tree. **Needs you** lists every pending permission request in the project, however deep, with its path; **Approve** answers it, and the same request disappears from every transcript that shows it. Arrow keys move through visible rows; Right expands and Left collapses or returns to the parent.

The sidebar foot shows harness usage windows when a harness has reported them (5-hour and weekly bars with the elapsed tick; open a row for the forecast, model mix and heaviest sessions from the last week), then the server connection. The connection cell shows the state and latency; open it for the endpoint, transport, protocol and the running **Frontend** and **Server** build commits. Production Nix packages embed their source commit at build time; local Cargo builds append `-local`. The authenticated `GET /v1/version` endpoint reports the running server build, without storing build metadata in the database. Unknown server versions are shown explicitly. This makes it possible to identify a browser tab still running an older frontend after a deployment.

A conversation opens on its session header: the path of the agents above it, the session name, the context gauge when the harness reported both occupancy and window (open it for the compaction history and **Compact now**, which sends `/compact`), the session's state with the agents below it, and **Stop** while it runs. A strip under the header splits the loaded transcript's tool calls by type. Tool calls open in three steps (collapsed, summary, expanded); failed calls start at their summary. A spawn appears as the spawned agent's tree row with **Open in tree**. Pending approvals from the session and every agent below it appear as approval blocks. The composer's ledger shows the harness, the model (reported, else selected, else the harness default) and, when the session's catalog describes the model, its effort steps; choosing a step updates the session's harness selection, and the model cell opens the model picker.

Open **Settings** with the gear in the sidebar head, the command palette, or **Ctrl+,** (**⌘+,** on macOS). Choose Dark, Light, or System appearance, and turn **High contrast** on or off for both modes. Dark is the initial default. Display settings are client-local and survive restarts; they do not change the server workspace.

The interface follows the orchestrator design in `design/orchestrator-ui/`, built from the `relay-ui` crate: a dark warm-neutral ground, square and cut-corner geometry, and three colour families that are never mixed — status (violet live, amber needs you, red failed), models and tool types. Every colour pair used for text is checked to reach at least 4.5:1 contrast in all four themes. Settings files written by versions with palette families still load; their family choices are kept in the file for older clients, and either high-contrast palette turns on high contrast.

If the settings file cannot be read or was written by a newer Relay, Relay does not overwrite it. Display changes apply to the current session only, and Settings shows the reason with two actions: **Retry reading** adopts the file once it is valid, and **Back up and save** keeps a byte-for-byte copy next to it (`settings.toml.unreadable-<UTC time>`, never replacing an existing backup) before saving current settings. If either step fails, the original file stays in place and saving remains paused.

Mosaic automatically follows the display scale reported by the operating system, including monitor changes. Interface scale adjusts text, spacing, and controls on top of that scale; 100% follows the operating system. Use a larger setting if your 4K display reports 100% scaling. Pixel resolution alone does not identify physical display DPI. Windows too narrow for the sidebar and a reading column move navigation into a drawer.

Geist Mono for text and Instrument Serif for titles are bundled with `relay-ui` under SIL OFL 1.1. No local font installation or download is required. Font provenance and licensing are documented in [the font assets](../crates/relay-ui/assets/fonts/README.md).

## Remote server

Run the server on the remote machine with its database on persistent storage. Leave the default loopback bind and create an SSH tunnel from each client machine:

```sh
ssh -N -L 7331:127.0.0.1:7331 your-server
```

Use the server token and the default client endpoint locally. Alternatively, place the server behind an existing TLS reverse proxy and set `RELAY_ENDPOINT=https://relay.example.com/`. Forward HTTP and WebSocket traffic for `/v1/*`, including the Authorization header. HTTPS endpoints use WSS for events. Relay does not terminate TLS itself. Use SSH or TLS to protect the bearer token in transit.

## Worker memory

Agent builds can use more memory than the machine has; a single Rust link or test binary can take 1.5–2 GB. On Linux, when `systemd-run --user --scope` works at startup, the server launches each harness through it in its own transient scope with `OOMPolicy=continue`, `MemoryHigh` and `MemoryMax`. The kernel throttles a worker above `MemoryHigh` and, above `MemoryMax`, OOM-kills a process inside that worker's scope only; usually the build, leaving the harness to report the failed command. When the server runs as a user service, each scope is bound to that service (`BindsTo=`), so stopping or restarting the server still stops every worker. `systemd-run` execs the harness in place, so process-group termination, the parent-death signal and lowered CPU priority are unchanged. Startup falls back to direct launch, without limits, when there is no reachable user manager or `systemd-run` predates `--expand-environment` (systemd 254): development shells without a session, containers, macOS, and the NixOS system service.

Harnesses raise their `oom_score_adj` to 500 so the kernel prefers them and their builds over the server. The Nix services set `OOMPolicy=continue`, so an OOM kill inside the service no longer stops the server and every worker, and lower the server's own score (`OOMScoreAdjust=100` for the user service, the lowest a user manager may assign; `-100` for the system service). A harness killed with SIGKILL during a turn fails that turn with "Worker build ran out of memory"; when its scope reports no OOM kills, the turn reports a plain SIGKILL instead.

The percentage limits are per worker, not a total, and Relay does not cap concurrent builds. On a 15 GB machine without swap, keep `MemoryMax` at or below about 6 GB and ask workers to build with `CARGO_BUILD_JOBS=2` and test only the crates they touch. Nix deployments set the limits with `services.relay.workerMemory.{scope,high,max}`; other deployments use the environment variables above.

## Foundation walkthrough

1. Open an issue card and follow its linked sessions. Demo issue URLs identify fixtures, not real GitHub work.
2. Open an agent’s profile from its conversation actions. Change project defaults, save, then inspect the project director's inherited values and review director's overrides.
3. Use the Agents header + to open New Director, or a row’s hover/focus + to open New Worker under that agent. Select the profile from the New Agent title. Choose a harness, model, scope, responsibilities, completion steps, worker limit, and permissions. `Inherit` removes a field override. Changed profile settings offer icon actions with tooltips: apply overrides to this agent, save a named profile, or discard changes.
4. Use `Export / edit TOML` to copy a declarative profile. Agents export overrides; omitted fields inherit. `Show effective profile` displays the full resolved profile. Import updates the draft; Save persists it.
5. Open a conversation. Click/select a recorded passage and type to create an anchored reply. Add more replies anywhere in the transcript; they share one next-message draft. Edit each reply beside its source or at the bottom. Paste text, copied files, or images at the cursor, including within replies. Recorded text stays unchanged.
6. Enter inserts a newline. Ctrl/Cmd+Enter sends the whole draft after it is saved. While an agent runs, this queues the message. Press again with an empty next draft to interrupt the current turn and send the just-queued message; there is no timing window. Typing another draft starts another message. Queue actions also offer Edit, Cancel, Send now, and explicit Resume queue after interruption. The session header's **Pause** cell (P outside text fields) holds queued turns after the current one; it then reads **Resume**.
7. Open the compact header menu for the linked issue, director profile, Changes, Details (usage/provenance), or Find. Tool calls appear inline with distinct terminal, file read, file change, search, web, agent, and fallback tool presentations. Their headers keep the target and a status glyph plus word visible when collapsed. Terminal output streams into a bounded viewport with its working directory and exit code; reads have numbered lines, searches use a ruled result list, and changes show additions/removals. Input and full recorded output remain available through disclosures; recorded output supports selection and anchored replies. Calls awaiting a matching native approval show Waiting for approval. Calls without a terminal result become Interrupted when their turn ends or the server restarts; older recorded activity shows Status unavailable when no lifecycle metadata was saved. Source context expands inline. Stop appears while the agent is active. Demo director/worker transcripts support drafting, but are fixtures and cannot execute turns.
8. Open a second client with the same token. Saved changes appear in both clients. Restart either client or the server; saved profiles and comments remain.

Cmd/Ctrl+K opens the command palette. Cmd/Ctrl+F opens literal transcript search with highlighted matches and previous/next navigation. Escape closes the palette/search. Tab and Shift+Tab traverse controls. Ctrl/Cmd+Home/End jumps to the conversation start or next message. Arrow keys cross text surfaces; Shift+arrows extend a draft selection across sibling text and inline files. Backspace/Delete at part boundaries join text or remove an inline object. Ctrl/Cmd+Z and redo restore shared draft edits and cursor targets.

Drafts autosave independently of transcript updates. Disconnected edits remain local; sending requires reconnection. Concurrent edits retain the local version and show Load shared / Restore local choices. Exact save retries keep the same request ID. Pasted files retain their IDs and bytes in local recovery until acknowledged. A restarted client never sends recovered work automatically. Use separate server/client terminals to verify reconnect and shared drafts; use Ctrl/Cmd+Enter on a live worker to test execution.

Conflicting saves do not overwrite newer server state. `Review latest state` retains your draft, refreshes its revision and inherited defaults, and lets you review before saving again. Network errors do not automatically retry writes. Retrying the same command reuses its request ID; the server prevents duplicate application, including after restart.

## Checks

```sh
nix develop --command just check
```

`just` lists available recipes. Individual commands include `just build`, `just test`, `just lint`, `just fmt`, `just fmt-check`, and `just nix-check`.

Tests cover server persistence, idempotency, authentication, profile inheritance, validation, conflicts, event synchronization, and headless UI interactions. A native Linux window is also required for graphics, windowing, and clipboard verification. macOS and Windows verification remains separate from Linux validation.

For a repeatable native GPU measurement of the mounted demo UI at large and small viewports, run:

```sh
nix develop --command cargo test --release -p relay-desktop native_fixture_gpu_cost -- --ignored --nocapture
```

The probe uses physical sizes 3771×2141 and 1123×848 with logical layout at scale 1.25 and embedded fonts. It compares clear, solid rectangle, and mounted synthetic Relay content after warmup, including fixture variants with glyph or shape commands removed. These variants retain clip/layer structure but may shrink isolation bounds; compare raster statistics and do not interpret duration differences as additive pipeline costs. Depth 1 waits after each submission and reports available GPU median/p95; depth 3 measures throughput with bounded submissions in flight. GPU timestamps are optional. These offscreen measurements exclude window presentation. See [issue #24](https://github.com/jens-hj/relay/issues/24).

For shape attribution, set `RELAY_GPU_PROBE_WORKLOAD=fixture` and run separately
with `MOSAIC_SHAPE_PROBE=all`, `solid-box-only`, `general-only`, and `none`.
These renderer selections omit draws after planning, preserving attachment sizes
and other commands; they differ from removing shape commands from the scene.
General instances include circular boxes that do not qualify for the square-box
path. The first multi-instance upload reports geometry and classification, using
stderr when no logger is installed. Bounds are potential coverage, not measured
fragment counts. The `circular_candidate` field records eligibility for the
rounded-box experiment; it does not select a pipeline. Evaluate shape changes
in three alternating release runs at each size, using 64 warmup frames and
192 measured frames with no builds or other tests running concurrently. Require
at least 10% improvement for the affected shape workload and no repeatable
full-fixture GPU regression over 5%. Keep throughput runs with
`MOSAIC_GPU_TIMINGS=0` separate from GPU timestamp runs. Local results do not
establish live Relay or LG/Niri performance.

The 2026-10-11 local Intel ADL-N/Mesa 26.1.3 comparison rejected the rounded-box
prototype: neither fixture contained a visible eligible instance, the small
general-shape workload did not improve, and full-fixture GPU medians remained
about 10.65 ms large and 2.515 ms small across three alternating release runs.
The speculative shader was removed; post-planning attribution probes and
regression coverage remain. These results do not establish UHD 770 performance.

For native presentation comparisons, launch synthetic continuous workloads without a server or credential file:

```sh
nix develop --command env RELAY_RENDER_PROBE=fixture MOSAIC_HUD=0 \
  MOSAIC_PRE_PRESENT_NOTIFY=0 MOSAIC_PRESENT_MODE=mailbox \
  MOSAIC_RENDER_TIMINGS=1 MOSAIC_GPU_TIMINGS=0 \
  RUST_LOG=mosaic_runtime=info,mosaic_render_wgpu=info \
  cargo run --locked --release -p relay-desktop
```

Repeat with `RELAY_RENDER_PROBE=clear` and `solid`, at both physical window sizes. The probe uses synthetic data and does not read settings or start networking. With the accompanying Mosaic diagnostics revision, `MOSAIC_GPU_TIMINGS=0` disables timestamp queries and dominates `MOSAIC_GPU_PASS_TIMINGS=1`; earlier Mosaic releases ignore this variable. Check the effective startup log. Enable `MOSAIC_FRAME_TRACE=1` for short request/dispatch/presentation traces, separately from throughput measurements.

On the LG display at 240.084 Hz, keep scale 1.25, output configuration and workload fixed. Warm up for 10 seconds and alternate three 20-second baseline/experiment runs. Compare notification on/off first, then FIFO/mailbox with latency 2. The existing latency 3 runs showed no clear gain. Record physical dimensions, adapter, effective settings, FPS and p95/p99. A mixed `composite-and-content` pass includes ordinary UI draws; its duration does not isolate compositing. CPU and GPU timing overlap and must not be added. Desktop validation precedes a Mosaic release and coordinated Relay Cargo/Nix pin update.

The diagnostics were checked locally on Intel ADL-N with Mesa 26.1.3/Vulkan. At 3771×2141, the synthetic fixture's serialized median GPU time was 10.72 ms, compared with 0.89 ms after removing shape commands while retaining the same attachment-area statistics. Removing glyphs left 9.82 ms, although that variant collapsed from three passes to one. Clear-only measured 0.05 ms. Bounded fixture throughput was about 91 FPS with GPU queries enabled and disabled. These results identify shape rendering as the main local fixture cost; they exclude presentation, use different hardware/content from the UHD 770 live-client capture, and do not establish target-desktop FPS or a per-pipeline additive breakdown.

The foundation was verified on Linux with Xvfb and Mesa software Vulkan: light/dark rendering, 820/1380-pixel layouts, keyboard navigation, clipboard copying, quoted comment posting, two-client synchronization, and persistence across a server restart. The live GitHub/Codex milestone adds real issue-linked worktree execution, client reconnect, exact-thread continuation, stop, and crash recovery. See the [dogfood validation record](dogfood-validation.md) for evidence and limits. See [local harness validation](harness-validation.md) for Claude Code, execution policies, and persistent local startup. Real desktop window decorations and macOS/Windows behavior still need validation on those environments.

## Named projects and local work

Choose **New Project** in the sidebar's project menu. Supply a name and an absolute root on the connected server; with `just dev`, this is your local machine. Initial repository, board and directory connections are optional. The project appears alongside existing projects with a local Backlog / In Progress / Done board and default director. Create a task or open the director to start planning without GitHub/GitLab credentials.

Open **Connections** from the project's menu or command palette. Repository shorthand such as `owner/repo` uses GitHub SSH; full SSH/HTTPS URLs also work. Relay clones beneath the project root and displays progress or a retryable error. Ordinary directory connections permit direct editing. Repositories get isolated session worktrees and separate reviews. Select fewer workspaces when starting a session if needed; resumed sessions keep their original selection.

The board selector keeps boards separate and remembers the selected board per project. **Publish board** supports a new or existing GitHub Projects/GitLab board, repository assignment per task, and named column mapping. Existing `gh`/`glab` authentication on the server is used. New GitLab boards offer Open/Closed mappings; use an existing label board for additional statuses. Removed connections retain their identities and can be restored from Connections. Verify the publication preview before submitting; interrupted writes may require checking the created issue/board remotely. The local board stays recoverable until the remote readback succeeds. Afterward, user task edits and moves update the remote provider; agent completion never changes board status automatically.

Settings use ruled segments for Dark / Light / System and a switch for high contrast. Interface scale uses a stepper from 80% to 200%, in 10% increments, on top of operating-system scaling. Harness status squares come with a word and distinguish ready, needs-sign-in, failed and pending checks; detailed diagnostics and executable configuration are under Details.

See [project workflows validation](projects-validation.md) for the named-project, publication, native UI and multi-workspace checks.

Tool-call presentation and persisted lifecycle metadata are tracked in [issue #21](https://github.com/jens-hj/relay/issues/21). Database schema 9 introduced persisted tool metadata; schema 10 also retains session commands and conversation history, preserving old transcript bodies while rejecting older servers that would discard new state.


External-network transport is tracked in [issue #26](https://github.com/jens-hj/relay/issues/26). Protocol 6/schema 13 require coordinated server, browser, and native-client updates. Command responses are small durable acknowledgements; clients obtain metadata and bounded history through revisioned WebSocket projections. Conversation history loads automatically near the top, retains scroll position, and supports search into unloaded history. Large messages are explicitly previews until loaded; Changes fetches review diffs on demand. The complete authenticated `/v1/snapshot` remains available for diagnostics and is not used by normal client traffic. Browser latency probes use `/v1/ping`, independently of transcript streaming.

Validation includes `cargo test -p relay-server transport::tests`, headless native paging/acknowledgement tests, `cargo check --target wasm32-unknown-unknown -p relay-desktop`, and `just check`, all inside this flake’s development environment. The transport test serves an archive exceeding 3.35 MB through a TCP proxy with 1 Mbit/s downstream bandwidth and 150 ms initial round-trip delay; it checks acknowledgements below 1 KiB and one second, bounded initial history, and exact retry without duplicate mutation.

See [deployment](deployment.md) for independent activation, rollback safeguards, and resumable public verification.

An isolated Chromium/WebGPU run with a 3,743,289-byte archive observed a 4,724-byte metadata reset, a 190,168-byte initial history update, and a 76-byte command acknowledgement received in 201 ms. No full-snapshot client requests were observed, and the independent ping route was exercised. Chromium’s software adapter hit the pinned Mosaic GPU timer’s “Buffer is already mapped” panic; the transport run therefore masked the optional `timestamp-query` adapter feature. This is a graphics validation limitation, not an acceptance claim for ordinary browser GPU timing. No dependency pin was changed.

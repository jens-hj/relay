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

Choose the live project, sync its board, select an issue, and start a worker under a director. The bundled implementation permission is Allow. Customized project defaults and explicit director overrides are preserved; existing unchanged bundled Ask defaults migrate to Allow. Ask workflow permission still requires approval when prompted. Scope, harness, implementation permission, and active-worker limits are checked by the server. Choose Codex or Claude Code in project defaults or the director profile. A worker remains bound to its original harness when its director changes.

Closing a desktop leaves the server and worker running. Reopen `just client` to reconnect, or use another machine with the same token and an SSH tunnel. Stopping the server interrupts active runs; they remain in history and must be continued explicitly after restart. `just dev` saves its token in `.env` and workspace in `data/local.sqlite3`, with first-run New Project setup in the main pane. Saved projects survive restart; no environment board configuration is needed afterward. It stops its server when its desktop closes. Use separate server/client processes for workers that should continue after closing the client. `just demo` uses an isolated temporary fixture database.

Worker turns run in isolated Git worktrees. Execution settings inherit from project defaults through director overrides to optional worker overrides (conversation menu). Automatic is the default: Codex uses workspace-write with approvalPolicy never; Claude uses native auto mode with its sandbox required. Ask uses Codex on-request or Claude default mode and displays supported native tool requests inline with Allow once/Deny. Unrestricted Access explicitly selects Codex danger-full-access or Claude bypassPermissions. Managed harness policies and account/version support still apply. These modes configure the harness boundary; responsibilities/completion and Merge/Deploy permissions remain workflow intent. Changing a running worker’s mode affects its next turn. Use Stop to interrupt current execution. Session details show the thread, branch, base commit, worktree, latest measured token usage, and a bounded diff including untracked files. A completed turn still needs review against its source issue. Review changes and validation evidence before integrating; Relay does not merge, push, deploy, or change GitHub board status automatically. Cached input token counts describe the completed turn; they do not predict cache expiry or the cost of the next message. Compact/reset actions remain unavailable in this milestone.

Use the [worker review checklist](review-checklist.md) before integrating a completed turn.

Mosaic tools are available separately through `nix run .#mosaic-fmt` and `nix run .#mosaic-cli`, so entering the shell does not build editor or packaging tools. Use `nix fmt` to format the flake.

`nix develop .#ui-test` also supplies Linux native-window verification tools: Xvfb, xdotool, ImageMagick, xclip, and Mesa. These stay out of the default shell. The desktop still needs a display; use Xvfb and Mesa software Vulkan when testing without a physical screen.

```sh
nix develop --command just dev
```

`just dev` builds the workspace, creates `.env` with a random token if neither configuration nor `RELAY_TOKEN` exists, starts a loopback server, waits for it to be ready, and opens the desktop with the same token. Closing the desktop or pressing Ctrl+C stops the processes it started. It leaves the database intact. Use `just dev 7440` to choose another local port. This recipe sets `RELAY_BIND` and `RELAY_ENDPOINT` for the local pair; use `just server` and `just client` for separate or remote processes.

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
| `RELAY_ENDPOINT` | Client | `http://127.0.0.1:7331/` |
| `RELAY_THEME` | Client | Optional startup override: `dark`, `light`, or `system`; defaults to saved settings, initially dark |
| `RELAY_SETTINGS_PATH` | Client | Optional settings file; otherwise the platform's local configuration directory, `relay/settings.toml` |

Keep tokens in your shell/session environment or an external secret manager. `just` also loads an optional `.env` file, allowing separate terminals to share your local configuration. `.env` and database files are ignored by Git; the binaries do not load `.env` themselves. The bundled reusable template is [profiles/default.toml](../profiles/default.toml); set `RELAY_DEFAULT_PROFILE` to your own complete template when starting a new workspace.

The sidebar groups directors under projects and worker sessions under their director. Click a project row or a director’s chevron to expand or collapse its group; opening a worker elsewhere reveals its ancestors. A project's board icon opens its board. A director's name opens its conversation or starts a task-linked planning conversation; its sliders icon always opens the profile. Expanded projects also offer director creation and project defaults. Arrow keys navigate the visible rows: Up/Down, Home/End, Right to expand or enter a group, and Left to collapse or return to its parent. Enter or Space activates the focused row; Tab reaches its secondary actions. Expansion stays intact during snapshot updates.

Open **Settings** with the gear at the bottom of the sidebar, the command palette, or **Ctrl+,** (**⌘+,** on macOS). Choose Dark, Light, or System appearance, with separate palettes for light and dark. Dark is the initial default. Display settings are client-local and survive restarts; they do not change the server workspace.

Mosaic automatically follows the display scale reported by the operating system, including monitor changes. Interface scale adjusts text, spacing, and controls on top of that scale; 100% follows the operating system. Use a larger setting if your 4K display reports 100% scaling. Pixel resolution alone does not identify physical display DPI. Drag the sidebar's right edge to resize it, or use its keyboard-accessible controls in Settings.

Reddit Sans for titles/subtitles and Zed Mono for remaining text are bundled under SIL OFL 1.1. No local font installation or download is required. Font provenance and licensing are documented in [the font assets](../crates/relay-desktop/assets/fonts/README.md).

## Remote server

Run the server on the remote machine with its database on persistent storage. Leave the default loopback bind and create an SSH tunnel from each client machine:

```sh
ssh -N -L 7331:127.0.0.1:7331 your-server
```

Use the server token and the default client endpoint locally. Alternatively, place the server behind an existing TLS reverse proxy and set `RELAY_ENDPOINT=https://relay.example.com/`. Forward HTTP and WebSocket traffic for `/v1/*`, including the Authorization header. HTTPS endpoints use WSS for events. Relay does not terminate TLS itself. Use SSH or TLS to protect the bearer token in transit.

## Foundation walkthrough

1. Open an issue card and follow its linked sessions. Demo issue URLs identify fixtures, not real GitHub work.
2. Open Directors. Change project defaults, save, then inspect the project director's inherited values and review director's overrides.
3. Create another director. Choose a harness, scope, responsibilities, completion steps, worker limit, and permissions. `Inherit` removes a field override.
4. Use `Export / edit TOML` to copy a declarative profile. Directors export overrides; omitted fields inherit. `Show effective profile` displays the full resolved profile. Import updates the draft; Save persists it.
5. Open a conversation. Click/select a recorded passage and type to create an anchored reply. Add more replies anywhere in the transcript; they share one next-message draft. Edit each reply beside its source or at the bottom. Paste text, copied files, or images at the cursor, including within replies. Recorded text stays unchanged.
6. Enter inserts a newline. Ctrl/Cmd+Enter sends the whole draft after it is saved. While an agent runs, this queues the message. Press again with an empty next draft to interrupt the current turn and send the just-queued message; there is no timing window. Typing another draft starts another message. Queue actions also offer Edit, Cancel, Send now, and explicit Resume queue after interruption.
7. Open the compact header menu for the linked issue, director profile, Changes, Details (usage/provenance), or Find. Tool activity and source context expand inline. Stop appears while the agent is active. Demo director/worker transcripts support drafting, but are fixtures and cannot execute turns.
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

The foundation was verified on Linux with Xvfb and Mesa software Vulkan: light/dark rendering, 820/1380-pixel layouts, keyboard navigation, clipboard copying, quoted comment posting, two-client synchronization, and persistence across a server restart. The live GitHub/Codex milestone adds real issue-linked worktree execution, client reconnect, exact-thread continuation, stop, and crash recovery. See the [dogfood validation record](dogfood-validation.md) for evidence and limits. See [local harness validation](harness-validation.md) for Claude Code, execution policies, and persistent local startup. Real desktop window decorations and macOS/Windows behavior still need validation on those environments.

## Named projects and local work

Choose **New Project** beneath the sidebar project list. Supply a name and an absolute root on the connected server; with `just dev`, this is your local machine. Initial repository, board and directory connections are optional. The project appears alongside existing projects with a local Backlog / In Progress / Done board and default director. Create a task or open the director to start planning without GitHub/GitLab credentials.

Open **Connections** from the project's menu or command palette. Repository shorthand such as `owner/repo` uses GitHub SSH; full SSH/HTTPS URLs also work. Relay clones beneath the project root and displays progress or a retryable error. Ordinary directory connections permit direct editing. Repositories get isolated session worktrees and separate reviews. Select fewer workspaces when starting a session if needed; resumed sessions keep their original selection.

The board selector keeps boards separate and remembers the selected board per project. **Publish board** supports a new or existing GitHub Projects/GitLab board, repository assignment per task, and named column mapping. Existing `gh`/`glab` authentication on the server is used. New GitLab boards offer Open/Closed mappings; use an existing label board for additional statuses. Removed connections retain their identities and can be restored from Connections. Verify the publication preview before submitting; interrupted writes may require checking the created issue/board remotely. The local board stays recoverable until the remote readback succeeds. Afterward, user task edits and moves update the remote provider; agent completion never changes board status automatically.

Settings use segmented Dark / Light / System and separate light/dark palette controls. Interface scale uses Mosaic's stepper from 80% to 200%, in 10% increments, on top of operating-system scaling. Harness status icons include text and distinguish current, unavailable and retained checks; detailed diagnostics and executable configuration are under Details.

See [project workflows validation](projects-validation.md) for the named-project, publication, native UI and multi-workspace checks.

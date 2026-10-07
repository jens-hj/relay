# Development

Relay requires SSH access to the private Mosaic repository on GitLab. Both Cargo and Nix use `ssh://git@gitlab.com/unincorporated/mosaic/mosaic.git`, pinned to the same published commit. They use your normal SSH configuration and agent; Cargo is configured to fetch through Git's CLI. Verify access with `git ls-remote git@gitlab.com:unincorporated/mosaic/mosaic.git HEAD`. Credentials are never embedded in the manifests or lockfiles.

## Local startup

The project's [flake.nix](../flake.nix) declares Relay's development environment. Enter `nix develop`, or use `direnv allow` with the checked-in `.envrc` to load it automatically. The default shell supplies Rust 1.89 with rustfmt, Clippy, rust-analyzer and Rust sources; Bash, just, Git, GitHub CLI, Codex CLI, pkg-config, curl, jq, OpenSSL and SQLite; and Linux graphics/windowing libraries. `nix develop .#server` provides the server environment without desktop libraries. Inputs are pinned in `flake.lock`, with shared dependency pins following Mosaic.

## Work on Relay through Relay

Authenticate on the server machine using `gh auth login` and `codex login`. GitHub board reads require `read:project` access in addition to repository access; if an existing CLI login lacks that scope, use `gh auth refresh --scopes read:project`. Existing CLI authentication and configured Codex model/provider are reused; Relay never copies credentials into its database. See [GitHub's Projects API authentication](https://docs.github.com/en/issues/planning-and-tracking-with-projects/automating-your-project/using-the-api-to-manage-projects#authentication).

```sh
nix develop
just doctor
just dogfood
```

Keep this server terminal running. In a second terminal, enter the same project's dev environment and run `just client`. `just dogfood` creates a random shared token in a Git-ignored, owner-readable `.env` when one does not exist; existing configuration is preserved. It connects `jens-hj/relay` to [GitHub Project 5](https://github.com/users/jens-hj/projects/5), uses the current repository as the server-owned source checkout, and defaults to `data/dogfood.sqlite3`. Supply another board with `just dogfood owner/repo owner NUMBER`; set `RELAY_REPO_PATH` to its matching local Git checkout.

Choose the live project, sync its board, select an issue, and start a worker under a director. The default implementation permission is Ask, so starting or continuing a turn requires explicit approval in the form. Scope, harness, implementation permission, and active-worker limits are checked by the server. The first harness is Codex; a Claude Code profile cannot start a run yet.

Closing a desktop leaves the server and worker running. Reopen `just client` to reconnect, or use another machine with the same token and an SSH tunnel. Stopping the server interrupts active runs; they remain in history and must be continued explicitly after restart. `just dev` is a short-lived demo convenience: it stops its server when its desktop closes, so use separate server/client processes to exercise durable remote execution.

Worker turns run in their own Git worktrees using Codex's workspace-write sandbox. Session details show the thread, branch, base commit, worktree, latest measured token usage, and a bounded diff including untracked files. A completed turn still needs review against its source issue. Review changes and validation evidence before integrating; Relay does not merge, push, deploy, or change GitHub board status automatically. Cached input token counts describe the completed turn; they do not predict cache expiry or the cost of the next message. Compact/reset actions remain unavailable in this milestone.

Use the [worker review checklist](review-checklist.md) before integrating a completed turn.

Mosaic tools are available separately through `nix run .#mosaic-fmt` and `nix run .#mosaic-cli`, so entering the shell does not build editor or packaging tools. Use `nix fmt` to format the flake.

`nix develop .#ui-test` also supplies Linux native-window verification tools: Xvfb, xdotool, ImageMagick, xclip, and Mesa. These stay out of the default shell. The desktop still needs a display; use Xvfb and Mesa software Vulkan when testing without a physical screen.

```sh
nix develop --command just dev
```

`just dev` builds the workspace, generates a random token unless `RELAY_TOKEN` is already set, starts a loopback server, waits for it to be ready, and opens the desktop with the same token. Closing the desktop or pressing Ctrl+C stops the processes it started. It leaves the database intact. Use `just dev 7440` to choose another local port. This recipe sets `RELAY_BIND` and `RELAY_ENDPOINT` for the local pair; use `just server` and `just client` for separate or remote processes.

For separate terminals, enter the development shell in each and supply the same token:

```sh
# First terminal:
export RELAY_TOKEN="$(openssl rand -hex 32)"
just server

# Second terminal, using the first terminal's token:
export RELAY_TOKEN="<same token>"
export RELAY_NAME="Your name"
just client
```

The default server address is `127.0.0.1:7331`. Both processes require the token. Tokens must contain at least 16 printable ASCII characters with no spaces; use a randomly generated token. The token grants access to the entire workspace. Comment author names are display labels, not authenticated identities.

| Variable | Process | Default / behavior |
| --- | --- | --- |
| `RELAY_TOKEN` | Both | Required; never logged |
| `RELAY_BIND` | Server | `127.0.0.1:7331` |
| `RELAY_DATABASE` | Server | `data/relay.sqlite3` |
| `RELAY_DEFAULT_PROFILE` | Server | Optional path to a complete TOML profile; used to seed a new database only |
| `RELAY_GITHUB_REPO` | Server | Live repository, `owner/repo`; omitted for demo mode |
| `RELAY_GITHUB_PROJECT_OWNER` | Server | GitHub user or organization that owns the board |
| `RELAY_GITHUB_PROJECT_NUMBER` | Server | Projects v2 number from the board URL |
| `RELAY_REPO_PATH` | Server | Matching local Git checkout; never supplied by a client |
| `RELAY_ENDPOINT` | Client | `http://127.0.0.1:7331/` |
| `RELAY_NAME` | Client | `Teammate`; editable in the comment composer |
| `RELAY_THEME` | Client | `system`; optionally `light` or `dark` |

Keep tokens in your shell/session environment or an external secret manager. `just` also loads an optional `.env` file, allowing separate terminals to share your local configuration. `.env` and database files are ignored by Git; the binaries do not load `.env` themselves. The bundled reusable template is [profiles/default.toml](../profiles/default.toml); set `RELAY_DEFAULT_PROFILE` to your own complete template when starting a new workspace.

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
5. In Sessions, Tab to a message or use its Comment action. Select a passage and use Ctrl/Cmd+Enter on the message to include it as a quote. Arrow keys navigate between messages. Feedback persists beside its source message.
6. Open a second client with the same token. Saved changes appear in both clients. Restart either client or the server; saved profiles and comments remain.

Cmd/Ctrl+K opens the command palette. Cmd/Ctrl+F opens transcript search. Escape closes the palette/search. Tab and Shift+Tab traverse controls. Unsaved comments remain after failed requests; the composer identifies their target message even if another session is opened.

Conflicting saves do not overwrite newer server state. `Review latest state` retains your draft, refreshes its revision and inherited defaults, and lets you review before saving again. Network errors do not automatically retry writes. Retrying the same command reuses its request ID; the server prevents duplicate application, including after restart.

## Checks

```sh
nix develop --command just check
```

`just` lists available recipes. Individual commands include `just build`, `just test`, `just lint`, `just fmt`, `just fmt-check`, and `just nix-check`.

Tests cover server persistence, idempotency, authentication, profile inheritance, validation, conflicts, event synchronization, and headless UI interactions. A native Linux window is also required for graphics, windowing, and clipboard verification. macOS and Windows verification remains separate from Linux validation.

The foundation was verified on Linux with Xvfb and Mesa software Vulkan: light/dark rendering, 820/1380-pixel layouts, keyboard navigation, clipboard copying, quoted comment posting, two-client synchronization, and persistence across a server restart. Build, all 15 tests, Clippy, Rust formatting, and the pinned Mosaic formatter passed. Real desktop window decorations and macOS/Windows behavior still need validation on those environments.

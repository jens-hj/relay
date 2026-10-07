# Development

Relay requires access to the private Mosaic repository on GitLab. Configure your normal Git HTTPS credential helper before resolving dependencies; credentials are never embedded in the manifests or lockfiles. Mosaic and the Nix input are pinned to the same published commit.

## Local startup

The project's [flake.nix](../flake.nix) declares Relay's development environment. Enter `nix develop`, or use `direnv allow` with the checked-in `.envrc` to load it automatically. The default shell supplies Rust 1.89 with rustfmt, Clippy, rust-analyzer and Rust sources; Git, pkg-config, curl, jq, OpenSSL and SQLite; and Linux graphics/windowing libraries. `nix develop .#server` provides the server environment without desktop libraries. Inputs are pinned in `flake.lock`, with shared dependency pins following Mosaic.

Mosaic tools are available separately through `nix run .#mosaic-fmt` and `nix run .#mosaic-cli`, so entering the shell does not build editor or packaging tools. Use `nix fmt` to format the flake.

```sh
export RELAY_TOKEN="$(openssl rand -hex 32)"
cargo run --locked -p relay-server
```

In another development shell, set the same `RELAY_TOKEN` and run:

```sh
export RELAY_NAME="Your name"
cargo run --locked -p relay-desktop
```

The default server address is `127.0.0.1:7331`. Both processes require the token. Tokens must contain at least 16 printable ASCII characters with no spaces; use a randomly generated token. The token grants access to the entire workspace. Comment author names are display labels, not authenticated identities.

| Variable | Process | Default / behavior |
| --- | --- | --- |
| `RELAY_TOKEN` | Both | Required; never logged |
| `RELAY_BIND` | Server | `127.0.0.1:7331` |
| `RELAY_DATABASE` | Server | `data/relay.sqlite3` |
| `RELAY_DEFAULT_PROFILE` | Server | Optional path to a complete TOML profile; used to seed a new database only |
| `RELAY_ENDPOINT` | Client | `http://127.0.0.1:7331/` |
| `RELAY_NAME` | Client | `Teammate`; editable in the comment composer |
| `RELAY_THEME` | Client | `system`; optionally `light` or `dark` |

Keep tokens in your shell/session environment or an external secret manager. `.env` and database files are ignored by Git; Relay does not automatically load `.env`. The bundled reusable template is [profiles/default.toml](../profiles/default.toml); set `RELAY_DEFAULT_PROFILE` to your own complete template when starting a new workspace.

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
cargo test --locked --workspace
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
nix run .#mosaic-fmt -- --check crates/relay-desktop/src
```

Tests cover server persistence, idempotency, authentication, profile inheritance, validation, conflicts, event synchronization, and headless UI interactions. A native Linux window is also required for graphics, windowing, and clipboard verification. macOS and Windows verification remains separate from Linux validation.

The foundation was verified on Linux with Xvfb and Mesa software Vulkan: light/dark rendering, 820/1380-pixel layouts, keyboard navigation, clipboard copying, quoted comment posting, two-client synchronization, and persistence across a server restart. Build, all 15 tests, Clippy, Rust formatting, and the pinned Mosaic formatter passed. Real desktop window decorations and macOS/Windows behavior still need validation on those environments.

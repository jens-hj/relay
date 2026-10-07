# Relay

Relay is a shared workspace for small teams to direct coding agents through their existing GitHub or GitLab projects. It brings the authoritative project board, issue-linked sessions, contextual review, and configurable directors together in a native Rust application built with Mosaic.

The first foundation is tracked in [issue #1](https://github.com/jens-hj/relay/issues/1). See [development](docs/development.md) for setup and [architecture](docs/architecture.md) for boundaries and next steps.

## Run

The project declares its local development environment in [flake.nix](flake.nix). Use `nix develop` or `direnv allow` with the included `.envrc`.

```sh
nix develop
export RELAY_TOKEN="$(openssl rand -hex 32)"
cargo run -p relay-server
# In a second terminal with the same token:
cargo run -p relay-desktop
```

The server seeds a clearly labeled demo project. Boards and transcripts are fixtures; director profiles and contextual comments are real, persisted server data. No coding agent is started. Codex and Claude Code execution, provider synchronization, and context controls are later integrations.

Relay keeps execution on the server and treats clients as reconnectable views. Directors coordinate workers, several directors can specialize in one project, and direct agent interaction remains part of the intended product. Declarative profiles expose scope, responsibilities, completion requirements, and action permissions. Context and usage controls will be visible wherever the connected harness provides reliable support.

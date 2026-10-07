# Relay

Relay is a shared workspace for small teams to direct coding agents through their existing GitHub or GitLab projects. It brings the authoritative project board, issue-linked sessions, contextual review, and configurable directors together in a native Rust application built with Mosaic.

The foundation is tracked in [issue #1](https://github.com/jens-hj/relay/issues/1), and the first live workflow in [issue #2](https://github.com/jens-hj/relay/issues/2). See [development](docs/development.md) for setup and [architecture](docs/architecture.md) for boundaries and next steps.

## Run

The project declares its local development environment in [flake.nix](flake.nix). Use `nix develop` or `direnv allow` with the included `.envrc`.

```sh
nix develop
just dev
```

`just dev` builds both processes, shares a fresh workspace token, and stops the server when you close the app or press Ctrl+C. Run `just` to list commands; `just check` runs the project's checks. With direnv enabled, run `just dev` directly.

Without remote configuration, the server seeds a clearly labeled demo project. Demo boards and transcripts are fixtures; director profiles and contextual comments are real, persisted server data.

To work on Relay through its [GitHub board](https://github.com/users/jens-hj/projects/5), authenticate GitHub and Codex on the server machine, then run:

```sh
just doctor
just dogfood
# In another terminal, with the same project's dev environment:
just client
```

`just dogfood` creates a local token in `.env` if needed, uses a separate dogfood database, and runs the server independently of the desktop. Sync the live board, select an issue, and start a Codex worker. Its transcript, measured usage, and review diff remain on the server when a client disconnects. Work happens in an isolated Git worktree; integration remains an explicit review step. Claude Code, GitLab synchronization, autonomous director delegation, and context lifecycle controls are later integrations.

Relay keeps execution on the server and treats clients as reconnectable views. Directors coordinate workers, several directors can specialize in one project, and direct agent interaction remains part of the intended product. Declarative profiles expose scope, responsibilities, completion requirements, and action permissions. Context and usage controls will be visible wherever the connected harness provides reliable support.

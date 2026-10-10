# Relay

Relay is a shared workspace for small teams to direct coding agents through their existing GitHub or GitLab projects. It brings the authoritative project board, issue-linked sessions, contextual review, and configurable directors together in a native Rust application built with Mosaic.

The foundation is tracked in [issue #1](https://github.com/jens-hj/relay/issues/1), the first live workflow in [issue #2](https://github.com/jens-hj/relay/issues/2), the continuous agent buffer in [issue #6](https://github.com/jens-hj/relay/issues/6), local Claude Code/Codex execution in [issue #7](https://github.com/jens-hj/relay/issues/7), and named projects and board publishing in [issue #8](https://github.com/jens-hj/relay/issues/8). See [development](docs/development.md) for setup and [architecture](docs/architecture.md) for boundaries and next steps.

The existing Mosaic client also builds to WebAssembly for browser access. See [remote access and Nix deployment](docs/remote-access-plan.md), tracked in [issue #12](https://github.com/jens-hj/relay/issues/12), for private owner login, HTTPS hosting, and persistent services.

## Run

The project declares its local development environment in [flake.nix](flake.nix). Use `nix develop` or `direnv allow` with the included `.envrc`.

```sh
nix develop
just dev
```

`just dev` builds both processes, saves a local workspace token in `.env`, and reopens `data/local.sqlite3`. Use `just dev --release` to build and run the release binaries, or add a port such as `just dev --release 7440`. Choose **New Project** beneath the sidebar projects, enter a name and a root directory on the server, and start with a local task board. Repository, board, and directory connections are optional and can be added later from the project menu. Repositories clone automatically using the server’s Git credentials. Saved projects reopen automatically. Closing the app or pressing Ctrl+C stops this local server. Run `just` to list commands; `just check` runs the project's checks. With direnv enabled, run `just dev` directly.

Settings → Harnesses shows the server’s installed Codex and Claude Code, version, authentication, and executable. Relay prefers your installed local harnesses over the Nix fallbacks and reuses their login/model configuration. Authenticate on the server with `codex login` or `claude auth login`. Choose the harness in project defaults or the director profile; existing workers keep their original harness.

`just demo` opens an isolated fixture preview. Demo boards and transcripts cannot launch harness turns; local tasks and synchronized remote issues can.

To work on Relay through its [GitHub board](https://github.com/users/jens-hj/projects/5), authenticate GitHub and your chosen harness on the server machine, then run:

```sh
just doctor
just dogfood
# In another terminal, with the same project's dev environment:
just client
```

`just dogfood` creates a local token in `.env` if needed, uses a separate dogfood database, and runs the server independently of the desktop. Sync the live board, select an issue, and start a worker. Its transcript, measured usage, and review diff remain on the server when a client disconnects. Work happens in an isolated Git worktree; integration remains an explicit review step. GitHub Projects and GitLab project/group issue boards are supported, including explicit task edits and moves. Autonomous director delegation and context lifecycle controls remain later integrations.

Projects can contain several repositories and ordinary directories. Sessions use isolated repository worktrees and directly edit selected directories; resuming keeps the same workspace set. Publish a local board to a new or existing remote board with repository assignments and column mappings. Provider writes retain recoverable progress and require confirmation when their outcome is uncertain.

Execution defaults to Automatic: Codex uses workspace-write with approvals disabled; Claude uses its native auto mode and sandbox. Ask routes supported native permission requests into the conversation with Allow once/Deny. Unrestricted Access enables the harness’s native bypass/full access mode. Execution settings inherit from project to director to worker; workflow action permissions remain separate.

Agent conversations are editable documents: typing on earlier text creates an anchored reply, and files/images paste inline at the cursor. Several replies share one saved draft. Enter adds a line; Ctrl/Cmd+Enter sends. Messages queue while the agent runs; pressing again with an empty next draft interrupts and promotes the just-queued message. Execution details live in the compact header menu.

Relay keeps execution on the server and treats clients as reconnectable views. Directors coordinate workers, several directors can specialize in one project, and direct agent interaction remains part of the intended product. Declarative profiles expose scope, responsibilities, completion requirements, and action permissions. Context and usage controls will be visible wherever the connected harness provides reliable support.

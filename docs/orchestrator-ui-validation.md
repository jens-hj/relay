# Orchestrator UI validation

Tracked in [#27](https://github.com/jens-hj/relay/issues/27); see the [plan](orchestrator-ui-plan.md) and the design pack in [`design/orchestrator-ui/`](../design/orchestrator-ui/). This records the evidence for the port on Linux.

## Automated checks

`nix develop --command just check` passes on `orchestrator-ui`. It runs workspace tests (core 22, server 154, desktop 161, relay-ui 71), strict Clippy, Rust/Mosaic/just/Nix formatting and `nix flake check`. `just build-web` builds the WASM client, and `relay-ui` also builds for `wasm32-unknown-unknown` on its own.

## Screenshots

Rendered offscreen with Mesa lavapipe over the demo orchestration fixture (`nix develop .#ui-test --command cargo test -p relay-desktop shell_screenshots -- --ignored`):

| Page | File |
| --- | --- |
| Manager conversation: tree, Needs you, approval block, context gauge, ledger | [conversation.png](assets/orchestrator-ui/conversation.png), [manager.png](assets/orchestrator-ui/manager.png) |
| Board with the issue inspector | [board.png](assets/orchestrator-ui/board.png) |
| Settings, profile, connections, command palette | [settings.png](assets/orchestrator-ui/settings.png), [profile.png](assets/orchestrator-ui/profile.png), [connections.png](assets/orchestrator-ui/connections.png), [palette.png](assets/orchestrator-ui/palette.png) |

The `relay-ui` gallery renders the same way (`cargo test -p relay-ui gallery_screenshots -- --ignored`, with `RELAY_UI_SPECIMEN` set to `agents`, `tools`, `conversation` or `composer`): [agent tree and surfaces](assets/orchestrator-ui/gallery-agents.png), [all 13 tool calls in three openings](assets/orchestrator-ui/gallery-tools.png), [composer ledger states](assets/orchestrator-ui/gallery-composer.png) and [the conversation specimen in the light theme](assets/orchestrator-ui/gallery-conversation-light.png).

## Live walkthrough

[walkthrough.mp4](assets/orchestrator-ui/walkthrough.mp4) ([GIF](assets/orchestrator-ui/walkthrough.gif)) records the native client against a demo server (`RELAY_DEMO=1`) under Xvfb with Mesa lavapipe at 1440×960. It covers the board and issue inspector, a manager conversation, the tool-usage strip hover, the context panel, transcript scrolling, focusing a folded branch and returning to the whole tree. Frames are captured with `xwd`, so the pointer is not visible. Without a window manager, Xvfb only delivers keys after `xdotool windowfocus`.

The walkthrough found that an open popover was laid out inside its anchor and pushed the header's context gauge out of place; popovers are now hosted as root attachments, with a regression test.

## Not validated

- A real Codex turn calling the Relay MCP tools (a real Claude Code session listed agents through them).
- macOS and Windows window chrome.
- Harness data that is not reported is hidden rather than shown: the context breakdown, usage suggestions, fetched page titles and Claude command exit codes.

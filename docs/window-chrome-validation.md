# Themed window chrome validation

Tracked in [issue #11](https://github.com/jens-hj/relay/issues/11).

Relay uses the window configuration from the pinned Mosaic `text-editor-app` example. On macOS, the native traffic lights sit over themed sidebar content; their 56px strip and clearance stay fixed as interface typography scales. Other desktop platforms use client decorations and themed window controls in the existing page headers. Profile headers now remain at the top outside the form scroll area, and notices appear below window chrome.

Noninteractive header text, status cells, headings, and gaps are drag targets. Interactive descendants retain their actions and editing behavior. Client resize targets cover the outer edges and corners. Before passing a drag or resize to the native window manager, Relay cancels Mosaic's pointer capture after the current handler returns. This keeps the next press independent when the window manager consumes the release. Window maximization/restoration uses the window control, matching the editor example's drag-only header gesture.

## Linux native verification

Used an isolated fixture server, database, and settings paths under `/tmp/relay-window-chrome`, Xvfb display `:112` (1800×1200), Openbox from the repository's pinned Nixpkgs revision, and Mesa software Vulkan. No real harness runs or provider writes occurred.

- Dragging the page heading moved the window from (100,100) to (140,130); dragging the empty title cell moved it to (180,160). Sidebar-heading dragging was also checked.
- Maximize changed 1380×900 to 1800×1200; restore returned to 1380×900 at its previous position.
- Minimize hid the client window; activation restored it.
- The east edge enlarged the native window to 1480×900. The southeast corner then resized it to 1380×800.
- Opening issue details kept the window controls at the far right; closing through the window control exited the client.
- Reviewed native dark board/issue-detail captures at 1380px and the light board at 820px. Captures and native verification logs are under `/tmp/relay-window-chrome`.

## Automated verification

The desktop tests cover platform configuration, header/control placement across pages at 1380/820px and 760px at 200% scale, notices, empty workspaces, issue inspectors, drag hit testing, editable-field exclusion, resize edges/corners, and a missing-release regression that immediately activates another control after a native gesture handoff.

`nix develop --command just check` passed: 242 workspace tests (6 core, 117 desktop, 109 server, and 10 workspace integration tests), Clippy with warnings denied, all required formatting checks, and flake evaluation. Native macOS and Windows behavior remains unverified on this Linux environment.

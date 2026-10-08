# Labelism interface validation

Tracked in [#9](https://github.com/jens-hj/relay/issues/9); see the [plan summary](labelism-plan.md). This records evidence for the native Labelism restyle on Linux.

## Automated checks

`nix develop --command just check` runs workspace tests, strict Clippy, Rust/Mosaic/just/Nix formatting and `nix flake check`. The result of the final run is recorded under [Final check](#final-check).

Desktop coverage added or kept by this work:

- **Contrast:** every palette meets 4.5:1 for text roles on every neutral background and for each paired text-on-fill color, including on-inverse text on the inverse fill. Focus rings, rules, status marks and the selector indicator meet 3:1. The original concept text colors fail the same check.
- **Selected states:** selected tree rows and inverse-filled primary buttons keep their fill while hovered and pressed, so on-inverse text never sits on a hover fill. Status glyphs in selected rows draw every mark in the on-inverse color.
- **Geometry:**
  - The page header is 56px and its rule meets the sidebar brand rule.
  - Column heads (40px) start on the header rule.
  - The board summary strip shares its top rule with the sidebar connection footer.
  - Card previews clip to three 17px lines.
  - The inspector opens on its identifier header, which holds Close.
  - The profile header is 74px at the page padding, the inheritance strip 54px and the save bar 50px.
  - The session header is 74px with the run strip directly below it.
  - Agent messages and the draft wrap within a 760 px reading column beside the 92 px author column.
- **Board actions menu:**
  - Escape closes it and returns focus to the trigger, and one click reopens it.
  - A press outside it closes it, including a press on another control, which still activates.
  - Menu items act before the menu closes. Completed operations stay reachable through Operation history.
- **Truthful readouts:**
  - Fixture projects show Fixture as their source and card status; a migrated local project keeps "Local board" and "Ready to scope".
  - Profile override counts are the overridden whole fields out of seven.
  - The run strip shows only recorded values; its approval cell is labelled "Next-turn approval", because the mode can change during a run.
  - Usage appears only when measured.
  - Approval requests name the action in plain words (for example "Run command" or "Change files") and show the recorded request details in full, pretty-printed when they are JSON, in a bounded scroll with positive height.
- **Drafts and focus:**
  - The new task draft survives closing and reopening the form.
  - Switching a 1380 px window from 100% to 200% scale moves the session into its narrow layout without rebuilding the draft surface: the focused editing node and draft text are kept.
  - At 760 px and 200% scale every session panel (actions, details, warnings, approval, recovery) stays reachable and the draft node is kept.
- **Bounded panels:**
  - Session actions, details, warnings, approval and recovery share a height budget and scroll within it, so the draft stays mounted and every control can be hit at 760 px and 200%.
  - The worker setup scrolls 24 directors and resources at 1600 px/100% and 760 px/200% without losing the prompt or Start worker.
  - Operation history shows 50 completed operations with readable provider results and a reachable Close.
- **Page header:** the shared header's eyebrow follows navigation between Settings, Connections, Publish and New Project.
- **Settings:** saved files without high-contrast keys keep their values; false keys are omitted; backups never replace an existing file; unreadable or newer files are never overwritten; a failed backup stays suspended; retry adopts a repaired file.
- **Typography:** a live `Readout` value keeps its 13px Zed Mono container typography when its value changes. A second regression records the pinned text behavior described below.

## Native captures

Linux, `nix develop .#ui-test`: Xvfb with Mesa lavapipe (software Vulkan).

Representative parent-reviewed native screenshots are saved in the repository: [Warm board](assets/labelism/board-warm.png), [Warm director profile](assets/labelism/profile-warm.png), [Neutral agent buffer](assets/labelism/agent-buffer-neutral.png), and [Warm settings](assets/labelism/settings-warm.png). These show the accepted views; the buffer includes the final approval-card and reading-column fixes.

- **Isolation:** capture servers and clients used task-owned PID files, temporary databases/settings and throwaway project roots under `/tmp/relay-labelism-capture` and `/tmp/relay-labelism-parent`. Dedicated Xvfb displays were used.
- **Seeded state:** a capture-only script sends real commands to the isolated server: it creates a project with a directory connection, creates a task, and starts a worker.
- **Fake harness:** the worker is a capture-only fake Codex app-server script kept outside the repository. It replays one agent message labelled "[Capture fixture output]", a token-usage report and a command approval request. A failed run was produced by stopping that fake process (a child of the capture server, by PID).
- **Demo data:** the read-only demo fixture project is shown as Fixture.
- **No real harness:** no real agent turns or remote provider writes were made.

Final round, frames in `/tmp/relay-labelism-capture/final2/` (earlier rounds are kept separately). Session frames and `compare-03-session.png` come from the build of `15a749f`: transcript content is capped at its 760 px reading column, and the approval card shows a readable action name with the recorded request details. The other frames come from `345fc01`, whose views are unchanged since. Names are `<palette>-<window>-s<sidebar>[-200pct]-<view>.png`.

| Frames | Shows |
|---|---|
| `compare-01-board.png`, `compare-02-profile.png`, `compare-03-session.png` | Concept render beside the native frame at 1600×1000, sidebar 300 |
| `warm-1600-s300-{demo,demo-detail,capture-detail,profile,settings}` | Warm light: fixture board with tinted label strips, inspector with anchored worker setup, director profile, settings modules |
| `neutral-1600-s300-{board,session}` | Neutral dark: seeded project board; worker session waiting for approval |
| `slate-1380-s220-{demo,demo-detail,capture-detail,profile,session,settings}` | Default Slate dark at the normal 220 px sidebar |
| `slate-760-s220-{board,capture-detail,profile,session}` | 760 px window: stacked columns, full-width inspector, stacked profile, narrow session |
| `slate-1380-s220-200pct-{board,capture-detail,profile,session}` | 200% scale: compact session header, bounded inspector |
| `hc-dark-1380-s220-{demo-detail,session}`, `hc-light-1380-s220-{demo-detail,profile}` | High contrast palettes |

**Differences from the concept, kept on purpose:**
- the sidebar tree replaces the concept's turn ruler and fixed legend;
- the session has no right rail: queued messages stay inline in the transcript, and usage and changes open from the session actions;
- mock-only readouts are left out: turn numbers, elapsed time, timestamps, keyboard grid hints, unsaved-change counts and per-file bars;
- profile origins are whole-field, not per action;
- Merge and Deploy stay editable.

## Framework notes

- **Button labels:** Mosaic buttons created from the DSL use `ButtonStyle::default()`, whose label has a fixed text style and an 8px radius. `controls::ButtonStyle` (the one remaining workaround) makes labels inherit typography and squares the corners.
- **Live text:** pinned Mosaic `text_dyn_styled` binds a live text leaf's content and style in separate effects. In the checked cases typography survives mount, content updates and a theme switch:
  - leaf attributes;
  - leaf classes;
  - styled parent containers;
  - token colors.

  The earlier note that static attributes are dropped on content updates did not reproduce at this pin and is withdrawn. The views put typography on parent containers anyway, following the example apps, so live values share one inherited style.
- **Text fitting:** single-line labels keep their full text at max-content width inside a clipped row with the text editor example's right-edge mask. The mask also dims text whose width falls within the last 18% of its container, even when it fits. Cells sized to their content therefore clip at a maximum width instead of fading.
- **Text wrapping in buttons:** `text-wrap:none` is not honored for text inside buttons, so labels use max-content text inside clipped rows.
- **Manual tooltips as menus:** a manual tooltip suppresses itself on Escape without clearing the caller's open state. The board actions menu therefore owns its dismissal. It handles Escape on the anchor and registers an owner-scoped press handler on the mounted root when the menu first opens, because views are built under a placeholder root that mounting replaces.
- **Rebuilt branches:** responsive layouts switch grid track templates instead of rebuilding branches, so editing surfaces keep their state and focus.

## Limitations

- **Platforms:** only Linux with software rendering was verified. macOS, Windows and hardware GPU rendering are not validated.
- **Fake harness:** it exercises the desktop's handling of real protocol shapes, not real Codex or Claude Code behavior.
- **Issue status ordering:** "most recent outcome" uses session creation order, because sessions carry no timestamps.
- **Thin glyphs on light fills:** dark text on light inverse fills renders lighter under linear-space blending, so labels on inverse fills are bold. Their contrast pairs pass, but the dark-theme selected rows look lighter than the concept.
- **Not shown, because no recorded data exists:** turn numbers, elapsed time and unsaved-change counts. Profile validation is a local check of the edited profile; the server rechecks it on save.

## Final check

`CARGO_TARGET_DIR=<shared> nix develop --command just check` exited 0 at `15a749f`:
- 98 desktop, 109 server, 6 core and 10 HTTP tests;
- strict Clippy on all workspace targets;
- Rust, Mosaic, just and Nix formatting;
- `nix flake check` (native x86_64-linux outputs; other platforms omitted).

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
- **Board actions menu:**
  - Escape closes it and returns focus to the trigger, and one click reopens it.
  - A press outside it closes it, including a press on another control, which still activates.
  - Menu items act before the menu closes. Completed operations stay reachable through Operation history.
- **Truthful readouts:**
  - Fixture projects show Fixture as their source and card status; a migrated local project keeps "Local board" and "Ready to scope".
  - Profile override counts are the overridden whole fields out of seven.
  - The run strip shows only recorded values; its approval cell is labelled "Next-turn approval", because the mode can change during a run.
  - Usage appears only when measured.
- **Drafts and focus:**
  - The new task draft survives closing and reopening the form.
  - At 760 px and 200% scale the session switches to its narrow layout without rebuilding the draft surface: focus and draft text are kept, and approval, queue and session actions fit the window.
- **Settings:** saved files without high-contrast keys keep their values; false keys are omitted; backups never replace an existing file; unreadable or newer files are never overwritten; a failed backup stays suspended; retry adopts a repaired file.
- **Typography:** a live `Readout` value keeps its 13px Zed Mono container typography when its value changes. A second regression records the pinned text behavior described below.

## Native captures

Linux, `nix develop .#ui-test`: Xvfb with Mesa lavapipe (software Vulkan).

- **Isolation:** each run used its own server process, temporary database, settings file and throwaway project directory under `/tmp/relay-labelism-capture`. Only processes recorded in that directory's PID files were started or stopped.
- **Seeded state:** a capture-only script sends real commands to the isolated server: it creates a project with a directory connection, creates a task, and starts a worker.
- **Fake harness:** the worker is a capture-only fake Codex app-server script kept outside the repository. It replays one agent message labelled "[Capture fixture output]", a token-usage report and a command approval request. A failed run was produced by stopping that fake process (a child of the capture server, by PID).
- **Demo data:** the read-only demo fixture project is shown as Fixture.
- **No real harness:** no real agent turns or remote provider writes were made.

CAPTURES

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
- **Not shown, because no recorded data exists:** turn numbers, elapsed time, unsaved-change counts and profile validation.

## Final check

FINAL

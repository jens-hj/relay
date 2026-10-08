# Labelism interface validation

Tracked in [#9](https://github.com/jens-hj/relay/issues/9); see the [plan summary](labelism-plan.md). This records evidence for the native Labelism restyle on Linux.

## Automated checks

- `nix develop --command just check` passed: workspace tests, strict Clippy, Rust/Mosaic/just/Nix formatting, and `nix flake check`.
- Relay desktop: 75 tests. Server, core and the remaining suites are unchanged and pass.
- New desktop coverage:
  - **Contrast:** every palette meets WCAG contrast for text roles on every neutral background (≥4.5:1). Focus rings, rules, status marks and the selector indicator meet ≥3:1. The original concept text colors are rejected by the same check.
  - **Settings:**
    - Saved files without high-contrast keys keep mode, family, scale, width and board selections.
    - False high-contrast keys are omitted when saving.
    - A backup never replaces an existing file.
    - An unreadable or newer settings file is never written by preference changes.
    - A failed backup keeps the original and stays suspended.
    - Explicit backup re-enables saving.
    - Retry adopts a repaired file.
  - **Usage math:** uncached input = input − cached; cached greater than input is clamped and reported; zero totals and `u64::MAX` values are handled.
  - **Issue status precedence:** waiting, then running, then queued, then the newest outcome. A newer completion is not hidden by an older failure.
  - **Controls:**
    - Sidebar descriptions carry status and capacity.
    - The session header uses status words.
    - Usage appears only when measured.
    - The execution selector sends the chosen mode.
    - Permission rows set exact values for every action by pointer and keyboard.
    - Palette selectors round-trip through High contrast.

## Native captures

Linux, `nix develop .#ui-test`: Xvfb with Mesa lavapipe (software Vulkan), window 1380×900 unless noted.

- **Isolation:** each run used a temporary database, settings file and throwaway project directory under `/tmp/relay-labelism-capture`.
- **Fake harness:** session states come from a **capture-only fake Codex app-server script** kept outside the repository. It replays an agent message, a token-usage report (48,213 input / 31,004 cached / 2,910 output) and a command approval request. Its message is labelled "[Capture fixture output]".
- **Demo data:** board states otherwise come from the built-in demo fixture.
- **No real harness:** no real agent turns or remote provider writes were made.

| Capture | Shows |
|---|---|
| `soft-dark-01-board-waiting` | Default Slate palette; card status from a pending approval; sidebar worker glyph |
| `soft-dark-02-issue-detail` | Selected card frame, issue block, director choice, policy readouts, disabled start |
| `soft-dark-03-demo-board-detail` | Fixture board with label tints, fixture statuses, linked sessions |
| `soft-dark-04-session-approval-queue` | Waiting header, Allow once / Deny strip, numbered queued message |
| `soft-dark-05-session-details-usage` | Session actions, sliding execution selector, measured usage meter |
| `soft-dark-06-profile-matrix` | Action matrix with responsibility/completion toggles and per-action sliding permissions |
| `soft-dark-06-settings` | Framed settings sections, three-way palette selectors, square stepper |
| `soft-dark-07-suspended-settings` | Settings file with an unknown key: saving suspended, Back up and save / Retry reading. The file was byte-identical afterwards. |
| `soft-light-paper-{board,settings,profile}` | Paper palette |
| `hc-dark-{board,settings}`, `hc-light-{board,settings}` | High contrast palettes |
| `scale200-soft-dark-{board,session}` | 200% interface scale |
| `narrow760-soft-dark-{board,profile}` | 760 px window |
| `keyboard-01…04` | Tab into the sidebar, focus ring, command palette search, settings focus |

**Where the captures were taken:**
- `soft-dark-06-settings` and `soft-dark-07-suspended-settings` were retaken at the final commit.
- The other captures were taken one commit earlier. That commit only squared the scale stepper parts and trimmed the settings error text.

**Keyboard walkthrough** (recorded):
- Tab moves focus into the sidebar; the focus ring is a square accent outline.
- Ctrl+K opens the command palette with its search focused, and Enter runs the filtered action.
- Ctrl+, opens Settings.
- Tree arrow navigation, selector arrows/Home/End, permission rows and queue actions are covered by the headless tests above.

## Findings fixed during native review

- Mosaic buttons have a default corner radius; shared styles set it to zero.
- In the pinned Mosaic, static text attributes are dropped when reactive text content updates. Text with reactive content uses reactive color, weight, family and transform values. Text inside buttons sets its own color.
- Thin dark glyphs on light fills render lighter under linear-space blending. Labels on inverse fills (selected segments, tabs, choices, primary buttons) are bold.
- `text-wrap:none` is not honored for text inside buttons. The sidebar's director capacity meter is drawn only when the full name fits, and capacity is always in the row's accessible description.
- The session actions menu now separates actions from the execution selector. The details pane shows usage first, and meter segments fill their row.

## Limitations

- **Platforms:** only Linux with software rendering was verified. macOS, Windows and hardware GPU rendering are not validated.
- **Fake harness:** it exercises the desktop's handling of real protocol shapes, not real Codex or Claude Code behavior.
- **Issue status ordering:** "most recent outcome" uses session creation order, because sessions carry no timestamps.
- **Narrow sidebar:** at the default 220 px sidebar width, the inline capacity meter usually does not fit. Capacity remains available through the row description and the profile editor.

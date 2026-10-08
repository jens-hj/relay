# Labelism interface plan

Tracked in [#9](https://github.com/jens-hj/relay/issues/9). This summarizes the accepted plan (revision 3) for bringing the Labelism concept designs into the native Mosaic client. The HTML concept files are design references only and are not part of the repository.

## Scope

A restyle of the existing views, without changing behavior or contracts. Every view, navigation path, command, and buffer behavior is kept. `relay-core`, `relay-server`, dependencies, and the Mosaic pin are unchanged.

## Visual rules

- Square geometry: no corner radius. One-pixel rules frame controls and regions. Hairline dividers are decorative.
- Every mark is a control, a reading, or a boundary. Construction guides, registration marks, rulers, coordinate letters, and ornamental hatching are not used.
- Reddit Sans for titles and subtitles; Zed Mono for all body text. Text is at least 11 logical px.
- Pastels are fills only. Text uses text-grade colors with at least 4.5:1 contrast on every neutral background it can appear on. Text on a pastel fill uses that fill's paired color.
- Mint means running only. Peach means a request is waiting for the user. Queued and completed work is neutral. Every status glyph comes with a word.
- Identifier blocks (issue numbers, session and profile blocks) use an inverse fill.
- Single-choice controls use a sliding square indicator.

## Themes and settings

- **Palettes:** Paper, Warm, Slate, and Neutral are retuned to soft off-white and off-black colors. Each mode adds a High contrast palette, which is the concept's stark ink-and-orange variant. Dark remains the default mode.
- **Preserved preferences:** mode, palette family, scale, sidebar width, and selected boards.
- **New fields:** `light_high_contrast` and `dark_high_contrast` are written only when true.
- **Unreadable or newer files:** a settings file that cannot be read, or that has unknown fields, suspends saving instead of being overwritten.
- **Recovery:** the user can retry reading, or back up and save. A backup is a verified byte-for-byte copy that never replaces an existing file. Any failure keeps the original and stays suspended.

## Truthful readouts

- **Issue status:** pending approval, then running, then queued, then the outcome of the most recently created worker session. Sessions have no timestamps, so "most recent" means creation order.
- **Usage:** the latest reported turn only. Both adapters report input including cached tokens. The meter shows uncached input, cached input, and output. The numbers show raw input (including cached), cached (a subset of input), and output.
- **Not shown:** mock-only elements such as the turn timeline, timestamps, turn numbers, elapsed time, Compact/Reset, unsaved-change counters, per-file line bars, and permission locks.
- **Permissions:** every action's permission stays editable. The profile caption states only what the server enforces: an Implement Deny blocks worker turns, and an Implement Ask requires approval each turn.

## Fidelity refinement

The second pass matches the concept geometry at the same viewport and follows the pinned Mosaic example conventions:

- **Structure:** grouped theme tokens (`ink`, `surface`, `rule`, `run`, `attention`, `tint`, `meter`, `status`), a separate icon scheme, one `#relay.*` style namespace (`styles.rs`) and focused view modules under `ui/`.
- **Chrome:** 56px page header with integrated cells, aligned with the 56px sidebar brand; 52px footers on both sides; framed modules with 30px caps heads.
- **Board:** 40px column heads on the header rule, full-height column rules, cards with a 22px tinted identifier strip, a three-line preview clipped by layout and a 26px foot; a 392px inspector that opens on its 74×92 identifier header with the worker setup anchored at the bottom.
- **Profile:** a framed 74px identifier header at the page padding, the inheritance strip, the action matrix and configuration modules, and a 50px save bar.
- **Session:** 74px header, 28px run strip, and a 92px author column beside content capped at 760px.
- **Text fitting:** labels keep their full text and fade at the edge, as in the text editor example; no width estimates.

## Deferred

- Keyboard grid addresses.
- Profile dirty counter.
- Per-line TOML origin highlighting.
- Per-file change bars.

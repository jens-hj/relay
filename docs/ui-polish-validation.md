# Screenshot polish validation

Tracking: [issue #10](https://github.com/jens-hj/relay/issues/10).

This pass implements the 17 screenshot comments following the Labelism design review.

| Comments | Result |
| --- | --- |
| 1 | Square SVG icon buttons with centered content; sidebar controls use a shared alignment rule. |
| 2 | Sender names wrap in their gutter. Sender and message glyphs share the first-line baseline; empty disclosure branches no longer add a gap above the message. |
| 3 | Capacity squares removed from the sidebar; capacity remains in the director tooltip and profile. |
| 4 | Visible Draft heading removed; the message separator and Send action identify the unsent buffer. |
| 5 | Prompt authors display as You, with a tinted surface and coloured left rule. Recorded author data stays intact. Demo prompts also use You. |
| 6–7 | Session counts fill the card footer. Internal rules are clipped inside the card border, including the selected border. |
| 8 | Soft palette label fills have stronger lilac, blue, green and sand colours. Existing text and control contrast checks still apply. |
| 9, 16–17 | New task, board actions and Commands occupy full header cells with matching heights and contained focus outlines. |
| 10 | A direct connections icon opens each project's connections page. |
| 11 | Connection type uses the shared sliding segmented picker, with keyboard navigation and scrolling when space is limited. |
| 12 | The built-in scale stepper has styled full-height decrement, value and increment cells, with inherited typography that scales with the interface. |
| 13 | Sidebar settings module removed. Double-click its right edge to restore the automatic width; Reset sidebar width is also in the command palette. Both clear Mosaic's retained resize size. At constrained widths, connection state uses a centred status dot and tooltip instead of wrapping the Server label. |
| 14 | Theme mode, light palette, dark palette and scale each have a reset icon, disabled at their default. Persistence stores only overrides. |
| 15 | Agent marks use status/accent colours. Running marks rotate over 2.4 seconds; waiting, queued and inactive marks stay still. A director's mark represents its own run, independently of worker capacity. |

## Settings compatibility

New local settings use `version = 2`. Missing fields follow application defaults. A file at all defaults contains only its version; selected board choices remain independent of appearance resets. Legacy settings still load, and are written in the sparse format when saved. Unsupported versions and malformed files retain the existing backup/recovery flow. Writes remain atomic.

## Mosaic conventions and lifetime handling

The UI uses shared named styles, SVG theme tokens, the existing sliding picker, Mosaic's built-in stepper and resizable sidebar. Palette changes do not rebuild the conversation buffer. No dependency or framework pin changes are included.

Native navigation reproduced the pinned Mosaic issue already recorded in the Labelism validation: ambient effects can outlive elements removed from rebuilt branches. Radio label/checked updates now share an element-owned semantic binding; focus registrations, custom animation controllers and scale synchronization dispose with their elements through Mosaic's existing removal hook. Navigation and removal regressions cover these lifetimes.

## Verification

All checks run through this project's Nix development environment. The desktop regressions cover actual glyph baselines, square icon centering, header-cell geometry and resolved fill, full-height footer cells, independent sparse resets, sidebar edge double-click, 200% narrow-window keyboard access, animation start/stop/removal, and navigating away from settings before preferences change.

Native captures use an isolated server/database/settings file and Xvfb display `:101`, with software Vulkan. Capture-only fake Codex runs supply activity state; they invoke no real model and make no provider writes. Temporary capture files are under `/tmp/relay-polish-capture/final`. Only the task-owned processes are stopped after review.

Final `just check` completed with `RELAY_CHECK_EXIT=0`: **229 tests** (desktop 104, server 109, core 6, HTTP 10), strict all-target Clippy, Rust/Mosaic/just/Nix formatting, and `nix flake check --no-build` on x86_64 Linux. Native review covered default Slate, Warm light, 760px, and high-contrast dark at 200%, including navigation through settings, sidebar-width reset via the palette, and scale/palette reset icons.

Saved native frames:

- [Conversation](assets/ui-polish/slate-session.png)
- [Board](assets/ui-polish/warm-board.png)
- [Connection picker](assets/ui-polish/connection-picker.png)
- [Settings](assets/ui-polish/slate-settings.png)
- [200% settings](assets/ui-polish/contrast-200-settings-controls.png)

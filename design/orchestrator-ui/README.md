# Orchestrator UI — design handoff

Everything an implementer needs to build the orchestrator's agent tree, conversation screen and tool calls in the agreed design language.

Start with `guide/design-guide.md`. It explains the principles, how the parts fit together and why each mark exists. Then open the boards in a browser to see every component and state at full size.

## Contents

| Path | What it is |
| --- | --- |
| `guide/design-guide.md` | The design guide: principles, foundations, orchestration model, conversation, context, usage, tool calls, build checklist |
| `boards/01-agent-tree.html` | Sidebar tree: whole tree, focused branch, status legend, row anatomy |
| `boards/02-conversation.html` | Full chat screen plus specimens: ledger composer (open, collapsed, max effort), usage tracker (resting and opened), context gauge (states and panel), fingerprint strip |
| `boards/03-tool-calls.html` | All 13 tool calls in collapsed, summary and expanded states |
| `screenshots/*.png` | Full-page renders of the three boards (system fonts; the HTML loads the real fonts) |
| `diagrams/approval-forwarding.svg` | How an approval travels from a sub-agent up to you and back (PNG alongside) |
| `tokens.json` | Colours, thresholds, type, shape, dither and motion values |

## Notes for implementation

- The boards are static HTML/CSS/SVG references, not production code. Hover works on the fingerprint strip; other interactions (opening tool calls, collapsing the ledger, expanding usage) are drawn as separate states.
- Fonts load from Google Fonts: Geist Mono and Instrument Serif.
- Dither is an SVG filter (fractal noise thresholded against a gradient's alpha, flooded with one colour). The filter definitions are inside each board; copy them rather than approximating with images.
- Where boards differ, the conversation board is canonical. The agent-tree board predates model colours, so its effort squares are violet; build them in the model's colour as the conversation board and guide show.
- Every number, file name, command and agent name in the boards is example data.
- Model colours, tool colours and status colours are separate families and must not be mixed (see the guide's Colour section).

## Open question

Permissions are assumed to only narrow as they pass down the tree (a child never gets more than its parent). Confirm before implementing the Spawn permission model.

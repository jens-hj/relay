# Commands and skills validation

Tracked by [issue #22](https://github.com/jens-hj/relay/issues/22).

The composer completes `/commands` and `$skills`, with pointer selection, arrow keys, Tab/Enter, and Escape. Skill references preserve multipart order and Unicode text, persist in shared drafts and submissions, and are revalidated on the server. Unknown dollar expressions remain text; escaped references, code, and quoted lines do not invoke skills. Claude translates a leading skill into its native slash invocation and embedded skills into explicit Skill tool calls whose invocation is checked. Codex uses native skill input items.

Model choices use discovered capabilities and lead to effort/default/fast choices. Selections affect the current session's next turn and survive reconnects. Planning uses native planning mode independently of execution permissions. Native compact runs preserve history identity. Clear and resume retain scoped conversation identities; fork creates native history and separate repository worktrees, copying staged changes, unstaged changes, and untracked files. Ordinary directory connections remain shared and concurrent turns using the same workspace are rejected.

Subprocess fixtures exercise both harness protocols, discovery without model turns, commands, skill dispatch, settings, compaction events, native forks, and repository isolation. HTTP tests cover catalog authentication, selection persistence, exact retries, and reconnects. Catalog discovery also runs for a director’s first-message draft. Commands requiring an existing conversation show that requirement. Delayed catalogs recompute suggestions in the focused editor without another keystroke; first-turn skill references are checked before creating a session. Headless Mosaic tests cover delayed discovery, first-message command suggestions, visible suggestion bounds, autocomplete, stable skill insertion, multi-step model selection, Escape, and IME input.

`just check` passed (workspace tests, Clippy with warnings denied, Rust/Mosaic/just/Nix formatting, and flake evaluation). Final focused server tests additionally passed staged-index copying, interrupted-fork cleanup, and rejected-skill draft retention. The WASM desktop also passed Clippy with warnings denied.

Explicit installed-harness smoke tests passed with Codex 0.161.0 and Claude Code 2.1.291 in temporary fixture repositories: initial turn, skill discovery, `$relay-command-check`, native compaction, and native history fork into independent worktrees. They use authenticated model turns and remain opt-in:

```sh
nix develop --command cargo test --locked -p relay-server installed_codex_command_smoke -- --ignored
nix develop --command cargo test --locked -p relay-server installed_claude_command_smoke -- --ignored
```

The known interactive commands are catalogued with reasons when the installed harness does not expose them through its headless protocol. Newly reported Claude commands and skills are retained automatically. Codex provides model and skill catalogs but no general CLI command discovery method, so terminal command names have a maintained fallback catalog. MCP displays reported connection status; its terminal configuration and authentication editors are not recreated. `/usage` and `/context` display recorded session measurements rather than account-wide billing or a full context breakdown. Codex compaction does not accept focus instructions through this protocol. Resume selects history retained by this Relay session, not arbitrary harness sessions from other projects.

Native command protocols were checked against the [Codex app-server documentation](https://learn.chatgpt.com/docs/app-server) and [Claude command documentation](https://code.claude.com/docs/en/commands). Actual process validation was performed on Linux. Platform-specific terminal commands remain unavailable where their native controls are absent.

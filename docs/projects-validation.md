# Project workflows validation

Tracking: [issue #8](https://github.com/jens-hj/relay/issues/8). Verified on Linux, 8 October 2026, using the repository's Nix environment and unchanged Mosaic pin `658ccfa168fcdccea790b81d591ff263e555ce61`.

## Automated checks

`nix develop --command just check` passed: 6 core tests, 63 desktop tests, 107 server tests, and 10 HTTP/workspace integration tests. Strict all-target Clippy, Rust/Mosaic/just/Nix formatting and the Linux flake evaluation passed. `cargo build --locked --workspace` also passed.

Coverage includes schema migration and durable command receipts; independent named projects; root validation and automatic Git cloning; local tasks without fabricated provider references; removed/restored connections; protocol compatibility; provider pagination, permission failures and retained confirmed state; journaled publication and explicit edits/moves; URL-based recovery; publishing into an already-connected destination with retained task/session/scope identities; and preservation across database reopen. Duplicate provider records become redirects rather than deleting history. Pending writes remain gated after identity consolidation.

The reconciliation follow-up passed 109 server tests and 10 HTTP/workspace integration tests, strict server Clippy and all formatting checks. Command-handler regressions reject malformed or unrelated typed results, pending/column/unknown keys and replacement of confirmed results without changing the snapshot. The real CLI-subprocess recovery test now passes its known issue through the command handler and confirms no duplicate issue creation.

Subprocess tests exercised both harness adapters with two repository worktrees, the default project directory and an additional ordinary directory. Edits stayed out of canonical repository checkouts, reviews covered both repositories, and continuation reused the same thread and workspace set despite adding another connection.

## Native and live checks

The native Mosaic client ran under Xvfb with Mesa software Vulkan and an isolated database/settings file. Checks covered whole-row sidebar hover, sidebar resizing, project expansion, keyboard navigation, light/dark switching, native New Project creation, compact harness icons/statuses, segmented appearance controls, and the built-in scale stepper. Rendering was inspected at 100% and 200%, including an 820-pixel window. Narrow windows constrain the sidebar and use an icon for the command palette.

The installed Codex 0.161.0 and Claude Code 2.1.291 each completed an actual local-task turn under Automatic execution using their existing login and configured model. Each recorded its thread and selected workspace and wrote the requested file with exact contents. The smoke server shut down afterward. No execution bypass was used.

A separate read-only connection to Relay's actual GitHub project (owner `jens-hj`, project 5) completed through the provider dispatcher, imported the four current columns and two issue memberships, and returned its connection to Ready. No remote board or issue was changed by this check.

## Limits

Provider publication and mutation paths were tested against real subprocess CLI stubs, not by writing to live GitHub/GitLab boards. Live GitLab access was not verified. New GitLab boards currently offer Open/Closed mappings; existing boards additionally support label lists. Unsupported filters and list types return an explicit error and retain the last confirmed board. API creation does not offer exactly-once guarantees: an uncertain result requires checking a known remote URL before continuation.

macOS/Windows native rendering and process cleanup were not tested in this pass. Update server and desktop together: this milestone uses schema 5 and protocol 2. Existing workspace data, checkouts, sessions and exact harness threads remain retained. Autonomous delegation and predictive cache warnings remain later milestones. Context lifecycle controls were subsequently implemented; see [command validation](commands-validation.md).

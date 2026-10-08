# Relay's first live workflow

Validation on 2026-10-07 used the pinned Nix development environment, Linux, Xvfb, and Mesa software Vulkan. The source repository was Relay itself, connected to [GitHub Project 5](https://github.com/users/jens-hj/projects/5). Implementation is tracked in [issue #2](https://github.com/jens-hj/relay/issues/2); the small live task is [issue #3](https://github.com/jens-hj/relay/issues/3).

## Independent verification

- The authenticated live sync imported the board's Status options and issues #2/#3, while preserving the separate demo project. Local issue IDs include the board identity.
- Started an issue #3 worker through the native UI, using the unchanged default director and explicit per-turn implementation approval. Relay created a separate worktree and branch from the committed source checkout.
- Closed and reopened the desktop during a real Codex turn. The server continued with the same session, thread, and worktree.
- Continued that exact thread through the UI to create `docs/review-checklist.md` and add its development-guide link. Inspected both actual files and the server's diff, including the untracked new file. No unrelated edits or truncation were present. The parent reviewed and integrated the uncommitted patch.
- Completed turns reported actual input, cached input, and output token counts. Unknown/incomplete usage remains unavailable; cache expiry is not inferred.
- Used the native Stop action during a real `sleep 90` tool command. The Codex process and its observed sandbox/code-mode/sleep descendants terminated; the session became Stopped and retained its changes.
- Killed the validation server during another real turn, then restarted with the same database. The observed Codex descendants terminated. The session became Interrupted with its original thread/worktree and immutable messages intact. Retrying the identical accepted request did not relaunch it. Continuation required a fresh explicit command.
- `just check` passed with 61 tests (2 core, 20 desktop, 33 server unit, 6 server HTTP), strict Clippy, Rust/Mosaic/just/Nix formatting, and flake evaluation. The disclosure refinement received a further desktop verification pass; the updated Codex pin received formatting and Nix checks.

The first live attempt exposed an obsolete default model in the flake's older Codex CLI. Its failed session and worktree were preserved. The environment now pins official Codex 0.161.0 with hash-verified bundled resources; a real invocation from that Nix shell succeeded before repeating the workflow. Relay still honors existing server-side model/provider configuration.

## Limits at the first milestone

The validation above covered GitHub Projects v2 Status boards and Codex. See [local harness validation](harness-validation.md) for the subsequent Claude Code integration. GitLab, Claude Code, autonomous director delegation, and compact/reset controls remain later work. Responsibilities and completion settings describe workflow intent; only the documented launch policy and Codex sandbox are enforced. Completed turns still require independent acceptance, and Relay exposes no merge/push/deploy or remote status mutation.

Linux process cleanup was tested against both fixtures and the actual Codex sandbox. Deliberately escaping descendants are not contained. macOS and Windows native behavior and crash cleanup have not been validated. Transcript support covers agent/reasoning messages, command results, and file changes; other event types may be ignored. Token counts describe the latest completed turn, not total session billing or predicted future cache state.

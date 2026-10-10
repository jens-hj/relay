# Browser and conversation integration validation

The browser client, owner authentication, Nix packages and service modules are tracked by the existing [browser access plan](remote-access-plan.md) and its issues #12–#18. Native window chrome is tracked in [issue #11](https://github.com/jens-hj/relay/issues/11). This integration also includes the pending first-director editor, image-paste, activity-indicator, transcript-following and Codex child-thread isolation changes, together with the streaming-limit fix.

On 2026-10-10, the combined source passed:

- `nix develop --command just check`: 6 domain tests, 129 desktop tests, 122 server tests, 1 executable runtime test and 10 server integration tests; workspace Clippy with warnings denied; required formatting checks; and Nix evaluation on x86_64-linux.
- `nix develop .#web --command cargo clippy --locked -p relay-desktop --target wasm32-unknown-unknown -- -D warnings`.
- The Chromium clipboard fixture in `crates/relay-desktop/tests/browser-clipboard.mjs`, covering an actual clipboard image paste, file-item fallback, text paste and focus gating.
- Nix builds of `relay-server` and `relay-web`.

The desktop tests check that the activity indicator's outline and half fill rotate around the same center, that the animation stops and restarts correctly, and that streaming follows the bottom while preserving a manually selected reading position. The first-director tests cover inline images, editor shortcuts and retaining later edits after acknowledgement. The server process test verifies ordered text/image input and exact retry behavior for the first director turn.

The queue-promotion fixture now waits for Relay to publish the fake harness's first item before expecting an interrupt with the recorded turn ID. A file written by the fake harness only proved that it emitted its response, allowing promotion to race Relay's read.

These checks do not establish physical-device clipboard or Safari acceptance. Deployment must additionally verify the running executable and served browser assets against packages built from the published main revision; a successful process restart alone is insufficient.

The follow-up for [issue #20](https://github.com/jens-hj/relay/issues/20) releases confirmed command rejections so they cannot block messages in every session. A rejected edit of a message that has already launched retains its draft and clears the obsolete queue target. Uncertain outcomes retain exact-envelope retries, and revision conflicts retain explicit review. The recovery regression verifies that the retained draft can be submitted with a fresh request identity.

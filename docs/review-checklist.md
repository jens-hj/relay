# Review a Relay worker turn

Use this checklist before integrating a completed worker turn.

- Open the linked source GitHub issue and compare the result with its
  acceptance criteria. For this checklist, see
  [issue #3](https://github.com/jens-hj/relay/issues/3).
  Completed means the Codex turn ended; it does not mean the issue's
  requirements have been independently accepted.
- Confirm the session's branch, base commit, and assigned worktree.
  Inspect every changed file and the diff, including untracked files.
  If Relay reports truncation, inspect the full changes in the worktree
  before deciding.
- Check the worker's validation evidence: commands actually run,
  outcomes, and remaining limitations. Codex finishing is not evidence
  that tests passed. Investigate unexpected or unrelated edits.
- If more work is needed, explicitly continue the existing session with
  concrete feedback; Relay resumes its recorded thread in the same
  worktree. To intentionally end an active run, use Stop and review
  any partial changes.
- Closing or disconnecting a client leaves the server-owned worker
  running; reconnect to inspect it. Stopping the server interrupts active
  runs. After restart, inspect their recorded state and explicitly
  continue any interrupted work.
- Leave merge, push, and deploy to human integration in this milestone.
  A completed turn does not automatically change GitHub board status.

# Schema 21 to 24 Migration and Recovery Plan

Status: **proposed, not executed, not independently reviewed or accepted.** Nothing in this document has been run against a live installation, a live database or a copy of one. Every command below is a proposal for the workspace owner. The [evidence](#evidence) section lists what was actually checked.

This is the separately reviewed migration and recovery plan that [deployment](deployment.md#preparation-and-recovery) requires for a version-changing release. Deploying it needs explicit owner authorization for the merge to `main` and for the deployment. This plan grants neither.

## Release

| Item | Value |
| --- | --- |
| Base | `ff326a7` (schema 21, protocol 14) |
| Changes | Schema 22 stores `SpawnGrant::selection_rationale`; schema 23 stores `Project::nix_commands`; schema 24 stores `Snapshot::claude_window_start` and adds the `window_start_claims` table ([Claude window start](claude-window-start.md)) |
| Protocol | 14, unchanged |
| Revision to deploy | The full commit hash of the published `main` commit that contains these changes, not a worktree commit |

How the migration behaves, from `crates/relay-server/src/lib.rs` (`Store::open`):

- **Automatic at startup.** The migration runs when the candidate server first opens the database. There is no separate migration command.
- **One transaction.** Everything runs in a single `BEGIN IMMEDIATE` … `COMMIT` transaction: the empty `window_start_claims` table is created, the stored snapshot is rewritten with defaults, `protocol_version` is set to 14 and `user_version` to 24. A crash before `COMMIT` leaves the schema 21 database intact.
- **Spawn grants.** Existing spawn grants load without a selection rationale. The field is omitted while absent.
- **Nix commands.** Every project loads with Nix Commands set to Off. The field is omitted from the snapshot while Off. The migration never turns it on; owners turn it on per project on the Connections page, which is a separate decision.
- **Claude window start.** The workspace loads with Claude Window Start Off and the claims table empty. The field is omitted from the snapshot until the owner changes it. While it is off the server sends no Claude message and runs no Claude Code check for it. The migration never turns it on; turning it on in Settings is a separate owner decision that authorizes background Claude usage, and this release does not include it.
- **Older servers.** A schema 21 server refuses any database newer than its schema ("Database schema is newer than this Relay server"). After migration, going back to the previous binary therefore **requires restoring the pre-migration database**.

## Helper Support

Before `previous_schema_version`, `nix/deploy-relay.py` took one `schema_version` and checked it both before and after activation, so it could not run a release that changes the schema:

- **`schema_version: 24`.** It refused safely with `pre-schema` and changed nothing.
- **`schema_version: 21`.** The candidate migrated the database to 24, then `local()` failed with `schema-protocol`. Because the schema no longer equalled 21, `failure()` did not switch back. The attempt ended `failed` with the candidate active on a migrated database and the public service not started.

The helper now reads an optional manifest field `previous_schema_version`:

1. **Two schema fields.** When `previous_schema_version` is missing it defaults to `schema_version`, so same-schema releases behave as before. Both must be non-negative integers with `previous_schema_version <= schema_version`; otherwise the helper refuses the manifest before reading the database.
2. **Before activation.** `activate` compares the live schema with `previous_schema_version` (`pre-schema`).
3. **After activation.** `local()` compares with `schema_version` (`schema-protocol`).
4. **Automatic restore.** `failure()` switches back to the previous bundle only while the schema still equals `previous_schema_version`, for example when the candidate failed before it migrated. Once the database is migrated, `failure()` records the failure and leaves recovery to the owner.
5. **Tests.** `nix/test_deploy_relay.py` covers manifest validation, the check before activation against `previous_schema_version`, the check after activation against `schema_version`, a full activation that migrates a real SQLite database from 21 to 24, no automatic switch-back once the schema has changed, and unchanged behaviour when the field is absent. Run them with `nix develop --command just deploy-check`.

**Do not work around these checks.** That means no:

- editing `PRAGMA user_version`,
- patching the checks for one attempt,
- reusing an attempt's state directory,
- starting a second server on the live database.

Without the helper, the only alternative is an owner-performed manual activation (see [Activation](#activation)). It loses the helper's lock, its durable status and its checks after activation, so it is not recommended.

## Prerequisites

- **Authorization.** The owner has explicitly authorized the merge to `main` and this deployment.
- **Checks.** `nix develop --command just check` passes on the published commit. As of this plan it has **not** been run (see [evidence](#evidence)).
- **Helper.** The published commit contains the [helper support](#helper-support), `nix develop --command just deploy-check` passes on it, and the manifest uses `previous_schema_version: 21` and `schema_version: 24`.
- **Matching builds.** Build the server and browser packages from the same immutable commit. Proposed, not run:

  ```sh
  nix build "git+file:///path/to/relay?rev=REVISION#relay-server" --out-link "$R/candidate-server"
  nix build "git+file:///path/to/relay?rev=REVISION#relay-web" --out-link "$R/candidate-web"
  ```

  Assemble the candidate unit bundle the way the installation's existing bundles are built. Keep the previous bundle and the previous server package as Nix GC roots until the release is accepted, because rollback needs exactly those binaries.
- **Protected state.** Create a new state directory and manifest for each attempt, outside Git: directory mode `0700`, manifest mode `0600`.
  - `credential_file` must be an existing token file with mode `0600` inside a `0700` directory. The manifest holds its path, never its value.
  - Never `cat`, `echo` or interpolate a token into a command line or a log.
- **Live schema.** Confirm, read-only, that the live database is at schema 21 and passes its integrity check. Proposed, not run:

  ```sh
  nix develop .#web --command python3 -I -c 'import sqlite3,sys; d=sqlite3.connect("file:"+sys.argv[1]+"?mode=ro",uri=True); print(d.execute("PRAGMA quick_check").fetchone()[0], d.execute("PRAGMA user_version").fetchone()[0])' "$DB"
  ```

  If the result is anything other than `ok 21`, stop. This plan covers only 21 to 24.

`DB` is the manifest's `database` (default `~/.local/share/relay/workspace.sqlite3`). `R` is a protected rehearsal directory, created with `install -d -m 0700 "$R"` outside every Git checkout.

## Backups

Back up with SQLite's backup API, never by copying the database file alone. In WAL mode, committed changes can still be in `workspace.sqlite3-wal`, and a plain copy of the main file can miss them or be inconsistent. The backup API reads one consistent snapshot, including committed WAL frames.

- **Helper backups.** The helper already does this. `activate` writes `database-backup.sqlite3` into the attempt's state directory, under `umask 077`: once while the server runs, and again after `relay.service` stops. The second copy is the authoritative pre-migration backup.
- **Manual backups** use the same API. Proposed, not run:

  ```sh
  nix develop .#web --command python3 -I -c 'import sqlite3,sys; s=sqlite3.connect(sys.argv[1]); d=sqlite3.connect(sys.argv[2]); s.backup(d); d.close(); s.close()' "$DB" "$R/pre-migration.sqlite3"
  chmod 0600 "$R/pre-migration.sqlite3"
  ```

- **Retention.** Keep the pre-migration backup, the previous bundle and the previous server package until the release is accepted. Backups contain user data: keep them mode `0600` in `0700` directories outside Git, and never attach or upload them.
- **What backups cover.** The database is the only state the migration changes. Project repositories and worktrees, connected directories, credential files and other units are neither backed up nor restored by this plan. They must not be touched.

## Rehearsal on a Copy

Rehearse against a copy before any live activation. The rehearsal must never open the live database, use the live port or use the administrator token.

1. **Copy.** Copy the live database into `$R/copy.sqlite3` with the backup API, as above. Keep an untouched second copy, `$R/before.sqlite3`, for comparison.
2. **Throwaway token.** Create a token file without printing it. Proposed, not run:

   ```sh
   ( umask 077; head -c 32 /dev/urandom | base64 -w0 > "$R/token" )
   ```

   A different token revokes browser sessions in the copy only. That is expected and does not affect the live database.
3. **Run the candidate.** Start the candidate server against the copy, isolated from the live installation: loopback on a spare port, a scratch `HOME`, and no `RELAY_PUBLIC_ORIGIN`, setup token or `RELAY_GITHUB_*` variables. Proposed, not run:

   ```sh
   env -i HOME="$R/home" PATH="$PATH" RELAY_DATABASE="$R/copy.sqlite3" \
     RELAY_TOKEN_FILE="$R/token" RELAY_BIND=127.0.0.1:7391 \
     "$R/candidate-server/bin/relay-server"
   ```

   Wait for "Relay server listening", then stop it with Ctrl-C.
   - The copy's snapshot still points at the real project repositories, so send no commands, connect no client and resume no session.
   - Claude Window Start loads Off in the copy, so the candidate sends no Claude message. Do not turn it on.
   - The checks below read only the database files.
4. **Check the copy.** Run a check script from a separate directory (for example `$R/scripts/check-copy.py`). Proposed, not run:

   ```python
   # Usage: python3 -I check-copy.py BEFORE.sqlite3 AFTER.sqlite3
   import json, sqlite3, sys

   def load(path):
       db = sqlite3.connect('file:' + path + '?mode=ro', uri=True)
       assert db.execute('PRAGMA integrity_check').fetchone()[0] == 'ok', path
       version = db.execute('PRAGMA user_version').fetchone()[0]
       snapshot = json.loads(db.execute('SELECT snapshot FROM workspace WHERE id=1').fetchone()[0])
       rows = {t: db.execute(f'SELECT * FROM {t} ORDER BY 1').fetchall() for t in ('drafts', 'receipts', 'assets')}
       tables = {r[0] for r in db.execute("SELECT name FROM sqlite_master WHERE type='table'")}
       claims = db.execute('SELECT COUNT(*) FROM window_start_claims').fetchone()[0] if 'window_start_claims' in tables else None
       return version, snapshot, rows, claims

   v0, s0, r0, c0 = load(sys.argv[1])
   v1, s1, r1, c1 = load(sys.argv[2])
   assert (v0, v1) == (21, 24), (v0, v1)
   assert (c0, c1) == (None, 0), (c0, c1)  # the claims table is new and empty
   assert s1['protocol_version'] == 14
   assert 'claude_window_start' not in s1  # Off is omitted
   for key in ('projects', 'sessions', 'agent_configs', 'issues', 'messages', 'connections', 'boards'):
       assert {v['id'] for v in s0.get(key, [])} <= {v['id'] for v in s1.get(key, [])}, key
   assert r0 == r1, 'drafts, receipts or assets changed'
   before = {p['id']: p for p in s0['projects']}
   for project in s1['projects']:
       assert 'nix_commands' not in project, project['id']  # Off is omitted
       assert project['defaults'].get('worker_models') == before[project['id']]['defaults'].get('worker_models')
   for session in s1['sessions']:
       assert 'selection_rationale' not in (session.get('spawn') or {}), session['id']
   print('ok')
   ```

   Run it with `nix develop .#web --command python3 -I "$R/scripts/check-copy.py" "$R/before.sqlite3" "$R/copy.sqlite3"`. Any failure stops the release until it is understood.
5. **Old binary refuses the migrated copy.** Start the previous server package against a further copy of the migrated database, the same way.
   - Expect it to exit with "Database schema is newer than this Relay server".
   - Check that the copy still reads `user_version` 24.
6. **Rollback works.** Restore `$R/before.sqlite3` to a fresh copy with the backup API and start the previous server package on it. Expect it to open at schema 21.
7. **Clean up.** Stop every rehearsal server, then delete or keep the rehearsal files as the owner decides. Never commit or upload them.

## Activation

**With the helper** (recommended), follow [deployment](deployment.md#run-the-workflow).

1. **Manifest.** Use `previous_schema_version: 21`, `schema_version: 24`, `protocol_version: 14`, the full `revision`, the candidate and previous bundles, and `server_executable` and `web_root` from the same build.
2. **Activate.** Run `activate` once, from an independent user job. Proposed, not run:

   ```sh
   systemd-run --user --unit=relay-deploy-RELEASE --property=Type=exec \
     /absolute/python3 /protected/release/deploy-relay.py activate \
     --manifest /protected/release/manifest.json
   ```

3. **Read the outcome.** Use `just deploy status --manifest …` and the recorded phase. Do not treat the job's exit or a restart as success.

How to handle each helper result:

| Result | Action |
| --- | --- |
| Refused before stopping the service: `pre-schema`, `pre-protocol`, `identical-bundles`, `previous-bundle`, `frontend-revision`, `unit-package`, `wrapper-target`, `attempt-already-started`, `permissions`, `file-type`, `checkout-path`, `configuration-or-lock` | Nothing has changed. Fix the input and start a new attempt with a new state directory and manifest. Never edit the database, its version or the checks. |
| `failed` after activation: `schema-protocol`, `history`, `drafts`, `local-revision`, `executable`, `local-readiness` | The database may be migrated. The helper must not switch back automatically. Go to [Rollback](#rollback). |
| Exit 2, `public-verification-pending` | The candidate stays active and locally healthy. Retry `verify` only, never `activate`. |
| `deployed` | Continue with [Validation](#validation). |

**Without the helper**, the owner would repeat the helper's steps by hand: stop the services, back up with the backup API after the stop, switch the active link, `daemon-reload`, start the service, then run the checks in [Validation](#validation) and start the public service. This route has no activation lock, no durable status and no automatic local checks, so it needs the owner present for the whole release.

## Validation

After activation, check:

- **Local endpoint.** `/v1/version` reports the candidate revision; `/v1/snapshot` reports `protocol_version` 14; the database reads `user_version` 24 and passes `quick_check`.
- **Running executable.** The `MainPID` of `relay.service` runs the candidate `server_executable`.
- **Data preserved.** No project, session, agent configuration, issue, message, connection or board ID present before activation is missing. Draft revisions did not go backwards.
- **New fields.** Every project's Nix Commands setting is Off, Claude Window Start is Off with an empty `window_start_claims` table, and no existing spawn grant has gained a rationale.
- **Public endpoint.** Public HTTPS shows the login gate, reports the candidate revision and serves browser assets whose hashes match `web_root`.

The helper's `local()` and `verify` already perform these checks except the new-field checks. After a successful activation the owner can read those from the stored snapshot with the read-only check above. Credentials are read only by the helper, inside its requests.

## Rollback

Use this when the candidate fails after migration and the owner decides to go back. **Never run the previous binary against a migrated database, and never restore data automatically.**

1. **Stop the services.** Stop `relay-funnel.service` and `relay.service`, and confirm `MainPID` is 0.
2. **Optional export.** If the migrated server was serving, the owner may first export its snapshot into the protected state directory, to recover work recorded after activation. That work is lost by the restore below.
3. **Move the migrated files aside together.** Move `workspace.sqlite3`, `workspace.sqlite3-wal` and `workspace.sqlite3-shm` into the protected state directory as one set; move them, do not delete them. **Never leave the migrated database's `-wal` or `-shm` beside a restored database:** SQLite would replay the migrated WAL onto it.
4. **Check the backup.** Confirm, read-only, that the pre-migration backup (`database-backup.sqlite3` from the attempt's state directory) passes `integrity_check` and reads `user_version` 21.
5. **Restore it.** Restore the backup to the `database` path with the backup API and set its mode to `0600`.
6. **Switch back.** Point the active link back at the previous bundle with an atomic symlink replacement, as the helper does, then run `daemon-reload` and start `relay.service`.
7. **Check the restored service.**
   - `/v1/version` reports the previous revision.
   - The database reads schema 21.
   - `MainPID` runs the previous server executable.
8. **Public checks.** Start `relay-funnel.service` and repeat the public checks.

Rollback restores only the database:

- Files that agents changed in project repositories, worktrees or connected directories after activation stay as they are. Review them by hand. Do not reset or delete them.
- Credential files, other units and unrelated persistent data are not touched.
- Interrupted agent turns stay interrupted. Rollback does not replay them.
- The restored database has no Claude Window Start setting or claims. Because the setting stays Off in this release, no claim is lost.

## Evidence

Actually done for this plan:

- **Static reading** of `nix/deploy-relay.py` (`activate`, `local`, `failure`, `schema` and the backup calls), `Store::open` in `crates/relay-server/src/lib.rs`, `crates/relay-server/src/main.rs`, `crates/relay-server/src/browser.rs` and `nix/service-options.nix`.
- **Code checks:**
  - The protocol is 14 both at `ff326a7` and after these changes.
  - Both new fields use `#[serde(default, skip_serializing_if = …)]`.

Not done:

- **Deployment.** No deployment or service change of any kind.
- **Live data.** No backup or read of a live database.
- **Rehearsal.** No rehearsal.
- **Tests and builds.** No Rust tests, Clippy, formatting checks, builds or `just deploy-check`. In the sandbox where these changes were integrated, Nix cannot reach the daemon. That includes the schema tests `schema_twenty_one_upgrades_to_twenty_four_with_every_new_setting_off`, `schema_twenty_three_keeps_spawn_rationales_and_loads_nix_commands_off` and `schema_twenty_three_upgrades_with_claude_window_start_off_and_an_empty_claim_table`, none of which has run. The Claude window-start source has not been compiled either; see [its validation](claude-window-start.md#validation).

Done for the helper support only:

- **Helper tests outside the project shell.** `python3 -B -I -m unittest discover -s nix -p test_deploy_relay.py` passed (23 tests) with a Python 3.14 interpreter from the Nix store, not the `.#web` shell. This does not replace `just deploy-check`.
- **Test sensitivity.** With either schema comparison reverted to `schema_version`, the new tests fail.

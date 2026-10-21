# Deployment

Deployment runs independently of the Relay service being replaced. Stopping that service can stop the deploying agent, so an agent's last message or a successful `systemctl restart` is not deployment confirmation. The deployment's durable status is authoritative.

## Health and Verification

Check the candidate locally before treating a public-network failure as an application failure. Public HTTPS reaches Relay through additional DNS, TLS, and proxy infrastructure; an EOF or timeout there does not establish that the candidate server is broken.

| Result | Deployment Decision |
| --- | --- |
| Local startup, revision, artifact, or data-preservation check fails | Report failure and attempt rollback only when ownership and schema checks permit it. |
| Local checks pass, but public DNS, TLS, connection, rate-limit, or gateway checks fail | Keep the healthy candidate active and report public verification pending. Retry verification without restarting. |
| Public revision, asset hash, or authentication checks fail | Report a verification failure; never claim deployment success. |
| Local and public checks pass | Record the deployment as verified, including the running revision. |

Public checks retain certificate verification and compare the served browser assets with the candidate package. Retrying a TLS connection must not silently disable authentication, accept a mismatched asset, or treat an HTTP authorization failure as success.

Public HTTP `408`, `429`, `502`, `503`, and `504` responses are retryable transport or availability failures. Authorization failures and application errors such as `401`, `403`, and `500` remain hard failures. A public failure is never reported as verified deployment success.

A truncated HTTP response body is a transport failure. Discard the partial body and use the same certificate-verified curl fallback for public requests. If that also fails in transport, retain the healthy candidate with public verification pending; never accept partial asset bytes or restart it to retry verification. A truncated local response fails local readiness.

## Preparation and Recovery

This helper handles releases that preserve the installed protocol and schema, and releases whose candidate migrates the database schema, the stored protocol, or both at startup. A schema- or protocol-changing release needs a separately reviewed migration and recovery plan; the helper refuses mismatched pre-deployment versions. The schema 21 to 25 release, which includes [Claude window start](claude-window-start.md) and [worker execution approval](architecture.md#worker-execution-approval), has a [proposed plan](schema-25-migration.md).

Build the server and browser packages from the same immutable commit, and preserve the previous package bundle as a Nix GC root. Prepare a private deployment directory outside Git with mode `0700`; keep its manifest, status, logs, and backups owner-readable. Manifests contain credential file paths, never credential values. Credentials are read only inside authenticated requests and never placed in command-line arguments or logs.

Use SQLite's backup API for a consistent backup. Preserve the existing database, credential paths, origin, port, and harness environment. Activate only the Relay units; do not apply unrelated host configuration changes. Keep the previous bundle and backup until acceptance.

Use the same helper for managed deployments so its activation lock can serialize them. The status command does not take that exclusive lock and remains readable while slow probes are in progress. Repeating activation for an existing attempt is rejected without restarting or rolling it back.

A deployment must not roll back another deployment's active bundle. It also must not start an older server against a database whose schema is incompatible with that server. Such cases require explicit recovery, not an automatic database restore that could discard later user data. Restarted agent turns remain interrupted; deployment does not replay them.

## Run the Workflow

The helper takes a protected JSON manifest and provides three commands:

```sh
just deploy activate --manifest /protected/release/manifest.json
just deploy verify --manifest /protected/release/manifest.json
just deploy status --manifest /protected/release/manifest.json
```

`activate` is a service-changing operation requiring explicit authorization. Run it from an independent user systemd job, using a copy of the helper and an absolute Python executable outside the service's control group:

```sh
systemd-run --user --unit=relay-deploy-RELEASE --property=Type=exec \
  /absolute/python3 /protected/release/deploy-relay.py activate \
  --manifest /protected/release/manifest.json
```

Do not use `--scope`, `--wait`, `PartOf=relay.service`, or `BindsTo=relay.service`. Keep the helper and its manifest available until the job and any verification retries have finished. The direct `just deploy activate` form is suitable only when its caller will survive the restart.

Use `verify` to resume verification after a public transport failure. It does not reactivate the bundle or restart Relay. A separate verification job or timer can invoke this command again while public verification is pending. A process that automatically retries must retry **verification**, not **activation**.

Data preservation is checked immediately after activation, before the candidate is marked locally healthy. Later verification checks current health and package identity; it does not compare live user data against an old baseline and mistake subsequent user deletions for deployment damage.

Exit codes distinguish outcomes: `0` means verified success, `2` means public verification is pending with the candidate retained, and `1` means a hard failure. The `status` command reports durable state; its successful exit only means that the status was read. Check the recorded phase before announcing deployment success.

The manifest includes absolute paths and package identities, for example:

```json
{
  "active_link": "/home/alex/.local/share/relay/deployment",
  "candidate_bundle": "/nix/store/CANDIDATE-relay-units",
  "previous_bundle": "/nix/store/PREVIOUS-relay-units",
  "database": "/home/alex/.local/share/relay/workspace.sqlite3",
  "credential_file": "/home/alex/.config/relay/admin-token",
  "server_executable": "/nix/store/CANDIDATE-relay-server/bin/relay-server",
  "web_root": "/nix/store/CANDIDATE-relay-web",
  "revision": "FULL_GIT_COMMIT",
  "protocol_version": 11,
  "schema_version": 18,
  "local_url": "http://127.0.0.1:7331",
  "public_url": "https://relay.example.org",
  "state_dir": "/home/alex/.local/share/relay/deployments/RELEASE"
}
```

Replace the example values with the built packages and existing installation. Protocol and schema versions must match the candidate; the example numbers are not a migration policy. For a release that migrates the database, set `previous_schema_version` to the live schema and `schema_version` to the candidate's schema. `activate` checks the live database against `previous_schema_version`, the checks after activation use `schema_version`, and the previous bundle is restored automatically only while the database is still at `previous_schema_version`. Without the field, both checks use `schema_version`. Likewise, for a release that moves the protocol, set `previous_protocol_version` to the live protocol and `protocol_version` to the candidate's: `activate` checks the running server against the previous protocol and, after activation, the candidate against `protocol_version`. The helper defaults to the `relay.service` and `relay-funnel.service` user units; specify `service` and `public_service` when the installation uses different names. The manifest and its parent must have modes `0600` and `0700`, and live deployment state must stay outside the checkout. Do not include the credential itself in the manifest.

## Regression Checks

Run `nix develop --command just deploy-check` to exercise deployment decisions with isolated fixtures. These checks belong to `just check`; they do not restart production services or contact the public deployment.

The regression that matters for the reported incident is: activate a locally healthy candidate, fail public TLS verification, retain that candidate with a pending status, then complete verification successfully without another restart.

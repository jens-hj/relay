Browser owner authentication (issue #14)

Set `RELAY_PUBLIC_ORIGIN` to the exact external HTTPS origin, for example
`https://relay.example.org` (no trailing slash). The trusted reverse proxy must
preserve the browser Origin header and terminate HTTPS. Forwarded headers are not
used to decide authentication or the public origin. Without this setting cookie
authentication fails closed. Native bearer authentication remains protocol 3.

Set exactly one of `RELAY_TOKEN` and `RELAY_TOKEN_FILE`; the latter reads a private
runtime file and trims its surrounding whitespace. Neither the secret nor its
file path is inherited by server-launched harnesses. The server also removes
`RELAY_SETUP_TOKEN_FILE` and `CREDENTIALS_DIRECTORY` from child environments.

Enrollment is disabled by default. An administrator can run:

    relay-server --initialize-setup-code /private/relay-setup-code

This creates a new 0600 file containing a random 256-bit code; it refuses to
replace existing files and does not print the code. Configure
`RELAY_SETUP_TOKEN_FILE=/private/relay-setup-code`, start the server, and enter
that file's contents in `/setup`. The code expires ten minutes after first load;
restarting does not rearm the same code. To open another window before enrollment,
create a new code file and restart with that path. No owner can be replaced through
setup. Recovery codes are shown once after enrollment. `/recover` changes the
password using one saved code, consumes it, and revokes all existing sessions.

Changing the native token invalidates existing sessions and enrollment codes on
next startup, preserving the owner and recovery codes. Stop the old server before
rotation; concurrently running servers with different native tokens are unsupported.

`RELAY_WEB_ROOT` points to trusted, immutable packaged `index.html`, JS, WASM and
assets. Every `/app/` file requires authentication, including the JS and WASM
bundle. Use relative or `/app/` paths in the generated index.
Uploaded workspace assets remain authenticated and dangerous types download as
application/octet-stream. All responses use no-store and nosniff.

`GET /auth/session` returns `{authenticated:true,username,csrf_token}` for a live
browser session, otherwise 401. Browser writes to `/v1/` and `/auth/logout` require
`x-relay-protocol: 2`, exact Origin and `x-relay-csrf` from that session. Logout is
`POST /auth/logout`. WebSocket upgrades on both event endpoints require exact
Origin with cookie authentication. Sessions expire after seven idle days or thirty
absolute days. Logout, recovery and expiry close open sockets within 250 ms plus
bounded socket-close time. Native bearer requests need neither Origin nor CSRF;
an explicit invalid Authorization header never falls back to cookies.

HTML forms use a secure pre-session cookie and hidden CSRF field plus exact Origin.
There is one shared budget of twenty form attempts per fifteen minutes, bounding
both owner/account and global attempts regardless of supplied username. Errors do
not disclose whether a login username exists. Optional passkeys are deferred.

The response content security policy allows Mosaic's generated inline CSS and
runtime styles. Scripts must come from same-origin external files; WASM compilation
is allowed with wasm-unsafe-eval. Packaging must externalize Mosaic's generated
inline module bootstrap; inline scripts remain blocked.

Authentication pages include a mobile viewport and the public same-origin stylesheet
`/auth/style.css`, with readable inputs, touch-sized controls and wrapping recovery
codes. Enrollment rejects invalid/expired/closed setup codes before password hashing,
then rechecks eligibility and consumes the code in the owner-creation transaction.

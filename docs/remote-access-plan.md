# Browser access and Nix deployment

Tracked in [#12](https://github.com/jens-hj/relay/issues/12), with packaging [#13](https://github.com/jens-hj/relay/issues/13), authentication [#14](https://github.com/jens-hj/relay/issues/14), Mosaic WASM [#15](https://github.com/jens-hj/relay/issues/15), and the HTTPS pilot [#16](https://github.com/jens-hj/relay/issues/16).

Relay's existing Mosaic application has native and WebAssembly targets. Both clients connect to one persistent server, SQLite workspace, task-linked harness threads, drafts, and assets. Browser devices need no installed Relay application, VPN, SSH client, or Tailscale client. The app requires working WebGPU; login and recovery are ordinary HTML.

## Connection and authentication

The live gmk pilot URL is https://mosaic-preview-gmk1.tailc5ce7.ts.net:8443/.

The server binds only `127.0.0.1:7331`. Tailscale Funnel on gmk supplies public TLS and forwards HTTP/WebSockets. Port 8443 has a separate Relay route; Mosaic previews on port 443 remain private. See [Funnel](https://tailscale.com/docs/features/tailscale-funnel) for port/bandwidth constraints. The inherited preview node name does not require preview access or client-side Tailscale.

There is one owner and one trusted workspace, without public signup or separate team accounts. Log in with the owner username/password. Credentials and recovery codes for this installation are in a protected runtime file outside the repositories and Nix store. Never commit, embed in Nix expressions, or print that file.

Passwords use Argon2id. Browser sessions have a seven-day idle and thirty-day absolute limit, with a `Secure; HttpOnly; SameSite=Lax; Path=/` host-only cookie. SQLite stores session hashes; JavaScript receives only a session-bound CSRF token. Writes require that token, protocol 2, and the exact configured HTTPS Origin. Both event streams validate Origin and close on revocation/expiry.

Sign out revokes the current session. `/recover` consumes one saved recovery code, changes the password, and revokes all prior sessions. Recovery codes are shown once during enrollment. Device-management screens and passkeys are future work.

Every workspace API, attachment, event stream and `/app/` file—including JS and WASM—requires authentication. HTML entries redirect to login; anonymous API/bundle requests fail closed. Uploaded active types download as octet-stream with `nosniff`. Responses use `no-store`; the app does not cache workspace responses or replay execution commands offline. The package precompresses static app files with gzip, negotiated behind the same login gate; account and workspace responses are not compressed by this feature.

Native bearer authentication remains compatible. An invalid explicit bearer never falls back to a cookie. GitHub/GitLab, Codex and Claude authentication stay on the server and remain independent of Relay login. See the [server authentication guide](../crates/relay-server/BROWSER_AUTH.md) for enrollment, rotation and HTTP details.

## Nix interfaces

Use the repository flake for Cargo/just:

```sh
nix develop --command just check
nix develop .#web --command just build-web
nix build .#relay-server .#relay-web
```

| Output | Purpose |
| --- | --- |
| `packages.<system>.relay-server` | Persistent server executable |
| `packages.<system>.relay-web` | Mosaic WASM, JS, snippets and browser shell |
| `apps.<system>.relay-server` | Run the packaged server |
| `homeManagerModules.default` | User service, credentials and optional HTTPS route |
| `nixosModules.default` | System service with a dedicated or existing user |
| `devShells.web` | Pinned Rust, wasm32 target, Mosaic CLI and wasm-bindgen |
| `devShells.browser` | Chromium/Node for verification |

Cargo/Nix retain Mosaic commit `658ccfa168fcdccea790b81d591ff263e555ce61` and Rust 1.89. Web vendoring reuses the exact fetched Mosaic flake tree at the matching Cargo revision, preserving private SSH URLs without giving the build credentials.

The standalone server flake has public toolchain inputs and no Mosaic dependency:

```sh
nix build ./nix/server#relay-server
nix run ./nix/server#relay-server
```

It exports the same server/service modules. Its filtered workspace includes core/server and embedded profiles. Update its lock after dependency changes:

```sh
nix develop .#web --command python3 nix/prepare-server-source.py . /tmp/relay-server-source --update-lock
```

The generated server lock must retain main-lock versions and contain no Git dependencies.

The implementation remains uncommitted for review; the new outputs are not yet in the current public GitHub revision. gmk consumes a filtered local snapshot. After publication, full web consumers can use `github:jens-hj/relay`; server-only consumers can use `github:jens-hj/relay?dir=nix/server`.

Example Home Manager configuration:

```nix
inputs.relay.url = "path:/home/gmk/.local/share/relay/source";

imports = [ inputs.relay.homeManagerModules.default ];
services.relay = {
  enable = true;
  webPackage = inputs.relay.packages.${pkgs.stdenv.hostPlatform.system}.relay-web;
  publicOrigin = "https://your-node.your-tailnet.ts.net:8443";
  funnel = {
    enable = true;
    socket = "/run/user/1000/tailscaled.sock";
    daemonService = "tailscaled.service"; # In the same systemd manager.
    httpsPort = 8443;
  };
};
```

Funnel expects an enrolled daemon with permission; the module neither enrolls it nor changes other ports. Another TLS proxy can forward loopback HTTP instead; configure its exact public Origin.

Secrets use runtime `tokenFile` and optional `setupTokenFile` outside the store. A separate initialization unit creates missing files before systemd loads credentials, preserving existing files on restart. The service supplies a stable executable, CA certificates, owner home/harness PATH, `UMask=0077`, graceful stop and restart on failure. Harness children do not inherit Relay credential variables.

Manual enrollment expires ten minutes after first load; restarting cannot rearm the same code. Generate a new private setup code before enrollment if needed. Existing owners cannot be replaced through setup. The pilot provisions its owner locally before public exposure, so normal visits need only browser login.

The NixOS module accepts `services.relay.user` and `homeDirectory`. A dedicated user needs harness/provider credentials and project access; Home Manager on gmk reuses the owner configuration.

## gmk startup and migration

Host integration is `~/repos/nix/hosts/gmk/relay.nix`. User lingering is declared and active. Enabled user units start at boot without interactive login:

- `relay-initialize.service`: private first-run files.
- `relay.service`: persistent server/owned workers.
- `relay-funnel.service`: HTTPS forwarding, restarted with its server/daemon.

The Nix closure is rooted at `~/.local/share/relay/deployment`. Only Relay units are installed directly for this pilot; unrelated pending Nix/Home Manager edits are preserved. Normal configuration activation can subsequently own these same declared units.

Refresh a reviewed local source snapshot with:

```sh
nix develop .#web --command python3 nix/snapshot-source.py . ~/.local/share/relay/source
cd ~/repos/nix
nix flake update relay
```

The snapshot copies Git-listed crate/Nix/profile sources and root manifests/locks, excluding runtime data, credentials and build outputs. Track new sources or add them with intent to add before snapshotting. Build the relevant configuration before restarting units.

State is `~/.local/share/relay/workspace.sqlite3`, schema 6. This is a new workspace, not an import of local/dogfood databases. The Relay project connects the existing GitHub repository/board; remote membership/status remain authoritative. A read-only verification task demonstrates real Codex execution and exact-thread continuation. A third turn used the authenticated public browser draft/submission APIs and completed after the submitting tab closed. Its project execution mode is Unrestricted Access as requested. HAPI remains available; conversation history has not been imported.

Use SQLite's [backup API](https://www.sqlite.org/backup.html) for live backups rather than copying a WAL database's main file. Protected backups before and after the public browser/recovery checks are under `~/.local/share/relay/backups/`; project directories/worktrees need separate backups. Do not downgrade a migrated database in place.

Client disconnects leave execution on the server. Stopping/rebooting interrupts active turns; continuation afterward is explicit. Persistent startup restores availability/history without replaying commands.

## Phone layout

Follow-up [#17](https://github.com/jens-hj/relay/issues/17) uses Mosaic's existing `safe-area:all` support on the shell. The runtime reads the browser's `env(safe-area-inset-*)` values and accounts for the canvas position, so system bands are reserved once and stay independent of interface scale. Installed apps on touch devices add 16 unscaled pixels of solid space beneath the top inset to separate the header from the system blur reported on the owner's phone. This spacing also applies when the browser has already laid its canvas below the status area. The shell paints a solid background into those bands; browser background and theme color follow the active Relay theme. The canvas uses the dynamic viewport height. This follows the [WebKit safe-area guidance](https://webkit.org/blog/7929/designing-websites-for-iphone-x/).

Navigation starts closed below 640 logical pixels, or below 1000 on a browser with a coarse primary pointer, retaining the phone layout in landscape. A menu cell is available in board, task, conversation, profile and settings headers. A rightward touch drag starting within 28 pixels of the safe left edge reveals the drawer alongside the finger. A leftward drag closes it; the close button, outside tap, Escape and a navigation choice also dismiss it. Vertical scrolling and gestures elsewhere remain with the content. A claimed swipe cancels the original control press. The drawer overlays the main view, preserving the conversation editor through opening, closing and window resizing.

The final phone changes passed 120 desktop tests, native all-target and WASM Clippy with warnings denied, and the required formatting checks. Mosaic input/layout tests cover safe-area changes, unscaled installed-app spacing, drawer movement during a swipe, vertical and non-edge gestures, cancellation, dismissal, navigation and preservation of the editor node across resizing. The Nix web package and gmk unit closure built and were deployed to the same HTTPS URL; the restart preserved the owner, project and completed worker thread at revision 45. Public browser login, anonymous compressed-asset denial and WASM streaming compilation passed on the rebuilt bundle. GPU readback captured the initial deployed Mosaic phone frame with emulated standalone mode and 59/34-pixel system bands. Subsequent readback timed out in the software GPU harness, preventing browser-level visual confirmation of a completed swipe. These are separate from the passing Mosaic touch tests. The owner's screenshot confirms the earlier app ran on a physical iPhone; the revised blur spacing and drawer still need review on that device.

## Phone keyboard

Follow-up [#18](https://github.com/jens-hj/relay/issues/18) adds Relay's browser input-method bridge without changing the Mosaic pin. The canvas runtime alone cannot activate the phone keyboard: [Apple's guidance](https://developer.apple.com/library/archive/technotes/tn2010/tn2262/_index.html) describes activation through an editable HTML input or textarea. Relay focuses a small browser textarea synchronously during the tap on a Mosaic editor. It stays in the visible viewport and uses a 16-pixel font to avoid focus zoom. It supplies input to the existing canvas editor; it does not store a separate draft.

Browser selections use UTF-16 units and Mosaic uses UTF-8 byte offsets. The bridge converts these offsets, computes text replacements on character boundaries, and sends selection, commit and preedit events through the existing IME handlers. This covers keyboard insertion/deletion, selection replacement, autocorrect and composition. Clipboard text stays with the browser input to avoid a duplicate canvas paste; file paste continues through Relay's upload path. Arrow/navigation keys and application shortcuts return to Mosaic. Coarse-pointer devices use this bridge; desktop pointer focus and native applications retain their existing path.

Relay's UI shell follows the visual viewport, allowing the keyboard to reduce the available app area. The existing stable main-content branch preserves the draft editor through this resize. Automated checks cover these boundaries; physical iPhone keyboard presentation and its final keyboard animation/scroll behavior still require device review.

The subsequent keyboard follow-up reveals the chat caret when its scroll viewport changes size, even without another edit. It adds a small vertical margin around the insertion point and preserves manual transcript scrolling between resizes. The browser bridge publishes visible dimensions to Relay's UI shell and wakes the Mosaic window driver when only the visual viewport changes; a window layout resize is not required. The pinned runtime measures its drawing surface from the document viewport, so canvas CSS dimensions and backing pixels remain matched to that full layout viewport. Relay's root fits the smaller visible area independently, avoiding a full-height chat layout or vertically compressed text. It uses visible dimensions in CSS pixels while zoomed, so magnification cannot restore space occupied by the keyboard. The canvas and browser input follow visual viewport offsets, keeping the app frame visible during Safari's keyboard panning and pinch zoom. The app reflows within that visible frame while the browser's magnification remains active. This follows the [visual viewport distinction](https://developer.mozilla.org/en-US/docs/Web/API/VisualViewport) between the visible area and the layout viewport.

Web window configuration enables browser gestures. The canvas reserves single-finger dragging for Relay and allows browser pinch zoom through `touch-action: pinch-zoom`; the viewport metadata contains no zoom limit. Finishing a multi-finger gesture does not focus a text input. Touch presses cancel compatibility mouse events so a subsequent mouse press cannot steal keyboard focus; [pointer-event cancellation does not suppress browser zoom](https://www.w3.org/TR/pointerevents/#attributes-and-default-actions). The browser's zoom scale is never reset by the input bridge.

The initial keyboard bridge passed 123 desktop tests, native and WASM Clippy with warnings denied, and all required formatting checks. A Chromium touch fixture under CSP covers trusted focus, insertion/deletion, Unicode selection replacement, composition, clipboard propagation, navigation keys, desktop focus preservation and a missed-resize-event fallback. After that Nix deployment, the actual public Mosaic WASM app passed a trusted touch on command search, Rust-backed insertion/deletion/replacement/composition, preservation of its text and active browser input during a 390×844 to 390×460 resize, matching CSS and canvas bitmap heights, and Escape dismissal. Public login, anonymous compressed-asset denial and WASM streaming compilation also passed (7,823,260 wire bytes; 29,368,653 decoded). These browser checks use software Vulkan and emulated phone input; they do not verify the physical iPhone keyboard. That service restart preserved the owner, project and completed worker thread at revision 45 without execution replay.

The cursor and zoom follow-up passed 125 desktop tests, native all-target and WASM Clippy with warnings denied, and required formatting checks. The added tests reproduce a long draft's occluded caret, verify resize-driven reveal without another edit, retain manual transcript scrolling, and fit a chat into a 195×230 visible frame while the drawing surface stays 390×900. The JavaScript fixture also covers the visible-area callback, compatibility mouse focus suppression, viewport offsets and multi-touch dismissal. The complete Nix WASM package passed browser staging, then the deployed public app passed real two-finger touch zoom in and out, trusted input focus, Rust-backed text editing and composition, visual-only keyboard resizing, zoomed visible-area sizing, native canvas pixel sizing, retained focus/text and Escape dismissal. Public login, anonymous compressed-asset denial and streaming compilation passed for the final bundle (7,822,376 wire bytes; 29,373,921 decoded). The final restart preserved the entire workspace and all drafts at revision 66, including five sessions and both completed worker threads. Relay and Funnel remain enabled for reboot. Physical iPhone keyboard animation, Safari viewport behavior and touch acceptance remain device checks; these browser tests use Chromium with software Vulkan and emulated phone input.

## Verification

On 2026-10-09, combined checks passed: 251 tests, all-target Clippy with warnings denied, Rust/Mosaic/just/Nix formatting, and flake evaluation. Server/WASM Nix packages, the standalone public-input server flake and the gmk unit closure built. After adding static gzip negotiation, all 128 server unit/integration tests passed serially; a parallel rerun had hit one existing promoted-queue interrupt-count race, which also passed in isolation. Clippy and formatting passed on the final sources. Runtime files use mode 0600. SQLite backup/restoration preserved schema, workspace and owner state.

Actual public HTTPS browser checks, with normal DNS and certificate verification, covered login/cookie flags, anonymous bundle/API denial, CSRF/hostile-Origin rejection, invalid bearer precedence, both streams, logout closure and independent devices. Browser recovery rotated the password, revoked old sessions and both streams, rejected the old password and consumed-code reuse, and preserved a working new login. Seven unused recovery codes remain in the protected handoff file. The final public WASM response negotiated gzip (7,792,501 wire bytes for 29,272,779 decoded bytes), preserved the login gate, and compiled through browser `WebAssembly.compileStreaming` under the configured CSP. The deployed WASM hash matches the binary used for GPU readback. Phone-sized login forms fit without horizontal overflow. A real Codex worker completed three read-only turns on the identical native thread without changed files; the public browser saved and submitted its third turn through the conversation API.

GPU texture readback verified the real Mosaic WASM interface using software Vulkan, including a populated Relay board locally and an initial Mosaic frame over the public route. Later-frame readback timed out under the software GPU backlog; headless compositor screenshots are unsuitable evidence with these test flags. Ordinary headless Chromium exposes WebGPU but has no adapter and correctly displays the unsupported-browser message. Software WebGPU is an automation aid, not a required user setting. Physical browser GPU presentation/performance, phone hardware, touch/IME, accessibility and host reboot have not been verified. The final service restart also restarted HTTPS forwarding and preserved the owner/project/completed thread without execution replay. Enabled units plus active lingering are separate evidence from an actual reboot.

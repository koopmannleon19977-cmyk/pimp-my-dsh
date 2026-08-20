# ADR-0004: Confine packaged desktop web runs with AppContainer

- **Status:** Accepted for packaged desktop-supervised web runs
- **Date:** 2026-08-20
- **Deciders:** `pimp-my-dsh` maintainers

## Context

Direct `pimp-dsh run` uses the pinned upstream Windows backend
(`@deepseek-ai/dsh-sandbox-windows-acl`). Its `WRITE_RESTRICTED` token
intersects write access only: the child can still read caller-readable files,
open sockets, and inspect processes. The direct CLI does not use the native
desktop launcher and remains intentionally outside this decision.

The packaged desktop supervisor owns a stronger native process boundary:
`CreateProcessW` with `CREATE_SUSPENDED`,
`PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES`, an explicit handle list, an
unnamed kill-on-close Job Object, assignment before resume, and a fixed
absolute Node executable. This makes a unique zero-capability AppContainer
practical without changing broad host ACLs.

The first complete rc.7 prototype authenticated its control pipe and reached
`ready`, but the confined child advertised its own loopback listener. The host
could not reach that listener (`WSAECONNREFUSED`, OS error 10061). That result
is retained as historical evidence: broad network capability SIDs and
machine-wide loopback exemptions would weaken the intended zero-network
boundary, so they remain rejected.

## Decision

Ship native read-side confinement for packaged Windows desktop-supervised web
runs only:

1. Create a unique, unprivileged AppContainer profile with zero capability
   SIDs for each run. Stage the authenticated runtime and managed rc.7 web
   profile inside its private root without rewriting the caller profile,
   workspace, `%TEMP%`, or volume-root DACL. The web profile disables
   credential/settings and user-patch HMR watchers, precreates an empty
   `.credentials.yaml` and AppContainer-virtualized Temp path, and exposes the
   authenticated runtime's native-module closure through per-package private
   symlinks.
2. Launch the staged child suspended, assign it to the Job, and only then
   resume it. The bridge and every web data pipe are created by the host with a
   DACL for SYSTEM, the current user, and that exact per-run AppContainer SID;
   remote pipe clients are rejected.
3. Import the pinned `confined-web-transport.js` Node preload from the verified
   staged runtime. It replaces the expected `127.0.0.1:<proxy-port>` listen
   with a `\\.\pipe\LOCAL\pimp-dsh-anchor-…` lifecycle anchor. A process-global
   acceptor also covers the duplicate-module instance used by the rc.7 child.
   For each browser connection, the host creates a fresh exact-SID data pipe
   and random connection token, then sends both in an authenticated, strictly
   sequenced `web-accept` control frame. After connecting, the child writes that
   control-delivered token; the host verifies it in constant time before
   forwarding any browser cookie, request, or body. First-instance creation
   prevents a pre-created server, and the token proof rejects a racing allowed
   client.
4. Keep TCP loopback in the trusted host proxy, not in the zero-capability
   child. The child reports the public host-proxy base URL, while only the
   desktop navigation path receives the private bootstrap URL. The bootstrap
   responds with an `HttpOnly`, host-only, `SameSite=Strict` cookie,
   `Cache-Control: no-store`, and `Referrer-Policy: no-referrer`; all other
   requests require that cookie. After authentication, the proxy tunnels raw
   bytes so HTTP, SSE, and WebSocket behavior is preserved without protocol
   translation.
5. Fail closed across profile/runtime staging, AppContainer creation, boot,
   control/data-pipe authentication, proxy startup, and teardown. A failure
   never falls back to an unconfined desktop child; the Job is reaped and
   confinement cleanup is attempted on every exit path.

Tool approvals and path checks remain defense-in-depth and are not counted as
OS read confinement. The AppContainer boundary also does not promise denial of
every ambient host object: files or other objects deliberately readable by
all application packages or otherwise world-readable may remain visible.

## Production gate

Two ignored local gates cover different seams:

- `private_real_web_run_serves_through_authenticated_host_pipe_proxy` in
  `apps/desktop/src-tauri/tests/full_run_confinement_contract_test.rs` exercises
  the real Node 24.19.0 / DSH 0.1.0-rc.7 transport components. It verified
  authenticated `hello`/`ready`, private bootstrap controls, HTTP 200 through
  the mutually authenticated data pipe, acknowledged shutdown, an empty Job,
  and AppContainer-profile cleanup.
- `packaged_supervisor_serves_and_stops_the_confined_web_run` in
  `apps/desktop/src-tauri/tests/supervisor_production_contract_test.rs` compiles
  with release behavior and invokes `Supervisor::run_lifecycle`. It verified
  the public-base/private-navigation split, rc.7 root response, graceful stop,
  run-history outcome, and cleared endpoint authority.

The automated gates do not launch the packaged Tauri WebView2 window. A
manual packaged desktop smoke verified that flow end to end: after the
release run reached `Running`, the embedded harness window rendered the real
rc.7 UI (including its testing-notice acknowledgement), the private bootstrap
303/cookie worked over WebView2, and a graceful UI stop emptied the Job and
removed the per-run AppContainer profile.

## Consequences

Packaged desktop-supervised web runs no longer rely on child TCP loopback and
do ship a native read boundary. The host retains loopback authority and the
child retains only the exact named-pipe access needed for its lifecycle and
proxied connections.

This is not universal Windows confinement. Direct `pimp-dsh run` remains
write-only and unconfined for reads, network, and process visibility.
Zero-capability AppContainer children may still read ambient host objects whose
ACLs grant broad package/world access. Community plugins still execute with
harness authority inside whichever run admits them, so review and an
empty-by-default allowlist remain required.

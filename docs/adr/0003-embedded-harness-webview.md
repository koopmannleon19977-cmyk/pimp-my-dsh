# ADR-0003: Embedded zero-capability harness webview as the product surface

- **Status:** Accepted
- **Date:** 2026-08-16
- **Deciders:** `pimp-my-dsh` maintainers

## Context

Phase 0 shipped the desktop supervisor as a **tray controller + lobby window**
(`apps/desktop/src/app.tsx`) and left the actual product surface — the
`dsh-web-app` agent UI — outside the app: `open_harness` handed the validated
loopback URL to the system browser (`supervisor.rs::open` → `ShellExecuteW`).

The product goal is a desktop app that **is** the DSH experience — it should
look like the existing web app / a Codex-style surface — while keeping the
harness architecture from the distribution layer. The architecture already
names the mechanism for this but did not build it as primary: "the harness page
opens in the system browser **or a separate zero-capability webview** — never
inside the privileged controller webview" (research §9, architecture.md
"Strict frontend / native split").

## Decision

**Host the harness web UI in a second, embedded, zero-capability WebView2
window as the primary product surface.** The controller lobby (label `main`)
stays the control surface; a new window (label `harness`) renders the
Rust-constructed loopback READY URL. `open_harness` opens/refocuses that window
instead of the system browser, and on reaching `Running` the supervisor opens it
automatically and demotes the lobby to the tray. The system-browser path remains
only for `reveal_log_folder`.

Two windows, one authority:

| Window | Label | Loads | Capabilities |
| --- | --- | --- | --- |
| Controller lobby | `main` | bundled assets | `capabilities/main-window.json` (`core:default`) |
| Product surface | `harness` | `about:blank` at startup (hidden); `navigate(loopback READY URL)` + `show()` on demand | **none** — no capability file scopes to `harness` |

## Security invariants (unchanged from Phase 0)

- The `harness` window has **zero** Tauri capabilities: no matching capability
  file, so deny-by-default. It cannot invoke any command or plugin; navigation
  exists only in Rust.
- The URL is constructed in Rust from the authenticated READY frame
  (`supervisor::validated_endpoint`), never from the renderer. `state_allows_open`
  (`Ready | Running`) gates it.
- The harness page keeps its own remote CSP. The controller's strict CSP lives
  as a `<meta>` in its own `index.html` (travels with the bundled content); the
  `harness` window has no Tauri-injected CSP, so the remote server's CSP governs
  and its module scripts are not blocked.
- Close-to-hide applies to **both** windows: the supervisor stays resident. The
  `harness` window is declared in `tauri.conf.json` (hidden, `about:blank`) so
  its WebView2 initializes at startup, then is only `navigate`d + shown — it
  never destroys, so `open_harness` always re-navigates and re-focuses it.

This **does not fork `dsh-web-app`** (ADR-0001) and **does not grant the
renderer shell/filesystem/opener access** (ADR-0002 security posture).

## Consequences

### Positive

- The desktop app presents the real DSH product UI natively, matching "looks
  like the web app / Codex app" without reimplementing it.
- No capability expansion; the load-bearing "JavaScript is a view" split is
  untouched.
- Bounded change: one endpoint refactor in the lifecycle core plus a ~20-line
  Tauri window helper.

### Negative

- The product surface is the upstream web app as-is; native chrome is a thin
  frame around it. Deeper native integration (sidebar, native terminal, diff
  panel) is future work and would each require their own zero-capability surface
  and a protocol extension — not part of this decision.
- WebView2 rendering remains web rendering, not OS-toolkit pixels (ADR-0002).

## Reassessment triggers

- The `dsh-web-app` surface must be extended with desktop-only gestures that
  cannot be expressed through the existing loopback app → re-evaluate whether a
  thin native chrome needs its own (still zero-capability) IPC surface.
- A requirement emerges for the product window to call into the controller →
  violates the zero-capability boundary; must be a separate ADR.

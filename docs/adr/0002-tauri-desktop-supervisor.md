# ADR-0002: Tauri 2.11.5 desktop supervisor for Windows

- **Status:** Accepted
- **Date:** 2026-08-16
- **Deciders:** `pimp-my-dsh` maintainers

## Context

`pimp-my-dsh` is a Windows-first distribution of DeepSeek Harness. The CLI
(`pimp-dsh`) provides setup, run, doctor, update-check, and migrate commands
but has no persistent presence in the Windows OS shell. It launches, runs a
single harness session, and exits. This leaves the user with no persistent
process supervision, no tray-based health reporting, and no graceful lifecycle
management across sessions.

Two approaches were considered to add persistent desktop presence:

1. **A separate Node.js tray application** that spawns and manages the CLI
   child process.
2. **A Tauri 2 desktop application** with a Rust supervisor core and a React
   view, consuming the existing CLI as a child process.

## Decision

**Ship a persistent per-user Tauri 2.11.5 tray controller on Windows.** The
Rust backend owns all lifecycle authority; the React frontend is a read-only
view. The existing `pimp-my-dsh` CLI remains the authoritative launch boundary
and is never reimplemented.

The desktop supervisor is delivered as a per-user NSIS installer (`*-setup.exe`)
with a resident tray icon. It provides:

- A closed 13-state lifecycle state machine with revision tracking.
- An unnamed kill-on-close Job Object for process containment.
- An authenticated named-pipe bridge to the harness child.
- Structured log pipeline with bounded events and secret redaction.
- Compatibility manifest verification before every process creation.
- Cooperative graceful stop with forced-stop fallback.
- No telemetry, no LAN, no daemon, no Windows service.

## Evidence

### Why Tauri over alternatives

| Criterion | Tauri 2.11.5 | WinUI 3 / WASDK 2.4 | Electron 43 | Wails 2.14 / v3-β | Flutter 3.47 |
| --- | --- | --- | --- | --- | --- |
| Tray (first-party) | **Yes** (`tray-icon`) | No (community only) | Yes | Community | Community |
| Single instance | Official plugin | `AppInstance` API | Built-in | DIY | DIY |
| Process-tree control | Job Object | `Process.Kill(tree)` | No (needs `taskkill`) | DIY | DIY |
| Frontend fit | High: existing web stack | Poor: XAML/C# | High JS, redundant runtime | Poor: Go | Poor: Dart, no web reuse |
| Runtime footprint | App + WebView2 (evergreen) | Runtime installer or large bundle | Full Chromium+Node per app | Small Go + WebView2 | Dart AOT |
| Updater | Built-in, signed | MSIX or DIY DIY | Squirrel/MSIX | No first-party | No first-party |
| Windows-only | Yes (at this stage) | **Yes only** | Yes | Yes | Cross-platform |

WinUI 3 was the strongest runner-up: truly native Fluent rendering and first-party
UIA accessibility. However, it fails the most important criterion for a **tray
supervisor** — there is no first-party notification-area API. The gap is documented
in open Microsoft issues. This disqualifies it for the supervisor use case.

Every other webview/lightweight stack fails two or more: no first-party updater,
no first-party tray, no first-party single-instance, or wrong language (Go, Dart,
Kotlin) for a TypeScript/Node team.

### Security posture

Tauri ships deny-by-default capabilities. The controller's `Cargo.toml` and Tauri
config grant no `shell`, `opener`, `filesystem`, or `updater` permissions to the
frontend. Spawning exists only in Rust, where the capabilities system cannot be
weakened by web content.

The controller communicates with the harness child over an authenticated named
pipe with current-user/SYSTEM DACL, `PIPE_REJECT_REMOTE_CLIENTS`, bounded 64 KiB
frames, and a random per-run token. No secret is persisted.

### WebView2 is web rendering, not native

Tauri on Windows uses **Microsoft Edge WebView2** to render its UI. This is a
Chromium-based web engine, not truly native rendering. The UI looks polished
but does not produce pixels via the OS toolkit. WebView2 is the Evergreen
runtime — auto-updated by Windows through Edge updates.

This is a design decision, not a defect. WebView2 provides:
- Polished, Fluent-style rendering without XAML
- A11y via Chromium's accessibility tree
- Small runtime footprint (evergreen, system-shared)
- No second full Chromium runtime (unlike Electron)

The documentation clearly states this is web rendering, not native, to set
accurate expectations.

### The existing CLI boundary is preserved

The controller does not reimplement `pimp-dsh setup`, `run`, `doctor`,
`update-check`, or `migrate`. It invokes `pimp-dsh run` through the validated
CLI boundary — managed-profile checks, no-global-patch rule, environment
promotion, exact pin, and end-of-options boundary. This continues the no-fork
decision from ADR-0001.

### Concrete vertical slice (Phase 0 acceptance)

The following are concrete, testable acceptance criteria for Phase 0:

1. **Shell scaffold & installer.** Clean Win11 install; tray icon appears;
   window close hides (controller resident); uninstall removes the app,
   shortcuts, and its registry entry while preserving state/logs and profiles.
2. **Launch with Job guarantees.** `CREATE_NO_WINDOW | CREATE_SUSPENDED`, unnamed
   `KILL_ON_JOB_CLOSE` Job assigned before resume; no `cmd.exe` ancestor, no
   console flash, no unsupervised fallback on any error.
3. **Authenticated readiness.** Named pipe with random run token, bounded frames;
   Rust validates run ID, literal scheme/host, pinned versions before READY.
4. **Cooperative stop.** Bridge invokes upstream disposal; 5 s+ bound → Job
   active-process zero = GRACEFUL; deadline → `TerminateJobObject` = FORCED.
   Forced stop is never reported as graceful.
5. **Crash & identity.** Kill-on-close teardown on supervisor exit; stale PID
   or foreign port owner never killed.
6. **Status/doctor/open-UI/logs.** Tray menu reports handle-derived status;
   renderer-supplied URLs and lifecycle actions rejected.
7. **Bounded logs & secrets.** 16 KiB/event, secret redaction, disk-full
   fallback to drain-and-discard.

## Consequences

### Positive

- The desktop supervisor is a thin, auditable layer over the existing CLI.
- The Rust core provides OS-level process supervision (Job Object) that no
  Node.js process can provide reliably on Windows.
- The frontend uses the existing web stack (React, Vite, Fluent UI) — no new
  language or UI toolkit.
- WebView2 rendering provides polished appearance with minimal footprint.
- The strict frontend/native split prevents web content from escaping the
  renderer — a fundamental security property.
- The compatibility manifest ensures the controller, Node payload, and DSH
  client are lockstep-pinned before any process is created.
- Per-user NSIS installer requires no elevation.
- The same Tauri core runs on macOS/Linux — the state machine, protocol, and
  UI commands stay identical; only the process adapter and transport change.

### Negative

- WebView2 is web rendering, not truly native. Users expecting OS-toolkit
  pixels will not get them (though the UI looks polished).
- Tauri's updater requires a private signing key. Losing it bricks updates
  for installed users. Key custody is an operational responsibility, not
  just a code change.
- The updater is not configured in Phase 0. Distribution distribution
  requires setting up HTTPS endpoint, key management, and dual signing
  (Authenticode + Tauri update signature).
- Unsigned local builds trigger SmartScreen warnings.
- The NSIS installer is Windows-only. MSIX/Store is not shipped.
- macOS/Linux parity requires different process adapters (POSIX process-group
  kill replaces Job semantics) but is the same Tauri codebase.
- A Tauri v2 → v3 migration will be required at some point. The updater config
  `createUpdaterArtifacts` "will be removed in v3".

### Unsupported claims (explicitly documented)

- **No updater in Phase 0.** Tauri's updater plugin is available but not
  configured. Enabling it requires a private signing key and HTTPS endpoint.
- **Unsigned local builds are development artifacts.** Production requires dual
  Authenticode + Tauri update signatures.
- **No Microsoft Store or MSIX.** The installer is NSIS per-user only.
- **No cross-platform in Phase 0.** Implementation targets Windows 10/11 x64
  only. macOS/Linux is Phase 3.

## Reassessment triggers

This decision is reassessed when any of the following occur:

1. **Tauri v2 reaches end of life** without a viable v3 migration path.
2. **Microsoft ships a first-party tray API** in a stable WASDK release, making
   a WinUI 3 rewrite feasible for the supervisor use case.
3. **WebView2 becomes unavailable** on Windows 10/11 (e.g., Microsoft removes
   the evergreen runtime from a future Windows update without a replacement).
4. **The security model requires frontend shell access** that cannot be
   expressed in Rust without exposing dangerous capabilities to the renderer.
5. **A required lifecycle feature cannot be implemented** within Tauri's
   capabilities system and would require weakening the frontend/native split.
6. **The cost of maintaining Tauri bindings exceeds the cost** of a simpler
   approach (e.g., a minimal C# WinForms tray app that spawns the CLI).

## Architecture reference

See [docs/architecture.md](../architecture.md#desktop-supervisor-architecture)
for the full desktop architecture, state machine, IPC protocol, bridge
contract, and manifest verification details.

See [docs/security-model.md#desktop-supervisor-security-model] for the
desktop-specific security boundaries including WebView2 rendering, bridge
security, Job Object containment, and secret handling.

See [docs/windows-support.md#desktop-supervisor-windows] for Windows-specific
details including installer modes, process primitives, and development unsigned
builds.

See [docs/roadmap.md] for the desktop phases and acceptance criteria.

See [docs/upstream-pin.md#desktop-supervisor-pins] for the complete desktop
version pins and compatibility manifest.

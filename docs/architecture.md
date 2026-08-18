# Architecture

`pimp-my-dsh` is a thin distribution over DeepSeek Harness. It composes the
upstream plugin bundles through a patch layer and one distribution-owned plugin,
then wraps them in a small CLI. It contains no copied upstream source.

## Layers

```
┌─────────────────────────────────────────────────────────┐
│ pimp-dsh CLI (src/cli.ts)                               │
│   setup · run · doctor · update-check · migrate         │
├─────────────────────────────────────────────────────────┤
│ Distribution plugin (src/plugin.ts)                     │
│   prompt/context · scoped Git · durable memory          │
├─────────────────────────────────────────────────────────┤
│ cordis.patch.yml                                        │
│   overrides upstream base rows by stable id             │
│   inserts the distribution plugin                       │
├─────────────────────────────────────────────────────────┤
│ Upstream bundles (exact npm dependency)                 │
│   @deepseek-ai/dsh-base                                 │
│   @deepseek-ai/dsh-web-app / dsh-headless               │
│   (read/search/edit/pwsh/sessions/skills/todo/subagents) │
└─────────────────────────────────────────────────────────┘
```

## Composition model

DeepSeek Harness composes a running agent from ordered layers. A **profile** is
a named composition stored in the harness home. A **bundle** is a distribution
format for Cordis config rows and the code they mount. A **patch** targets a row
by stable id and replaces its whole config, or inserts new rows.

`pimp-my-dsh` uses this mechanism rather than forking:

1. `cordis.patch.yml` overrides upstream base rows by stable id. It disables
   telemetry, disables web tools, and keeps LSP opt-in.
2. The same patch inserts the distribution-owned plugin.
3. `setup` copies a shipped data-only profile patch into a freshly staged,
   exact manifest and installs that completed profile under the harness home.

The upstream base bundle (`@deepseek-ai/dsh-base`) is the first layer of every
profile: model adapters, tools, persistence, sandbox and approval policy,
settings, credentials, and telemetry. The `web` profile adds `web-app`; every
other shipped profile adds `headless`, providing a concrete one-shot surface.

## Distribution-owned code

The distribution owns exactly two source files:

- **`src/cli.ts`** — the `pimp-dsh` binary. It owns `setup`, `run`, `doctor`,
  `update-check`, and `migrate`. It does not reimplement upstream tools.
- **`src/plugin.ts`** — a Cordis plugin that contributes prompt/context
  guidance, a read-only scoped Git tool, and append-only durable memory.

Everything else is upstream, consumed through the published npm artifact.

## CLI contract

| Command | Behavior |
| --- | --- |
| `setup --profile <name> [--force] [--json]` | Build an exact profile in staging with scripts/hooks disabled, then atomically install it under the harness home. Reject redirected/unmanaged profile paths. `--force` replaces only distribution-owned profiles and does not retain added bundles. |
| `run --profile <name> -- <app args...>` | Validate the exact managed marker, manifest, patch, and bundle link; reject the higher-precedence global home patch and configuration inside the writable workspace; then boot through upstream `dsh`. Forwarded arguments follow an end-of-options boundary and cannot become launcher flags. |
| `doctor [--profile <name>] [--json]` | Diagnose environment and profile state. |
| `update-check [--json]` | Check for a newer distribution release. No telemetry. |
| `migrate --profile <name> [--apply] [--json]` | Upgrade profile patch data. Dry run by default. |

Structured CLI results are JSON-capable and stable. Secret values are never
logged.

## Environment variables

The distribution reads these variables by name; values are never logged:

- `DSH_HOME` — harness home (upstream).
- `PIMP_DSH_API_KEY` — model provider API key.
- `PIMP_DSH_BASE_URL` — model provider base URL.
- `PIMP_DSH_MODEL` — model identifier.
- `PIMP_DSH_ENABLE_LSP` — explicit LSP opt-in.

`PIMP_DSH_*` values must come from the parent process environment. The wrapper
promotes them to protected `DSH_PIMP_*` names and removes the public names
before DSH loads a repository `.env`. `DSH_PERMISSION_MODE` is disclosed for
completeness but shipped profiles own safe defaults. The wrapper forces
`DSH_TELEMETRY_DISABLED=1`; `DSH_TELEMETRY_MODE` is intentionally ignored.

## Hardening decisions

The distribution narrows upstream's posture in four ways, each documented in
[security-model.md](security-model.md):

1. **Telemetry off** — the wrapper forces upstream's hard kill switch and the
   bundle disables the telemetry backend.
2. **Web off** — the SSRF-prone HTTP fetch provider is not enabled.
3. **LSP opt-in** — language servers run unsandboxed, so they require explicit
   consent.
4. **Community plugins gated** — a reviewed allowlist gate, not auto-install.

## Desktop supervisor architecture

The desktop supervisor is a per-user Tauri 2.11.5 application (`apps/desktop/`)
that sits above the existing `pimp-my-dsh` CLI and provides persistent tray
presence, process supervision, and structured health reporting on Windows
10/11 x64.

### Components

```
┌─────────────────────────────────────────────────────────┐
│ Tray / Window (apps/desktop/ React, zero capabilities)  │
│   get_snapshot · start_harness · stop_harness           │
│   run_doctor · open_harness · reveal_log_folder         │
│   set_theme · set_fixed_port                            │
├─────────────────────────────────────────────────────────┤
│ Tauri 2.11.5 core (Rust, src-tauri/)                    │
│   State machine · Job Object · process supervisor        │
│   Bridge (named pipe, token-auth) · log pipeline         │
│   Provider (packaged / debug) · compatibility manifest   │
├─────────────────────────────────────────────────────────┤
│ DeepSeek Harness CLI (Node, child process)              │
│   `pimp-dsh run --profile web` — validated boundary      │
└─────────────────────────────────────────────────────────┘
```

### Rendering technology

Tauri on Windows uses **Microsoft Edge WebView2** to render the control
surface. This is a web engine, not truly native rendering. The UI looks
polished but does not produce pixels via the OS toolkit. WebView2 is the
Evergreen runtime — auto-updated by Windows through Edge updates — but
air-gapped deployments may choose `offlineInstaller` or `fixedVersion` at
the cost of shipping browser patches themselves.

### Strict frontend / native split

**Rust owns all authority.** The Tauri controller owns:

- The closed 13-state [state machine](#state-machine)
- The unnamed [kill-on-close Job Object](https://learn.microsoft.com/windows/win32/procthread/job-objects)
- Live process and Job handles
- The authenticated bridge connection
- The log pipeline
- The provider (packaged or debug) and the compatibility manifest

**JavaScript is a view only.** The React frontend (`apps/desktop/`) has
**zero** Tauri capabilities: no `shell`, no `opener`, no `filesystem`, no
`updater`. All spawning authority exists only in Rust. The controller
webview loads only bundled assets under strict CSP. The harness web UI renders
in a second, embedded, **zero-capability** webview (label `harness`) — the
primary product surface — never inside the privileged controller webview.

### IPC: Rust to React

The renderer communicates with Rust through eight commands. All are
parameterless except `set_theme({theme})` and `set_fixed_port({port: number|null})`;
accepted values are closed enums or ranges.

| Command | Purpose |
| --- | --- |
| `get_snapshot` | Fetch the current Snapshot |
| `start_harness` | Begin a supervised run |
| `stop_harness` | Gracefully or forcibly stop a run |
| `run_doctor` | Delegate to `pimp-dsh doctor` |
| `open_harness` | Open/refocus the embedded zero-capability `harness` webview (Rust constructs the READY URL) |
| `reveal_log_folder` | Open the log directory in File Explorer |
| `set_theme` | Change the UI theme |
| `set_fixed_port` | Opt into a fixed port instead of dynamic |

Rust emits a `supervisor://snapshot` channel carrying a complete **Snapshot v1**,
never deltas. The renderer never supplies URLs, paths, executables, argv,
environment, PIDs, pipe names, run tokens, or lifecycle targets. Stale renderer
data is never accepted because mutations carry no revision or target authority.

### State machine

Closed `State` enum — every serialized transition increments `revision` exactly
once and records a stable kebab-case `Reason`.

| State | User-facing label |
| --- | --- |
| `stopped` | Stopped |
| `preflighting` | Starting |
| `starting` | Starting |
| `ready` | Ready |
| `running` | Running |
| `stopping` | Stopping |
| `stopped-graceful` | Stopped |
| `stopped-forced` | Forced stop |
| `failed-start` | Needs attention |
| `crashed` | Needs attention |
| `unmanaged` | Needs attention |
| `update-pending` | Needs attention |
| `updating` | Needs attention |

Rules:

- Start is idempotent in `preflighting`, `starting`, `ready`, `running`.
- Stop is idempotent in `stopping`, `stopped-graceful`, `stopped-forced`.
- Start during `stopping` is rejected.
- Exactly one lifecycle mutex owns all transition execution.

### Provider launch contract

Two providers exist; both return only backend-owned absolute `node.exe`, CLI
entry, working directory, and explicit environment. Rust always invokes Node
with fixed argv:

```
CLI run --profile web -- --host 127.0.0.1 --port <0|validated fixed>
```

using `CreateProcessW` with `CREATE_NO_WINDOW | CREATE_SUSPENDED`, no shell,
and an explicit inherited-handle allowlist.

| Provider | When | Verifies |
| --- | --- | --- |
| **Packaged** | Production builds | Manifest, target, all exact versions, `node.exe` SHA-256, deterministic payload tree hash. Never consults PATH. |
| **Development** | Debug config only | Absolute workspace identity plus installed versions. |

The runtime payload lives under `apps/desktop/src-tauri/runtime/` and is
generated only by staging.

### Compatibility manifest v1

Before any process is created, the provider verifies:

```
{schemaVersion:1, protocolVersion:1, controllerVersion:'0.1.0',
 node:{version:'24.19.0', sha256}, pnpmVersion:'11.7.0',
 distributionVersion:'0.1.0', dshVersion:'0.1.0-rc.6',
 target:'x86_64-pc-windows-msvc', payloadSha256}
```

`additionalProperties:false`. If the manifest, target, versions, SHA-256, or
payload hash mismatch, preflight fails before process creation. The
distribution version, controller version, Node version, pnpm version, and DSH
version are all exact pins — never ranges.

### Child bridge v1

The harness child communicates with the controller over a versioned,
authenticated pipe:

- **Transport:** named pipe (Windows), current-user/SYSTEM DACL, `PIPE_REJECT_REMOTE_CLIENTS`
- **Protocol:** length-prefixed UTF-8 JSON, max 64 KiB, `additionalProperties:false`
- **Common fields:** `{protocolVersion:1, type, runId, token, sequence}`
- **Token:** 64 lowercase hex chars, memory-only, per-run random
- **Sequence:** strictly increases per authenticated connection
- **Child to Rust types:** `hello`, `ready`, `health`, `stopping`, `stopped`, `error`
- **Rust to child:** `shutdown`

On `ready`, the child sends additional fields: `{profile:'web', host:'127.0.0.1',
port:1..65535, url:'http://127.0.0.1:<port>', distributionVersion:'0.1.0',
dshVersion:'0.1.0-rc.6'}`. Rust authenticates token/run/version/sequence before
type-specific parsing and constructs the endpoint itself from host+port. A
supplied URL must exactly equal the normalized result but never becomes
authority.

On `health` (sent every 30 s), the child sends `{checks:[{id,status,message}]}`;
the controller stores them verbatim in `snapshot.health` for the lobby's health
panel and records the receipt time. A watchdog in the run loop declares the
heartbeat stale when no frame arrives within 75 s (2.5 intervals) and appends
a supervisor-owned `supervisor-heartbeat` error check; the next child frame
replaces the list and clears staleness. Stop detection stays handle-based —
these are self-reported liveness facets, never authority.

### Logs

`LogEvent` struct: `{runId:string|null, revision:u64, sequence:u64,
timestamp:string, source:'supervisor'|'stdout'|'stderr'|'lifecycle'|'doctor',
level:'trace'|'info'|'warning'|'error', message:string}`. UTF-8 replacement,
ANSI stripping, HTML text rendering, 16 KiB per event, bounded in-memory
queue. Disk writer failure switches once to drain-and-discard with one
supervisor error — lifecycle remains operable. Secret redaction covers the
token and values of case-insensitive `PIMP_DSH_*`/`DSH_PIMP_*` environment
names before any sink.

### Data and log paths

State files, logs, and bridge artifacts live under the per-user application
data directory. The harness home (`DSH_HOME`) and managed profile directory
must remain outside the writable workspace. This keeps the agent from creating
the watched global patch after preflight or mutating its own managed profile
during a session.

### Close and quit

Closing the application window hides the tray controller (it remains resident
while a harness run exists). An explicit **Quit** follows the stop policy — it
never silently detaches the harness.

### Relationship to the existing CLI

The desktop supervisor composes with — not replaces — the existing
`pimp-my-dsh` CLI. The CLI remains the authoritative launch boundary:
managed-profile checks, no-global-patch rule, environment promotion, exact
pin, and end-of-options boundary are all preserved. The controller never
reimplements them. This continues the no-fork decision recorded in
[ADR-0001](adr/0001-no-fork.md) and the new Tauri decision in
[ADR-0002](adr/0002-tauri-desktop-supervisor.md).

## Why no fork

The decision to consume upstream as an exact npm dependency is recorded in
[ADR-0001](adr/0001-no-fork.md), with evidence and reassessment triggers.

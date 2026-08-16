# Windows Desktop Launcher & Process-Supervisor Research

Status: research artifact (no implementation). Owner: `DesktopLauncherResearch` agent, 2026-08-16.
Purpose: source-grounded input for the OMP goal prompt that will define a Windows-first
native desktop launcher/control surface for `pimp-my-dsh`. Every material factual claim
is linked to a primary/official source (URL inline; see [Source index](#source-index)).
Skip list (explicit non-goals): implementation code, formatters, linters, builds, tests,
generic secondary blog posts.

## 1. Terminology: three different artifacts

The goal prompt must keep these distinct — they differ in footprint, lifecycle
ownership, and stop semantics:

| Tier | Artifact | What it is | Supervision? |
| --- | --- | --- | --- |
| A | **Desktop shortcut + script** (`.lnk` pointing at `node.exe`/`.cmd`/`.ps1`) | One click starts the harness; no resident process after start. | None (or PID-file bookkeeping in the script). |
| B | **Native launcher executable** (compiled `.exe`: Tauri/.NET/WinUI/Electron) | Starts the harness as a child process and owns the lifecycle while it runs. | During run only. |
| C | **Tray / process-supervisor app** | Persistent resident process with a notification-area icon; starts, stops, monitors, exposes status/doctor/log/UI actions. | Continuous. |

Tiers B and C are usually the same binary (a tray app that also launches), but the
Phase-0 scope decision matters: a pure Tier-A shortcut costs almost nothing, while any
Tier-B/C `.exe` triggers runtime, signing, and update obligations (see §5).

## 2. Current CLI lifecycle facts (repo grounding)

Any launcher must treat the existing CLI as the **only lifecycle authority**
(no-fork ADR: [docs/adr/0001-no-fork.md](adr/0001-no-fork.md); roadmap Phase 0:
[docs/roadmap.md](roadmap.md)). Grounding facts:

- `pimp-dsh run --profile <name>` is a **blocking foreground process**: it executes
  `spawnSync(process.execPath, [dshBin(), '--profile', profile, ...], { stdio: 'inherit' })`
  and then `process.exit(child.status)` ([src/cli.ts](../src/cli.ts)). There is **no
  daemon mode, no pid file, no stop command** today.
- The `web` profile "serves the Web UI at `http://127.0.0.1:3080` by default"
  ([README.md](../README.md) line 69-70) — the port is the natural liveness probe for a
  supervisor.
- The CLI already uses `spawnSync` with `shell: false` and `windowsHide: true` for
  package-manager work ([src/cli.ts](../src/cli.ts)) — the correct Windows spawn posture
  to avoid a console flash and a `cmd.exe` process in the tree.
- Project convention for background-process cleanup is `taskkill /T`:
  "Background processes are terminated with `taskkill /T`, which kills the process
  tree. This is the Windows equivalent of POSIX process-group termination."
  ([docs/windows-support.md](windows-support.md#process-cleanup)).
- Secrets: the CLI reads `PIMP_DSH_API_KEY` / `PIMP_DSH_BASE_URL` / `PIMP_DSH_MODEL` /
  `PIMP_DSH_ENABLE_LSP` from the environment and promotes them to protected upstream
  names, then deletes the public ones ([src/cli.ts](../src/cli.ts)). A launcher must
  therefore pass environment through **live** and must never persist or log env values.

## 3. Windows platform primitives (official)

### 3.1 Shortcuts (`.lnk`)

- A Shell link is created via `IShellLink` + `IPersistFile` (COM). The `.lnk` stores:
  target path, **working directory**, **arguments**, **show command** (an `SW_` value
  from `ShowWindow`), icon location, description, hotkey
  ([Microsoft Learn: Shell Links](https://learn.microsoft.com/en-us/windows/win32/shell/links)).
- "A shortcut can exist on the desktop or anywhere in the Shell's namespace" (same page)
  → Start Menu placement is just saving the `.lnk` into the Start Menu programs folder.
- The show command matters for Tier A: the `.lnk` can start the console host minimized
  (`SW_SHOWMINNOACTIVE`) so no window pops on top.

### 3.2 Process launch without a console flash

- `CREATE_NO_WINDOW` (0x08000000): "The process is a console application that is being
  run without a console window."
  ([Process Creation Flags](https://learn.microsoft.com/en-us/windows/win32/procthread/process-creation-flags)).
  Node exposes the equivalent via `windowsHide: true` (already used by the CLI); Rust/.NET
  native launchers must set the flag themselves.
- `CREATE_NEW_PROCESS_GROUP` (0x00000200) creates a process group rooted at the child
  (same page) — relevant if a future supervisor wants `GenerateConsoleCtrlEvent`
  semantics instead of `taskkill /T`.

### 3.3 Process-tree stop semantics

- `taskkill /t`: "Ends the specified process and any child processes started by it."
  ([taskkill reference](https://learn.microsoft.com/en-us/windows-server/administration/windows-commands/taskkill)).
  `/f` forces. This matches the project's documented convention.
- **Job Objects** are the stronger, kernel-managed mechanism: `CreateJobObject`,
  `AssignProcessToJobObject`, `TerminateJobObject`; "by default any child processes it
  creates using `CreateProcess` are also associated with the job"; and with
  `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`, "closing the last job object handle terminates
  all associated processes and then destroys the job object itself." Nested jobs exist
  since Windows 8; breakaway flags can exempt children — a supervisor should set neither
  breakaway limit if it wants the whole tree.
  ([Job Objects](https://learn.microsoft.com/en-us/windows/win32/procthread/job-objects)).
- Node's `subprocess.kill()` is **not** a tree kill: "On Windows, where POSIX signals do
  not exist, the `signal` argument will be ignored except for `'SIGKILL'`, `'SIGTERM'`,
  `'SIGINT'` and `'SIGQUIT'`, and the process will always be killed forcefully and
  abruptly (similar to `'SIGKILL'`)." The docs' tree caveat is stated for Linux only.
  ([Node child_process](https://nodejs.org/api/child_process.html#subprocesskillsignal)).
  Consequence: a supervisor that wants the whole tree on Windows uses `taskkill /T` on
  the recorded root PID, or a Job Object, not `child.kill()`.
- .NET has a first-party tree kill: `Process.Kill(bool entireProcessTree)` — "Immediately
  stops the associated process, and optionally its child/descendent processes", with the
  caveat that `HasExited`/`WaitForExit` do **not** reflect descendants
  ([System.Diagnostics.Process.Kill](https://learn.microsoft.com/en-us/dotnet/api/system.diagnostics.process.kill)).

### 3.4 Single-instance / stale-state detection

- Classic mechanism for unpackaged apps: a **named mutex**
  ("Traditionally, unpackaged apps are multi-instanced by default … Typically this is
  done using a single named mutex to indicate if an app is already running.")
- Windows App SDK offers `Microsoft.Windows.AppLifecycle.AppInstance.FindOrRegisterForKey`
  plus `GetInstances` and activation redirection for WinUI 3 apps
  ([App instancing](https://learn.microsoft.com/en-us/windows/apps/windows-app-sdk/applifecycle/applifecycle-instancing)).
- For the harness itself (which is not our code), staleness detection must be empirical:
  PID file + `tasklist`/process query + TCP probe of `127.0.0.1:3080`. A port held by a
  **foreign** process must never be killed — report and stop.

## 4. Candidate stacks

### 4.1 Tier A — `.lnk` + Node wrapper script (no new runtime)

What it is: a small Node script (or `.cmd`/`.ps1`) shipped in the npm package plus a
`.lnk` installed via `IShellLink` (practically: PowerShell `WScript.Shell` COM, which is
a thin wrapper over the same `IShellLink` object). The script: probe port → if running:
report + optionally open UI; else spawn `node <cli> run --profile web` with
`windowsHide: true`, write a PID file (PID + profile + timestamp) under `DSH_HOME`,
redirect stdio to a log file; on `stop`: `taskkill /T /PID <recorded>` (escalate `/F`).

- Zero new runtime, zero signing burden, fits the no-fork distribution model exactly;
  the CLI remains the only lifecycle authority.
- Limits: no tray icon, no continuous supervision, console window if the `.lnk` show
  command is not set minimized/NO_WINDOW.

### 4.2 Tier B — Tauri 2 native launcher

Official facts:

- Prereqs/rendering: Rust + Microsoft C++ Build Tools + **WebView2 runtime**
  ("Tauri uses Microsoft Edge WebView2 to render content on Windows")
  ([Tauri prerequisites](https://v2.tauri.app/start/prerequisites/)).
- Packaging: NSIS `-setup.exe` or WiX v3 MSI; MSI can only be built on Windows; MSI
  build requires the **VBSCRIPT** optional feature, which "is currently enabled by
  default on most Windows installations, but is being deprecated and may be disabled in
  future Windows versions"; WebView2 install modes: `downloadBootstrapper` (+0 MB,
  needs internet, default), `embedBootstrapper` (~1.8 MB), `offlineInstaller` (~127 MB),
  `fixedVersion` (~180 MB)
  ([Windows Installer](https://v2.tauri.app/distribute/windows-installer/)).
- Process spawn: `@tauri-apps/plugin-shell` — `Command.create(program, args).execute()`
  / `.spawn()`; capability-gated: the default permission only allows opening
  `http(s)://`, `tel:`, `mailto:` links; running an arbitrary command requires an
  explicit `shell:allow-execute` / `shell:allow-spawn` scope (name/cmd/args can be
  restricted, e.g. only `pimp-dsh`)
  ([Shell plugin](https://v2.tauri.app/plugin/shell/)).
- Tray: first-class — Cargo feature `tray-icon`, JS API `TrayIcon.new(options)` with
  menu + click events
  ([System Tray guide](https://v2.tauri.app/learn/system-tray/)).
- Single instance: official plugin, Windows fully supported
  ([Single Instance](https://v2.tauri.app/plugin/single-instance/)).
- Updates: updater plugin **requires** signed updates ("Tauri's updater needs a
  signature … This cannot be disabled"); Windows artifacts = `myapp-setup.exe` + `.sig`
  and `myapp.msi` + `.sig`; `installMode`: `passive` (default) / `basicUi` / `quiet`
  ([Updater plugin](https://v2.tauri.app/plugin/updater/)).
- Signing: not required to run, but required for Microsoft Store and to avoid the
  SmartScreen "not trusted" warning on browser downloads; supported via `signtool`
  (OV/EV) or Azure Key Vault
  ([Windows Code Signing](https://v2.tauri.app/distribute/sign/windows/)).
- Optional sidecar mechanism (`externalBin`) if a portable Node were ever bundled —
  not recommended for Phase 0
  ([Embedding External Binaries](https://v2.tauri.app/develop/sidecar/)).

### 4.3 Tier B — .NET (WinUI 3 / Windows App SDK, or WPF)

Official facts:

- One-file distribution: `PublishSingleFile` + `SelfContained` + RID
  (`dotnet publish -r win-x64`); "The size of the single file in a self-contained
  application is large since it includes the runtime and the framework libraries";
  optional `EnableCompressionInSingleFile`; native runtime binaries stay separate unless
  `IncludeNativeLibrariesForSelfExtract`; signing hook exists via `PrepareForBundle` /
  `GenerateSingleFileBundle` MSBuild targets
  ([.NET single-file overview](https://learn.microsoft.com/en-us/dotnet/core/deploying/single-file/overview)).
- WinUI 3 runtime burden (framework-dependent): the Windows App SDK runtime is 4 MSIX
  packages (Framework/Main/Singleton/DDLM); unpackaged apps must run
  `WindowsAppRuntimeInstall.exe --quiet` (or install the packages themselves), initialize
  via the Bootstrapper API, and the **Visual C++ Redistributable is a requirement**;
  machine-wide install needs elevation (`0x80070005`)
  ([Deployment architecture](https://learn.microsoft.com/en-us/windows/apps/windows-app-sdk/deployment-architecture),
  [Unpackaged deployment guide](https://learn.microsoft.com/en-us/windows/apps/windows-app-sdk/deploy-unpackaged-apps)).
- Self-contained WinAppSDK: `WindowsAppSDKSelfContained=true` bundles the runtime with
  the app; **`PublishSingleFile` is supported only for unpackaged, self-contained
  WinUI 3 apps (Windows App SDK 1.5+)**, extracts to a temp dir at first launch, and is
  **not supported for packaged (MSIX) apps**
  ([Package and deploy overview](https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/),
  [Self-contained deployment guide](https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/self-contained-deploy/deploy-self-contained-apps)).
- MSIX (alternative packaging): package identity, "All MSIX packages must be signed
  before installation", differential (64 KB block) updates, clean uninstall, app
  container semantics
  ([What is MSIX?](https://learn.microsoft.com/en-us/windows/msix/overview)).
- Single instance: `AppInstance.FindOrRegisterForKey` (WinUI 3, packaged and
  unpackaged) or a named mutex
  ([App instancing](https://learn.microsoft.com/en-us/windows/apps/windows-app-sdk/applifecycle/applifecycle-instancing)).
- **Tray gap (critical)**: WinUI 3 has **no first-party tray API**. The WinRT
  `SystemTray` class is unusable on desktop
  ([WindowsAppSDK issue #4063](https://github.com/microsoft/WindowsAppSDK/issues/4063));
  tray support is an open discussion with no commitment
  ([discussion #519](https://github.com/microsoft/WindowsAppSDK/discussions/519),
  [discussion #3394](https://github.com/microsoft/WindowsAppSDK/discussions/3394)).
  The de-facto solution is the community library
  [H.NotifyIcon](https://github.com/HavenDV/H.NotifyIcon) (WPF/WinUI). WPF has the
  alternative of hosting the WinForms `NotifyIcon`. Both paths are community code or a
  legacy control, not a supported first-party API.
- Known MSIX tray bugs: Shell_NotifyIcon notification click can launch a **second
  instance** of a packaged app
  ([issue #6031](https://github.com/microsoft/WindowsAppSDK/issues/6031)); tray icon
  re-registers when the MSIX version changes the exe path
  ([issue #6408](https://github.com/microsoft/WindowsAppSDK/issues/6408)).

### 4.4 Tier B — Electron

Included only because the task demanded primary evidence on overhead:

- Process model: Chromium-style multi-process — one main process (Node.js) + one
  renderer process **per window** + optional utility processes
  ([Electron Process Model](https://www.electronjs.org/docs/latest/tutorial/process-model)).
- Distribution: the app is "Electron's prebuilt binaries" plus a `resources/app` folder
  (or `app.asar` archive) — i.e. **every app ships its own Chromium + Node.js runtime**
  ([Application Packaging](https://www.electronjs.org/docs/latest/tutorial/application-distribution)).
- Tray: first-class main-process API; on Windows the tray GUID is tied to the code
  signature when signed, else to the exe path — "Changing the path to the executable
  will break the creation of the tray icon"
  ([Tray](https://www.electronjs.org/docs/latest/api/tray)).
- Updates: `autoUpdater` on Windows = Squirrel.Windows (traditional installers) or the
  MSIX updater; Squirrel requires an AUMID of `com.squirrel.<id>.<exe>` and
  `app.setAppUserModelId`
  ([autoUpdater](https://www.electronjs.org/docs/latest/api/auto-updater)).

## 5. Comparison table

| Criterion | Tier A: `.lnk` + Node script | .NET 8+ (WPF/WinUI, single-file) | Tauri 2 | Electron |
| --- | --- | --- | --- | --- |
| One clickable `.exe`? | No (`.lnk` → installed `node.exe`) | Yes (`PublishSingleFile`, unpackaged self-contained) | Yes (NSIS `-setup.exe` or MSI) | Yes (installer via Forge/builder) |
| Install footprint | ~KB (script + `.lnk`), reuses installed Node | Self-contained = .NET runtime + app (docs: "large since it includes the runtime"); WinUI adds WinAppSDK payload | App (small) + WebView2: +0/+1.8/+127/+180 MB by mode | Chromium + Node per app (prebuilt binaries + `resources/app`) |
| Runtime prerequisites | Node 22.19+/24 (already required by product) | None if fully self-contained; VCRedist required for unpackaged WinAppSDK | WebView2 runtime + MSVC-built binary; installers handle WebView2 | None (all bundled) |
| Process-tree stop | `taskkill /T` on recorded PID (project convention) | `Process.Kill(entireProcessTree: true)` (first-party) or Job Object | Spawn handle + `taskkill /T` or Job Object (win32) | `child.kill()` = single process; tree kill needs `taskkill /T` |
| Tray / continuous supervision | None | Only via community `H.NotifyIcon` (WinUI) or WinForms `NotifyIcon` (WPF) — no first-party API | First-class (`tray-icon`) | First-class |
| Single-instance of the launcher | Port probe + PID file (empirical) | `AppInstance.FindOrRegisterForKey` or named mutex | Official single-instance plugin | Built-in app API |
| Update mechanics | `pnpm`/npm update (existing CLI update path) | MSIX differential updates (packaged) or custom; single-file re-download | Updater plugin (NSIS/MSI, **signed updates mandatory**) | autoUpdater (Squirrel.Windows / MSIX) |
| Signing implications | None (script) | MSIX requires signing; unsigned exe triggers SmartScreen on download | Not required to run; needed for Store/SmartScreen; updater always requires signing keys | Same SmartScreen/Store constraints |
| Fit with Node CLI | Exact fit — same language, no new runtime, CLI stays the authority | Good (spawn node with `CREATE_NO_WINDOW`); second language + runtime to maintain | Good (shell plugin spawns `pimp-dsh`; capability scope can lock args); Rust toolchain + WebView2 added | Poor — bundles a second, different Node/Chromium runtime for a Node-native product; largest attack surface |

## 6. Recommendation (conservative Phase-0)

**Tier A now: extend the existing CLI with a launcher command (pure Node, zero new
dependencies) and install a Start Menu/Desktop `.lnk` that points at it.** No new
runtime, no fork, no signing, no updater keys; the CLI remains the only lifecycle
authority; every action is a thin composition of the primitives in §3.

Proposed initial scope (each item maps to a verifiable acceptance criterion):

1. **Start exactly one managed harness process**: probe `127.0.0.1:3080` (TCP); if
   free, spawn `node <pimp-dsh cli> run --profile web` with `shell: false`,
   `windowsHide: true`, stdio redirected to a per-profile log file under `DSH_HOME`;
   atomically write a state file `{pid, profile, startedAt, version}` (no secrets).
2. **Detect already-running and stale state**: port occupied → check recorded PID via
   `tasklist`; same tree alive → report `running` (exit 0, optionally open UI); PID
   dead but port busy → report `port-in-use by foreign process` (never kill); PID file
   stale (process gone) → clean up state file, treat as absent.
3. **Stop exactly the managed tree**: `taskkill /T /PID <recorded>` first, escalate to
   `/F` after a grace timeout; verify port free and process gone; delete state file.
   Acceptance: after stop, zero processes of the recorded tree remain (`tasklist`
   check) and port 3080 is free.
4. **Expose doctor / open-UI / log access**: launcher subcommands delegate to
   `pimp-dsh doctor`, open `http://127.0.0.1:3080` via the default browser, and print
   the log path. Acceptance: each action works headlessly and returns structured,
   non-interactive output.
5. **Avoid storing secrets**: the launcher never writes `PIMP_*`/`DSH_*` env values to
   any file; log redirection captures stdout/stderr only. Acceptance: state/log dir
   contains no occurrences of configured key values.
6. **Shortcut install/uninstall is idempotent**: `.lnk` created via `IShellLink`
   (PowerShell `WScript.Shell` COM) targeting the resolved `node.exe` with the exact
   launcher arguments; reinstall replaces it; uninstall removes only the shortcut and
   launcher state files, never profile data.

**Deferred, not rejected**: a Tier B/C tray supervisor. If Phase-0 usage shows demand,
the designated upgrade path is **Tauri 2** (first-party tray + single-instance +
shell-plugin spawn with capability-scoped `pimp-dsh` args + NSIS installer + signed
updater), with a Job Object or `taskkill /T` for tree stop and the port probe retained
as the liveness oracle for the harness.

## 7. Explicitly rejected alternatives

- **Electron**: bundles a second Chromium+Node runtime into a Node-native product
  (per-official distribution model), highest footprint/attack surface, and adds nothing
  Phase-0 needs. Rejected outright.
- **WinUI 3 tray supervisor now**: no first-party tray API (community `H.NotifyIcon`
  only), plus WinAppSDK runtime installation (4 MSIX packages + VCRedist) or a large
  self-contained payload — heavy cost for the only feature (tray) Phase 0 does not need.
- **MSIX packaging now**: mandatory signing, package identity, and container semantics
  imposed on what is today a plain npm CLI; the documented MSIX tray defects (§4.3)
  make it a poor fit for a supervisor before the product even needs one. Revisit only
  if Store distribution is requested.
- **Bundling a portable Node sidecar** (Tauri `externalBin`): removes the Node
  prerequisite but adds ~30-90 MB per arch to the installer, a second supply chain to
  patch, and breaks the product's "runs on your installed, supported Node" contract
  ([package.json `engines`](../package.json)). Rejected.
- **A tray process that is also the harness's parent** without the port probe: any
  supervisor that does not treat `127.0.0.1:3080` as the liveness oracle will false-
  positive on stale PID files and cannot distinguish "harness crashed" from "harness
  still starting". The probe is mandatory in every tier.

## 8. Lifecycle / state-machine outline

States (supervisor-owned; single managed harness per profile):

```
ABSENT ──start()──▶ STARTING ──port up + process alive──▶ RUNNING
  ▲                     │                                   │
  │                     └─timeout────────────────────▶ FAILED_START (state file removed)
  │                                                           
STOPPING ◀──stop()───── RUNNING ──child exit/port loss────▶ CRASHED (state file kept, PID re-checked)
  │                                                            │
  ▼                                                            ▼
STOPPED (verify: port free, tree gone)                STALE ──next start()──▶ cleanup + STARTING
```

Transitions:

- `start()` = probe port → (busy) report `already-running` (match PID vs state file →
  `RUNNING` or `PORT_IN_USE_FOREIGN`); (free) spawn, write state file, poll port with
  deadline → `RUNNING` or `FAILED_START`.
- `stop()` = read PID → `taskkill /T /PID` → grace poll → `/F` → verify port free →
  remove state file → `STOPPED`.
- `crash` = child exit or port release observed → `CRASHED`; next `start()` treats a
  dead-PID state file as `STALE` and cleans before starting.
- All state files are advisory; the **port + live PID are the source of truth**; the
  state file is never trusted alone (PID reuse risk, §9).

## 9. Key Windows edge cases

1. **No POSIX signals.** Node `kill()` on Windows is a forceful single-process
   termination; tree semantics require `taskkill /T` or a Job Object
   ([Node](https://nodejs.org/api/child_process.html#subprocesskillsignal),
   [taskkill](https://learn.microsoft.com/en-us/windows-server/administration/windows-commands/taskkill)).
2. **PID reuse.** "if the process identifier (PID) has been reassigned to another
   process, the signal will be delivered to that process instead"
   ([Node](https://nodejs.org/api/child_process.html#subprocesskillsignal)) → always
   verify (image name + creation time or liveness) before killing a recorded PID.
3. **Console flash / tree pollution.** Spawn with `windowsHide`/`CREATE_NO_WINDOW` and
   `shell: false`; spawning via `cmd /c` inserts `cmd.exe` as the tree root, changing
   what `taskkill /T` kills
   ([Node spawn docs](https://nodejs.org/api/child_process.html#spawning-bat-and-cmd-files-on-windows)).
4. **`.bat`/`.cmd` are not directly spawnable** on Windows without a shell
   ([Node](https://nodejs.org/api/child_process.html#spawning-bat-and-cmd-files-on-windows))
   → the `.lnk` target should be the resolved absolute `node.exe`, not a wrapper `.cmd`.
5. **Environment variable case-insensitivity** on Windows ([Node](https://nodejs.org/api/child_process.html))
   → pass-through env must avoid duplicate keys (`PATH`/`Path`).
6. **Port 3080 may be held by a foreign process or the harness may bind elsewhere.**
   Foreign holder → never kill; report. The supervisor should treat the port probe as
   the liveness oracle and degrade to process-state checks if the probe is ambiguous.
7. **Job Object constraints.** Assigning a process that is already in a job can fail;
   breakaway flags would let harness children escape; nested jobs need Windows 8+
   ([Job Objects](https://learn.microsoft.com/en-us/windows/win32/procthread/job-objects)).
   `taskkill /T` has none of these constraints and matches project convention.
8. **Signing/SmartScreen.** Any future Tier B `.exe` downloaded via browser triggers
   SmartScreen until code-signed; updater ecosystems (Tauri) additionally require
   private signing keys that cannot be lost
   ([Tauri signing](https://v2.tauri.app/distribute/sign/windows/),
   [Tauri updater](https://v2.tauri.app/plugin/updater/)).
9. **MSIX-specific tray/instance defects** if MSIX is ever chosen: notification click
   spawning a second instance; tray icon re-registration on version updates; tray
   identity tied to exe path when unsigned
   ([#6031](https://github.com/microsoft/WindowsAppSDK/issues/6031),
   [#6408](https://github.com/microsoft/WindowsAppSDK/issues/6408),
   [Electron Tray](https://www.electronjs.org/docs/latest/api/tray)).
10. **Install-environment drift.** `.lnk` targets a resolved `node.exe` at install time;
    Node upgrades can invalidate the absolute path → store the target and re-resolve
    (`where node`) on every launcher run, falling back to PATH.

## 10. Open risks

- **No graceful-shutdown API.** The upstream harness on Windows can be interrupted only
  by console Ctrl-C or forced termination; `taskkill` (optionally `/F`) may skip
  harness cleanup. Risk of unsaved state; needs an upstream check or a documented
  "stop is force-stop" posture.
- **Port probe is indirect.** The harness exposes no documented health endpoint; if
  upstream ever makes 3080 optional or ports dynamic, liveness detection breaks.
  `doctor` output should be extended (JSON) to expose the bound URL.
- **Upstream CLI contract drift.** Launcher spawns the exact `dsh` bin path and args
  (`--profile`); upstream renames/version bumps (currently pinned `0.1.0-rc.6`) would
  break it — covered by the existing pin + `update-check`/`migrate` commands, but the
  launcher must be part of the same release train.
- **PID-file/state races.** Two concurrent launcher starts can both pass the probe and
  double-spawn; needs an atomic state-file create (`wx` flag, as the CLI already does)
  or a mutex around `start()`.
- **Tray dependency risk (future tiers).** Tauri: WebView2 servicing, Rust toolchain,
  VBSCRIPT deprecation for MSI builds, updater key custody. .NET/WinUI: no first-party
  tray API — pinned to community `H.NotifyIcon` maintenance.
- **Secret hygiene.** Env-based keys flow into the child; log capture must never include
  env dumps, and the state file must not mirror `process.env`.
- **No machine-wide guarantee.** Phase-0 scope is per-user (user Start Menu/Desktop,
  `DSH_HOME`); machine-wide deployment (Program Files, service) is out of scope and
  changes ACL/elevation behavior.

## 11. Terminology for the goal prompt (exact terms)

`taskkill /T` (tree stop) · `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` · `CREATE_NO_WINDOW` ·
`CREATE_NEW_PROCESS_GROUP` · `IShellLink`/`IPersistFile` `.lnk` · `windowsHide: true`,
`shell: false` · PID file + port probe `127.0.0.1:3080` · `DSH_HOME` state dir ·
no-fork boundary · CLI as sole lifecycle authority · single managed process · stale/
foreign-port detection · no persisted secrets · `pimp-dsh doctor` · idempotent shortcut
install/uninstall.

## 12. Source index

1. https://learn.microsoft.com/en-us/windows/win32/shell/links
2. https://learn.microsoft.com/en-us/windows-server/administration/windows-commands/taskkill
3. https://learn.microsoft.com/en-us/windows/win32/procthread/job-objects
4. https://learn.microsoft.com/en-us/windows/win32/procthread/process-creation-flags
5. https://nodejs.org/api/child_process.html
6. https://learn.microsoft.com/en-us/dotnet/core/deploying/single-file/overview
7. https://learn.microsoft.com/en-us/dotnet/api/system.diagnostics.process.kill
8. https://learn.microsoft.com/en-us/windows/apps/windows-app-sdk/deployment-architecture
9. https://learn.microsoft.com/en-us/windows/apps/windows-app-sdk/deploy-unpackaged-apps
10. https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/
11. https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/self-contained-deploy/deploy-self-contained-apps
12. https://learn.microsoft.com/en-us/windows/apps/windows-app-sdk/applifecycle/applifecycle-instancing
13. https://learn.microsoft.com/en-us/windows/msix/overview
14. https://github.com/microsoft/WindowsAppSDK/discussions/519
15. https://github.com/microsoft/WindowsAppSDK/discussions/3394
16. https://github.com/microsoft/WindowsAppSDK/issues/4063
17. https://github.com/microsoft/WindowsAppSDK/issues/6031
18. https://github.com/microsoft/WindowsAppSDK/issues/6408
19. https://github.com/HavenDV/H.NotifyIcon
20. https://v2.tauri.app/start/prerequisites/
21. https://v2.tauri.app/distribute/windows-installer/
22. https://v2.tauri.app/plugin/shell/
23. https://v2.tauri.app/learn/system-tray/
24. https://v2.tauri.app/plugin/single-instance/
25. https://v2.tauri.app/plugin/updater/
26. https://v2.tauri.app/distribute/sign/windows/
27. https://v2.tauri.app/develop/sidecar/
28. https://www.electronjs.org/docs/latest/tutorial/process-model
29. https://www.electronjs.org/docs/latest/tutorial/application-distribution
30. https://www.electronjs.org/docs/latest/api/tray
31. https://www.electronjs.org/docs/latest/api/auto-updater
32. Repo: README.md, src/cli.ts, docs/windows-support.md, docs/roadmap.md, docs/adr/0001-no-fork.md, package.json

# Desktop Control Surface for `pimp-my-dsh` — Stack & Lifecycle Decision Record

- Status: **research/decision artifact** (no implementation). Owner: `ModernDesktopMatrix` agent.
- Last revised: **2026-08-16 (revision 2)** — incorporates the read-only lifecycle/security re-evaluation (`LifecycleSecurity` review, same date) of the exact-pinned `@deepseek-ai/dsh@0.1.0-rc.6` lifecycle, the repo CLI, and official Windows/Node/Tauri/Fetch/WebSocket documentation.
- Purpose: define the stack and lifecycle architecture for a **modern, polished, effective** native desktop control surface (tray + process supervisor) for the Windows-first `pimp-my-dsh` harness, and produce the evidence base for the OMP goal prompt.
- Skip list (explicit non-goals): implementation code, formatters, linters, builds, tests, generic secondary blog posts. Every material factual claim links to a primary/official source (see [Source index](#source-index)); each matrix row carries an evidence/confidence grade.
- Evidence convention: **High** = verbatim on a primary source re-read today (2026-08-16); **Medium** = primary source, stable content, carried from a prior verified read or a longstanding official page not re-read today; **Low** = inference or community-only. **Design recommendation** marks architecture we prescribe in the goal prompt; it is not quoted as vendor syntax.

## 1. Decision (summary)

**Ship a persistent per-user Tauri 2 (v2.11.x) tray/controller.** Closing the window hides it; it remains resident while a harness run exists. It is **not** a Windows service.

- **Authority:** the **Rust backend is the only supervisor** and owns the state machine, an **unnamed kill-on-close Job Object**, the live process handles, logs, the updater drain, and the browser-open decision. **JavaScript is a view; the Node CLI is a validated foreground launch adapter.** The controller JS gets **no shell plugin**.
- **Identity:** the retained **process HANDLE, Job handle, and random run ID are the authority**. PID, image path, creation time, port, and state file are **diagnostics only** — never grounds to kill or adopt a process.
- **Port:** default launch is **dynamic** (`--host 127.0.0.1 --port 0`, OS-assigned). A port probe is an optional health facet, not ownership/liveness truth. Fixed 3080 is opt-in compatibility; a foreign owner causes a clear error and is never killed or opened.
- **Stop:** **cooperative upstream bounded disposal first** — the pinned rc.6 harness disposes the whole application on SIGINT/SIGTERM with a 5-second forced-exit bound; the controller invokes that same path over a private bridge, then falls back to `TerminateJobObject` only on deadline, and reports the stop as **forced**, not graceful.
- **No separate daemon / PID lifecycle:** CLI commands that independently detach, store PIDs, or start/stop behind the controller are rejected. Future command-line control, if any, is a thin authenticated client of the running Tauri supervisor.
- **Superseded recommendations (explicit):** `.lnk` + terminal script architecture, PID-file authority, mandatory fixed-port probe, and an independent Node daemon. A shortcut may remain an installer-created entry point into the native app — it is not the architecture.
- **Stack rationale unchanged:** Tauri 2 over WinUI 3/WASDK 2.4 (runner-up; still no first-party tray), Electron 43, Wails, Flutter 3.47, Slint, and the lightweight Rust/Kotlin alternatives — see §4 and §8.

## 2. Terminology: three distinct things

| Term | Meaning in this record |
| --- | --- |
| **Native-looking** | Rendered with OS/web primitives that match platform conventions (Fluent-style controls, tray menu, taskbar behavior) without necessarily using the OS's own UI toolkit. Tauri (WebView2), Electron, Wails, Neutralino are native-looking via web rendering. |
| **Native shell integration** | Participation in OS surfaces: notification-area (tray) icon, taskbar/Start Menu entries, file associations, jump lists, notifications. Independent of rendering technology. |
| **Truly native rendering** | Pixels produced by the OS toolkit or GPU-native engine — WinUI 3 (XAML/DirectX), WPF, Avalonia, Flutter (Impeller), Slint/iced/egui (wgpu/GL), Win32. Tauri/Electron/Wails are **not** truly native rendering; they host a web engine. |

Three delivery tiers:

| Tier | Artifact | Supervision |
| --- | --- | --- |
| A | Desktop shortcut + Node script (`.lnk` → `node.exe`) | None (superseded as architecture; shortcut only as an entry point into the native app) |
| B | Native launcher `.exe` | During run only |
| C | **Persistent tray / process-supervisor app (chosen)** | Continuous |

Tiers B and C are the same binary here: a resident controller that also launches.

## 3. Current-release snapshot (dated evidence, checked 2026-08-16)

| Stack | Latest stable | Release date | Source | Confidence |
| --- | --- | --- | --- | --- |
| Tauri | **2.11.5** | 2026-07-01 | [S1] | High (re-checked today) |
| Windows App SDK / WinUI 3 | **2.4.0** (2.0 line stable since 2.0.1, 2026-04-29; 1.8 line still serviced: 1.8.10, 2026-07-14) | 2026-08-13 | [S2][S3][S4] | High (re-checked today) |
| .NET (runtime) | **10.0.11** (LTS; .NET 11 in preview 7) | 2026-08-11 | [S5][S6] | High (re-checked today) |
| Avalonia | **11.3.17** | 2026-05-27 | [S7] | High (re-checked today) |
| Electron | **43.4.0** (Chromium 150, Node 24.18.1; 44.0.0-beta with Chromium 152) | 2026-08-11 | [S8][S9] | High (re-checked today) |
| Wails | v2 line stable: **2.14.0**; **v3.0.0-beta.8** (beta; not GA) | 2026 (beta.8: 2026-08) | [S10][S11] | High for v3-beta status; Medium for exact v2 patch |
| Flutter (desktop) | **3.47** — Impeller now the **default renderer on Windows** | 2026-08-12 | [S12][S13] | High (re-checked today) |
| Slint | **1.17.1** — `SystemTrayIcon` first shipped in 1.17.0 (2026-06-24) | 2026-07-07 | [S14][S15] | High (re-checked today) |
| Dioxus | **0.7.10** (0.8.0 in alpha) | 2026 (0.8-alpha: 2026-07-31) | [S16] | High for version; Medium for claims |
| iced | **0.14.0** | 2025-12-07 | [S17] | High (re-checked today) |
| egui | **0.36.1** | 2026-08 | [S18] | High (re-checked today) |
| Neutralinojs | **6.8.0** | 2026-06-03 | [S19] | High (re-checked today) |
| Kotlin Compose Multiplatform | **1.11.1** | 2026-05 | [S20] | High (re-checked today) |
| `@deepseek-ai/dsh` (upstream pin) | **0.1.0-rc.6** (exact pin; see §5) | per repo `docs/upstream-pin.md` | [S58]–[S62] | High (read today from `node_modules`) |

## 4. Candidate stacks

### 4.1 Tauri 2 (chosen)

Official facts (Tauri v2 docs, [S21]–[S28]):

- **Rendering:** WebView2 on Windows ("Tauri uses Microsoft Edge WebView2 to render content on Windows" — prerequisites [S21]); WKWebView on macOS, WebKitGTK on Linux. The WebView2 *Evergreen runtime* is preinstalled on Windows 10/11 machines that receive Edge updates; the Tauri installer handles the residual case (§ installer modes). Confidence: High.
- **Installer:** NSIS `-setup.exe` or **WiX v3** `.msi`; `.msi` can only be built on Windows [S22]. WebView2 install modes: `downloadBootstrapper` (+0 MB, needs internet, default), `embedBootstrapper` (~1.8 MB), `offlineInstaller` (~127 MB), `fixedVersion` (~180 MB), `skip` (not recommended) [S22]. Confidence: High (page re-read today).
- **Tray:** first-class — `tray-icon` Cargo feature, `TrayIcon.new()` JS API with native menu and click events [S23]. Confidence: Medium (carried verified read).
- **Single instance:** official plugin, Windows fully supported [S24]. Confidence: Medium (carried).
- **Process spawn:** `@tauri-apps/plugin-shell` exists with capability-scoped `Command.create(program, args)` [S25] — but **this controller does not use it at all**. Spawning happens in **Rust** (where Job Object assignment, handle retention, and suspend/resume are possible); the JS view has no shell plugin. The plugin's permission model is cited only as evidence of Tauri's deny-by-default capability system (default permission = open `http(s)://`, `tel:`, `mailto:` only; explicit `shell:allow-execute`/`shell:allow-spawn` scopes otherwise) [S25]. Confidence: High (page re-read today).
- **Open-URL:** the `shell.open` API is documented under the separate **Opener plugin** [S25]. Browser-open is decided **in Rust** from the validated READY URL; the JS view never supplies the URL (see §9/§11 `OPEN-01`).
- **Updates:** updater plugin **requires signed updates** ("Tauri's updater needs a signature … This cannot be disabled"); Windows artifacts are `-setup.exe`/`.msi` + `.sig`; `installMode` = `passive` (default) / `basicUi` / `quiet`; endpoints must be HTTPS in production [S26]. Confidence: High (page re-read today). Updater config `createUpdaterArtifacts` "**will be removed in v3**" [S26] — a concrete v2→v3 migration risk (§12).
- **Code signing:** not required to run; required for Microsoft Store and to avoid SmartScreen "not trusted" on browser downloads; supported via `signtool` (OV/EV) or Azure Key Vault [S27]. Confidence: Medium (carried).
- **Sidecar:** `externalBin` for bundling extra binaries [S28] — relevant only for the optional app-private bundled Node payload (§12), not for the bridge.
- **Accessibility/testing:** WebView2 inherits Chromium's accessibility tree; E2E via WebDriver (`tauri-driver`) [S29]. Confidence: Medium.
- **Footprint:** Rust core + web assets (a few MB); runtime add is WebView2, shared system-wide and evergreen-patched by Microsoft.

### 4.2 Windows App SDK 2.4 / WinUI 3 (runner-up) — plus WPF and Avalonia

- **Versions/channels:** Stable 2.4.0 (2026-08-13); 2.0.1 first SemVer-major stable (2026-04-29); 1.8.x servicing continues (1.8.10, 2026-07-14) [S2][S3][S4]. Confidence: High.
- **Deployment (unpackaged, framework-dependent):** Windows App Runtime installer (Framework/Main/Singleton/DDLM packages), Bootstrapper initialization, **VCRedist required**; machine-wide install needs elevation [S30][S31]. Self-contained (`WindowsAppSDKSelfContained=true`) bundles the runtime; **`PublishSingleFile` only for unpackaged self-contained apps (1.5+) and not for packaged (MSIX)** [S32]. Confidence: Medium (carried).
- **MSIX:** mandatory signing before installation, differential (64 KB block) updates, app-container semantics [S33]. Confidence: Medium (carried).
- **Single instance:** `AppInstance.FindOrRegisterForKey` / `GetInstances` + activation redirection (packaged and unpackaged) [S34]. Confidence: Medium (carried).
- **Process tree kill (.NET):** `Process.Kill(bool entireProcessTree)` first-party [S35]; Job Objects also available. Confidence: Medium (carried).
- **Tray gap (critical, still true in 2.x):** no first-party notification-area API. WinRT `SystemTray` unusable on desktop; discussions #519/#3394 open with no commitment; nothing tray-related in 2.0–2.4 stable release notes ([S36][S37][S38][S4]). Community path: `H.NotifyIcon` [S39]. Known MSIX tray defects: notification click can launch a second instance ([S40]); tray icon re-registers on exe-path change ([S41]). Confidence: High for "no first-party API" (re-checked today); Medium for issue threads (carried).
- **Microsoft's own guidance** ("Choose your app framework", updated 2026-08-09): "WinUI … is the recommended UI framework for new Windows apps"; WPF "actively maintained" and modernizable with WASDK; the feature table shows WinUI is **not cross-platform** [S42]. Confidence: High (page re-read today).
- **Avalonia 11.3** (cross-platform .NET/XAML alternative): **first-party `TrayIcon`** with native menu, supported on Windows/macOS and Linux (StatusNotifierItem/AppIndicator DEs) [S43]; runs on .NET 10; XAML-styled rather than Fluent-native on Windows; same "second language for a TS/Node team" cost. Confidence: High (docs re-checked today).

### 4.3 Electron 43

- Multi-process model: one main process (Node) + a renderer **per window** + utility processes [S44]; distribution ships Electron's prebuilt Chromium+Node binaries + `resources/app` — **every app bundles a second, full Chromium+Node runtime** [S45]. Confidence: Medium (carried; official model unchanged).
- Tray: first-class; on Windows the tray GUID is tied to the code signature when signed, else to the exe path ("Changing the path to the executable will break the creation of the tray icon") [S46]. Confidence: Medium (carried).
- Updates: `autoUpdater` = Squirrel.Windows or MSIX updater; Squirrel requires a specific AUMID (`com.squirrel.<id>.<exe>`) [S47]. Confidence: Medium (carried).
- Current runtime: v43.4.0 = Chromium 150 + Node 24.18.1 [S8]. Rejected: for a product that *is* Node and already has a web UI, Electron adds the largest redundant footprint and attack surface for zero unique capability (§8).

### 4.4 Wails (v2.14 stable / v3 beta)

- Go + system WebView (WebView2 on Windows); footprint mirrors Tauri but the backend is **Go** — a second language and ecosystem for a TS/Node team [S10][S48].
- v2.14.0 stable; **v3 in beta (v3.0.0-beta.8)** with "API is stable but pre-release" warnings; v2→v3 migration is a real task [S11][S48]. No first-party updater; tray/single-instance from community libraries. Rejected (§8).

### 4.5 Flutter desktop 3.47

- **Truly native rendering** (Impeller — shaders precompiled at build time — now the default renderer on Windows/macOS/Linux, replacing Skia; Vulkan on Windows) [S12][S13]. Confidence: High.
- Dart is a new language; no first-party tray (community `system_tray`), no first-party updater; MSIX packaging; Windows **multi-window API still experimental** as of 3.44.8/3.47; desktop accessibility historically weaker than UIA/web on Windows [S49][S50]. Confidence: Medium.
- Rejected: the app is a *control surface around an existing web UI*, which Flutter's canvas cannot reuse (§8).

### 4.6 Slint 1.17

- Declarative UI DSL compiled to native code (Rust/C++/JS/Python); cross-platform; `SystemTrayIcon` (Windows/macOS/Linux) **only landed in 1.17.0 (2026-06-24)** [S14][S15] — one release old. Confidence: High.
- No first-party single-instance, installer, updater, or signing story; small widget ecosystem; immature accessibility. Rejected: everything beyond rendering would be hand-rolled, and the web UI cannot be reused (§8).

### 4.7 Other lightweight alternatives (one column in the matrix)

- **Dioxus 0.7.10** (0.8 in alpha): Rust + WebView2 on desktop; younger than Tauri, no first-party updater/tray plugin ecosystem [S16]. Confidence: Medium.
- **iced 0.14.0** / **egui 0.36.1**: native GPU-rendered Rust UIs; tray via community crates; no installer/updater/single-instance story; minimal OS accessibility tree [S17][S18]. Confidence: Medium.
- **Neutralinojs 6.8.0**: ~2 MB WebView runner; has a tray API; no first-party updater; small ecosystem [S19]. Confidence: Medium.
- **Compose Multiplatform 1.11.1**: Kotlin/JVM; tray via AWT interop, packaging via jpackage; worst stack fit for this repo [S20]. Confidence: Medium.

### 4.8 Tier A (superseded)

A `.lnk` + Node wrapper script is no longer the architecture. It may remain an **installer-created entry point into the native controller**; it never owns lifecycle state, PIDs, or ports.

## 5. Current CLI & upstream lifecycle facts (repo grounding, read 2026-08-16)

Any controller must treat the existing CLI as the **only validated launch boundary** (no-fork ADR: [docs/adr/0001-no-fork.md](adr/0001-no-fork.md); roadmap: [docs/roadmap.md](roadmap.md)). Grounding facts — two corrections to the previous record are marked:

- **`run` is a blocking foreground process** executed via `spawnSync(..., { stdio: 'inherit', env: harnessEnvironment(), shell: false, windowsHide: false })` and then `process.exit(child.status)` ([src/cli.ts](../src/cli.ts) ≈L375–388) [S63]. **Correction 1:** the current `run` path uses **`windowsHide: false`** (console-visible) — the prior record's `windowsHide: true` claim applied to package-manager subprocesses, not to `run`. The controller therefore supplies hidden-window semantics itself (`CREATE_NO_WINDOW`, §6) and pipes stdio; it never relies on the CLI hiding anything.
- **Upstream graceful disposal is a real, bounded contract (exact pin 0.1.0-rc.6):** the harness boot controller defines `const PROCESS_SHUTDOWN_TIMEOUT_MS = 5e3` and installs `process.on("SIGTERM", ...)` / `process.on("SIGINT", ...)` handlers that `await app.current?.fiber.dispose()` before the forced-exit bound (SIGTERM maps to exit 0, SIGINT to 130) ([node_modules/@deepseek-ai/dsh/lib/profile-boot-DG5t9aNs.js](../node_modules/@deepseek-ai/dsh/lib/profile-boot-DG5t9aNs.js) L9–62 and L223–239) [S58]. **Correction 2:** the prior record's "no graceful-shutdown API / stop is force-stop" posture is wrong. The controller invokes this same whole-app disposal path cooperatively over the control bridge and only escalates on deadline (§9).
- **Dynamic readiness is supported without log parsing:** the pinned web CLI documents `--port 0` ("listen port; pass 0 to let the OS pick a free one") and rejects `0.0.0.0` exposure ([node_modules/@deepseek-ai/dsh-web-app/lib/startup.js](../node_modules/@deepseek-ai/dsh-web-app/lib/startup.js) L21–45) [S59]. The WebServer records the actual bound port after bind (`this.listenedPort = this.server.address().port`) and exposes `get port(): number` on its public service; its disposal path closes HTTP **and upgraded (WebSocket) sockets** ([node_modules/@deepseek-ai/dsh-host-webserver/lib/index.js](../node_modules/@deepseek-ai/dsh-host-webserver/lib/index.js) L168–190; [lib/types/index.d.ts](../node_modules/@deepseek-ai/dsh-host-webserver/lib/types/index.d.ts) L37–65) [S60][S61].
- **Loopback reachability is not authority:** the pinned connection carrier validates Host/Origin/Fetch Metadata but "The fence is a reachability policy, not authentication; the Web carrier provides no authentication layer" ([node_modules/@deepseek-ai/dsh-client-connection/README.md](../node_modules/@deepseek-ai/dsh-client-connection/README.md)) [S62]. Controller start/stop/update/log authority therefore belongs to **Tauri IPC and the private child bridge** (§9), never to `/api` on the harness port.
- **Secrets:** the CLI reads `PIMP_DSH_API_KEY` / `PIMP_DSH_BASE_URL` / `PIMP_DSH_MODEL` / `PIMP_DSH_ENABLE_LSP` from the environment, promotes them to protected upstream names, then deletes the public ones ([src/cli.ts](../src/cli.ts)). The controller passes environment through **live** and never persists or logs env values (§10).

## 6. Windows platform primitives (grounding for the lifecycle architecture)

- **Hidden suspended launch:** `CREATE_NO_WINDOW` (0x08000000) — "The process is a console application that is being run without a console window"; `CREATE_SUSPENDED` (0x00000004) — the primary thread starts suspended so the controller can **assign the Job before the child runs** (closing the pre-assignment escape race) [S52]. Rust: `std::os::windows::process::CommandExt::creation_flags` or direct `CreateProcessW`.
- **Explicit handle inheritance:** `CreateProcessW` inherits only handles marked inheritable unless `PROC_THREAD_ATTRIBUTE_HANDLE_LIST` (via `UpdateProcThreadAttribute`) supplies an explicit allowlist; the controller passes a fixed allowlist (child stdio pipes + the bridge pipe) so no stray handle (e.g., the Job handle) leaks into the child [S64][S65]. The Job handle is retained **non-inheritable** by the controller.
- **Job Objects (unnamed, kill-on-close):** `CreateJobObject` (with a `NULL` name — no global name to squat), `AssignProcessToJobObject`, `TerminateJobObject`; "by default any child processes it creates using `CreateProcess` are also associated with the job"; with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`, "closing the last job object handle terminates all associated processes and then destroys the job object itself"; nested jobs exist since Windows 8 [S54][S66][S67]. Optional: associate a completion port (`JOB_OBJECT_ASSOCIATE_COMPLETION_PORT`) for exit notifications [S68].
- **Tree stop:** `taskkill /t` ("Ends the specified process and any child processes started by it") remains the documented repo fallback [S53]; the primary mechanism is `TerminateJobObject` [S67]. Node's `subprocess.kill()` is **not** a tree kill on Windows [S55] — the controller never relies on it.
- **PID reuse is real:** "if the process identifier (PID) has been reassigned to another process, the signal will be delivered to that process instead" [S55]; Microsoft documents PID-reuse hazards explicitly [S69]. **Consequence (authority rule):** the controller never kills or adopts a process based on a persisted PID, image path, or port owner. Only its own live handles + run ID authorize action.
- **Console control is limited:** `GenerateConsoleCtrlEvent` can only send to console processes that share the caller's console and cannot target a process group of a process in a different console [S70] — another reason the cooperative bridge (not console signals) carries shutdown.
- **Named pipe security (bridge transport):** DACL limited to the current user and SYSTEM, `PIPE_REJECT_REMOTE_CLIENTS`, first-instance creation (reject pre-created pipe squatting), bounded frame sizes, `CreateNamedPipe` flags [S71][S72].
- **Single instance:** named mutex (classic unpackaged approach) or WASDK `AppInstance.FindOrRegisterForKey` [S34]; the Tauri controller uses the official single-instance plugin [S24].
- **`.bat`/`.cmd` are not directly spawnable** without a shell [S55] → the controller always spawns the resolved absolute `node.exe` with a fixed argv (no `cmd /c`, no PATH fallback), so `cmd.exe` is never inserted into the tree.
- **Environment case-insensitivity on Windows** → deduplicate `PATH`/`Path` when passing env through live [S55].

## 7. Comparison matrix

Criteria × stack. `[S#]` = source index; **Conf** = evidence confidence (H/M/L). "Tier A" column = `.lnk` + Node script (superseded).

| Criterion | Tier A (entry point only) | **Tauri 2.11** (chosen) | WinUI 3 / WASDK 2.4 | Avalonia 11.3 (.NET 10) | Electron 43 | Wails 2.14 / v3-β | Flutter 3.47 | Slint 1.17 | Lightweight (Dioxus/iced/egui/Neutralino/CMP) |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| One clickable app | `.lnk` → native app | Yes (NSIS/MSI) [S22] — H | Yes (MSIX or self-contained) [S32] — M | Yes (jpackage/single-file) — M | Yes — H | Yes (NSIS) [S48] — M | Yes (MSIX) [S49] — M | Yes (self-built installer) — M | Yes (varies) — M |
| Rendering | Terminal | WebView2 (web) [S21] — H | XAML (truly native) [S42] — H | XAML (GPU) [S43] — H | Chromium — H | WebView2 [S48] — M | Impeller GPU (truly native) [S12] — H | Native GPU (DSL) [S14] — H | Mixed; mostly GPU-native |
| **Tray (first-party)** | None | **Yes** (`tray-icon`) [S23] — M | **No** (community `H.NotifyIcon` only) [S36][S37][S39] — H | **Yes** (`TrayIcon`) [S43] — H | Yes [S46] — M | Community library — M | Community package — M | Yes, new in 1.17 [S15] — H | Dioxus/Neutralino partial; iced/egui community — M |
| Single instance | None | Official plugin [S24] — M | `AppInstance` API or mutex [S34] — M | Named mutex (DIY) — M | Built-in — M | DIY — M | DIY — M | DIY — M | DIY — M |
| Process-tree control | `taskkill /T` on recorded PID (never authority) | **Unnamed kill-on-close Job Object; cooperative upstream disposal first** [S54][S58] — H | `Process.Kill(entireProcessTree)` first-party [S35] — M | Same as WinUI (.NET) — M | `child.kill()` ≠ tree; needs `taskkill` [S55] — H | DIY (Go) — M | DIY (Dart FFI) — L | DIY — L | DIY — L |
| Frontend fit (TS/Node team) | Exact (same language) | High: web UI reused for the view; Rust confined to the supervisor core | Poor: XAML/C# second stack; harness UI is web | Poor: XAML/C# | High JS, but redundant Node | Poor: Go | Poor: Dart, web UI not reusable | Poor: DSL | Mixed (Rust/Kotlin/JS) |
| Runtime footprint | ~KB, reuses installed Node | App (MBs) + WebView2 (evergreen, system-shared) [S22] | Runtime installer (4 packages + VCRedist) or large self-contained [S30][S31] | .NET self-contained (~60–150 MB) | Chromium+Node per app (~150–250 MB) [S45] | Small (Go) + WebView2 | Dart AOT (~20–50 MB) | Small native | Small-to-medium |
| Installer / signing / updater | npm update; no signing | NSIS/MSI; `signtool`; **updater built-in, signed updates mandatory** [S22][S26][S27] | MSIX differential + Store; signed mandatory; unpackaged = DIY updater [S33] | DIY updater; standard signing | Forge/builder; Squirrel/MSIX updater [S47] | NSIS; **no first-party updater** [S48] | MSIX; no first-party updater | DIY everything | DIY everything |
| Accessibility | Terminal only | Chromium a11y via WebView2 — M | UIA first-party — H | UIA — M | Chromium a11y — M | Chromium a11y — M | Weaker on Windows — L | Immature — L | Mostly poor (iced/egui) — L |
| Testing | CLI contract tests (existing) | Rust unit + WebDriver (`tauri-driver`) [S29] — M | MSTest + WinAppDriver — M | xUnit + UI tests — M | Playwright — H | Go tests — M | widget tests + integration_test — M | Slint test harness — M | Rust tests — M |
| macOS/Linux path | N/A (Windows `.lnk`) | Same codebase (WKWebView/WebKitGTK) [S21] — H | None (Windows-only) [S42] — H | Yes (first-party) [S43] — H | Yes | Yes | Yes | Yes | Yes |
| Maturity / governance | N/A | 2.11.x, large ecosystem, stable since 2.0 (2024) — H | Microsoft; 2.x new major (Apr 2026), 1.8 still serviced [S2] — H | Community/OSS company — M | Most battle-tested — H | v3 in beta (transition risk) [S11] — H | Google; desktop "supported" [S49] — M | Young tray API — H | Small — M |

## 8. Decision rationale

**Why Tauri 2 wins for this product:**

1. **The product is already web-shaped.** The harness *is* a Node CLI plus a web UI. A Tauri controller supplies what the web UI cannot: OS shell presence (tray, single instance, autostart, notifications, updater) — with the view written in the team's existing web stack and the harness UI untouched in the user's browser.
2. **Every lifecycle feature is first-party and officially documented:** tray (`tray-icon`), single-instance plugin, NSIS/MSI installers, signed updater with `passive` install mode, `signtool` signing path [S22]–[S27]. WinUI 3 fails the most important one (tray) and every other webview/lightweight stack fails two or more (updater, single instance, installer).
3. **Security posture is a design feature:** Tauri ships deny-by-default capabilities [S25] and a strict frontend/native split (§9/§10). The supervisor-specific authority rules (live handles, unnamed Job Object, authenticated bridge) are enforced in Rust, where the capabilities system cannot be weakened by web content.
4. **Footprint discipline without sacrificing polish:** the runtime add is the evergreen WebView2 (shared, Microsoft-serviced); the app itself is a few MB; installers choose `downloadBootstrapper` (+0 MB) or `offlineInstaller` (+127 MB) [S22].
5. **Cross-platform path is real:** the same Tauri core runs on macOS/Linux [S21], which WinUI 3 cannot offer and Electron/Wails only match with their own costs. The state machine, protocol, and UI commands stay identical; only the process adapter/transport/signing packaging changes (§12, `PORTABLE-01`).
6. **Maturity:** 2.11.5 with monthly patch cadence through 2026 [S1]; the v2→v3 risk is already visible in the updater docs ("This setting will be removed in v3") [S26] and is managed by pinning + a documented migration trigger (§12).

**Why not WinUI 3 (the strongest native alternative):** truly native Fluent rendering and first-party UIA accessibility are real advantages — but the tray gap is disqualifying for a *tray supervisor* today [S36][S37], unpackaged deployment adds a runtime installer + VCRedist burden [S30][S31] (or a large self-contained payload), the stack is Windows-only [S42], and XAML/C# is a second frontend stack that cannot reuse the harness's web UI. **Re-evaluation trigger:** if Microsoft ships a first-party notification-area API in a stable WASDK release, re-run this matrix; the supervisor core (state machine, bridge protocol, failure invariants, §9) is deliberately platform-independent to make that swap bounded.

**Why not the rest:** Electron = second full Chromium+Node runtime inside a Node product, largest attack surface, nothing unique [S44][S45]. Wails = Go backend + no first-party updater + v3 beta transition [S48][S11]. Flutter = new language, community-only tray/updater, experimental multi-window, weaker Windows a11y, cannot reuse a web UI [S49][S50]. Slint/iced/egui = hand-roll installer/updater/tray/a11y; Slint's tray API is one release old [S15]. Dioxus/Neutralino/CMP = thin ecosystems or worst-fit languages (§4.7). MSIX-now = mandatory signing + container semantics before the product needs them, plus the documented MSIX tray defects [S40][S41]; revisit only for Store distribution.

## 9. Lifecycle architecture (Tauri supervisor — the supervised-bridge design)

**Process model.** Three components, one authority:

- **Controller (Rust, resident):** owns the state machine, the **unnamed kill-on-close Job Object**, the live process/Job handles, stdout/stderr draining, logs, the updater drain, and the browser-open decision. Closing the window hides it; explicit Quit follows the stop policy and never silently detaches the harness.
- **Harness (Node CLI, foreground child):** launched only through the validated `pimp-dsh run` boundary — the existing managed-profile checks, no-global-patch rule, environment promotion, exact pin, and end-of-options boundary remain authoritative in the CLI; the controller does **not** reimplement them.
- **Private readiness/control bridge:** a versioned contract through a supported distribution/upstream composition seam (the repo already carries `cordis.patch.yml` over upstream). **Design recommendation** for the Windows transport: a current-user/SYSTEM-only local named pipe with a **random per-run token**, `PIPE_REJECT_REMOTE_CLIENTS`, first-instance creation (no pre-created pipe), bounded frames, and **no persisted secret** [S71][S72]. This is a prescribed protocol, not existing vendor syntax.

**Launch sequence (fails closed at every step):**

1. Resolve the absolute non-null Node/CLI path (no shell, no PATH fallback, no unquoted executable ambiguity); verify the version contract (§12 compat manifest) before creating any process.
2. `CreateProcessW` with `CREATE_NO_WINDOW | CREATE_SUSPENDED`, an explicit environment, and a fixed argv; stdio = drained pipes; handle inheritance limited by an explicit allowlist (`PROC_THREAD_ATTRIBUTE_HANDLE_LIST`) [S52][S64][S65].
3. Create the **unnamed** Job Object; set `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`; `AssignProcessToJobObject` **while the child is still suspended**; retain non-inheritable Job/process handles.
4. Resume the primary thread only after assignment. Any setup/assignment error → terminate the child, close handles, state = `FAILED_START`. **Never fall back to an unsupervised spawn.**
5. Pass `--host 127.0.0.1 --port 0` explicitly to the web child [S59]. The child reports the actual host/port **only after WebServer initialization** (it already exposes `get port(): number` after recording `server.address().port`) [S60][S61]. The controller validates the authenticated run ID, the literal scheme/host, the pinned versions, and Job membership before declaring READY or opening a browser.

**Stop sequence (cooperative first, forced only on deadline):**

1. Send cooperative shutdown over the bridge; the child invokes the same whole-app disposal path used by upstream SIGINT/SIGTERM (`await app.current?.fiber.dispose()` with `PROCESS_SHUTDOWN_TIMEOUT_MS = 5e3`) [S58].
2. Wait beyond the upstream 5-second bound **and** for Job active-process zero (HTTP and upgraded WebSocket sockets are closed by WebServer disposal [S61]); then classify **GRACEFUL**.
3. On deadline: `TerminateJobObject` [S67], wait until the Job is empty, classify **FORCED** and report it prominently (never call a forced stop graceful). `taskkill /T /F` remains the documented repo fallback [S53].

**Identity & crash rules:**

- Authority = the retained process **HANDLE**, the **Job handle**, and the **random run ID**. PID, image path, creation time, port, and the state file are **diagnostics**. The controller never kills or adopts from a stale PID or from whoever owns a port.
- Supervisor hard-killed (root + grandchild alive) → both die via kill-on-close without any PID lookup. Next launch acquires the abandoned lock and reports the interrupted run; persisted RUNNING state is treated as **interrupted history**, never adopted. Unexpected survivors are **UNMANAGED** and are not killed.
- Root dies while a descendant remains in the Job → run is CRASHED, the residual Job is terminated, and no orphan becomes an authority.

**Port policy:** default start is dynamic (`--port 0`); the managed run succeeds on an OS-selected port and never inspects or terminates whoever holds 3080. A user-requested fixed 3080 that is foreign/busy → start fails with `PORT_IN_USE_FOREIGN`; no kill, adoption, browser open, or stale-state overwrite. A port probe is an optional health facet — never ownership/liveness truth.

**State machine** (advisory state file only; live handles are truth):

```
ABSENT ──start()──▶ STARTING (suspended spawn, Job assign, bridge setup)
  ▲                    │  readiness frame validated → READY (browser open) → RUNNING
  │                    └─any setup/assign/timeout error──────────▶ FAILED_START (no unsupervised fallback)
STOPPING ◀──stop()── RUNNING ──child exit / handle signal / Job event──▶ CRASHED
  │ (cooperative → 5s+ bound → TerminateJobObject)                        │
  ▼                                                                       ▼
STOPPED (GRACEFUL|FORCED; Job empty, pipes drained)      STALE ──next start()──▶ interrupted history + cleanup
```

**Frontend split:** controller assets are bundled and strict-CSP; Tauri permissions are per-window and minimal; **no JavaScript shell, opener, filesystem, or updater access**. The harness page opens in the system browser or a separate **zero-capability** webview — never inside the privileged controller webview. The Rust backend constructs the current READY loopback URL; renderer/deep-link-supplied URLs, paths, executables, args, environment, or lifecycle actions are ignored or rejected (`OPEN-01`, §11).

**No daemon:** there is no separate Node daemon and no PID-file lifecycle. Future CLI control, if required, is a thin authenticated client of the running Tauri supervisor — not a second authority.

## 10. Security boundaries

1. **No JS execution authority:** the view has no shell plugin, no arbitrary opener/filesystem/updater permissions. Spawning exists only in Rust, with absolute non-null path + fixed argv, explicit env, and an explicit inherited-handle allowlist [S25][S64][S65]. Tauri capabilities are per-window and minimal [S73].
2. **Web isolation:** controller webview loads only bundled assets with strict CSP; the harness page never renders inside the privileged webview (system browser or a zero-capability webview instead). Cross-origin requests, invalid Host headers, cross-site Fetch Metadata, and missing/invalid run sessions are rejected before RPC/body dispatch [S74][S75] — and the controller does not depend on the web carrier for authority ("reachability policy, not authentication" [S62]).
3. **Bridge trust:** random per-run token, current-user/SYSTEM DACL, `PIPE_REJECT_REMOTE_CLIENTS`, first-instance creation, bounded frames, version check; forged/replayed/oversized/wrong-version/wrong-token/wrong-host frames close the channel and terminate the Job (`START-05`, `IPC-01`). No secret is persisted; documentation does not overclaim resistance to fully malicious same-user code.
4. **Secrets never at rest:** `PIMP_*`/`DSH_*` env values flow to the child process only; state files and logs contain none; diagnostic export has a review/redaction gate; corrupt advisory state is ignored/quarantined (`LOG-03`).
5. **Update integrity:** Tauri update signatures are mandatory and cannot be disabled; endpoints HTTPS; embedded pubkey [S26]. Controller/Node/CLI/profile data/DSH pin family is one compatibility manifest: incompatible wire/pin → preflight fails before process creation (`UPDATE-03`); tampered/unsigned/wrong-key/wrong-arch/downgrade artifacts are rejected before execution or install mutation (`UPDATE-01`); partial downloads are never executable (`UPDATE-04`).
6. **Update as a state transition:** an install blocks new starts, requests graceful stop, requires Job active-process zero and closed logs/control handles before invoking the updater; otherwise defer or require explicit force (`UPDATE-02`).
7. **Dual signing:** Authenticode sign + timestamp the Windows artifacts (`signtool`) **and** separately enforce Tauri's mandatory update signatures [S26][S27][S76]; updater private-key custody is an operational prerequisite (§12).
8. **Least privilege:** per-user NSIS install; no service, no elevation, no machine-wide registry writes; autostart only via explicit user opt-in.
9. **Job containment as fail-safe:** kill-on-close means a supervisor crash/exit/update takes the whole harness tree down with it — deliberate; the controller never leaves an unsupervised child behind.

## 11. Recommended architecture — phased but complete first vertical slice

Goal-prompt phrasing: *ship one persistent per-user Tauri 2 tray controller: Rust owns an unnamed kill-on-close Job Object and live handles; the validated CLI is a foreground child; `--port 0` plus an authenticated versioned readiness/control pipe replaces fixed-port polling; upstream's bounded whole-app disposal is invoked cooperatively before `TerminateJobObject` fallback; PID/port/state files are diagnostics; JS is a view with no shell plugin.* No product code in this record.

### Phase 0 — the complete vertical slice (definition of done for the first goal)

1. **Shell scaffold & installer.** Tauri 2.11 app (Rust core + minimal local-asset view), NSIS per-user `-setup.exe`, WebView2 `downloadBootstrapper` [S22]. Acceptance: clean Win11 install; tray icon appears; window close hides (controller resident); uninstall removes app, shortcuts, and its registry entry while preserving controller state/logs and `DSH_HOME` profiles.
2. **Launch with Job guarantees.** Resolved absolute `node.exe` + CLI entry, `CREATE_NO_WINDOW | CREATE_SUSPENDED`, explicit env + handle allowlist, unnamed `KILL_ON_JOB_CLOSE` Job assigned before resume; `--host 127.0.0.1 --port 0` [S59]. Acceptance: `START-01`–`START-04` (below); no `cmd.exe` ancestor, no console flash, no unsupervised fallback on any setup error.
3. **Authenticated readiness.** Versioned current-user/SYSTEM named pipe, random run token, first-instance creation, bounded frames; child reports actual host/port only after WebServer init; Rust validates run ID, literal scheme/host, pinned versions, Job membership before READY/browser-open. Acceptance: `START-05`, `IPC-01`; the controller opens a browser only from the Rust-constructed READY URL.
4. **Cooperative stop.** Bridge invokes upstream whole-app disposal; wait beyond the 5 s upstream bound and for Job active-process zero → GRACEFUL; deadline → `TerminateJobObject` → wait Job empty → **FORCED** (prominent). Acceptance: `STOP-01`–`STOP-03`; a forced stop is never reported as graceful.
5. **Crash & identity.** Kill-on-close teardown on supervisor exit; interrupted history never adopted; stale PID/foreign port owner never killed (`CRASH-01`–`CRASH-03`, `IDENTITY-01`); port probe is an optional health facet, fixed 3080 opt-in with `PORT_IN_USE_FOREIGN` (`PORT-01`, `PORT-02`).
6. **Status/doctor/open-UI/logs.** Tray menu reports handle-derived status (`stopped|starting|ready|running|stopping|forced|failed-start|unmanaged-foreign-port`), delegates to `pimp-dsh doctor`, reveals logs; "Open Web UI" = Rust constructs the current READY loopback URL and opens the system browser (or a zero-capability webview). Acceptance: every action headless; renderer-supplied URLs/actions rejected (`OPEN-01`).
7. **Bounded logs & secrets.** Both pipes always drained; memory/event sizes and disk retention bounded; invalid UTF-8/ANSI/HTML rendered text-safe and revision/run scoped; disk-full → drain-to-discard + one visible fault, never deadlock the child (`LOG-01`, `LOG-02`). No `PIMP_*`/`DSH_*` value in any state file or captured log (`LOG-03`).

### Failure invariants (observable acceptance criteria for every phase)

| ID | Scenario | Required outcome |
| --- | --- | --- |
| START-01 | Two controller processes / simultaneous Start gestures | Exactly one per-user supervisor and one managed Job/root; second launch redirects activation and exits; concurrent starts coalesce or one reports already-starting/running. |
| START-02 | Job configuration/assignment or handle-list setup fails | Primary thread never resumed; process terminated/handles closed; `FAILED_START`; never an unsupervised spawn. |
| START-03 | Spaces in CLI path / malicious cwd or PATH entry | Absolute non-null path + fixed argv launch only the intended Node/CLI; no `cmd.exe`, shell expansion, PATH fallback, or unquoted ambiguity. |
| START-04 | Child/wrapper exits early, bind fails, or readiness deadline expires | Residual Job members terminated; pipes drained; one actionable failure/log ID; never opens a browser or reports RUNNING. |
| START-05 | Forged/replayed/oversized/wrong-version/wrong-token/wrong-host readiness frame | Reject frame, close channel, terminate Job, protocol-failure report; no supplied URL is opened. |
| PORT-01 | Managed default start while 3080 is occupied | Run succeeds on an OS-selected port (passed `--port 0`); the 3080 owner is never inspected or terminated. |
| PORT-02 | Fixed 3080 requested and foreign/busy | Start fails `PORT_IN_USE_FOREIGN`; no kill, adoption, browser open, or stale-state overwrite. |
| STOP-01 | Normal cooperative stop | Whole-app disposal runs; HTTP + upgraded sockets, sessions/plugins, log pipes settle; Job reaches active-process zero; UI records GRACEFUL with exit info. |
| STOP-02 | Shutdown handler hangs / descendants ignore it | After the grace deadline `TerminateJobObject`; controller waits for active-process zero and reports FORCED prominently. |
| STOP-03 | Repeated Stop/Start during STOPPING | Operations serialized/idempotent; no second timer, signal storm, or stale transition. |
| CRASH-01 | Supervisor hard-killed with root + grandchild alive | Both die via kill-on-close without PID lookup; next launch acquires the abandoned lock and reports the interrupted run. |
| CRASH-02 | Root dies while a descendant remains in the Job | Run is CRASHED; residual Job terminated; no orphan becomes a new authority. |
| CRASH-03 | WebView/window crashes or user closes window | Rust supervisor/Job continue; recreated window gets a fresh revisioned snapshot; Quit follows stop policy, never silently detaches the harness. |
| IDENTITY-01 | Stale/reused PID or foreign process with matching image/path/port | No process killed or adopted; without live handles/run channel it is UNMANAGED; persisted PID cannot authorize action. |
| IPC-01 | Remote machine, other user, pre-created pipe, or unauthenticated local client connects | Remote/wrong-DACL connections fail; first-instance/rand-token/version checks reject spoofing; no state transition. |
| WEB-01 | Cross-origin HTTP/WebSocket, invalid Host, cross-site Fetch Metadata, missing/invalid run session, remote content in a webview | Rejected before RPC/body dispatch; no side effect; remote harness content has zero controller permissions. |
| OPEN-01 | Renderer/deep link supplies URL/path/executable/args/env/lifecycle action | Backend ignores/rejects raw authority; `open_harness` constructs the current READY loopback URL; deep links only allowlisted inert navigation/focus. |
| LOG-01 | Child floods stdout/stderr, invalid UTF-8/ANSI/HTML, renderer falls behind | Both pipes remain drained; memory/event and disk retention bounded; UI responsive; rendering text-safe and revision/run scoped. |
| LOG-02 | Disk full / log write failure | Drains to discard; one visible logging fault; stop still works; child never deadlocks. |
| LOG-03 | State/log dir redirected or corrupt; output contains configured secret values | Reparse/ownership/schema checks fail closed; corrupt advisory state ignored/quarantined; env/tokens never added to output; export has review/redaction gate. |
| UPDATE-01 | Tampered/unsigned/wrong-key/wrong-arch/channel/downgrade update | Rejected before execution or install mutation; current version stays runnable. |
| UPDATE-02 | Install requested while a managed process remains | Starts blocked; graceful stop requested; requires Job active-process zero + closed logs/control handles before updater; else defer or explicit force. |
| UPDATE-03 | Desktop/CLI/Node/DSH pin or wire contract incompatible | Preflight fails before process creation and offers the signed matching update/repair; never uses another CLI found on PATH. |
| UPDATE-04 | Download/install interrupted or updater key rotates | Partial artifact never executable; previous version or documented signed repair available; old supported versions traverse planned trust-root rotation without disabling verification. |
| PORTABLE-01 | Future macOS/Linux implementation | Portable state machine, protocol, UI commands, state/log formats unchanged; only process adapter/transport/signing packaging changes; Windows Job semantics are not emulated with a cross-platform PID-file daemon. |

### Phase 1 — operational polish

Autostart opt-in (per-user HKCU Run key via the first-party `tauri-plugin-autostart` — supersedes the earlier `.lnk`/IShellLink plan [S51]: same least-privilege posture, no COM dependency, far smaller diff); structured `doctor --json`; crash/restart policy knob (never/always/ask); state-change notifications (Windows toasts via `tauri-plugin-notification`; toasts need an AppUserModelID, so unpackaged dev builds may not display them — verify with the installed NSIS build); per-profile state.

### Phase 2 — distribution & trust

Dual signing (Authenticode `signtool` + Tauri update signatures [S26][S27][S76]); updater enabled with `passive` install mode, HTTPS endpoint, key custody procedure (private key offline + backup; planned trust-root rotation for old supported versions, `UPDATE-04`); update-as-state-transition enforcement (`UPDATE-02`); optional WiX v3 `.msi` for managed fleets [S22]; Microsoft Store/MSIX only if requested (§8 triggers).

### Phase 3 (optional, future) — macOS/Linux parity

Same Tauri core, state machine, protocol, and failure invariants; only the process adapter (POSIX process-group kill replaces Job semantics — **not** a PID-file daemon), transport, and signing packaging change (`PORTABLE-01`).

## 12. Migration & upgrade considerations

- **From any Tier-A shortcut (if shipped earlier):** the shortcut becomes an entry point into the native controller; it never owns lifecycle state. Uninstall removes only shortcut + controller state files, never profile data; the state-file schema carries a `format` version field.
- **Tauri v2 → v3:** updater config `createUpdaterArtifacts` "will be removed in v3" [S26]; pin Tauri 2.11.x, subscribe to the v3 migration guide, and treat a major upgrade as its own release train — never folded into a harness release.
- **Compatibility manifest:** controller, Node runtime ownership, CLI, profile data, and the exact DSH client/host pin family are one manifest; preflight fails before process creation on mismatch and offers the signed matching update/repair (`UPDATE-03`). Preferred polished distribution bundles an app-private exact Node/CLI payload; an external install must be absolute-path/version checked and fail closed.
- **WebView2 servicing:** evergreen runtime auto-updates on Win10/11; air-gapped deployments may choose `offlineInstaller`/`fixedVersion` [S22] at the cost of shipping browser patches ourselves (§10).
- **Node upgrades:** never persist a bare absolute `node.exe` path as the sole spawn target; re-resolve at runtime and verify against the repo `engines` (22.19+/24+) and the compat manifest.
- **Upstream CLI drift:** the controller spawns through the validated `pimp-dsh run` boundary; `update-check`/`migrate` remain the only upgrade paths; the bridge protocol is versioned so a CLI/harness change fails closed instead of silently misbehaving (`UPDATE-03`).
- **Updater key custody:** losing the signing private key bricks updates for installed users ("you will NOT be able to publish new updates") [S26]; key escrow/backup and cert renewal are operational prerequisites for Phase 2; plan the trust-root rotation path for already-installed versions (`UPDATE-04`).
- **WinUI 3 re-evaluation trigger:** if a stable Windows App SDK release adds a first-party notification-area API, re-run §7; the supervisor core (state machine, bridge protocol, failure invariants) is the portability boundary that keeps that swap bounded.
- **Graceful-shutdown contract:** the 5-second upstream disposal bound [S58] is the cooperative window; if upstream changes it, the controller's grace deadline is a configuration that follows the pin, not a constant.
- **No PID-file daemon portability:** future platforms keep the same state machine and reject a cross-platform PID-file daemon design (`PORTABLE-01`).

## 13. Corrections to the previous artifact (explicit, both revisions)

1. **`windowsHide` corrected:** current `run` uses `windowsHide: false` + `stdio: 'inherit'` ([src/cli.ts](../src/cli.ts) ≈L375–388) [S63]; the prior "CLI already uses windowsHide:true" claim applied only to package-manager subprocesses. The controller supplies hidden-window semantics itself.
2. **"No graceful shutdown" corrected:** the exact-pinned rc.6 harness has bounded whole-app disposal on SIGINT/SIGTERM (`PROCESS_SHUTDOWN_TIMEOUT_MS = 5e3`, `app.current?.fiber.dispose()`) [S58]. Stop is cooperative-first; force is an escalation reported as FORCED, never the default posture.
3. **Fixed-port-probe authority removed:** the web CLI supports `--port 0` (OS-assigned) and exposes the actual bound port [S59][S60][S61]. The prior "port probe is the mandatory liveness oracle in every tier" claim is superseded: the port is an optional health facet, and PID/port/state files are diagnostics — **live handles + run ID are authority**.
4. **PID-file authority removed:** PID reuse hazards [S55][S69] and the live-handle rule (`IDENTITY-01`) make any persisted-PID kill/adopt a violation. Tier A is demoted to an installer-created entry point, not an architecture.
5. **No separate daemon:** CLI commands that detach/store PIDs/start-stop behind the controller are rejected; future CLI control is a thin authenticated client of the running Tauri supervisor.
6. **Job Object design refined:** unnamed Job Object, `CREATE_SUSPENDED` + assign-before-resume, explicit `PROC_THREAD_ATTRIBUTE_HANDLE_LIST` allowlist, retained non-inheritable handles, kill-on-close as deliberate crash fail-safe [S52][S54][S64]–[S68].
7. **JS is view-only:** the controller webview gets no shell plugin, no arbitrary opener/fs/updater permissions; browser-open is a Rust-side decision from the validated READY URL [S25].
8. **VBScript/WiX claim removed (revision 1):** the current Tauri installer page contains no VBScript statement (verified by full-text search 2026-08-16); WiX v3 remains the documented MSI toolset [S22].
9. **Windows App SDK versions refreshed (revision 1):** 2.4.0 stable (2026-08-13); 2.0.1 first SemVer-major stable (2026-04-29); 1.8.10 serviced; tray gap re-verified — no first-party API in 2.0–2.4 release notes [S2][S3][S4].
10. **Tray-gap scope corrected (revision 1):** Avalonia 11 has first-party `TrayIcon` [S43]; the gap is WinUI-3-specific.
11. **Tauri v3 warning + installMode semantics added (revision 1):** [S26].

## 14. Source index

Checked 2026-08-16 unless noted.

- [S1] Tauri 2.11.5 release — https://github.com/tauri-apps/tauri/releases/tag/tauri-v2.11.5
- [S2] Windows App SDK downloads (stable 2.4.0, 2026-08-13; 1.8.10, 2026-07-14) — https://learn.microsoft.com/en-us/windows/apps/windows-app-sdk/downloads
- [S3] Windows App SDK 2.0 release notes (stable pivot) — https://learn.microsoft.com/en-us/windows/apps/windows-app-sdk/release-notes/windows-app-sdk-2-0
- [S4] Windows App SDK release channels — https://learn.microsoft.com/en-us/windows/apps/windows-app-sdk/release-channels
- [S5] .NET 10.0.11 download page — https://dotnet.microsoft.com/en-us/download/dotnet/10.0
- [S6] .NET & .NET Framework August 2026 servicing — https://devblogs.microsoft.com/dotnet/dotnet-and-dotnet-framework-august-2026-servicing-updates/
- [S7] Avalonia 11.3.17 release — https://github.com/AvaloniaUI/Avalonia/releases/tag/11.3.17
- [S8] Electron releases — https://releases.electronjs.org/
- [S9] Electron v43.4.0 — https://github.com/electron/electron/releases/tag/v43.4.0
- [S10] Wails releases — https://github.com/wailsapp/wails/releases
- [S11] Wails v3 Beta announcement — https://v3.wails.io/blog/wails-v3-beta/
- [S12] What's new in Flutter 3.47 — https://flutter.dev/blog/whats-new-in-flutter-3-47
- [S13] Flutter SDK archive (release schedule) — https://docs.flutter.dev/install/archive
- [S14] Slint 1.17.1 release — https://github.com/slint-ui/slint/releases/tag/v1.17.1
- [S15] Slint 1.17 Released (SystemTrayIcon, drag & drop, tooltips) — https://slint.dev/blog/slint-1.17-released
- [S16] Dioxus releases / crates.io — https://crates.io/crates/dioxus
- [S17] iced changelog (0.14.0) — https://github.com/iced-rs/iced/blob/master/CHANGELOG.md
- [S18] egui changelog / releases — https://github.com/emilk/egui/blob/main/CHANGELOG.md
- [S19] Neutralinojs v6.8.0 — https://github.com/neutralinojs/neutralinojs/releases/tag/v6.8.0
- [S20] Compose Multiplatform releases (1.11.x) — https://github.com/JetBrains/compose-multiplatform/releases
- [S21] Tauri prerequisites (WebView2) — https://v2.tauri.app/start/prerequisites/
- [S22] Tauri Windows Installer (NSIS/WiX v3; WebView2 modes) — https://v2.tauri.app/distribute/windows-installer/ (re-read 2026-08-16; VBScript claim absent)
- [S23] Tauri System Tray guide — https://v2.tauri.app/learn/system-tray/
- [S24] Tauri Single Instance plugin — https://v2.tauri.app/plugin/single-instance/
- [S25] Tauri Shell plugin (capabilities; permission table; Opener pointer) — https://v2.tauri.app/plugin/shell/ (re-read 2026-08-16)
- [S26] Tauri Updater plugin (signed updates mandatory; installMode; v3 warning) — https://v2.tauri.app/plugin/updater/ (re-read 2026-08-16)
- [S27] Tauri Windows Code Signing — https://v2.tauri.app/distribute/sign/windows/
- [S28] Tauri Embedding External Binaries (sidecar) — https://v2.tauri.app/develop/sidecar/
- [S29] Tauri WebDriver testing — https://v2.tauri.app/develop/tests/webdriver/
- [S30] WASDK deployment architecture — https://learn.microsoft.com/en-us/windows/apps/windows-app-sdk/deployment-architecture
- [S31] WASDK unpackaged deployment guide — https://learn.microsoft.com/en-us/windows/apps/windows-app-sdk/deploy-unpackaged-apps
- [S32] WASDK self-contained / single-file deployment — https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/self-contained-deploy/deploy-self-contained-apps
- [S33] What is MSIX? — https://learn.microsoft.com/en-us/windows/msix/overview
- [S34] WASDK app instancing — https://learn.microsoft.com/en-us/windows/apps/windows-app-sdk/applifecycle/applifecycle-instancing
- [S35] `Process.Kill(Boolean)` — https://learn.microsoft.com/en-us/dotnet/api/system.diagnostics.process.kill
- [S36] WinUI 3 NotifyIcon discussion — https://github.com/microsoft/WindowsAppSDK/discussions/3394
- [S37] System tray API discussion — https://github.com/microsoft/WindowsAppSDK/discussions/519
- [S38] WinRT `SystemTray` unusable on desktop — https://github.com/microsoft/WindowsAppSDK/issues/4063
- [S39] H.NotifyIcon (community) — https://github.com/HavenDV/H.NotifyIcon
- [S40] MSIX tray second-instance defect — https://github.com/microsoft/WindowsAppSDK/issues/6031
- [S41] MSIX tray re-registration defect — https://github.com/microsoft/WindowsAppSDK/issues/6408
- [S42] Windows developer platform overview (framework guidance + feature table) — https://learn.microsoft.com/en-us/windows/apps/get-started/ (re-read 2026-08-16)
- [S43] Avalonia TrayIcon docs — https://docs.avaloniaui.net/controls/navigation/trayicon (re-checked 2026-08-16)
- [S44] Electron Process Model — https://www.electronjs.org/docs/latest/tutorial/process-model
- [S45] Electron Application Packaging/Distribution — https://www.electronjs.org/docs/latest/tutorial/application-distribution
- [S46] Electron Tray API — https://www.electronjs.org/docs/latest/api/tray
- [S47] Electron autoUpdater — https://www.electronjs.org/docs/latest/api/auto-updater
- [S48] Wails Windows packaging — https://v3.wails.io/guides/build/windows/
- [S49] Flutter desktop support / building Windows apps — https://docs.flutter.dev/platform-integration/windows/building
- [S50] Flutter supported deployment platforms — https://docs.flutter.dev/reference/supported-platforms
- [S51] Shell links (`IShellLink`) — https://learn.microsoft.com/en-us/windows/win32/shell/links
- [S52] Process creation flags (`CREATE_NO_WINDOW`, `CREATE_SUSPENDED`, `CREATE_NEW_PROCESS_GROUP`) — https://learn.microsoft.com/en-us/windows/win32/procthread/process-creation-flags
- [S53] taskkill reference — https://learn.microsoft.com/en-us/windows-server/administration/windows-commands/taskkill
- [S54] Job Objects — https://learn.microsoft.com/en-us/windows/win32/procthread/job-objects
- [S55] Node `child_process` (kill semantics, PID reuse, .bat/.cmd, env case) — https://nodejs.org/api/child_process.html
- [S56] WASDK app instancing (named mutex context) — https://learn.microsoft.com/en-us/windows/apps/windows-app-sdk/applifecycle/applifecycle-instancing
- [S57] Repo sources: `README.md`, `src/cli.ts`, `package.json` (`engines`), `docs/windows-support.md` (taskkill convention), `docs/roadmap.md`, `docs/adr/0001-no-fork.md`, `docs/security-model.md`, `docs/upstream-pin.md`
- [S58] Upstream bounded disposal (rc.6): `node_modules/@deepseek-ai/dsh/lib/profile-boot-DG5t9aNs.js` L9–62, L223–239 — `const PROCESS_SHUTDOWN_TIMEOUT_MS = 5e3`; SIGTERM/SIGINT → `await app.current?.fiber.dispose()`; SIGTERM→exit 0, SIGINT→130
- [S59] Upstream web CLI `--port 0`: `node_modules/@deepseek-ai/dsh-web-app/lib/startup.js` L21–45 — "listen port; pass 0 to let the OS pick a free one"; rejects `0.0.0.0`
- [S60] Upstream bound-port API: `node_modules/@deepseek-ai/dsh-host-webserver/lib/types/index.d.ts` L37–65 — `get port(): number`
- [S61] Upstream bind + disposal: `node_modules/@deepseek-ai/dsh-host-webserver/lib/index.js` L168–190 — `this.listenedPort = this.server.address().port`; disposal closes HTTP and upgraded sockets
- [S62] Upstream trust model: `node_modules/@deepseek-ai/dsh-client-connection/README.md` — "The fence is a reachability policy, not authentication; the Web carrier provides no authentication layer."
- [S63] Current `run` spawn: `src/cli.ts` ≈L375–388 — `spawnSync(..., { stdio: 'inherit', env: harnessEnvironment(), shell: false, windowsHide: false })`
- [S64] `CreateProcessW` executable-name and inheritance rules — https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-createprocessw
- [S65] Explicit inherited handle list (`PROC_THREAD_ATTRIBUTE_HANDLE_LIST`) — https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-updateprocthreadattribute
- [S66] Job limits / kill on close — https://learn.microsoft.com/en-us/windows/win32/procthread/job-object-limits
- [S67] `AssignProcessToJobObject` / `TerminateJobObject` — https://learn.microsoft.com/en-us/windows/win32/api/jobapi2/nf-jobapi2-assignprocesstojobobject ; https://learn.microsoft.com/en-us/windows/win32/api/jobapi2/nf-jobapi2-terminatejobobject
- [S68] Job completion association — https://learn.microsoft.com/en-us/windows/win32/api/winnt/ns-winnt-jobobject_associate_completion_port
- [S69] Process handles and identifiers (PID reuse) — https://learn.microsoft.com/en-us/windows/win32/procthread/process-handles-and-identifiers
- [S70] Console control limitations — https://learn.microsoft.com/en-us/windows/console/generateconsolectrlevent
- [S71] Named pipe security and access rights — https://learn.microsoft.com/en-us/windows/win32/ipc/named-pipe-security-and-access-rights
- [S72] `CreateNamedPipe` flags (`PIPE_REJECT_REMOTE_CLIENTS`) — https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-createnamedpipew
- [S73] Tauri v2 capabilities — https://v2.tauri.app/security/capabilities/
- [S74] Tauri v2 IPC — https://v2.tauri.app/concept/inter-process-communication/
- [S75] Fetch CORS protocol — https://fetch.spec.whatwg.org/#http-cors-protocol
- [S76] WebSocket Origin security — https://www.rfc-editor.org/rfc/rfc6455#section-10.2
- [S77] Microsoft SignTool — https://learn.microsoft.com/en-us/windows/win32/seccrypto/signtool

## 15. Terminology for the goal prompt (exact terms, syntax vs. design)

Documented identifiers that MAY appear as syntax in the goal (verbatim vendor surface): `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` · `CREATE_NO_WINDOW` · `CREATE_SUSPENDED` · `CREATE_NEW_PROCESS_GROUP` · `PROC_THREAD_ATTRIBUTE_HANDLE_LIST` · `PIPE_REJECT_REMOTE_CLIENTS` · `CreateJobObject` / `AssignProcessToJobObject` / `TerminateJobObject` · `JOB_OBJECT_ASSOCIATE_COMPLETION_PORT` · `taskkill /T` · `--host 127.0.0.1 --port 0` (upstream web CLI) · `PROCESS_SHUTDOWN_TIMEOUT_MS = 5e3` + `app.current?.fiber.dispose()` (upstream disposal) · `signtool` · Tauri `tray-icon` / single-instance plugin / updater `passive` installMode.

Design recommendations that MUST NOT be quoted as vendor syntax: the versioned readiness/control pipe protocol (random run token, first-instance creation, bounded frames), the run-ID framing, the per-window capability set, and the update-as-state-transition sequence. The OMP goal must present these as requirements with the §11 failure invariants as acceptance criteria, not as documented API syntax.

Concepts: persistent per-user controller (not a service) · Rust = only supervisor · JS = view only, no shell plugin · unnamed kill-on-close Job Object · assign-before-resume · live handles + run ID as authority · PID/port/state file = diagnostics · dynamic port by default · fixed 3080 opt-in → `PORT_IN_USE_FOREIGN` · cooperative upstream disposal (5 s bound) before `TerminateJobObject` → FORCED · no daemon, no PID-file lifecycle · compatibility manifest (controller + Node + CLI + profile data + DSH pin family) · strict frontend/native split · dual signing (Authenticode + Tauri update signatures) · bounded logs, no persisted secrets.

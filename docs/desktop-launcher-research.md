# Desktop Control Surface for `pimp-my-dsh` — Stack & Lifecycle Decision Record

- Status: **research/decision artifact** (no implementation). Owner: `ModernDesktopMatrix` agent.
- Last revised: **2026-08-16**. Supersedes the previous "Windows Desktop Launcher & Process-Supervisor Research" (2026-08-15, `DesktopLauncherResearch` agent) in this file.
- Purpose: define the stack and lifecycle architecture for a **modern, polished, effective** native desktop control surface (launcher + tray supervisor) for the Windows-first `pimp-my-dsh` harness, and produce the evidence base for the OMP goal prompt.
- Skip list (explicit non-goals): implementation code, formatters, linters, builds, tests, generic secondary blog posts. Every material factual claim links to a primary/official source (see [Source index](#source-index)); each matrix row carries an evidence/confidence grade.
- Evidence convention: **High** = verbatim on a primary source re-read today (2026-08-16); **Medium** = primary source, stable content, carried from the prior agent's verified read (2026-08-15) or a longstanding official page not re-read today; **Low** = inference or community-only. Dates are the release dates published by the upstream project.

## 1. Decision (summary)

**Adopt Tauri 2 (v2.11.x) as the desktop shell for a Tier-C tray supervisor/launcher** — a persistent, signed, updatable `.exe` that starts/stops/monitors the harness, with the existing Node CLI remaining the *only lifecycle authority* for the harness process itself (no-fork ADR: [docs/adr/0001-no-fork.md](adr/0001-no-fork.md)).

- **Runner-up:** WinUI 3 on Windows App SDK 2.4 — first-class Windows-native rendering and lifecycle APIs, but still **no first-party tray/notification-area API** (§4.2), Windows-only, a second UI language (XAML/C#) for a TypeScript/Node team, and a heavier unpackaged deployment (runtime installers or a large self-contained payload).
- **Explicitly rejected:** Electron (redundant Chromium+Node runtime inside a Node-native product, largest footprint/attack surface), Wails (Go stack, no first-party updater, v3 transition in progress), Flutter desktop (new language, no first-party tray/updater, weaker Windows a11y), Slint / iced / egui / Dioxus / Neutralino / Compose Multiplatform (young or thin ecosystems; see §4.6–4.7), MSIX packaging now (signing + container semantics before the product needs them; documented MSIX tray defects).
- The previous Tier-A recommendation (`.lnk` + Node wrapper script) is **demoted to an optional bootstrap**, not the goal. It remains valid as a zero-runtime fallback while the Tauri slice is built (§4.8).
- The Tier-C supervisor treats **`127.0.0.1:3080` (TCP probe) + live PID as the liveness oracle** and never trusts the state file alone; stop semantics = Job Object termination with `taskkill /T` fallback (§8).

## 2. Terminology: three distinct things

| Term | Meaning in this record |
| --- | --- |
| **Native-looking** | Rendered with OS/web primitives that match platform conventions (Fluent-style controls, tray menu, taskbar behavior) without necessarily using the OS's own UI toolkit. Tauri (WebView2), Electron, Wails, Neutralino are native-looking via web rendering. |
| **Native shell integration** | Participation in OS surfaces: notification-area (tray) icon, taskbar/Start Menu entries, file associations, jump lists, notifications. Independent of rendering technology. |
| **Truly native rendering** | Pixels produced by the OS toolkit or GPU-native engine — WinUI 3 (XAML/DirectX), WPF, Avalonia, Flutter (Impeller/Skia), Slint/iced/egui (wgpu/GL), Win32. Tauri/Electron/Wails are **not** truly native rendering; they host a web engine. |

Three delivery tiers (unchanged from prior record):

| Tier | Artifact | Supervision |
| --- | --- | --- |
| A | Desktop shortcut + Node script (`.lnk` → `node.exe`) | None (PID-file bookkeeping in the script) |
| B | Native launcher `.exe` | During run only |
| C | **Tray / process-supervisor app (chosen)** | Continuous |

Tiers B and C are the same binary here: a tray app that also launches.

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

## 4. Candidate stacks

### 4.1 Tauri 2 (chosen)

Official facts (Tauri v2 docs, [S21]–[S28]):

- **Rendering:** WebView2 on Windows ("Tauri uses Microsoft Edge WebView2 to render content on Windows" — prerequisites [S21]); WKWebView on macOS, WebKitGTK on Linux. The WebView2 *Evergreen runtime* is preinstalled on Windows 10/11 machines that receive Edge updates; the Tauri installer handles the residual case (§ installer modes). Confidence: High.
- **Installer:** NSIS `-setup.exe` or **WiX v3** `.msi`; `.msi` can only be built on Windows [S22]. WebView2 install modes: `downloadBootstrapper` (+0 MB, needs internet, default), `embedBootstrapper` (~1.8 MB), `offlineInstaller` (~127 MB), `fixedVersion` (~180 MB), `skip` (not recommended) [S22]. Confidence: High (page re-read today).
- **Tray:** first-class — `tray-icon` Cargo feature, `TrayIcon.new()` JS API with native menu and click events [S23]. Confidence: Medium (carried verified read).
- **Single instance:** official plugin, Windows fully supported [S24]. Confidence: Medium (carried).
- **Process spawn:** `@tauri-apps/plugin-shell` `Command.create(program, args).execute()/.spawn()`; **capability-gated** — the default permission set only allows opening `http(s)://`, `tel:`, `mailto:` links; running a command requires explicit `shell:allow-execute` / `shell:allow-spawn` scope entries, which can pin `name`/`cmd`/`args` (with arg validators) [S25]. Confidence: High (page re-read today; permission table and default-permission block read verbatim). Note: the plugin does **not** hand the caller a native process handle, so Job Object containment (§8) is implemented in Rust command handlers, not via the JS plugin.
- **Open-URL:** the `shell.open` API is now documented under the separate **Opener plugin** [S25]. Used for "open the harness web UI in the default browser".
- **Updates:** updater plugin **requires signed updates** ("Tauri's updater needs a signature … This cannot be disabled"); Windows artifacts are `-setup.exe`/`.msi` + `.sig`; `installMode` = `passive` (default) / `basicUi` / `quiet`; endpoints must be HTTPS in production [S26]. Confidence: High (page re-read today). Updater config `createUpdaterArtifacts` "**will be removed in v3**" [S26] — a concrete v2→v3 migration risk (§10).
- **Code signing:** not required to run; required for Microsoft Store and to avoid SmartScreen "not trusted" on browser downloads; supported via `signtool` (OV/EV) or Azure Key Vault [S27]. Confidence: Medium (carried).
- **Sidecar:** `externalBin` for bundling extra binaries (e.g., a portable Node) — explicitly not used here [S28].
- **Accessibility/testing:** WebView2 inherits Chromium's accessibility tree; E2E via WebDriver (`tauri-driver`) — Medium confidence (docs pages [S29] carried; not re-read today).
- **Footprint:** the app itself is Rust + web assets (typically a few MB); the runtime add is WebView2, shared system-wide and evergreen-patched by Microsoft.

### 4.2 Windows App SDK 2.4 / WinUI 3 (runner-up) — plus WPF and Avalonia

- **Versions/channels:** Stable 2.4.0 (2026-08-13); 2.0.1 was the first SemVer-major stable (2026-04-29); 1.8.x servicing continues (1.8.10, 2026-07-14). Stable channel = production-ready with long-term API stability [S2][S3][S4]. Confidence: High.
- **Deployment (unpackaged, framework-dependent):** Windows App Runtime installer (`WindowsAppRuntimeInstall.exe`) installing Framework/Main/Singleton/DDLM packages, Bootstrapper initialization, **VCRedist required**; machine-wide install needs elevation [S30][S31]. Self-contained (`WindowsAppSDKSelfContained=true`) bundles the runtime; **`PublishSingleFile` is supported only for unpackaged self-contained apps (1.5+) and not for packaged (MSIX) apps** [S32]. Confidence: Medium (carried from prior verified read; 2.x docs retain the same architecture).
- **MSIX:** mandatory signing before installation, differential (64 KB block) updates, app-container semantics [S33]. Confidence: Medium (carried).
- **Single instance:** `AppInstance.FindOrRegisterForKey` / `GetInstances` + activation redirection (packaged and unpackaged) [S34]. Confidence: Medium (carried).
- **Process tree kill (.NET):** `Process.Kill(bool entireProcessTree)` — first-party [S35]; Job Objects also available. Confidence: Medium (carried).
- **Tray gap (critical, still true in 2.x):** no first-party notification-area API. The WinRT `SystemTray` type is unusable on desktop; discussions #519/#3394 remain open with no commitment, and nothing tray-related appears in the 2.0–2.4 stable release notes ([S36][S37][S38][S4]). Community path: `H.NotifyIcon` [S39]. Known MSIX tray defects: notification click can launch a second instance ([S40]); tray icon re-registers when the MSIX version changes the exe path ([S41]). Confidence: High for "no first-party API" (re-checked today); Medium for the specific issue threads (carried).
- **Microsoft's own guidance** ("Choose your app framework", updated 2026-08-09): "WinUI … is the recommended UI framework for new Windows apps"; WPF "actively maintained" and modernizable with WASDK; feature table shows WinUI is **not cross-platform** and WPF has no sandboxing [S42]. Confidence: High (page re-read today).
- **Avalonia 11.3** (the cross-platform .NET/XAML alternative): **first-party `TrayIcon`** with native menu, supported on Windows/macOS and Linux (StatusNotifierItem/AppIndicator DEs) [S43]; runs on .NET 10; cross-platform but its Windows look is XAML-styled rather than Fluent-native, and it inherits the "second language for a TS/Node team" cost. Confidence: High (docs re-checked today).

### 4.3 Electron 43

- Multi-process model: one main process (Node) + a renderer **per window** + utility processes [S44]; distribution ships Electron's prebuilt Chromium+Node binaries + `resources/app` — i.e. **every app bundles a second, full Chromium+Node runtime** [S45]. Confidence: Medium (carried; official model unchanged).
- Tray: first-class; on Windows the tray GUID is tied to the code signature when signed, else to the exe path ("Changing the path to the executable will break the creation of the tray icon") [S46]. Confidence: Medium (carried).
- Updates: `autoUpdater` = Squirrel.Windows or the MSIX updater; Squirrel requires a specific AUMID (`com.squirrel.<id>.<exe>`) [S47]. Confidence: Medium (carried).
- Current runtime: v43.4.0 ships Chromium 150 + Node 24.18.1 [S8]. Rejected: for a product that *is* Node and already has a web UI, Electron adds the largest redundant footprint and attack surface for zero unique capability (§6).

### 4.4 Wails (v2.14 stable / v3 beta)

- Go + system WebView (WebView2 on Windows), so the footprint argument mirrors Tauri, but the backend is **Go** — a second language *and* a second package ecosystem for a TS/Node team [S10][S48].
- v2.14.0 is the stable line; **v3 is in beta (v3.0.0-beta.8)** with "API is stable but pre-release" warnings; migration from v2 is a real task [S11][S48]. Building on a beta for a load-bearing supervisor is disqualifying; building on v2 means an inevitable v3 migration on a short horizon.
- No first-party updater; tray/single-instance come from community libraries (systray, etc.). Rejected (§6).

### 4.5 Flutter desktop 3.47

- **Truly native rendering** (Impeller — shaders precompiled at build time — now the default renderer on Windows/macOS/Linux, replacing Skia; Vulkan on Windows) [S12][S13]. Confidence: High.
- Dart is a new language; no first-party tray (community `system_tray`), no first-party updater; MSIX packaging; Windows **multi-window API still experimental** as of 3.44.8/3.47; desktop accessibility (screen-reader parity) historically weaker than UIA/web on Windows [S49][S50]. Confidence: Medium.
- Rejected for this product: the app is a *control surface around an existing web UI*, which Flutter's native canvas cannot reuse (§6).

### 4.6 Slint 1.17

- Declarative UI DSL compiled to native code (Rust/C++/JS/Python bindings); cross-platform; `SystemTrayIcon` (Windows/macOS/Linux) **only landed in 1.17.0 (2026-06-24)** [S14][S15] — one release old. Confidence: High.
- No first-party single-instance, installer, updater, or signing story; small widget ecosystem; accessibility support immature relative to WebView2/UIA. Rejected (§6): everything beyond rendering would be hand-rolled, and the web UI cannot be reused.

### 4.7 Other lightweight alternatives (one column in the matrix)

- **Dioxus 0.7.10** (0.8 in alpha): Rust + WebView2 on desktop, web/mobile from one codebase; younger than Tauri, no first-party updater/tray plugin ecosystem, desktop renderer explicitly WebView-based [S16]. Confidence: Medium.
- **iced 0.14.0** / **egui 0.36.1**: native GPU-rendered Rust UIs (retained/Elm-style vs immediate mode); tray via community crates (`tray-icon`), no installer/updater/single-instance first-party story; accessibility support is minimal-to-none (no OS a11y tree) [S17][S18]. Confidence: Medium.
- **Neutralinojs 6.8.0**: ~2 MB WebView runner with JS + native backend extensions; has a tray API; no first-party updater; small ecosystem; long-term support breadth well below Tauri [S19]. Confidence: Medium.
- **Compose Multiplatform 1.11.1**: Kotlin/JVM, hardware-accelerated desktop rendering; tray via AWT interop, packaging via jpackage; Kotlin+JVM is the worst stack fit for this repo [S20]. Confidence: Medium.

### 4.8 Tier A (retained as bootstrap, not the goal)

Unchanged from the prior record: a Node wrapper script + `IShellLink` `.lnk` (PowerShell `WScript.Shell` COM) with probe→spawn→PID-file→`taskkill /T` logic. Zero new runtime and zero signing burden; no tray, no supervision, and no path to a polished control surface. Keep the option documented for emergency installs, but **do not target it in the OMP goal**.

## 5. Windows platform primitives (grounding for the lifecycle architecture)

- **Shortcuts (`.lnk`):** `IShellLink` + `IPersistFile` COM; stores target, working directory, arguments, show command (`SW_*`), icon, hotkey; Start Menu placement = saving the `.lnk` into the Start Menu programs folder [S51]. Used only for the optional Tier-A bootstrap and for an opt-in autostart entry.
- **No console flash:** `CREATE_NO_WINDOW` (0x08000000) — "The process is a console application that is being run without a console window" [S52]. Rust exposes this via `std::os::windows::process::CommandExt::creation_flags`; Node via `windowsHide: true` (already the repo convention in `src/cli.ts`). `CREATE_NEW_PROCESS_GROUP` (0x00000200) exists if a future supervisor wants `GenerateConsoleCtrlEvent` semantics instead of `taskkill /T` [S52].
- **Tree stop:** `taskkill /t` — "Ends the specified process and any child processes started by it" [S53]. **Job Objects** are the kernel-managed mechanism: `CreateJobObject`, `AssignProcessToJobObject`, `TerminateJobObject`; "by default any child processes it creates using `CreateProcess` are also associated with the job"; with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`, "closing the last job object handle terminates all associated processes and then destroys the job object itself"; nested jobs exist since Windows 8 [S54]. Node's `subprocess.kill()` is **not** a tree kill on Windows [S55] — the supervisor never relies on it.
- **Single instance:** named mutex (classic unpackaged approach) or WASDK `AppInstance.FindOrRegisterForKey` [S56]; the Tauri supervisor uses the official single-instance plugin [S24].
- **PID-reuse safety:** a recorded PID may be reassigned ("if the process identifier (PID) has been reassigned to another process, the signal will be delivered to that process instead") [S55] → always verify image name + creation time before killing; the port probe is the primary liveness oracle.
- **`.bat`/`.cmd` are not directly spawnable** without a shell [S55] → the supervisor always spawns the resolved absolute `node.exe` with the CLI JS entry path, never `cmd /c`, so `cmd.exe` is never inserted into the tree (which would change what `taskkill /T` kills).
- **Environment case-insensitivity on Windows** → deduplicate `PATH`/`Path` when passing env through live [S55].

## 6. Comparison matrix

Criteria × stack. `[S#]` = source index; **Conf** = evidence confidence (H/M/L). "Tier A" column = `.lnk` + Node script.

| Criterion | Tier A (bootstrap) | **Tauri 2.11** (chosen) | WinUI 3 / WASDK 2.4 | Avalonia 11.3 (.NET 10) | Electron 43 | Wails 2.14 / v3-β | Flutter 3.47 | Slint 1.17 | Lightweight (Dioxus/iced/egui/Neutralino/CMP) |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| One clickable app | `.lnk` → installed node | Yes (NSIS/MSI) [S22] — H | Yes (MSIX or self-contained) [S32] — M | Yes (jpackage/single-file) — M | Yes — H | Yes (NSIS) [S48] — M | Yes (MSIX) [S49] — M | Yes (self-built installer) — M | Yes (varies) — M |
| Rendering | Terminal | WebView2 (web) [S21] — H | XAML (truly native) [S42] — H | XAML (GPU) [S43] — H | Chromium — H | WebView2 [S48] — M | Impeller GPU (truly native) [S12] — H | Native GPU (DSL) [S14] — H | Mixed; mostly GPU-native |
| **Tray (first-party)** | None | **Yes** (`tray-icon`) [S23] — M | **No** (community `H.NotifyIcon` only) [S36][S37][S39] — H | **Yes** (`TrayIcon`) [S43] — H | Yes [S46] — M | Community library — M | Community package — M | Yes, new in 1.17 [S15] — H | Dioxus/Neutralino partial; iced/egui community — M |
| Single instance | Port probe + PID file | Official plugin [S24] — M | `AppInstance` API or mutex [S34][S56] — M | Named mutex (DIY) — M | Built-in — M | DIY — M | DIY — M | DIY — M | DIY — M |
| Process-tree APIs | `taskkill /T` (project convention) [S53] | Job Object + `taskkill /T` via win32 in Rust [S54] — H | `Process.Kill(entireProcessTree)` first-party [S35] — M | Same as WinUI (.NET) — M | `child.kill()` ≠ tree; needs `taskkill` [S55] — H | DIY (Go) — M | DIY (Dart FFI) — L | DIY — L | DIY — L |
| Frontend fit (TS/Node team) | Exact (same language) | High: web UI reused for control surface; Rust confined to thin supervisor layer | Poor: XAML/C# second stack; harness UI is web | Poor: XAML/C# | High JS, but redundant Node | Poor: Go | Poor: Dart, web UI not reusable | Poor: DSL | Mixed (Rust/Kotlin/JS) |
| Runtime footprint | ~KB, reuses installed Node | App (MBs) + WebView2 (evergreen, system-shared) [S22] | Runtime installer (4 packages + VCRedist) or large self-contained [S30][S31] | .NET self-contained (~60–150 MB) | Chromium+Node per app (~150–250 MB) [S45] | Small (Go) + WebView2 | Dart AOT (~20–50 MB) | Small native | Small-to-medium |
| Installer / signing / updater | npm update; no signing | NSIS/MSI; `signtool`; **updater built-in, signed updates mandatory** [S22][S26][S27] | MSIX differential + Store; signed mandatory; unpackaged = DIY updater [S33] | DIY updater; standard signing | Forge/builder; Squirrel/MSIX updater [S47] | NSIS; **no first-party updater** [S48] | MSIX; no first-party updater | DIY everything | DIY everything |
| Accessibility | Terminal only | Chromium a11y via WebView2 — M | UIA first-party — H | UIA — M | Chromium a11y — M | Chromium a11y — M | Weaker on Windows — L | Immature — L | Mostly poor (iced/egui) — L |
| Testing | CLI contract tests (existing) | Rust unit + WebDriver (`tauri-driver`) [S29] — M | MSTest + WinAppDriver — M | xUnit + UI tests — M | Playwright — H | Go tests — M | widget tests + integration_test — M | Slint test harness — M | Rust tests — M |
| macOS/Linux path | N/A (Windows .lnk) | Same codebase (WKWebView/WebKitGTK) [S21] — H | None (Windows-only) [S42] — H | Yes (first-party) [S43] — H | Yes | Yes | Yes | Yes | Yes |
| Maturity / governance | N/A | 2.11.x, large ecosystem, stable since 2.0 (2024) — H | Microsoft; 2.x new major (Apr 2026), 1.8 still serviced [S2] — H | Community/OSS company — M | Most battle-tested — H | v3 in beta (transition risk) [S11] — H | Google; desktop "supported" [S49] — M | Young tray API — H | Small — M |

## 7. Decision rationale

**Why Tauri 2 wins for this product:**

1. **The product is already web-shaped.** The harness *is* a Node CLI plus a web UI on `127.0.0.1:3080`. A Tauri supervisor gives the one thing the web UI cannot: OS shell presence (tray, single instance, autostart, notifications, updater) — with the control-surface UI written in the team's existing web stack, and the harness UI untouched in the user's browser.
2. **Every lifecycle feature is first-party and officially documented:** tray (`tray-icon`), single-instance plugin, capability-scoped process spawning (shell plugin), NSIS/MSI installers, signed updater with `passive` install mode, `signtool` signing path [S22]–[S27]. WinUI 3 fails the most important one (tray) and every other webview/lightweight stack fails two or more (updater, single instance, installer).
3. **Security posture is a design feature, not bolted on:** the capability system ships **deny-by-default** for process execution [S25]; the supervisor can pin exactly `node.exe + cli.js + --profile <name>` with arg validators. This directly serves the no-fork/security-model constraints in this repo.
4. **Footprint discipline without sacrificing polish:** the runtime add is the evergreen WebView2 (shared, Microsoft-serviced, present on current Win10/11); the app itself is a few MB. The installers can choose `downloadBootstrapper` (default, +0 MB) or `offlineInstaller` (+127 MB) for air-gapped users [S22].
5. **Cross-platform path is real:** the same Tauri core runs on macOS/Linux [S21], which WinUI 3 cannot offer and Electron/Wails only match with their own costs.
6. **Maturity:** 2.11.5 with monthly patch cadence through 2026 [S1]; v2→v3 risk is already visible in the updater docs ("This setting will be removed in v3") [S26] and is managed by pinning + a documented migration trigger (§10).

**Why not WinUI 3 (the strongest native alternative):** truly native Fluent rendering and first-party UIA accessibility are real advantages — but the tray gap is disqualifying for a *tray supervisor* today [S36][S37], unpackaged deployment adds a runtime installer + VCRedist burden [S30][S31] (or a large self-contained payload), the stack is Windows-only [S42], and XAML/C# is a second frontend stack that cannot reuse the harness's web UI. **Re-evaluation trigger:** if Microsoft ships a first-party notification-area API in a stable WASDK release, re-run this matrix; the supervisor's platform-independent core (§8, process model) is deliberately structured to make that swap cheap.

**Why not the rest:** Electron = second full Chromium+Node runtime inside a Node product, largest attack surface, nothing unique [S44][S45]. Wails = Go backend + no first-party updater + v3 beta transition [S48][S11]. Flutter = new language, community-only tray/updater, experimental multi-window, weaker Windows a11y, and it cannot reuse a web UI [S49][S50]. Slint/iced/egui = hand-roll installer/updater/tray/a11y; Slint's tray API is one release old [S15]. Dioxus/Neutralino/CMP = thin ecosystems or worst-fit languages (§4.7). MSIX-now = mandatory signing + container semantics before the product needs them, plus the documented MSIX tray defects [S40][S41]; revisit only for Store distribution.

## 8. Lifecycle architecture (Tauri supervisor)

**Process model.** Two processes, one authority:

- **Supervisor** = the Tauri app (resident while the harness runs; exits when stopped or via tray "Quit").
- **Harness** = `node.exe <cli.js> run --profile <name>` spawned by a **Rust command handler** (not the JS shell plugin, because Job Object containment needs the process handle). Spawn uses `CREATE_NO_WINDOW` (creation_flags) so no console window ever appears [S52]; stdio redirected to a per-profile log file under `DSH_HOME`; environment passed through **live** (never persisted), with Windows env-key case deduplication [S55].
- **Containment:** the handler creates a Job Object (`CreateJobObject`), assigns the child (`AssignProcessToJobObject`), and sets `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` [S54]. Consequences: (a) the whole harness tree dies atomically if the supervisor crashes — a deliberate fail-safe posture, not a bug; (b) `stop` = `TerminateJobObject` after a grace poll (with `taskkill /T /F` as the documented fallback matching repo convention [S53]).
- **Liveness oracle:** TCP probe of `127.0.0.1:3080` + PID verification (image name + creation time). A port held by a **foreign** process is reported and never killed. The probe is mandatory in every tier — it is the only signal that distinguishes "crashed" from "still starting".

**State machine** (supervisor-owned; one managed harness per profile; carried forward from the prior record because it is stack-independent):

```
ABSENT ──start()──▶ STARTING ──port up + process alive──▶ RUNNING
  ▲                     │                                   │
  │                     └─timeout────────────────────▶ FAILED_START (state file removed)
  │
STOPPING ◀──stop()───── RUNNING ──child exit/port loss────▶ CRASHED (state kept, PID re-checked)
  │                                                            │
  ▼                                                            ▼
STOPPED (verify: port free, tree gone)              STALE ──next start()──▶ cleanup + STARTING
```

- `start()`: probe port → busy: match PID vs state file → `RUNNING` or `PORT_IN_USE_FOREIGN`; free: spawn into Job Object, write state file atomically, poll port with deadline → `RUNNING` or `FAILED_START`.
- `stop()`: `TerminateJobObject` → grace poll → `taskkill /T /F` fallback → verify port free + tree gone → remove state file → `STOPPED`.
- State file: `{pid, profile, startedAt, version, port}` — **no secrets, no env mirrors**; advisory only; port + live PID are the source of truth (PID-reuse safety [S55]).
- Supervisor single-instance: official single-instance plugin; a second launch focuses the existing instance [S24].
- **UI surfaces:** tray menu (Start/Stop/Status/Open Web UI/Logs/Doctor/Quit) + an optional small dashboard window rendered from **local bundled assets only** (no remote content in the webview). "Open Web UI" uses the Opener plugin to open `http://127.0.0.1:3080` in the **default browser** [S25] — the harness UI is never loaded inside the privileged webview.
- `doctor`/`update-check`/`migrate` remain CLI commands; the supervisor invokes the CLI in a controlled console (hidden) and renders its structured output.

## 9. Security boundaries

1. **Deny-by-default execution:** shell capability grants only `shell:allow-spawn` scoped to the resolved `node.exe` + CLI entry + `--profile <validated name>` arg pattern; everything else (`execute`, arbitrary cmds) remains denied [S25]. The spawn handler additionally re-validates program path and args in Rust before creating the process.
2. **No remote content in the webview:** the dashboard loads only local assets; CSP enforced; no `http://`/`https://` navigation from the supervisor window; the harness UI stays in the user's browser.
3. **Secrets never at rest:** `PIMP_*`/`DSH_*` env values flow to the child process only; state files and logs contain none (log redirection captures stdout/stderr; env is never dumped; acceptance test greps state/log dirs for configured values).
4. **Update integrity:** updater public key embedded, updates signature-verified (cannot be disabled) [S26]; endpoints HTTPS in production; `passive` install mode (no silent elevation); key custody documented (§10) — losing the private key permanently blocks updates for installed users.
5. **Code signing & reputation:** EV/OV certificate via `signtool` [S27] before any public release to keep SmartScreen quiet; Store distribution optional later.
6. **Least privilege:** per-user NSIS install; no service, no elevation, no machine-wide registry writes; autostart only via explicit user opt-in.
7. **Harness tree isolation:** the Job Object guarantees the harness tree cannot survive the supervisor; the supervisor never kills PIDs it did not record and never kills a foreign port holder.
8. **Update/telemetry posture:** no analytics SDKs; update checks are opt-in and go only to the configured release endpoint.

## 10. Recommended architecture — phased but complete first vertical slice

Goal-prompt phrasing: *build a Tauri 2 (2.11.x) Windows tray supervisor for `pimp-my-dsh` that owns start/stop/status of exactly one managed harness process per profile, with the CLI as the sole lifecycle authority.* No product code in this record; each item below maps to verifiable acceptance criteria.

### Phase 0 — the complete vertical slice (definition of done for the first goal)

1. **Shell scaffold & installer.** Tauri 2.11 app (Rust core + minimal local-asset dashboard), NSIS per-user `-setup.exe`, WebView2 `downloadBootstrapper` mode [S22]. Acceptance: install on a clean Win11 VM with no dev tools; tray icon appears; uninstall removes the app and its state dir entry.
2. **Start exactly one managed harness process.** Resolved `node.exe` (re-resolved at runtime: `where node` + version check against `engines`) spawns the CLI with `--profile <name>`, `CREATE_NO_WINDOW`, stdio → `DSH_HOME/logs/<profile>.log`, environment passed live; child assigned to a `KILL_ON_JOB_CLOSE` Job Object; state file written atomically. Acceptance: exactly one node tree exists, no console flash, no `cmd.exe` ancestor.
3. **Stop exactly the managed tree.** `TerminateJobObject` after grace; `taskkill /T /F` fallback; verify port 3080 free and recorded PID gone (image name + creation time); clear state file. Acceptance: post-stop, zero processes of the recorded tree remain and the port is free.
4. **Status, doctor, open-UI, logs.** Tray menu actions report probe+PID-derived status (`stopped|starting|running|crashing|port-in-use-foreign`), delegate to `pimp-dsh doctor`, open `http://127.0.0.1:3080` via the Opener plugin in the default browser, and reveal the log path. Acceptance: every action works headless and returns the same status strings the CLI contract tests expect.
5. **Single instance + crash handling.** Single-instance plugin: second launch focuses the existing instance [S24]; harness exit/port loss transitions to `CRASHED` with a restart action; stale state files are cleaned before the next start. Acceptance: double-launch creates one process; kill the harness → tray shows `crashed`, restart works.
6. **Secrets hygiene.** No `PIMP_*`/`DSH_*` value appears in any state file or captured log. Acceptance: grep-based contract test over `DSH_HOME` after a seeded run.

### Phase 1 — operational polish

Autostart opt-in (Start Menu Startup `.lnk` via `IShellLink` [S51]); structured `doctor --json` integration; crash/restart policy knob (never/always/ask); notification on state change via the notification plugin; per-profile state.

### Phase 2 — distribution & trust

Code signing via `signtool` (OV→EV) [S27]; updater enabled with `passive` install mode, HTTPS endpoint, key custody procedure (private key offline + backup; rotation = re-release) [S26]; optional WiX v3 `.msi` for managed fleets [S22]; Microsoft Store/MSIX only if requested (revisit §6 triggers).

### Phase 3 (optional, future) — macOS/Linux parity

Same Tauri core and state machine; platform adapters swap Job Object/taskkill for POSIX process-group kill; tray + NSIS equivalents (DMG/AppImage). This is why the supervisor core must stay platform-agnostic behind a small `spawn`/`stop`/`probe` interface.

## 11. Migration & upgrade considerations

- **From the Tier-A bootstrap (if shipped before Phase 0 lands):** uninstall only the `.lnk` and launcher state files, never profile data; the supervisor's state-file schema supersets the bootstrap's (`{pid, profile, startedAt, version, port}`) with a `format` version field for forward compatibility.
- **Tauri v2 → v3:** updater config `createUpdaterArtifacts` "will be removed in v3" [S26]; pin Tauri (2.11.x) in the lockfile, subscribe to the v3 migration guide, and treat a major upgrade as its own release train — never fold it into a harness release.
- **WebView2 servicing:** evergreen runtime auto-updates on Win10/11; only air-gapped deployments should consider `offlineInstaller`/`fixedVersion` [S22], at the cost of shipping browser patches ourselves — a security boundary decision (§9, item 4).
- **Node upgrades:** never persist a bare absolute `node.exe` path as the sole spawn target; re-resolve at runtime and verify the installed Node satisfies the repo `engines` (22.19+/24+). A Node version bump is then invisible to the supervisor.
- **Upstream CLI drift:** the supervisor spawns the exact `dsh` bin path and `--profile` arg from the same release train; `update-check`/`migrate` remain the only upgrade paths; the arg validator in the capability scope must be kept in sync with new CLI flags (deliberately narrow — widening it is a security decision, §9.1).
- **Updater key custody:** losing the signing private key bricks updates for installed users (official docs: "you will NOT be able to publish new updates") [S26]; key escrow/backup and cert renewal are operational prerequisites for enabling Phase 2.
- **WinUI 3 re-evaluation trigger:** if a stable Windows App SDK release adds a first-party notification-area API, re-run §6; the platform-independent supervisor core (spawn/stop/probe/state machine) is the portability boundary that keeps that swap bounded.
- **Port 3080 contingency:** if upstream makes the UI port dynamic or optional, the liveness oracle must switch to `doctor --json` output; the supervisor treats the probe endpoint as configuration, not a constant.
- **Graceful shutdown:** the harness has no documented shutdown API; the supervisor's default stop is force-stop (`TerminateJobObject`/`taskkill /F`) with the posture documented to users ("stop is force-stop; save work in the web UI first"). If upstream adds a graceful-shutdown signal, adopt it behind the same platform adapter.

## 12. Corrections to the previous artifact (explicit)

1. **VBScript/WiX claim removed.** The prior record claimed MSI builds require the VBSCRIPT optional feature "being deprecated and may be disabled in future Windows versions" ([previous §4.2]). The current Tauri installer page [S22] contains **no VBScript statement** (verified by full-text search of the live page, 2026-08-16). The claim is stale; WiX v3 remains the documented MSI toolset.
2. **Windows App SDK versions refreshed:** 2.4.0 stable (2026-08-13) replaces "1.5+/1.8" framing; 2.0.1 was the first SemVer-major stable (2026-04-29); 1.8.10 remains the serviced 1.8 line [S2][S3][S4]. The tray gap claim is **re-verified** against the 2.0–2.4 stable release notes and the still-open discussions — no first-party API as of 2026-08.
3. **Electron versioned:** 43.4.0 stable (Chromium 150, Node 24.18.1), 44 beta (Chromium 152) [S8][S9]; the prior record was version-less.
4. **Recommendation changed by design:** the prior record's "Tier A now, Tauri deferred" is superseded per the assignment ("most modern, best, effective… not merely minimum footprint"). Tier A is demoted to optional bootstrap; **Tauri 2 Tier C is the target architecture**.
5. **Avalonia added with a first-party tray correction:** the prior record implied the .NET ecosystem uniformly lacked tray support; Avalonia 11 has first-party `TrayIcon` [S43]. The gap is **WinUI 3-specific**, and it stands.
6. **Flutter/Slint/Wails/Dioxus/iced/egui/Neutralino/CMP added** with dated versions; Slint's tray API (1.17.0, 2026-06-24) is new and immature [S15].
7. **Tauri updater nuance:** the prior record omitted the "`createUpdaterArtifacts` will be removed in v3" migration warning and the `installMode` semantics; both are now in §4.1/§11 [S26].
8. **Spawn mechanism refined:** Job Object containment requires the child's process handle, which the JS shell plugin does not expose; the architecture now specifies a Rust-side spawn handler (§8) rather than implying the JS `Command` API suffices.

## 13. Source index

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
- [S52] Process creation flags — https://learn.microsoft.com/en-us/windows/win32/procthread/process-creation-flags
- [S53] taskkill reference — https://learn.microsoft.com/en-us/windows-server/administration/windows-commands/taskkill
- [S54] Job Objects — https://learn.microsoft.com/en-us/windows/win32/procthread/job-objects
- [S55] Node `child_process` (kill semantics, PID reuse, .bat/.cmd, env case) — https://nodejs.org/api/child_process.html
- [S56] WASDK App instancing (named mutex context) — https://learn.microsoft.com/en-us/windows/apps/windows-app-sdk/applifecycle/applifecycle-instancing
- [S57] Repo sources: `README.md`, `src/cli.ts`, `package.json` (`engines`), `docs/windows-support.md` (taskkill convention), `docs/roadmap.md`, `docs/adr/0001-no-fork.md`, `docs/security-model.md`

## 14. Terminology for the goal prompt (exact terms)

`tray-icon` Cargo feature · `TrayIcon.new()` · single-instance plugin · shell plugin capability scope (`shell:allow-spawn` with `name/cmd/args` validators) · Opener plugin · `CREATE_NO_WINDOW` · `CREATE_NEW_PROCESS_GROUP` · Job Object (`CreateJobObject` / `AssignProcessToJobObject` / `TerminateJobObject` / `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`) · `taskkill /T` fallback · PID-file + port probe `127.0.0.1:3080` · `DSH_HOME` state dir · atomic state file (`wx`) · no persisted secrets · CLI as sole lifecycle authority · NSIS per-user installer · WebView2 `downloadBootstrapper` · signed updater (`passive`, HTTPS endpoints, pubkey) · `signtool` OV/EV · per-user scope, no elevation · local-assets-only webview · foreign-port never killed.

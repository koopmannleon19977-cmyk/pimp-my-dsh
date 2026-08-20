# Windows support

`pimp-my-dsh` is Windows-first. This page records the exact support surface and
its limits.

## Supported Node.js versions

| Node.js | Status |
| --- | --- |
| 22.19.0 | CI-verified |
| 24 | Supported (primary) |
| 26 | CI-verified |

The package manifest declares `^22.19.0 || >=24.0.0`.

## Automated setup

Install the Windows baseline with:

```powershell
pimp-dsh setup --profile windows
```

The command uses the bundled, exact-pinned pnpm runtime with lifecycle scripts
and pnpm hooks disabled, stages the complete profile, and atomically moves it
under the canonical `DSH_HOME`. The `windows` overlay is intentionally empty:
the upstream base rows plus the distribution patch already select PowerShell,
disable Bash, and mount the Windows ACL sandbox by `process.platform`.

## Shell backend

| Backend | Windows status |
| --- | --- |
| PowerShell (`pwsh`) | **Active.** The model-facing `pwsh` tool and the `pwsh-sandbox` executor are mounted. |
| Bash | **Disabled.** The upstream `bash-sandbox` and `tool-bash` rows carry `disabled: !!js process.platform === 'win32'`; bash has no Windows runner. |

The upstream base bundle gates both shell stacks by platform on its own rows:
`bash-sandbox`/`tool-bash` are disabled on win32, and their twins
`pwsh-sandbox`/`tool-pwsh` mount on win32 only. Exactly one shell stack is
active per host.

## Direct CLI sandbox

Direct `pimp-dsh run` resolves to the ACL restricted-token runner chain
(`dsh-sandbox-local` → `@deepseek-ai/dsh-sandbox-windows-acl`).

| Property | Value |
| --- | --- |
| Mechanism | `WRITE_RESTRICTED` token with workspace + private-temp SIDs |
| Enforcement | `partial` |
| Default mode | `workspace-write` |
| Write boundary | Workspace + private per-session temp subdirectory |
| Escalation | `danger-full-access` via approval prompt |
| Reads | **Not restricted** |
| Network | **Not restricted** |
| Process visibility | **Not restricted** |

The partial-enforcement boundaries are documented in
[docs/security-model.md](security-model.md#direct-cli-partial-write-confinement).

This table applies to direct CLI runs. They remain write-only and unconfined
for reads, network, and process visibility; the packaged desktop-supervised
web-run boundary is separate.

## Process cleanup

Background processes are terminated with `taskkill /T`, which kills the process
tree. This is the Windows equivalent of POSIX process-group termination.

## LSP

Language-server navigation is **opt-in only** (`PIMP_DSH_ENABLE_LSP`).
Configured language servers run **unsandboxed**. See
[docs/security-model.md](security-model.md#lsp-explicit-opt-in-unsandboxed).

## Persistent bash PTY

Persistent bash PTY sessions are **not supported on Windows**. The upstream
terminal backend that provides persistent bash PTYs has no Windows runner. The
PowerShell executor provides foreground and background execution, but not a
persistent interactive PTY.

## Known Windows limitations

- **WMI/CIM cmdlets fail under confinement.** `Authenticated Users` is absent
  from the restricting list, so the WMI namespace security check fails
  (`0x80041003`). CIM cmdlets and `Get-ComputerInfo` are unavailable in every
  confined mode.
- **PowerShell language mode differs by confined mode.** Under `read-only`,
  PowerShell starts in ConstrainedLanguage (`Add-Type`, non-core .NET static
  calls, COM, and reflection fail). Under the shipped `workspace-write` path,
  the private-temp capability lets the AppLocker probe complete, so pwsh stays
  in FullLanguage unless host-wide policy says otherwise.
- **First confined write on a large workspace is slow.** The workspace ACE is
  materialized with eager full-tree propagation, paid once per workspace per
  machine.
- **`whoami` and token-inspection cmdlets fail under the restricted token.**
  This is diagnostic noise of the restriction scheme, not an operational
  failure.
- **FAT-class volumes are writable** under confined modes (no ACL support).

## Desktop supervisor (Windows)

### Packaged web-run AppContainer

Packaged Windows desktop-supervised web runs use a unique zero-capability
AppContainer profile, private physicalized runtime/profile roots, an empty
credential file, AppContainer-virtualized Temp path, authenticated per-package
native-module links, and disabled credential/settings/HMR watchers. The host
creates the control
and per-connection data pipes with the exact per-run AppContainer SID. A pinned
Node preload holds the child server on a `LOCAL` named-pipe lifecycle anchor
and accepts authenticated, sequenced `web-accept` requests through a
process-global acceptor; the child does not open a TCP listener.
For each data connection the child writes the token received on the
authenticated control channel; the host verifies it before forwarding cookies
or request bytes, so another allowed current-user pipe client cannot receive or
inject browser traffic.

The trusted host owns the loopback proxy. Status reports its public base URL,
while desktop navigation receives a private bootstrap URL that installs an
`HttpOnly`, host-only, `SameSite=Strict` cookie with no-store and no-referrer
protections. The proxy then tunnels raw HTTP, SSE, and WebSocket bytes over
authenticated host-created data pipes.

The rc.7 component gate
`private_real_web_run_serves_through_authenticated_host_pipe_proxy` and the
release-behavior Supervisor gate
`packaged_supervisor_serves_and_stops_the_confined_web_run` both pass. Together
they cover HTTP 200, private/public endpoint separation, graceful shutdown,
empty Job, history outcome, and per-run profile removal. A packaged desktop
smoke additionally verified the real WebView2 path: the embedded harness
window rendered the rc.7 UI after the private bootstrap 303 and cookie, and
the graceful UI stop removed the per-run AppContainer profile. Startup and
transport failures never fall back to an unconfined desktop child.

This boundary does not apply to direct `pimp-dsh run`, and it does not make
every ambient host object unreadable. Objects with broad package/world-readable
ACLs may remain visible inside the zero-capability AppContainer.

### WebView2 on Windows

Tauri uses **Microsoft Edge WebView2** to render the desktop controller UI.
WebView2 is the Evergreen runtime — preinstalled and auto-updated via Edge
updates on Windows 10/11. Five installer modes:

| Mode | Size | Requirement |
| --- | --- | --- |
| `downloadBootstrapper` | +0 MB | Internet connection, default |
| `embedBootstrapper` | ~1.8 MB | Internet (bootstrapper bundled in installer) |
| `offlineInstaller` | ~127 MB | None (bundled installer) |
| `fixedVersion` | ~180 MB | None (specific WebView2 version bundled) |
| `skip` | +0 MB | WebView2 already installed (**not recommended**) |

Air-gapped deployments must use `offlineInstaller` or `fixedVersion`. The
installer picks one mode at build time.

### NSIS installer

The desktop ships an NSIS per-user installer (`*-setup.exe`):

- **Per-user:** no elevation required, no machine-wide registry writes.
  Installs under `AppData\Local` by default.
- **Uninstall:** removes the application, Start menu/Desktop shortcuts, and
  per-user uninstall registry entry. Controller state/logs and `DSH_HOME`
  profile data are preserved.
- **Unsigned local builds** trigger SmartScreen "not trusted" warnings when
  downloaded via browser. They are development artifacts only.
- **Production builds** require dual Authenticode signing (Phase 2):
  `signtool` with an OV or EV certificate for the `.exe` and `.msi`.


### Browser egress confinement (optional, per machine)

`scripts/confine-browser.ps1` pins Windows Firewall outbound block rules on the
exact Playwright Chromium executable (loopback, RFC1918, link-local,
multicast), so agent-driven browsing cannot reach internal services while
public internet stays available. Apply once per machine, elevated:

```powershell
pwsh -NoProfile -ExecutionPolicy Bypass -File scripts/confine-browser.ps1 -Apply
```

Check status unelevated (`-Verify`, exit 0 = confined) and remove the rules
with `-Cleanup` (elevated). Re-run `-Apply` after any Playwright pin upgrade —
the new `chromium-XXXX` directory gets a fresh rule set. While confined,
browsing to `127.0.0.1` (including the harness web UI) is intentionally
impossible.
### Tauri tray and single instance

- **Tray:** first-party `tray-icon` Cargo feature. Native notification-area
  icon with context menu. Supported on Windows, macOS, Linux.
- **Single instance:** official Tauri plugin. Windows fully supported via
  named mutex. Prevents multiple resident controllers per user.

### Process lifecycle primitives

- **Hidden suspended launch:** `CREATE_NO_WINDOW` (no console flash) +
  `CREATE_SUSPENDED` (primary thread starts suspended so the Job can be
  assigned before execution).
- **Explicit handle inheritance:** `PROC_THREAD_ATTRIBUTE_HANDLE_LIST` limits
  inherited handles to stdio pipes and the bridge pipe only. The Job handle
  is retained non-inheritable by the controller.
- **Unnamed kill-on-close Job Object:** `CreateJobObject` with a `NULL` name
  (no global name to squat), `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`. Closing
  the last handle terminates all processes and destroys the job. Nested jobs
  are supported (Windows 8+).
- **Tree stop:** primary mechanism is `TerminateJobObject`. The existing
  `taskkill /T` fallback remains in repo docs. Node's `subprocess.kill()` is
  **not** tree-kill on Windows — the controller never relies on it.
- **PID reuse:** Windows PID reuse is real. The controller never kills or
  adopts by PID, image path, or port. Authority is the live process HANDLE,
  Job handle, and random run ID only.

### Unsigned development artifacts

Locally built NSIS installers have no Authenticode signature and are
**development only**:

- They trigger SmartScreen "not trusted" warnings on browser downloads.
- Use them only for local development and testing.
- Production distribution requires Authenticode signing (`signtool`) — Phase 2.

### No Windows service

The controller is a per-user resident application, not a Windows service.
No elevation, no machine-wide registry writes, no service controls. Autostart
  is an explicit opt-in via a per-user HKCU Run key (first-party `tauri-plugin-autostart`); no machine-wide registry writes and no COM/IShellLink code.

### Data and log paths on Windows

State files, logs, and bridge pipe artifacts live under the per-user
application data directory. The harness home (`DSH_HOME`) and managed profile
directory must remain outside the writable workspace.

Supervisor settings (theme, fixed port, restart policy, notification
opt-in) persist per supervised profile at
`%LOCALAPPDATA%\pimp-my-dsh\state\<profile>.json` and are written after each
change. Run history is a single global `runs.json` in the same directory.
Persistence is best-effort: the in-memory value stays authoritative if a
disk write fails.

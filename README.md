# pimp-my-dsh

A **Windows-first** distribution of [DeepSeek Harness](https://github.com/deepseek-ai/deepseek-harness)
(`dsh`). It consumes upstream as an exact npm dependency — it never forks it —
and adds a safe feasibility prototype with an install/run/doctor workflow.

> **Status: feasibility prototype.** This is not a production-hardened product.
> Read [Security model](docs/security-model.md) before running it against
> anything you care about.

## What this is

DeepSeek Harness is an agent harness where *everything is a plugin*, powered by
[Cordis](https://github.com/cordiverse/cordis). `pimp-my-dsh` composes the
upstream plugin bundles through a patch layer and a single distribution-owned
plugin, then wraps them in a small CLI that owns setup, run, and diagnosis.

It does **not** reimplement upstream native tools. Read, search, edit, pwsh,
sessions, skills, todo, and ordinary subagents come from the upstream bundle.
Distribution code composes and hardens them and adds an isolated-worktree
provider where the upstream request seam has no per-child workspace override.

## What this is not

- **Not a fork.** Upstream is consumed as a published npm artifact. See
  [ADR-0001](docs/adr/0001-no-fork.md).
- **Not a logged-in browser or GitHub automation tool.** Web fetch and web
  search stay disabled. Browser automation is isolated, headless, and opt-in;
  there is no existing-profile control or GitHub write automation.
- **Not a full sandbox.** On Windows, the sandbox restricts writes only and is
  explicitly partial. See [Security model](docs/security-model.md).

## Requirements

- **Windows 10/11** (primary target) or Ubuntu (CI-verified).
- **Node.js** `^22.19.0 || >=24.0.0`.
- **pnpm** `11.7.0`.
- **PowerShell** (Windows shell backend; bash is disabled on Windows).

Install pnpm separately; Node.js 25+ no longer bundles Corepack.

### Supported runtime matrix

| Host | Architecture | Node.js | Status |
| --- | --- | --- | --- |
| Windows 10/11 | x64 | 22.19, 24, 26 | Primary; CI matrix plus local Windows 11 smoke |
| Ubuntu (`ubuntu-latest`) | x64 | 22.19, 24, 26 | Secondary; CI matrix |
| macOS | any | any | Unsupported and unverified |
| Windows/Linux | arm64 | any | Unsupported and unverified |

All rows use pnpm `11.7.0`, DeepSeek Harness `0.1.0-rc.6`, and the exact
dependencies in `pnpm-lock.yaml`. The package engine range permits later
Node.js releases, but they are not promoted to the verified matrix until CI
covers them. Browser automation uses the pinned Playwright MCP package and
remains opt-in. See [Windows support](docs/windows-support.md) for shell,
sandbox, process, LSP, and persistent-terminal limitations.

## Install from GitHub

```sh
git clone https://github.com/koopmannleon19977-cmyk/pimp-my-dsh.git
cd pimp-my-dsh
pnpm install --frozen-lockfile --ignore-scripts
pnpm run build
```

The package is not published to npm yet. From the checkout, invoke its binary
through the `pimp-dsh` package script as shown below.

## Setup

Create a profile from the shipped templates:

```sh
pnpm pimp-dsh setup --profile web
```

`setup` builds the complete profile in a staging directory, installs exact
dependencies with package hooks and lifecycle scripts disabled, then moves it
under `DSH_HOME`. It rejects redirected paths and existing unmanaged profiles.
`--force` atomically replaces only a profile previously owned by this
distribution; it does not preserve additional dependencies or bundles.

On Windows, `pimp-dsh setup --profile windows` installs the headless Windows
baseline. Platform-gated base rows select PowerShell, disable Bash, and mount
the ACL write-confinement backend automatically.

## Run

```sh
pnpm pimp-dsh run --profile web -- --port 8080
```

Everything after `--` is forwarded as application arguments behind upstream's
end-of-options boundary; it cannot override the validated profile or add
launcher patch overlays. The `web` profile serves the Web UI at
`http://127.0.0.1:3080` by default.

For security, `run` rejects a global `$DSH_HOME/cordis.patch.yml`: upstream
would compose it above the managed profile. The harness home and profile must
also remain outside the writable workspace so an agent cannot create that
watched patch after preflight. Use the shipped profile templates and the
dedicated settings/credentials stores instead.

For a one-shot task:

```sh
pnpm pimp-dsh run --profile headless -- "Summarize this repository"
```

## Doctor

```sh
pnpm pimp-dsh doctor
```

`doctor` reports Node and platform details, pinned DSH availability, profile
state, and configuration flags without exposing credential values. Use `--json`
for a stable machine-readable result.

## Update check

```sh
pnpm pimp-dsh update-check
```

Reports whether a newer `pimp-my-dsh` release is available. It makes no
telemetry request, sends no machine data, and never installs automatically.
Updates remain an explicit package-manager action so the exact version and
artifact can be reviewed before profile migration.

## Migrate

```sh
pnpm pimp-dsh migrate --profile web          # dry run (default)
pnpm pimp-dsh migrate --profile web --apply  # apply
```

`migrate` validates the distribution ownership marker, reports source and
target bundle versions, and is a dry run unless `--apply` is passed. Applying a
required migration installs the complete current profile in staging and swaps
it atomically; it never patches an unowned profile or downgrades a newer one.

## Configuration

Environment variables are read by name; values are never logged.

Set the `PIMP_DSH_*` variables in the parent process environment, not in a
repository `.env` file. Before launching DSH, the wrapper promotes them to
protected `DSH_PIMP_*` names and removes the public names from the child
environment. Upstream rejects project `.env` overrides of protected bootstrap
variables.

| Variable | Purpose |
| --- | --- |
| `DSH_HOME` | Harness home directory (upstream). Defaults to `~/.dsh`. |
| `PIMP_DSH_API_KEY` | API key for the configured model provider. |
| `PIMP_DSH_BASE_URL` | Base URL for the configured model provider. |
| `PIMP_DSH_MODEL` | Model identifier. |
| `PIMP_DSH_ENABLE_LSP` | Explicit opt-in for language-server navigation. |
| `PIMP_DSH_ENABLE_BROWSER` | Explicit opt-in for isolated headless Chrome automation. |

The upstream `DSH_PERMISSION_MODE` variable is disclosed for completeness, but
the shipped profiles own safe defaults. The wrapper forces
`DSH_TELEMETRY_DISABLED=1`, removes mode/endpoint overrides from the child
environment, and the bundle disables the telemetry row. `DSH_TELEMETRY_MODE`
therefore cannot reactivate telemetry.

## Windows support

| Area | Status |
| --- | --- |
| Node.js | 24 supported; CI also covers 22.19 and 26 |
| Shell backend | PowerShell active; bash disabled |
| Sandbox | ACL restricted-token runner, `enforcement: partial` |
| Write boundary | `workspace-write` plus approval prompt |
| Process cleanup | `taskkill /T` tree termination |
| LSP | Opt-in only; servers run unsandboxed |
| Persistent bash PTY | Not supported on Windows |

Full details: [docs/windows-support.md](docs/windows-support.md).

## Capabilities

| Capability | Status |
| --- | --- |
| Read / search / edit workspace files | Enabled (upstream) |
| PowerShell execution | Enabled, write-confined (partial) |
| Bash execution | Disabled on Windows |
| Sessions / skills / todo | Enabled (upstream) |
| Parallel subagents | Enabled (native fresh children; four-call pool; depth 3) |
| Isolated subagents | Enabled (`subagent_worktree`; one-shot, approval-gated, retained branch) |
| Scoped Git status / diff / log | Enabled (`pimp_git_read`, read-only) |
| Durable memory | Enabled (`pimp_memory`, append-only under `DSH_HOME`) |
| GitHub repository / issue / PR / file reads | Enabled (`pimp_github_read`, fixed read operations) |
| Telemetry | Disabled unconditionally |
| Web fetch | Disabled (SSRF risk) |
| Web search | Opt-in (`pimp_web_search`, fixed-host Tavily API) |
| Browser automation | Opt-in, isolated Chrome; risk-gated, unsafe server code denied |
| LSP navigation | Opt-in, unsandboxed |
| Community plugins | Reviewed allowlist gate only |

Full matrix: [docs/capabilities.md](docs/capabilities.md).

Browser automation uses Microsoft's pinned `@playwright/mcp` through the
first-party DSH MCP client. It starts a fresh in-memory, headless Chrome
profile, blocks service workers, omits image payloads, and receives no provider
credentials. Page content remains untrusted. Only bounded inspection tools run
without approval. Navigation, clicks, typing, uploads, script evaluation,
storage access, unknown future browser tools, and other stateful operations
enter the approval pipeline; profiles without an interactive answerer fail
closed. Arbitrary code execution in the unsandboxed browser server is denied.
Browser network egress is not confined, so the capability remains disabled
unless `PIMP_DSH_ENABLE_BROWSER=1`.

`subagent_worktree` creates a unique branch and worktree under
`DSH_HOME/pimp-my-dsh/worktrees`, initializes its index from `HEAD`, and copies
the current contents of tracked workspace files without copying untracked
files. Sparse/skip-worktree indexes, non-UTF-8 index paths, submodules, linked
directory ancestors, and tracked links that are dangling or escape the
repository fail closed. The child runs there under the inherited sandbox mode
with approval
pinned to `never`. The worktree and branch remain for human review; nothing is
merged or deleted automatically. Worktree creation itself requires an
interactive approval channel.

## Desktop supervisor (Windows)

A per-user tray controller built with **Tauri 2.11.5** (Rust core + React view) sits above the
existing `pimp-my-dsh` CLI. It provides persistent tray presence, process supervision, and
structured health reporting — all on Windows 10/11 x64.

> **Rendering note:** Tauri on Windows uses Microsoft Edge WebView2 to render its UI. This is a
> web engine, not truly native rendering. The control surface looks and feels polished but it does
> not produce pixels via the OS toolkit. See
> [docs/security-model.md#desktop-rendering](docs/security-model.md#desktop-rendering).

### What it owns

| Responsibility | How |
| --- | --- |
| **Lifecycle authority** | Rust backend; owns the closed state machine (13 states), the unnamed kill-on-close [Job Object](https://learn.microsoft.com/windows/win32/procthread/job-objects), and all live process/Job handles. JavaScript is a read-only view. |
| **Process launch** | `CreateProcessW` with `CREATE_NO_WINDOW | CREATE_SUSPENDED`, an explicit inherited-handle allowlist, and fixed `node.exe` absolute path. No shell, no PATH fallback, no `cmd.exe`. |
| **Port selection** | Default start is **dynamic** (`--port 0`, OS-assigned). A fixed 3080 is opt-in; a foreign/busy 3080 fails with `PORT_IN_USE_FOREIGN` — never inspected, never killed. |
| **Child bridge** | Named-pipe transport, versioned v1 contract, random per-run 64-char hex token, `PIPE_REJECT_REMOTE_CLIENTS`, bounded 64 KiB frames, first-instance creation only. |
| **Graceful / forced stop** | Bridge sends cooperative shutdown (upstream's `PROCESS_SHUTDOWN_TIMEOUT_MS = 5s`); deadline elapses → `TerminateJobObject`. A forced stop is **never** reported as graceful. |
| **Close / quit** | Closing the window hides the tray controller (resident). Explicit Quit follows the stop policy and never silently detaches the harness. |
| **Authority identity** | Live process HANDLE, Job handle, and random run ID. PID, image path, creation time, port, and state files are **diagnostics only** — never grounds to kill or adopt. |
| **Logs** | `LogEvent` struct with 16 KiB/event limit, secret redaction of `PIMP_DSH_*`/`DSH_PIMP_*` env values, bounded in-memory queue, disk writer fallback to drain-and-discard. |
| **Runtime manifest** | `compatibility` manifest v1 verified before launch: controller `0.1.0`, Node `24.19.0`, pnpm `11.7.0`, distribution `0.1.0`, DSH `0.1.0-rc.6`, target `x86_64-pc-windows-msvc`, `node.exe` SHA-256, payload tree hash. |
| **Data / log paths** | State, logs, and bridge artifacts live under the per-user app data directory. The harness home (`DSH_HOME`) and managed profile directory must remain outside the writable workspace. |
| **NSIS installer** | Per-user `*-setup.exe`, `downloadBootstrapper` WebView2 mode (+0 MB on systems with Edge updates). Uninstall removes the app, shortcuts, and per-user uninstall registry entry while preserving controller state/logs and `DSH_HOME` profile data. |
| **Signing boundary** | Dual signing is wired for Phase 2: the release workflow injects an Authenticode certificate (PFX secret + thumbprint) and signs updater artifacts with the Tauri private key. Development builds are unsigned — **development only, not for distribution**. |
| **Updater key boundary** | Tauri's updater requires a signed private key; losing it bricks updates. The keypair is generated (public key committed, private key gitignored), custody/rotation rules live in SECURITY.md, and `scripts/release-setup.sh` walks the offline move + CI secret setup. |

### What it does not do

- **No shell plugin in the renderer.** JavaScript has no `shell`, `opener`, `filesystem`, or `updater` capabilities. Spawning exists only in Rust.
- **No telemetry, no LAN, no daemon.** The controller sends no machine data and never runs as a Windows service.
- **No logged-in browser or desktop automation.** The controller renders the harness web UI in an embedded zero-capability webview (never inside the privileged controller webview); it does not drive a logged-in browser.
- **No unsigned installer for production.** The locally built NSIS `*-setup.exe` has no Authenticode signature and is a development artifact only. Production releases require dual Authenticode + Tauri update signatures.

### Architecture in brief

```
┌─────────────────────────────────────────────────────────┐
│ Tray / Window (React, no capabilities)                  │
│   get_snapshot · start_harness · stop_harness           │
│   run_doctor · open_harness · reveal_log_folder         │
│   set_theme · set_fixed_port                            │
├─────────────────────────────────────────────────────────┤
│ Tauri 2.11.5 core (Rust)                                │
│   State machine · Job Object · process supervisor        │
│   Bridge (named pipe, token-auth) · log pipeline         │
│   Provider (packaged / debug) · compatibility manifest   │
├─────────────────────────────────────────────────────────┤
│ DeepSeek Harness CLI (Node, child process)              │
│   `pimp-dsh run --profile web` — validated boundary      │
└─────────────────────────────────────────────────────────┘
```

See [docs/architecture.md](docs/architecture.md) for the full architecture.
See [docs/security-model.md](docs/security-model.md) for the security model including the
desktop-specific boundaries.
See [docs/windows-support.md](docs/windows-support.md) for Windows-specific notes.
See [docs/adr/0002-tauri-desktop-supervisor.md](docs/adr/0002-tauri-desktop-supervisor.md)
for the Tauri decision record.

## Upstream version pin

This distribution pins `@deepseek-ai/dsh` and every direct
`@deepseek-ai/dsh-*` package to the exact version `0.1.0-rc.6`.

There is a known skew: the published npm artifact is `0.1.0-rc.6`, while the
upstream `master` branch's `apps/cli/package.json` still declares
`0.1.0-rc.5`. The npm pin is authoritative for this distribution. See
[docs/upstream-pin.md](docs/upstream-pin.md).

## Roadmap

Phase-gated, no dates promised. See [docs/roadmap.md](docs/roadmap.md).

## Documentation

- [Architecture](docs/architecture.md)
- [Security model](docs/security-model.md)
- [Windows support](docs/windows-support.md)
- [Capability matrix](docs/capabilities.md)
- [Upstream version pin](docs/upstream-pin.md)
- [Roadmap](docs/roadmap.md)
- [ADR-0001: no fork](docs/adr/0001-no-fork.md)
- [ADR-0002: Tauri desktop supervisor](docs/adr/0002-tauri-desktop-supervisor.md)

## Security

Report vulnerabilities privately. See [SECURITY.md](SECURITY.md).

## License

[MIT](LICENSE). Third-party notices: [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).

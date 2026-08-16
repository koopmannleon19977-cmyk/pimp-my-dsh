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
sessions, skills, todo, and subagents all come from the upstream bundle. The
distribution's job is composition and hardening, not duplication.

## What this is not

- **Not a fork.** Upstream is consumed as a published npm artifact. See
  [ADR-0001](docs/adr/0001-no-fork.md).
- **Not a browser or GitHub automation tool.** Web fetch, web search, and
  browser automation are disabled. There is no GitHub write automation.
- **Not a full sandbox.** On Windows, the sandbox restricts writes only and is
  explicitly partial. See [Security model](docs/security-model.md).

## Requirements

- **Windows 10/11** (primary target) or Ubuntu (CI-verified).
- **Node.js** `^22.19.0 || >=24.0.0`.
- **pnpm** `11.7.0`.
- **PowerShell** (Windows shell backend; bash is disabled on Windows).

Install pnpm separately; Node.js 25+ no longer bundles Corepack.

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
telemetry request and sends no data about your machine.

## Migrate

```sh
pnpm pimp-dsh migrate --profile web          # dry run (default)
pnpm pimp-dsh migrate --profile web --apply  # apply
```

`migrate` upgrades a profile's patch data to the current distribution format.
It is a dry run unless `--apply` is passed.

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
| Sessions / skills / todo / subagents | Enabled (upstream) |
| Scoped Git status / diff / log | Enabled (`pimp_git_read`, read-only) |
| Durable memory | Enabled (`pimp_memory`, append-only under `DSH_HOME`) |
| Telemetry | Disabled unconditionally |
| Web fetch / search | Disabled (SSRF risk) |
| Browser automation | Disabled |
| LSP navigation | Opt-in, unsandboxed |
| Community plugins | Reviewed allowlist gate only |

Full matrix: [docs/capabilities.md](docs/capabilities.md).

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

## Security

Report vulnerabilities privately. See [SECURITY.md](SECURITY.md).

## License

[MIT](LICENSE). Third-party notices: [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).

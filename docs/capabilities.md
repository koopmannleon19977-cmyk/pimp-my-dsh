# Capability matrix

Current capability status for `pimp-my-dsh` `0.1.0`. This is a point-in-time
snapshot; the roadmap is in [roadmap.md](roadmap.md).

## Core agent capabilities (upstream)

| Capability | Status | Notes |
| --- | --- | --- |
| Read workspace files | Enabled | Upstream `dsh-tool-fs` |
| Search workspace files | Enabled | Upstream `dsh-tool-fs-search` |
| Edit workspace files | Enabled | Upstream `dsh-tool-str-replace-editor` |
| PowerShell execution | Enabled, write-confined | `pwsh-sandbox` + `tool-pwsh`; partial enforcement |
| Bash execution | Disabled on Windows | No Windows runner |
| Sessions | Enabled | Upstream session log |
| Skills | Enabled | Upstream skill registry |
| Todo | Enabled | Upstream `dsh-tool-todo` |
| Subagents | Enabled | Upstream `dsh-tool-subagent` |
| Goals | Enabled | Upstream goal domain |
| Background jobs | Enabled | Upstream `dsh-tool-jobs` |

## Distribution-owned hardening

| Capability | Status | Notes |
| --- | --- | --- |
| Telemetry | **Disabled unconditionally** | `DSH_TELEMETRY_MODE` ignored |
| Web fetch | **Disabled** | SSRF primitive; no safe provider |
| Web search | **Disabled** | No safe provider |
| Browser automation | **Disabled** | Not shipped |
| LSP navigation | **Opt-in, unsandboxed** | `PIMP_DSH_ENABLE_LSP` |
| Community plugins | **Reviewed allowlist gate** | Policy, not code; no registry |
| GitHub write automation | **Not present** | No such capability exists |

## CLI surface

| Command | Purpose |
| --- | --- |
| `setup --profile <name> [--force] [--json]` | Atomically create an exact managed profile from a shipped template |
| `run --profile <name> -- <app args...>` | Validate and boot a managed profile; forwarded args cannot become launcher flags |
| `doctor [--profile <name>] [--json]` | Diagnose environment and profile state |
| `update-check [--json]` | Check for a newer distribution release |
| `migrate --profile <name> [--apply] [--json]` | Upgrade profile patch data (dry run by default) |

## Not claimed

This distribution does **not** claim:

- Browser automation of any kind.
- GitHub write automation (issue/PR creation, pushes, etc.).
- A community plugin registry or catalog.
- Full sandbox isolation on Windows.
- Network egress control.

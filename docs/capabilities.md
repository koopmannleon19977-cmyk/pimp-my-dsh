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
| Parallel subagents | Enabled, bounded | Native fresh-session `spawn` provider; continuable background children; four starts per rolling tool pool; depth 3 |
| Goals | Enabled | Upstream goal domain |
| Background jobs | Enabled | Upstream `dsh-tool-jobs` |

## Distribution-owned hardening

| Capability | Status | Notes |
| --- | --- | --- |
| Telemetry | **Disabled unconditionally** | `DSH_TELEMETRY_MODE` ignored |
| Web fetch | **Disabled** | SSRF primitive; no safe provider |
| Web search | **Opt-in** | `pimp_web_search`: fixed-host Tavily API (`PIMP_DSH_ENABLE_WEB_SEARCH` + `PIMP_DSH_WEB_SEARCH_KEY`); redirects rejected, bounded query/response, key never echoed |
| Browser automation | **Opt-in, risk-gated** | Bounded inspection allowlist; navigation, interactions, storage, and unknown tools ask; unsandboxed server code denied |
| LSP navigation | **Opt-in, unsandboxed** | `PIMP_DSH_ENABLE_LSP` |
| Scoped Git reads | **Enabled** | `pimp_git_read`: fixed status/diff/log in the calling agent workspace; helper execution, lazy fetch, secrets, and arbitrary args disabled |
| GitHub reads | **Enabled** | `pimp_github_read`: bounded repo, issue, PR, file, and issue/PR search through trusted `gh` |
| Worktree subagents | **Enabled, approval-gated** | `subagent_worktree`: one-shot child on unique retained branch; tracked working state copied; untracked files excluded; sparse indexes, non-UTF-8 paths, submodules, and unsafe links rejected; no automatic merge/delete |
| Durable memory | **Enabled** | `pimp_memory`: canonical non-linked JSONL under `DSH_HOME`; bounded recall |
| Community plugins | **Reviewed allowlist gate** | Machine-readable checklist + [authoring guide](plugin-authoring.md) + exact-pin manifest validation; empty by default; no registry |
| GitHub write automation | **Opt-in, approval-gated** | `pimp_github_write`: PR/issue/comment via fixed-argv `gh`; every call asks; push deferred (human step) |
| Docs MCP (Context7) | **Opt-in** | `mcp-context7`: preconfigured streamable HTTP to the fixed host `mcp.context7.com`; no local code, no key required; `PIMP_DSH_ENABLE_CONTEXT7` + optional `PIMP_DSH_CONTEXT7_KEY` |

All shipped tools use DSH canonical structured values for runtime/UI consumers
and separate bounded text renderers for model-facing results. The
distribution-owned Git result reports `{ operation, output, truncated }`;
GitHub reads report `{ operation, repository, data, truncated }`; memory reports
`{ operation, records }`.

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

- Browser network isolation, logged-in profile control, or desktop computer use.
- GitHub pushes (PR/issue/comment writes are approval-gated; pushing branches stays a human step).
- A community plugin registry or catalog.
- Full sandbox isolation on Windows.
- Network egress control.

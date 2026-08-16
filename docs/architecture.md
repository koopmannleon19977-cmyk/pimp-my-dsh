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

## Why no fork

The decision to consume upstream as an exact npm dependency is recorded in
[ADR-0001](adr/0001-no-fork.md), with evidence and reassessment triggers.

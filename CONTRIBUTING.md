# Contributing

Thank you for your interest in `pimp-my-dsh`.

`pimp-my-dsh` is a **Windows-first distribution** of DeepSeek Harness. It
consumes upstream as an exact npm dependency and never forks it. The design
decision and its reassessment triggers are recorded in
[docs/adr/0001-no-fork.md](docs/adr/0001-no-fork.md).

## What this project is

- A thin composition layer: a `cordis.patch.yml` that overrides upstream bundle
  rows by stable id, plus a single distribution-owned plugin.
- A CLI (`setup`, `run`, `doctor`, `update-check`, `migrate`) that owns the
  install/run/diagnose workflow.
- Profile patch files under `profiles/` that are data copied by `setup`, never
  separate code.

## What this project is not

- It is **not a fork** of DeepSeek Harness. Do not copy upstream source into
  this repository.
- It does **not** reimplement upstream native tools (read, search, edit, pwsh,
  sessions, skills, todo, subagents). Those come from the upstream bundle.
- It does **not** ship browser automation or GitHub write automation.

## Ways to contribute

- **Report issues** — bugs, documentation gaps, and Windows-specific problems.
- **Improve documentation** — the docs in `docs/` and the root `*.md` files.
- **Propose a community plugin for the reviewed allowlist gate** — update
  [`schema/community-plugin-allowlist-v1.json`](schema/community-plugin-allowlist-v1.json)
  only after the human review of source, license, exact version, package
  integrity, permission surface, and Windows behavior described in
  [docs/security-model.md](docs/security-model.md#community-plugins).
- **Harden the distribution** — telemetry stays off, web stays off, and the
  Windows sandbox boundary stays honestly disclosed. Changes that weaken these
  guarantees are out of scope.

## Development setup

Prerequisites:

- Node.js `^22.19.0 || >=24.0.0`
- pnpm `11.7.0`

```sh
pnpm install --frozen-lockfile --ignore-scripts
pnpm run typecheck
pnpm run test
pnpm run build
```

The CI workflow (`.github/workflows/ci.yml`) runs the same steps on Node
22.19.0, 24, and 26 across Windows and Ubuntu.

## Conventions

- ESM TypeScript.
- Upstream packages are pinned to exact `0.1.0-rc.6`; never use dist-tags or
  carets for `@deepseek-ai/dsh` or any `@deepseek-ai/dsh-*` package.
- The CLI must never overwrite an existing profile patch without explicit
  `--force`.
- The CLI must never log secret values.
- Structured CLI results are JSON-capable and stable.

## Security

Report vulnerabilities privately. See [SECURITY.md](SECURITY.md).

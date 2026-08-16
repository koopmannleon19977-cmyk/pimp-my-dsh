# ADR-0001: Consume DeepSeek Harness as an exact npm dependency (no fork)

- **Status:** Accepted
- **Date:** 2026-08-16
- **Deciders:** `pimp-my-dsh` maintainers

## Context

`pimp-my-dsh` is a Windows-first distribution of DeepSeek Harness (`dsh`). The
upstream project is a large, rapidly iterating TypeScript monorepo in developer
preview, with an explicit warning that there will be compatibility-breaking
changes.

Two ways to build a distribution were considered:

1. **Fork** the upstream repository and modify it in place.
2. **Consume** upstream as a published npm dependency and compose/harden it
   through a patch layer.

## Decision

**Consume upstream as an exact npm dependency; do not maintain a fork at the
current evidence point.**

`pimp-my-dsh` depends on `@deepseek-ai/dsh@0.1.0-rc.6` and every direct
`@deepseek-ai/dsh-*` package at the exact version `0.1.0-rc.6`. The
distribution composes upstream bundles through `cordis.patch.yml`, adds
distribution-owned plugins and a small CLI, and contains no copied upstream
source.

## Evidence

| Question | Verified evidence | Decision impact |
| --- | --- | --- |
| Are required extension seams public? | Source imports use published package roots only. Model rows, tools (`ctx.tools.register`), prompt sections (`ctx.systemPrompt`), approval events (`tools/pre-execute`), sessions, MCP, and subagent providers (`ctx.subagents.registerProvider`) compose without upstream source edits. | No missing seam currently requires a fork. |
| Can bundle overrides survive upstream drift? | The distribution targets 16 upstream row ids. `tests/patch-contract.test.ts` resolves the pinned published `@deepseek-ai/dsh-base` patch and fails when a target disappears or is renamed. | Exact pins plus a release-blocking contract test are adequate; silent drift is not accepted. |
| Is the security model replaceable? | `cordis.patch.yml` replaces sandbox, permission, approval, shell, telemetry, and web-tool rows. `src/plugin.ts` adds a fail-closed pre-execution gate. Browser and LSP capabilities remain disabled until explicit opt-in. | Current controls need no core patch. Windows read/network confinement remains a capability limit, not a hidden claim. |
| Does a custom UI require core changes? | The web bundle is an ordinary patch composition. `packages/host/frontend-static` serves a replaceable SPA, `packages/client/*` exposes client plugins and UI slots, and `packages/host/apiproxy` publishes a transport-independent TypeScript/fetch contract. | A branded web client, desktop wrapper, or alternate carrier can be a bundle/plugin. The unversioned prerelease wire protocol must remain lockstep-pinned. |
| Is the release cycle manageable? | npm published six DSH release candidates between 2026-08-10 19:41 UTC and 2026-08-13 12:35 UTC. This repository pins 10 direct DSH packages and records a release gate in `docs/upstream-pin.md`. | Cadence is high, but it argues for exact pins and gated upgrades, not a permanently rebased fork. |
| Has the composed product actually run? | The contract suite exercises package/profile ownership, tools, approval, worktree isolation, migrations, and drift checks. Windows smoke runs booted headless and web profiles, exercised the approval UI, and ran an isolated child in its retained worktree. | The no-fork conclusion rests on executed composition, not architecture documents alone. |

The current limitations do not fire a fork trigger:

1. Windows ACL confinement restricts writes but not reads, network, or process
   visibility. Replacing that backend is a sandbox-plugin project; forking the
   agent loop would not solve it.
2. Browser network egress is not confined. Browser automation therefore stays
   opt-in and approval-gated instead of being enabled by default.
3. The API proxy explicitly has no protocol-version field because its client
   and host currently ship together. An independently released custom client
   must pin the matching DSH package family until upstream versions that wire
   contract.
4. Upstream is in developer preview and warns of compatibility-breaking
   changes. Every new pin must pass the upgrade gate; the existing pin remains
   supported when it does not.

## Consequences

### Positive

- The distribution stays small and auditable: a patch layer, one plugin, a CLI,
  and profile data.
- Upstream security fixes and Windows improvements are inherited by re-pinning.
- The no-fork posture is honest and easy to verify: no upstream source is
  present in the repository.

### Negative

- The distribution is coupled to upstream's release cadence and its
  compatibility-breaking changes.
- The patch layer targets upstream rows by stable id; if upstream renames or
  removes a row, the patch breaks and must be updated.
- The distribution cannot fix an upstream bug in place; it must wait for an
  upstream release or work around it in the patch layer.

## Reassessment triggers

This decision is reassessed when any of the following occur:

1. **Upstream stops publishing npm artifacts**, or the published artifact
   becomes unusable for composition.
2. **A required change cannot be expressed through the patch layer** and would
   require modifying upstream source.
3. **Upstream changes its license** or distribution terms in a way that
   prevents consumption as a dependency.
4. **The patch layer breaks repeatedly** against upstream releases, and the
   cost of maintaining the patch exceeds the cost of a fork.
5. **A security fix is blocked** because it requires an upstream change that is
   not being released in a timely manner.
6. **The npm/source version skew** grows beyond a cosmetic lag and starts
   causing real divergence between what is documented and what is consumed.

Each trigger is evaluated on its own evidence. A trigger firing does not
automatically mean forking; it means re-opening the decision with fresh
evidence.

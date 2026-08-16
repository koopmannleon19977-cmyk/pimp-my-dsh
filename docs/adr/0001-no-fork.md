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

**Consume upstream as an exact npm dependency. Never fork it.**

`pimp-my-dsh` depends on `@deepseek-ai/dsh@0.1.0-rc.6` and every direct
`@deepseek-ai/dsh-*` package at the exact version `0.1.0-rc.6`. The
distribution composes upstream bundles through `cordis.patch.yml` (overriding
base rows by stable id and inserting the distribution-owned plugin) and adds a
small CLI. It contains no copied upstream source.

## Evidence

The evidence gathered to support this decision:

1. **Upstream is designed for composition, not forking.** The upstream
   architecture document states that "there is no privileged core to patch: you
   extend dsh by mounting a plugin beside the others." A profile is a named
   composition of ordered bundle layers, and a patch targets a row by id and
   replaces its whole config. This is the documented extension mechanism.

2. **The distribution's needs are compositional.** The required changes —
   disabling telemetry, disabling web tools, keeping LSP opt-in, and adding
   distribution-owned prompt/context guidance — are all expressible as
   patch-layer overrides and a single plugin. None require modifying upstream
   source.

3. **Upstream publishes a consumable npm artifact.** `@deepseek-ai/dsh` is
   published to the npm registry with a `bin` entry and a full dependency
   closure. The published artifact is MIT-licensed.

4. **A fork would create an unbounded maintenance burden.** Upstream iterates
   rapidly (multiple release candidates in a single week). A fork would require
   continuously rebasing against upstream, and would risk diverging from the
   security fixes and Windows sandbox improvements that upstream ships.

5. **A fork would weaken the security story.** The upstream Windows sandbox
   (`@deepseek-ai/dsh-sandbox-windows-acl`) is under active development. A fork
   would either lag behind those fixes or duplicate them.

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

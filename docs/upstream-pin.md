# Upstream version pin

`pimp-my-dsh` pins `@deepseek-ai/dsh` and every direct `@deepseek-ai/dsh-*`
package to the exact version `0.1.0-rc.6`. Dist-tags and carets are never used
for these packages.

## The pin

| Package | Pinned version |
| --- | --- |
| `@deepseek-ai/dsh` | `0.1.0-rc.6` |
| every direct `@deepseek-ai/dsh-*` | `0.1.0-rc.6` |

The pin is exact (`0.1.0-rc.6`, not `^0.1.0-rc.6` and not `latest`). This is
deliberate: upstream is in developer preview and iterates rapidly with
compatibility-breaking changes. An exact pin makes the composed tree
reproducible and makes the distribution's patch layer (which targets upstream
rows by stable id) auditable against a known upstream artifact.

## The skew: npm rc.6 vs source-master rc.5

There is a known version skew between the published npm artifact and the
upstream source tree:

- The npm registry publishes `@deepseek-ai/dsh@0.1.0-rc.6` (published
  2026-08-13).
- The upstream `master` branch's `apps/cli/package.json` still declares
  `"version": "0.1.0-rc.5"`.

The npm artifact is **authoritative** for this distribution. `pimp-my-dsh`
consumes the published package, not a source checkout, so the composed tree is
built from `0.1.0-rc.6` regardless of what the source tree's manifest says.

The skew exists because the upstream release pipeline publishes a built npm
tarball whose version is bumped at release time, while the source manifest is
bumped on a different cadence. It is a cosmetic source-tree lag, not a
functional difference in the consumed artifact.

## Why not fork

The decision to consume upstream as an exact npm dependency rather than fork it
is recorded in [ADR-0001](adr/0001-no-fork.md).

## Reassessment triggers

The pin is reassessed when:

- A new upstream release is published.
- A security fix lands in upstream.
- The distribution's patch layer breaks against a new upstream version.
- Upstream changes its license or distribution terms.
- The skew between npm and source-master grows beyond a cosmetic lag.

## Upgrade gate

A new DSH release is never adopted by changing the dist-tag alone:

1. Change all direct `@deepseek-ai/dsh*` dependencies to the same exact
   version and regenerate `pnpm-lock.yaml`.
2. Run `tests/patch-contract.test.ts`; it resolves the pinned
   `@deepseek-ai/dsh-base` artifact and fails if any distribution override
   target was removed or renamed.
3. Run the full contract suite, then boot both a headless profile and the web
   profile on Windows.
4. Repeat the approval, Windows sandbox, browser, and isolated-worktree smoke
   scenarios.
5. Update this document and the ADR evidence before releasing the new pin.

Any missing row or public API blocks the release. The old exact pin remains
supported until the replacement passes the entire gate.

See [ADR-0001](adr/0001-no-fork.md#reassessment-triggers) for the full list.

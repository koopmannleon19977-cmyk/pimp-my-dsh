# Roadmap

Phase-gated. No dates are promised. Each phase is gated on the completion of the
previous phase and on upstream releases.

## Phase 0 — Feasibility prototype (current)

**Goal:** prove that a Windows-first distribution can compose and harden
upstream without forking it.

- [x] Consume `@deepseek-ai/dsh@0.1.0-rc.6` as an exact npm dependency.
- [x] Compose upstream bundles through `cordis.patch.yml` by stable id.
- [x] Ship a distribution-owned plugin with stable prompt/context guidance.
- [x] Add fixed-operation, read-only Git inspection.
- [x] Add append-only durable memory under the harness home.
- [x] Disable telemetry unconditionally.
- [x] Disable web fetch/search and browser automation.
- [x] Keep LSP opt-in and disclose its unsandboxed risk.
- [x] Ship `setup`, `run`, `doctor`, `update-check`, `migrate` CLI commands.
- [x] Document the partial Windows sandbox honestly.
- [x] Record the no-fork ADR with reassessment triggers.
- [x] Minimal keyless CI on Windows and Ubuntu.

## Phase 1 — Hardening

**Goal:** close the largest gaps in the current posture without changing the
no-fork decision.

- [ ] Add a safe public-network web provider (if one exists upstream) and gate
      it behind explicit opt-in, keeping the SSRF primitive disabled.
- [ ] Add read-side confinement on Windows (pair the ACL write boundary with a
      read-side policy or AppContainer capability token).
- [ ] Add a community-plugin review checklist artifact that the reviewed
      allowlist gate consumes.
- [ ] Add `doctor` checks for the Windows sandbox boundaries (FAT volumes,
      hard-link aliases, `Everyone` grants).
- [ ] Add a `--json` schema version field to structured CLI results.

## Phase 2 — Distribution maturity

**Goal:** make the distribution safe for broader use.

- [ ] Track upstream releases and re-pin on a documented cadence.
- [ ] Add a signed release artifact and provenance attestation.
- [ ] Add a migration path for profile patch data across distribution versions.
- [ ] Add Windows-specific integration tests for the sandbox boundaries.
- [ ] Publish a plugin-authoring guide for the reviewed allowlist gate.

## Phase 3 — Ecosystem

**Goal:** grow a reviewed, safe plugin ecosystem.

- [ ] Maintain a reviewed allowlist of community plugins with exact-version
      pins.
- [ ] Add tooling to verify allowlist pins against published artifacts.
- [ ] Document the review process publicly.

## Explicitly out of scope

- Forking DeepSeek Harness.
- Reimplementing upstream native tools.
- Browser automation.
- GitHub write automation.
- A community plugin registry or catalog with automatic discovery.
- Claiming full sandbox isolation on Windows.

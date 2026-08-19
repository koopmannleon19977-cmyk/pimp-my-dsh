# ADR-0004: Keep Windows read-side confinement as a native follow-up

- **Status:** Prototype complete — production integration deferred
- **Date:** 2026-08-19
- **Deciders:** `pimp-my-dsh` maintainers

## Context

The pinned upstream Windows backend (`@deepseek-ai/dsh-sandbox-windows-acl`)
creates a `WRITE_RESTRICTED` token. Its restricting SIDs participate in write
access checks only. A confined child can still read every file available to the
caller and can open sockets.

The existing desktop launcher already owns the correct process boundary:
`CreateProcessW` with `CREATE_SUSPENDED`, an explicit handle-list attribute, an
unnamed kill-on-close Job Object, assignment before resume, and a fixed
absolute Node executable. The CLI distribution does not share this native
launcher; it runs through the upstream Node sandbox seam.

A real read boundary therefore needs more than a tool or plugin policy. The
candidate Windows mechanism is an AppContainer or restricted token attached to
the child through `PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES`. The child
would also need explicit read grants for every legitimate root: the runtime,
managed profile, harness home, workspace, and private temporary directory.
Those grants must be safe for arbitrary user paths, reversible, race-resistant,
and compatible with pnpm hard links. AppContainer access to arbitrary paths is
not implicit, and broad host DACL rewrites would be a worse security boundary
than the current partial sandbox.

## Decision

Do not ship a fake read-only mode or silently mutate broad host ACLs. Keep the
current write-only enforcement and make the missing capability explicit:

- `doctor` continues to report `read-side-confinement` as `unavailable`.
- The production roadmap item remains open until the native path covers the
  real Node payload, managed profile, harness home, workspace, and private
  temporary directory.
- Tool approval hooks and path checks may remain defense-in-depth, but they are
  not counted as OS read confinement.
- A native child launch must fail closed if its AppContainer identity cannot be
  created, configured, or attached before the child resumes.

## Completed native prototype

`platform::confinement::Confinement` now creates a unique, unprivileged
AppContainer profile per run. The profile-owned directory is the only staging
root for the fixture executable and readable payload; the implementation never
rewrites a caller profile, workspace, `%TEMP%`, or volume-root DACL.

`Job::create_suspended_with` adds
`PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES` beside the existing explicit
stdio handle list before `CreateProcessW`. It retains the suspended
create → Job assignment → `ResumeThread` ordering. The existing
`create_suspended` remains the production default, so the prototype is opt-in.

`tests/confinement_contract_test.rs` executes the prototype on Windows:

1. a unique AppContainer profile is created without elevation;
2. the staged child reads a fixture file in its private profile root;
3. that same child is denied a caller-readable file outside the root;
4. normal cleanup removes the private root and deletes the profile;
5. a missing staged child fails before resume and its profile is removed.

This satisfies the prototype boundary without claiming production confinement.
It does not yet stage and grant the real Node payload, DSH home, workspace,
temp directory, hard links, junctions, crashes, or descendant processes.

## Consequences

The current product remains honestly partial and usable. The cost is that a
malicious model or plugin can still read caller-readable files on Windows. The
native work is isolated from the verified CLI hardening commit, so it can be
reviewed and tested without weakening the shipped default.

Revisit when a native helper can satisfy the prototype matrix on Windows 10/11
without broad persistent ACL changes, or when upstream ships a supported
read-capable sandbox seam.

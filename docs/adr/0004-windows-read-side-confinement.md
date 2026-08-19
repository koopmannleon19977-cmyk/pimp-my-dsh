# ADR-0004: Keep Windows read-side confinement as a native follow-up

- **Status:** Deferred — native implementation required
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

- `doctor` reports `read-side-confinement` as `unavailable`.
- The roadmap item remains open until a native child-launch path exists.
- Tool approval hooks and path checks may remain defense-in-depth, but they are
  not counted as OS read confinement.
- The desktop and CLI paths must fail closed if a future native token cannot be
  created, configured, or attached before the child resumes.

## Smallest acceptable native prototype

A future prototype is complete only when it demonstrates all of these on a
supported Windows target:

1. Creates a per-run AppContainer/restricted token without elevation.
2. Attaches `SECURITY_CAPABILITIES` before `ResumeThread`.
3. Grants read access only to an explicit temporary fixture root and the
   minimum runtime payload needed by the child.
4. Proves that a caller-readable file outside that root cannot be opened.
5. Preserves the existing Job Object and inherited-handle invariants.
6. Removes temporary ACL grants on normal and failed startup paths, and reports
   cleanup failure instead of falling back to an unrestricted child.
7. Runs as an opt-in prototype before it becomes the default launcher path.

The prototype must not use the user's whole profile or drive as its read root,
and it must not claim production confinement until the same matrix covers the
real Node payload, DSH home, workspace, temp directory, hard links, junctions,
crashes, and descendant processes.

## Consequences

The current product remains honestly partial and usable. The cost is that a
malicious model or plugin can still read caller-readable files on Windows. The
native work is isolated from the verified CLI hardening commit, so it can be
reviewed and tested without weakening the shipped default.

Revisit when a native helper can satisfy the prototype matrix on Windows 10/11
without broad persistent ACL changes, or when upstream ships a supported
read-capable sandbox seam.

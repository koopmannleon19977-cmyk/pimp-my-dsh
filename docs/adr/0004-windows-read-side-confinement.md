# ADR-0004: Keep Windows read-side confinement as a native follow-up

- **Status:** Production no-go — zero-capability loopback transport blocked
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
- No desktop opt-in setting is shipped: the authenticated private run reaches
  `ready`, but its loopback endpoint is unreachable from the supervisor.
- Tool approval hooks and path checks remain defense-in-depth; they are not
  counted as OS read confinement.
- A native child launch must fail closed if its AppContainer identity, private
  staging, authenticated pipe, or endpoint cannot be established.

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

The Windows contract matrix now covers:

1. a unique AppContainer profile without elevation or capability SIDs;
2. allowed private-root reads and denial of caller-profile reads;
3. hard-link and junction aliases into external roots;
4. assign-before-resume, root crash, descendant Job containment, and teardown;
5. authenticated source/destination runtime manifests and physicalized pnpm
   hard links;
6. real Node 24.19.0 / DSH 0.1.0-rc.7 `help` and
   `doctor --json --runtime-only`;
7. a physicalized rc.7 managed `web` profile with private DSH home, workspace,
   application data, and temporary paths;
8. normal, failed-startup, crash, read-only-file, and large-profile cleanup.

## Production decision gate

`tests/full_run_confinement_contract_test.rs` executes the complete private
web run. On Windows 11 it proves:

- the per-run AppContainer SID is admitted to the otherwise user+SYSTEM named
  pipe;
- the child connects and sends authenticated protocol-v1 `hello` and `ready`;
- `ready` advertises a valid `127.0.0.1` dynamic endpoint;
- the external supervisor cannot connect to that endpoint
  (`WSAECONNREFUSED` / OS error 10061);
- the Job is reaped and the profile root is removed within the bounded cleanup.

Adding Internet/private-network capability SIDs or a machine-wide loopback
exemption would violate the zero-network boundary. Therefore the prototype is
not wired into `Supervisor::run_lifecycle`, no renderer toggle is exposed, and
the ordinary launcher remains the default.

## Consequences

The current product remains honestly partial and usable. The cost is that a
malicious model or plugin can still read caller-readable files on Windows. The
native work is isolated from the verified CLI hardening commit, so it can be
reviewed and tested without weakening the shipped default.

Revisit only when Windows offers a narrowly scoped, per-run loopback transport
that does not grant general network access, when the product replaces HTTP
loopback with an authenticated non-network transport, or when upstream ships a
supported read-capable sandbox seam.

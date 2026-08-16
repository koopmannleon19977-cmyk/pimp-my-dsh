# Windows support

`pimp-my-dsh` is Windows-first. This page records the exact support surface and
its limits.

## Supported Node.js versions

| Node.js | Status |
| --- | --- |
| 22.19.0 | CI-verified |
| 24 | Supported (primary) |
| 26 | CI-verified |

The package manifest declares `^22.19.0 || >=24.0.0`.

## Automated setup

Install the Windows baseline with:

```powershell
pimp-dsh setup --profile windows
```

The command uses the bundled, exact-pinned pnpm runtime with lifecycle scripts
and pnpm hooks disabled, stages the complete profile, and atomically moves it
under the canonical `DSH_HOME`. The `windows` overlay is intentionally empty:
the upstream base rows plus the distribution patch already select PowerShell,
disable Bash, and mount the Windows ACL sandbox by `process.platform`.

## Shell backend

| Backend | Windows status |
| --- | --- |
| PowerShell (`pwsh`) | **Active.** The model-facing `pwsh` tool and the `pwsh-sandbox` executor are mounted. |
| Bash | **Disabled.** The upstream `bash-sandbox` and `tool-bash` rows carry `disabled: !!js process.platform === 'win32'`; bash has no Windows runner. |

The upstream base bundle gates both shell stacks by platform on its own rows:
`bash-sandbox`/`tool-bash` are disabled on win32, and their twins
`pwsh-sandbox`/`tool-pwsh` mount on win32 only. Exactly one shell stack is
active per host.

## Sandbox

On Windows, the sandbox seam resolves to the ACL restricted-token runner chain
(`dsh-sandbox-local` → `@deepseek-ai/dsh-sandbox-windows-acl`).

| Property | Value |
| --- | --- |
| Mechanism | `WRITE_RESTRICTED` token with workspace + private-temp SIDs |
| Enforcement | `partial` |
| Default mode | `workspace-write` |
| Write boundary | Workspace + private per-session temp subdirectory |
| Escalation | `danger-full-access` via approval prompt |
| Reads | **Not restricted** |
| Network | **Not restricted** |
| Process visibility | **Not restricted** |

The partial-enforcement boundaries are documented in
[docs/security-model.md](security-model.md#windows-sandbox-partial-write-confinement).

## Process cleanup

Background processes are terminated with `taskkill /T`, which kills the process
tree. This is the Windows equivalent of POSIX process-group termination.

## LSP

Language-server navigation is **opt-in only** (`PIMP_DSH_ENABLE_LSP`).
Configured language servers run **unsandboxed**. See
[docs/security-model.md](security-model.md#lsp-explicit-opt-in-unsandboxed).

## Persistent bash PTY

Persistent bash PTY sessions are **not supported on Windows**. The upstream
terminal backend that provides persistent bash PTYs has no Windows runner. The
PowerShell executor provides foreground and background execution, but not a
persistent interactive PTY.

## Known Windows limitations

- **WMI/CIM cmdlets fail under confinement.** `Authenticated Users` is absent
  from the restricting list, so the WMI namespace security check fails
  (`0x80041003`). CIM cmdlets and `Get-ComputerInfo` are unavailable in every
  confined mode.
- **PowerShell language mode differs by confined mode.** Under `read-only`,
  PowerShell starts in ConstrainedLanguage (`Add-Type`, non-core .NET static
  calls, COM, and reflection fail). Under the shipped `workspace-write` path,
  the private-temp capability lets the AppLocker probe complete, so pwsh stays
  in FullLanguage unless host-wide policy says otherwise.
- **First confined write on a large workspace is slow.** The workspace ACE is
  materialized with eager full-tree propagation, paid once per workspace per
  machine.
- **`whoami` and token-inspection cmdlets fail under the restricted token.**
  This is diagnostic noise of the restriction scheme, not an operational
  failure.
- **FAT-class volumes are writable** under confined modes (no ACL support).

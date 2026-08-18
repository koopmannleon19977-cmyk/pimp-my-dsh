# Security Policy

## Reporting a vulnerability

**Do not open a public issue.** Report security vulnerabilities privately.

Send a report to the maintainers through a private channel (for example, the
repository's private security advisory mechanism if enabled, or direct email to
a maintainer). Include:

- A description of the vulnerability and its impact.
- Steps to reproduce, including the exact `pimp-my-dsh` version, operating
  system, and Node.js version.
- Any relevant logs or error output, with secrets redacted.

The maintainers will acknowledge receipt and coordinate a fix and disclosure.
Please do not disclose the issue publicly until a fix is available.

## Supported versions

| Version | Status |
| --- | --- |
| `0.1.0` | Feasibility prototype. Security fixes are applied on a best-effort basis. |

`pimp-my-dsh` is a **feasibility prototype**, not a production-hardened product.
It is not recommended for untrusted workloads or for hosts that can reach
sensitive internal networks.

## Security model

`pimp-my-dsh` is a thin distribution over DeepSeek Harness. It composes the
upstream plugin bundles through a patch layer and a single distribution-owned
plugin. It does not add native execution primitives of its own; the security
posture is inherited from upstream and narrowed by this distribution's
configuration.

The full model is documented in [docs/security-model.md](docs/security-model.md).
The key facts:

### Telemetry is disabled unconditionally

This distribution disables all outbound telemetry. The upstream
`DSH_TELEMETRY_MODE` environment variable is **intentionally ignored**: setting
it to `FULL` or `FEEDBACK_ONLY` has no effect, because the distribution's patch
layer removes the telemetry backend from the composed tree. No session content,
tool data, prompts, or workspace paths are transmitted by this distribution.

### Windows sandbox enforcement is partial

On Windows, command execution is confined by the upstream ACL restricted-token
runner (`@deepseek-ai/dsh-sandbox-windows-acl`). This mechanism restricts
**writes only** and reports `enforcement: 'partial'`:

- Reads, network access, and process visibility are **not** restricted.
- Objects whose DACL grants `Everyone` write access remain writable.
- NTFS hard links are file-object aliases, so a workspace grant can be reached
  through an external alias.
- FAT-class volumes have no ACLs and are writable under confined modes.

This is a write-boundary sandbox, not a full isolation boundary. Do not treat a
confined session as a security boundary against a malicious model or plugin.

### Web fetch stays disabled; browser automation is isolated opt-in

The upstream HTTP fetch provider is an **SSRF primitive**: it does not block
private, loopback, link-local, or multicast destinations. This distribution
does not enable `web_fetch` or `web_search`.

`PIMP_DSH_ENABLE_BROWSER=1` separately enables the pinned Microsoft Playwright
MCP server. It uses an in-memory headless Chrome profile, receives the MCP
client's credential-scrubbed environment and blocks service workers. Only a
small allowlist of bounded inspection tools runs directly. Navigation,
interactions, storage access, and unknown future browser tools require approval
and fail closed without an answerer. Arbitrary code execution in the
unsandboxed browser server is denied. Browser network egress is **not confined**.
Do not enable it where the browser could reach sensitive internal services.

### LSP is explicit opt-in and unsandboxed

Language-server navigation is disabled by default. It can be enabled only by an
explicit opt-in (`PIMP_DSH_ENABLE_LSP`). Configured language servers run
**unsandboxed** — they execute with the full authority of the harness process.
Only enable LSP with language servers you trust.

### Community plugins are gated by review, not auto-installed

This distribution does not auto-install, auto-activate, or catalog community
plugins. A community plugin enters the distribution only through the
**reviewed allowlist gate**: a human reviews its source, license, exact
version, permission surface, and Windows behavior, then pins it to an exact
version in the allowlist. There is no implemented plugin registry and no
automatic plugin discovery.

### GitHub integration is read-only

`pimp_github_read` resolves `gh` from an absolute executable path outside the
workspace and exposes only fixed GET operations for repositories, issues, pull
requests, UTF-8 files, and bounded searches. It uses the GitHub CLI's normal
host credential store without returning tokens to the model. GitHub writes,
arbitrary API paths, alternate hosts, extensions, and caller-supplied CLI
arguments are outside the tool schema.

### Delegated agents cannot prompt for more authority

Ordinary parallel subagents use DSH's native in-process `spawn` provider, with
independent sessions and the parent's workspace. DSH snapshots the parent's
sandbox override at delegation and pins each child's approval policy to `never`;
an unattended child therefore cannot widen authority through an approval
prompt. The distribution caps each rolling parallel-safe tool pool at four
calls and delegation depth at three.

`subagent_worktree` is a separate one-shot provider. Creating it requires
approval because `git worktree add` mutates repository metadata. It creates a
unique branch under `pimp-agent/`, initializes the child index from `HEAD`, and
copies only currently tracked workspace paths. Repository hooks and checkout
filters are suppressed. Untracked files are excluded; sparse/skip-worktree
indexes, non-UTF-8 index paths, submodules, linked directory ancestors, and
tracked links that are dangling or escape the repository fail closed. Nothing
is merged or deleted automatically. The retained path and branch are returned
for explicit review and cleanup, including on child infrastructure failure.

## Secrets handling

The CLI never logs secret values. Environment variables that carry credentials
(`PIMP_DSH_API_KEY`) are read but never echoed, printed, or included in
structured CLI output. Structured CLI results (`--json`) contain only
non-secret status and diagnostic fields.

## Release signing & updater key custody

Desktop releases use two independent signatures:

1. **Tauri update signature** — an ed25519 keypair signs every updater
   artifact. The public key is committed (`tauri.conf.json →
   plugins.updater.pubkey`); the private key and its password live in
   `keys/` (gitignored) and are the crown jewels of the update channel.
2. **Authenticode certificate** — signs the Windows binaries/installer. The
   release workflow consumes it as GitHub secrets (`CERT_PFX_BASE64`,
   `CERT_PFX_PASSWORD`); no certificate is committed or shipped.

**Custody rules (updater key):**

- Move the private key and password offline (password manager + encrypted
  backup) before the first public release; `scripts/release-setup.sh` stages
  this move and the CI secret upload.
- CI secrets (`TAURI_SIGNING_PRIVATE_KEY`,
  `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`) are a working copy, not the backup.
- **Losing the private key or its password bricks updates** for every
  installed user: new versions can no longer be signed. There is no recovery.
- **Rotation plan:** before a key must rotate (leak, machine loss), document
  the old supported version range whose updates keep the old key; ship one
  final old-key-signed release that carries a new-key build, then re-point
  the endpoint and retire the old key. Rotation is a breaking event for
  in-place updates and must be a planned, announced transition.

**Release blocker:** `.github/workflows/release.yml` hard-fails when any of
the four signing secrets is missing — unsigned artifacts never leave
development. Free signing paths for open-source projects (SignPath
Foundation; Microsoft Azure Trusted Signing free tier) are noted in
`scripts/release-setup.sh` for when a certificate is chosen.

## Secret scanning

This repository relies on GitHub's native secret scanning and push protection,
which are enabled at the repository level (not in the CI workflow). These
features detect committed secrets and block pushes that introduce them. The CI
workflow itself contains no secrets and uses only pinned major action versions.

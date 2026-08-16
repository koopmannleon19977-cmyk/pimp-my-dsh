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

### Web fetch and browser automation are disabled

The upstream HTTP fetch provider is an **SSRF primitive**: it does not block
private, loopback, link-local, or multicast destinations. This distribution
does not enable `web_fetch`, `web_search`, or any browser automation. There is
no safe public-network provider configured, so these capabilities stay off.

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

## Secrets handling

The CLI never logs secret values. Environment variables that carry credentials
(`PIMP_DSH_API_KEY`) are read but never echoed, printed, or included in
structured CLI output. Structured CLI results (`--json`) contain only
non-secret status and diagnostic fields.

## Secret scanning

This repository relies on GitHub's native secret scanning and push protection,
which are enabled at the repository level (not in the CI workflow). These
features detect committed secrets and block pushes that introduce them. The CI
workflow itself contains no secrets and uses only pinned major action versions.

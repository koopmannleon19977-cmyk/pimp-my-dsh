# Security model

`pimp-my-dsh` is a thin distribution over DeepSeek Harness. It composes the
upstream plugin bundles through a patch layer and one distribution-owned plugin.
That plugin adds fixed-argument Git and GitHub reads, append-only durable
memory, approval policy, and the isolated-worktree subagent provider. All other
execution authority is inherited from upstream and narrowed by this
distribution's configuration.

This document is the authoritative statement of that posture. It is honest about
what is and is not a boundary.

## Threat model

The primary threats this distribution is designed to reduce:

1. **Outbound data exfiltration via telemetry** — a fresh upstream install can
   report session content, tool data, prompts, and workspace paths. This
   distribution eliminates that path.
2. **Server-side request forgery (SSRF)** — the upstream HTTP fetch provider
   does not block private or loopback destinations. This distribution disables
   it.
3. **Unintended filesystem writes by a model or plugin** — the upstream Windows
   sandbox restricts writes. This distribution keeps that boundary and
   discloses its limits.
4. **Supply-chain risk from community plugins** — this distribution admits
   community plugins only through a human review gate.

This distribution does **not** attempt to defend against a malicious model or a
malicious plugin that has already been granted execution authority. The Windows
sandbox is a write boundary, not an isolation boundary.

## Distribution-owned Git and memory tools

`pimp_git_read` resolves `git` from absolute `PATH` entries before entering the
repository, then invokes only `status`, `diff`, or `log` with fixed arguments.
The child receives an allowlisted, credential-free environment. System/global
Git config, pagers, hooks, fsmonitor, signature programs, credential helpers,
external diff/text conversion, and lazy object fetching are disabled.
Repository-declared clean/process filters are enumerated and neutralized before
`status` or `diff`; unsupported filter names fail closed. Success and error
output are capped at 16,000 characters. The calling agent's canonical workspace
must be the repository root; worktree children therefore inspect their own
branch rather than the harness process directory. Arbitrary arguments and
mutating operations are outside the schema.

`pimp_memory` writes newline-delimited JSON only to the canonical private
directory `DSH_HOME/pimp-my-dsh/memory.jsonl`. The directory and log must be
non-linked; multiply-linked files are rejected. Notes and queries are capped at
4,096 characters before normalization. Recall reads at most the newest 1 MiB and
returns at most ten records. The log is shared by every session and workspace
using that harness home. Any of those sessions can recall its records, so notes
must not contain credentials or sensitive values. This is a trusted
distribution-plugin write outside the workspace sandbox, not model-selected
filesystem access.

## Isolated worktree delegation

`subagent_worktree` requires one-call approval before it mutates repository
metadata. It creates a random `pimp-agent/` branch under the canonical
`DSH_HOME/pimp-my-dsh/worktrees` root, initializes the child index from `HEAD`,
and copies the current contents of index-tracked files. Every mutating Git
command runs with a private empty hooks directory; checkout and content filters
never run. Untracked files are not copied. Sparse/skip-worktree indexes,
non-UTF-8 index paths, submodules, linked directory ancestors, and links that
are dangling or escape the repository fail closed.

The child receives the worktree as its session workspace, inherits the parent's
sandbox mode, and has approval pinned to `never`. The provider never merges or
deletes a successful child. It returns the retained path and branch on normal,
child-level, and infrastructure failure so a human can review and remove them.

## Telemetry: disabled unconditionally

Upstream telemetry honors a hard process-level kill switch. The wrapper forces
`DSH_TELEMETRY_DISABLED=1`, removes mode and endpoint overrides, and the bundle
also disables the telemetry backend row. Therefore:

- No session content, tool data, prompts, or workspace paths are transmitted.
- Setting `DSH_TELEMETRY_MODE=FULL` or `DSH_TELEMETRY_MODE=FEEDBACK_ONLY` has
  **no effect**.
- Later user/profile patch layers cannot bypass the process-level kill switch.
- The `update-check` CLI command performs a version lookup only and sends no
  data about the local machine.

## Windows sandbox: partial write confinement

On Windows, command execution is confined by the upstream ACL restricted-token
runner (`@deepseek-ai/dsh-sandbox-windows-acl`). The mechanism duplicates the
caller's token into a `WRITE_RESTRICTED` token whose restricting SIDs carry
workspace and private-temp write capabilities.

The runner reports `enforcement: 'partial'`. This is deliberate and honest:

- **Writes are restricted; reads, network, and process visibility are not.**
  `WRITE_RESTRICTED` intersects write accesses only. A confined child can read
  any caller-readable file and open sockets.
- **`Everyone` grants remain ambient write authority.** `Everyone` must stay in
  the restricting list for early DLL initialization and CNG to work. An
  external NTFS object whose DACL grants `Everyone` a requested write right
  clears both access checks and stays writable.
- **Hard links are file-object aliases.** An inheritable workspace ACE
  propagated onto an existing hard link changes the one underlying file
  security descriptor, so the same object is writable through an external
  alias. Ordinary pnpm installations use hard links into their
  content-addressable store, so rejecting multiply-linked files is not viable.
- **FAT-class volumes have no ACLs** and are writable under confined modes.
- **Console isolation is unavailable.** Confined children share the host
  console; stdio redirection is pipe-based.

The shipped default mode is `workspace-write`: the workspace and a private
per-session temp subdirectory carry write grants, and other ACL-addressable
writes are denied except for the documented boundaries above. Escalation to
`danger-full-access` requires an approval prompt.

**Do not treat a confined session as a security boundary against a malicious
model or plugin.** The sandbox reduces accidental or careless writes; it does
not contain a determined adversary.

## Web fetch disabled; browser automation isolated and opt-in

The upstream HTTP fetch provider (`@deepseek-ai/dsh-web-fetch-http`) is an
**SSRF primitive**: it does not block private, loopback, link-local, multicast,
or otherwise non-public destinations, and it does not perform
DNS-resolve-then-validate. It remains disabled together with `web_search`.

Browser automation is a separate, explicit capability. Setting
`PIMP_DSH_ENABLE_BROWSER=1` starts the exact-pinned Microsoft Playwright MCP
server through the first-party DSH MCP client with these controls:

- A fresh in-memory, headless Google Chrome profile; no existing login state.
- The MCP child's ambient environment is scrubbed of DSH and credential-shaped
  variables.
- Service workers are blocked, image responses are omitted, and browser output
  is bounded under `DSH_HOME`.
- Only bounded, non-credential inspection tools may run directly.
- Navigation, interactions, storage access, and every unknown future browser
  tool require one-call approval. Without an approval answerer they fail closed.
- Arbitrary JavaScript in the unsandboxed browser server is denied.

These controls do not confine browser network egress by themselves, and the
Playwright origin filters are not a security boundary across redirects. The
distribution ships optional **egress confinement** via
`scripts/confine-browser.ps1 -Apply` (elevated): Windows Firewall outbound
block rules pinned on the exact Playwright Chromium executable for loopback,
RFC1918 private, link-local, and multicast destinations — public internet
stays reachable. Until that one-time per-machine step runs, do not enable
browser automation where Chrome can reach sensitive internal services. The
distribution does not support logged-in profile control or desktop computer use.

## Web search: fixed-host provider, opt-in

`pimp_web_search` (opt-in via `PIMP_DSH_ENABLE_WEB_SEARCH=1` plus
`PIMP_DSH_WEB_SEARCH_KEY`) queries the **Tavily API at one fixed HTTPS
endpoint** and returns its results as untrusted text. Why this is not the
SSRF primitive:

- The client can only ever reach `api.tavily.com` on port 443 — the provider's
  servers fetch pages, not the local process. Loopback/link-local/internal
  destinations are unreachable through this tool.
- Redirects are rejected (`redirect: "error"`): a 3xx cannot smuggle a
  follow-up request to an internal address.
- The API key travels in the POST body (never the URL) and is never included
  in tool output or error messages; the existing log redaction covers the
  `DSH_PIMP_*` name family.
- Query length (512 chars), result count (1-10), per-field text, and the
  streamed response body (1 MiB) are all bounded.

Result URLs are **data, not instructions**: the tool returns them as text and
never fetches them. The SSRF-capable upstream HTTP fetch provider remains
disabled regardless of this opt-in.
## GitHub writes: fixed argv, approval-gated, push deferred

`pimp_github_write` (PR / issue / comment) extends the read-only provider with
exactly three write operations. Every invocation enters the approval pipeline
— without an approval answerer the tool fails closed — and the human reviews
the repository, branch, and content before anything is sent.

- All operations run through the trusted `gh` executable with code-built
  argument vectors: no shell, no `--force`, `--web`, `--fill`, or agent-supplied
  flags beyond validated scalars (repository pattern, bounded title/body,
  integer numbers).
- The PR head is the current branch, read via the scrubbed Git environment;
  the branch must already exist on the remote.
- **`push` is deliberately not part of v1.** Git credential plumbing (credential
  helpers, SSH config `ProxyCommand`, repo-controlled remote URLs) is an
  authority surface of its own; pushing stays a human step until it has a
  credential-safe design. A PR on an unpushed branch fails with a clean gh
  error and the agent tells the user to push.
- The provider errors are bounded and never echo credentials.

## Curated docs MCP: Context7, preconfigured but opt-in

The distribution ships one preconfigured docs MCP server (`mcp-context7`) so
enabling documentation search is a single environment variable
(`PIMP_DSH_ENABLE_CONTEXT7=1`) instead of config work. It is opt-in like every
other external surface: without the flag the row is dormant and no connection
is ever attempted.

- Transport is **streamable HTTP to the single fixed host**
  `https://mcp.context7.com/mcp` — no third-party code runs locally (no stdio
  child process, no package install), and the client can only reach that host
  through this row.
- No key is required; the optional `PIMP_DSH_CONTEXT7_KEY` only raises rate
  limits and is sent as a Bearer header, covered by the `DSH_PIMP_*` log
  redaction.
- `failOnStartupError: false`: a Context7 outage degrades the session (tools
  absent) but never blocks it.

## LSP: explicit opt-in, unsandboxed

Language-server navigation (go-to-definition, find-references,
go-to-implementation, hover) is disabled by default. It is enabled only by an
explicit opt-in (`PIMP_DSH_ENABLE_LSP`).

Configured language servers run **unsandboxed**. They execute with the full
authority of the harness process — the same authority as the model and the
shell tools. A language server is arbitrary code; enabling LSP means trusting
that code.

Only enable LSP with language servers you trust, and prefer servers that do not
need network access.

## Community plugins: reviewed allowlist gate

This distribution does **not** auto-install, auto-activate, or catalog
community plugins. There is no implemented plugin registry and no automatic
plugin discovery.

A community plugin enters the distribution only through the **reviewed
allowlist gate**, which is a policy, not code:

1. A human reviews the plugin's source, license, exact version, permission
   surface, and Windows behavior.
2. The plugin is pinned to an exact version in the allowlist.
3. The allowlist is the only path by which a community plugin is installed.

The gate is deliberately conservative. A plugin that requests broad filesystem
or network access, or that has not been reviewed for Windows behavior, is not
admitted.

## Setup and secrets handling

- `setup` invokes the distribution's exact `pnpm@11.7.0` JavaScript entry
  through `node` with `shell: false`; attacker-controlled checkout paths are
  never parsed by `cmd.exe`.
- Dependency installation passes `--ignore-scripts` and `--ignore-pnpmfile`,
  supplies a minimal allowlisted environment without `PIMP_DSH_*` secrets, and
  runs in a fresh staging directory.
- Profile names are allowlisted, writes are lexically contained under
  `DSH_HOME`, and existing symbolic-link/junction redirects are rejected by
  canonical-path checks.
- A completed staging profile atomically replaces only a profile carrying this
  distribution's valid ownership marker. Existing dependency/bundle entries
  are not preserved, so `--force` cannot admit unreviewed plugins.
- `run` revalidates the exact marker, manifest, shipped patch, and installed
  bundle link before invoking DSH. Missing or modified profiles are rejected;
  upstream's automatic `web`/`headless` initialization is never reachable.
- Forwarded app arguments follow an explicit upstream end-of-options boundary,
  so `--profile` and `--patch` tokens cannot become launcher authority.
- A global `$DSH_HOME/cordis.patch.yml` is rejected because upstream composes it
  above the managed profile and it could otherwise weaken these controls.
- The harness home and managed profile directory must remain outside the
  writable workspace. This keeps the agent from creating the watched global
  patch after preflight or mutating its own managed profile during a session.
- The CLI never logs secret values.
- Public `PIMP_DSH_*` values are promoted into protected `DSH_PIMP_*` child
  variables before upstream repository `.env` loading. The public names are
  removed from the child environment.
- `PIMP_DSH_API_KEY` is read but never echoed, printed, or included in
  structured CLI output.
- Structured CLI results (`--json`) contain only non-secret status and
  diagnostic fields.

## Desktop supervisor security model

### WebView2 rendering

Tauri on Windows renders the control surface using **Microsoft Edge WebView2**.
This is a Chromium-based web engine, not truly native rendering. The UI looks
and feels polished, but it does not produce pixels via the OS toolkit (WinUI,
WPF, Win32, or Impeller). WebView2 ships with Windows 10/11 as part of Edge
updates and is evergreen — Microsoft patches it automatically. This has two
consequences:

- **Air-gapped deployments** cannot rely on the default `downloadBootstrapper`
  mode. They must use `offlineInstaller` (~127 MB) or `fixedVersion`
  (~180 MB) to ship their own WebView2, accepting that browser patches become
  the project's responsibility rather than Microsoft's.
- **Accessibility** is Chromium's accessibility tree, inherited through
  WebView2. It is not UIA-native, but it is functional for screen readers.

### Strict frontend / native split

The Tauri controller enforces a hard boundary between the Rust supervisor and
the React view:

**Rust owns all authority.** The Rust backend owns:
- The closed 13-state lifecycle state machine
- The unnamed kill-on-close Job Object
- Live process and Job handles
- The authenticated named-pipe bridge
- The log pipeline
- The provider (packaged or debug) and compatibility manifest verification

**JavaScript has zero capabilities.** The React frontend (`apps/desktop/`) is
configured with **no** Tauri capabilities:
- No `shell` plugin — no spawning, no executing, no shell expansion
- No `opener` plugin — no URL launching from the renderer
- No `filesystem` plugin — no file read/write from the renderer
- No `updater` plugin — no update logic from the renderer

All authority flows from Rust to React through the snapshot channel and command
API. The renderer never supplies URLs, paths, executables, argv, environment,
PIDs, pipe names, run tokens, or lifecycle targets. Stale renderer data is
never accepted because mutations carry no revision or target authority.

The controller webview loads only bundled assets under strict CSP. The harness
web UI opens in the system browser or a separate zero-capability webview —
never inside the privileged controller webview.

### Bridge security

The child bridge (named pipe on Windows) enforces:

- **DACL:** current-user and SYSTEM only
- **Transport flag:** `PIPE_REJECT_REMOTE_CLIENTS` — no remote connections
- **First-instance creation:** rejects pre-created pipe squatting
- **Bounded frames:** 64 KiB maximum, UTF-8, length-prefixed JSON
- **Token:** random 64-char lowercase hex per run, memory-only, never persisted
- **Version check:** protocol version must match v1
- **Sequence:** strictly increases per authenticated connection

Forged, replayed, oversized, wrong-version, wrong-token, or wrong-host frames
close the channel and terminate the Job. No secret is persisted across runs.

### Job Object containment

The unnamed kill-on-close Job Object is the fail-safe containment mechanism:

- **Unnamed** — no global name to squat or collide with
- **KILL_ON_JOB_CLOSE** — closing the last handle terminates all associated processes
- **Assign before resume** — child starts `CREATE_SUSPENDED`, Job assigned, then resumed
- **Non-inheritable handles** — the Job handle is not passed to the child
- **Explicit allowlist** — `PROC_THREAD_ATTRIBUTE_HANDLE_LIST` limits inherited handles
  to stdio pipes and the bridge pipe only

Kill-on-close means a supervisor crash, exit, or update takes the whole harness
tree down with it. This is deliberate: the controller never leaves an
unsupervised child behind.

### Graceful / forced stop

The stop sequence is cooperative-first with a hard deadline:

1. Bridge sends `shutdown` to the child, which invokes upstream's whole-app
   disposal (`await app.current?.fiber.dispose()` with `PROCESS_SHUTDOWN_TIMEOUT_MS = 5e3`).
2. Controller waits beyond the 5-second upstream bound **and** for Job active-process
   zero (HTTP and upgraded WebSocket sockets close by WebServer disposal).
3. If active processes remain: `TerminateJobObject` is called. The controller waits
   until the Job reaches active-process zero.
4. **A forced stop is never reported as graceful.** The UI label is `stopped-forced`,
   not `stopped-graceful`.

### Secret handling

- `PIMP_DSH_*`/`DSH_PIMP_*` environment values flow to the child process only.
- State files and logs contain no secret values.
- Secret redaction covers both the bridge token and any `PIMP_*`/`DSH_*` env values
  before any sink (stdout, stderr, disk, display).
- The diagnostic export has a review/redaction gate.
- Corrupt or redirected state/log directories are ignored or quarantined.

### What the desktop does not do

- **No telemetry.** The controller sends no machine data to any endpoint.
- **No LAN access.** The controller has no network outbound beyond what the harness
  child legitimately opens.
- **No Windows service.** The controller runs as a per-user resident application.
  No elevation, no machine-wide registry, no service controls.
- **No logged-in browser.** The harness web UI opens in the system browser or a
  zero-capability webview. No existing login state, no profile control.
- **No desktop automation.** No keyboard/mouse injection, no screen capture,
  no OS-level interaction beyond tray presence and file explorer.

### Unsigned development artifacts

Locally built NSIS installers have no Authenticode signature. They are
**development only** and must not be distributed. Browser downloads can trigger
SmartScreen "not trusted" warnings. Production distributions require:

1. **Authenticode signing** — `signtool` with OV or EV certificate for the
   Windows executable (`*-setup.exe`).
2. **Tauri update signatures** — a separate private key for the Tauri updater
   plugin. This is mandatory and cannot be disabled.

Dual signing is a **Phase 2** goal. The private key for Tauri updates is an
operational responsibility: losing it bricks updates for installed users. Key
escrow, backup, and planned trust-root rotation for old supported versions are
part of Phase 2 planning.

### Unsupported claims

The desktop supervisor does **not** claim:

- An updater is shipped in Phase 0. Tauri's updater plugin is available but
  not configured. Enabling it requires a private signing key and HTTPS endpoint.
- Microsoft Store or MSIX distribution. The installer is NSIS per-user only.
- Full sandbox isolation. The harness child runs with the harness process's
  authority; the sandbox applies to child commands, not the harness itself.
- Cross-platform parity in Phase 0. The implementation targets Windows 10/11 x64
  only. macOS/Linux support uses the same Tauri core but different process
  adapters and transport (see [ADR-0002](adr/0002-tauri-desktop-supervisor.md)).

## What this distribution does not protect against

- A malicious model or plugin that has been granted execution authority.
- Reads of caller-readable files by a confined child.
- Network egress by a confined child.
- A malicious language server (LSP is unsandboxed).
- A malicious community plugin admitted through the review gate.
- Host compromise via a vulnerability in upstream or in a dependency.

## Reporting

Report vulnerabilities privately. See [SECURITY.md](../SECURITY.md).

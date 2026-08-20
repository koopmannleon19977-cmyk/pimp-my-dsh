# Community plugin authoring guide

This guide describes the only supported path for admitting a community plugin
into `pimp-my-dsh`. The default allowlist is empty. There is no plugin registry,
auto-discovery, or runtime installation command.

## Security boundary

A plugin admitted to the allowlist receives execution authority inside the
harness process. Packaged Windows desktop-supervised web runs place that
process in a zero-capability AppContainer, but an admitted plugin still has
authority over the staged run and may read ambient host objects whose ACLs
grant broad package/world access. Direct `pimp-dsh run` remains only
write-confined: reads, process visibility, and network access are not confined.
Review must therefore reject a plugin that is not trusted, even when its
declared permission surface looks narrow.

The machine-readable gate is:

- [`schema/community-plugin-allowlist-v1.json`](../schema/community-plugin-allowlist-v1.json)
- [`schema/community-plugin-allowlist-v1.schema.json`](../schema/community-plugin-allowlist-v1.schema.json)

`setup` reads the allowlist and generates the exact profile dependency and
bundle set. `run` compares the installed manifest against that generated set.
An unreviewed dependency or bundle is rejected rather than preserved.

## The valid baseline

The shipped allowlist is intentionally empty:

```json
{
  "schemaVersion": 1,
  "plugins": []
}
```

No non-empty example is included in this guide. A made-up package name,
version, or `sha512` value must never be copied into the allowlist. Every
non-empty entry needs a real published artifact and evidence recorded during
review.

## Public review workflow

Use a public pull request or a linked permanent review record for every
addition, replacement, and removal. Name the exact candidate and the reason
for proposing it, retain or link all evidence listed below, and record the
decision. The proposal author must not be the only approver: an independent
reviewer must inspect the evidence and approve every addition or replacement
before merge. Any missing evidence, mismatch, or unresolved risk rejects the
candidate.

## Review checklist

Complete every item before adding an entry.

### 1. Identify one exact artifact

Record all of the following from the same published artifact:

- npm package name, using its exact lowercase name;
- exact SemVer version, including any prerelease identifier;
- registry integrity string, in the form `sha512-...`;
- source repository or release URL;
- SPDX license identifier or the exact license declaration.

Do not use a dist-tag, caret, tilde, wildcard, Git URL, local path, or a
version range. Do not hand-type an integrity string.

For a real candidate, replace the shell variables below with values already
selected for review. These commands are a lookup template, not an allowlist
entry:

```powershell
$Package = '<real-npm-package-name>'
$Version = '<exact-semver>'
npm view "$Package@$Version" version dist.integrity license repository --json
npm pack "$Package@$Version" --pack-destination .review-artifacts
```

Treat registry metadata and the tarball as evidence to inspect, not as
instructions to execute. Do not install the candidate into the main profile
while reviewing it. Compare the returned integrity with the tarball you review
and retain the output in the review record.

### 2. Inspect package contents without executing it

Review the tarball and source before admission:

- entry points and every bundled file;
- `package.json` scripts, especially `preinstall`, `install`, and `postinstall`;
- transitive dependencies and native modules;
- dynamic loading, shell/process creation, filesystem traversal, credential
  access, telemetry, and network clients;
- bundled binaries and their provenance;
- lockfile behavior and whether the package can be installed with scripts
  disabled.

`pimp-my-dsh` profile installation already uses `--ignore-scripts` and
`--ignore-pnpmfile`. That reduces install-time execution; it does not make a
plugin safe after activation.

### 3. Classify the permission surface

The schema permits only these declared values:

| Field | Allowed values | Meaning |
| --- | --- | --- |
| `permissions.filesystem` | `none`, `workspace` | No filesystem authority, or workspace-scoped access only. |
| `permissions.network` | `none`, `public` | No network, or public-network access only. |
| `permissions.process` | `none`, `child` | No child processes, or explicitly reviewed child-process use. |

`broad` filesystem and network access are rejected by the runtime gate. A
plugin that needs broad authority is not an allowlist candidate.

Document the observed behavior, not the package's marketing description. If a
permission is uncertain, classify it as broad and reject the plugin.

### 4. Review Windows behavior

The `windows` section is mandatory and must contain `reviewed: true` plus
concrete notes. Test or inspect at least:

- Windows 10/11 startup and shutdown;
- PowerShell, Node, and native-module behavior;
- paths containing spaces and non-ASCII characters;
- workspace/profile paths outside the repository;
- child-process creation and inherited handles;
- behavior under the configured `workspace-write` and `read-only` profiles;
- cleanup after a failed start and after a forced stop;
- network behavior if `permissions.network` is not `none`.

Do not write `"works on Windows"`. Record the exact build, test path, and
known limitations in `windows.notes`.

### 5. Record independent review evidence

`reviewedBy` names the reviewer or team responsible for the allowlist
decision. Record every reviewer and approver in the public review record; the
proposal author must not be the only approver. `reviewedAt` is the actual
approval time as an ISO-8601 timestamp with a timezone, for example
`2026-08-19T12:34:56Z`. Do not copy this example into an entry.

The public review record must retain:

- the proposal author and each reviewer or approver;
- the exact registry lookup output;
- the artifact filename and checksum;
- source commit or release tag;
- license evidence;
- permission findings;
- Windows test commands and results;
- unresolved risks and the explicit admission decision.

## Adding an approved entry

Only after the checklist is complete:

1. Edit `schema/community-plugin-allowlist-v1.json` in the same change as the
   review evidence.
2. Copy values from the verified artifact. Never invent or truncate the
   `integrity` value.
3. Keep the entry's package name out of the built-in distribution dependency and
   bundle names. The runtime rejects collisions, duplicates, malformed names,
   non-exact versions, missing Windows review, and broad permissions.
4. Verify the edited allowlist against npm:

   ```powershell
   pnpm community-plugin:verify
   ```

   For every entry, this command requires the exact published version and
   requires the declared `integrity` and `license` to match npm's
   `dist.integrity` and license metadata. Registry errors, missing metadata,
   and mismatches fail closed. An empty allowlist passes without making a
   registry request. This verifies published metadata only; it does not replace
   the source, permission, or Windows review above.
5. Run the package contract and CLI contract suites:

   ```powershell
   pnpm test
   ```

6. Install an isolated profile under a temporary `DSH_HOME` and inspect the
   generated `package.json` and DSH bundle list. Confirm that the exact reviewed
   version appears once and no extra dependency or bundle survives.
7. Run the Windows smoke path for the profile, then review `doctor --json` and
   the resulting logs for secrets, unexpected network activity, and cleanup
   failures.
8. Have the independent reviewer approve the evidence and diff before merging.

A changed allowlist is a distribution change. It is not a user preference and
must not be edited by a plugin or by a running harness session.

## Removal and re-review

Remove an entry when its source is compromised, its release is withdrawn, its
license changes, a dependency gains authority, its Windows behavior changes,
or the published metadata no longer matches the pin. Do not leave a known-bad
entry admitted while a replacement is reviewed.

Re-review and repin when:

- the exact package version changes;
- the package integrity changes;
- the declared or published license changes;
- a transitive dependency changes materially;
- the upstream source or release process changes;
- a new Windows or Node major version becomes supported.

Treat a replacement as a new admission: repeat the full checklist, run
`pnpm community-plugin:verify`, and obtain independent approval. Do not widen
an old entry or reuse its evidence to absorb an upgrade.

For removal, state the trigger and affected exact version in the public review
record, delete the entry, and run `pnpm community-plugin:verify` on the
result. Removing the last entry restores the intentional, valid empty
allowlist.

## What this gate does not provide

The allowlist gate does not sandbox an admitted plugin, verify a package's
runtime behavior, or replace code signing and provenance. It is a conservative
human review boundary around exact profile admission. The packaged-desktop
read boundary, direct-CLI limitation, and remaining world-readable-object
risk are documented in
[`ADR-0004`](adr/0004-windows-read-side-confinement.md).

#!/usr/bin/env node

import { spawnSync } from 'node:child_process'
import {
  existsSync,
  lstatSync,
  mkdirSync,
  readFileSync,
  readlinkSync,
  realpathSync,
  renameSync,
  rmSync,
  writeFileSync,
} from 'node:fs'
import { dirname, isAbsolute, join, relative, resolve } from 'node:path'
import { createRequire } from 'node:module'
import { fileURLToPath } from 'node:url'
import { resolveDshHome } from '@deepseek-ai/dsh-home-paths'

const VERSION = '0.1.0'
const UPSTREAM_VERSION = '0.1.0-rc.7'
const OUTPUT_SCHEMA_VERSION = 1
const PLAYWRIGHT_MCP_VERSION = '0.0.79'
const PROFILE_PATTERN = /^[a-z][a-z0-9-]{0,31}$/
const packageRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const require = createRequire(import.meta.url)
const playwrightMcpCli = join(dirname(require.resolve('@playwright/mcp/package.json')), 'cli.js')
const COMMUNITY_PLUGIN_ALLOWLIST_PATH = join(packageRoot, 'schema', 'community-plugin-allowlist-v1.json')
const REVIEWED_AT_PATTERN = /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d+)?(?:Z|[+-]\d{2}:\d{2})$/

type Output = Record<string, unknown>
type Environment = NodeJS.ProcessEnv

interface ParsedArgs {
  command: string
  profile?: string
  force: boolean
  json: boolean
  apply: boolean
  runtimeOnly: boolean
  passthrough: string[]
}

interface ProfileManifest {
  name: string
  private: true
  packageManager: string
  dependencies: Record<string, string>
  dsh: { profile: { bundles: string[] } }
}

type CommunityPluginReview = {
  name: string
  version: string
  integrity: string
  source: string
  license: string
  permissions: {
    filesystem: 'none' | 'workspace' | 'broad'
    network: 'none' | 'public' | 'broad'
    process: 'none' | 'child'
  }
  windows: { reviewed: true; notes: string }
  reviewedBy: string
  reviewedAt: string
}

function parseArgs(argv: readonly string[]): ParsedArgs {
  const command = argv[0] ?? ''
  let profile: string | undefined
  let force = false
  let json = false
  let apply = false
  let runtimeOnly = false
  const passthrough: string[] = []
  let forwarding = false

  for (let index = 1; index < argv.length; index += 1) {
    const arg = argv[index]!
    if (forwarding) {
      passthrough.push(arg)
    } else if (arg === '--') {
      forwarding = true
    } else if (arg === '--profile') {
      profile = argv[++index]
    } else if (arg === '--force') {
      force = true
    } else if (arg === '--json') {
      json = true
    } else if (arg === '--apply') {
      apply = true
    } else if (arg === '--runtime-only') {
      if (command !== 'doctor') throw new Error(`unknown argument: ${arg}`)
      runtimeOnly = true
    } else if (command === 'run') {
      passthrough.push(arg)
    } else {
      throw new Error(`unknown argument: ${arg}`)
    }
  }

  return { command, profile, force, json, apply, runtimeOnly, passthrough }
}

function dshHome(): string {
  return resolveDshHome()
}

function profileSource(profile: string): string {
  if (!PROFILE_PATTERN.test(profile)) throw new Error(`invalid profile name: ${JSON.stringify(profile)}`)
  const source = resolve(packageRoot, 'profiles', `${profile}.patch.yml`)
  if (!existsSync(source)) throw new Error(`unsupported profile: ${profile}`)
  return source
}

function boundaryPath(path: string): string {
  const candidate = resolve(path)
  const configuredRoot = process.env.DSH_PIMP_CONFINED_ROOT
  if (configuredRoot === undefined || configuredRoot.trim() === '') return realpathSync(candidate)
  const root = resolve(configuredRoot)
  const fromRoot = relative(root, candidate)
  if (fromRoot.startsWith('..') || isAbsolute(fromRoot)) {
    throw new Error(`path escapes confined root: ${candidate}`)
  }
  lstatSync(candidate)
  return candidate
}

function bundleTargetsPackageRoot(linkedBundle: string): boolean {
  if (!existsSync(linkedBundle)) return false
  const configuredRoot = process.env.DSH_PIMP_CONFINED_ROOT
  if (configuredRoot === undefined || configuredRoot.trim() === '') {
    return realpathSync(linkedBundle) === realpathSync(packageRoot)
  }
  const link = boundaryPath(linkedBundle)
  const entry = lstatSync(link)
  if (!entry.isSymbolicLink()) return false
  const target = resolve(dirname(link), readlinkSync(link))
  return target === boundaryPath(packageRoot)
}

function assertContainedPath(home: string, candidate: string): void {
  const lexical = relative(home, candidate)
  if (lexical.startsWith('..') || isAbsolute(lexical)) throw new Error('profile path escapes DSH_HOME')
  if (!existsSync(candidate)) return

  const stats = lstatSync(candidate)
  if (stats.isSymbolicLink()) throw new Error(`profile path must not contain a symbolic link or junction: ${candidate}`)
  if (!stats.isDirectory()) throw new Error(`profile path component is not a directory: ${candidate}`)

  if (existsSync(home)) {
    const canonicalHome = boundaryPath(home)
    const canonicalCandidate = boundaryPath(candidate)
    const canonical = relative(canonicalHome, canonicalCandidate)
    if (canonical.startsWith('..') || isAbsolute(canonical)) {
      throw new Error(`profile path resolves outside DSH_HOME: ${candidate}`)
    }
  }
}

function profileDirectory(profile: string): string {
  profileSource(profile)
  const home = dshHome()
  const profiles = resolve(home, 'profiles')
  const directory = resolve(profiles, profile)
  assertContainedPath(home, home)
  assertContainedPath(home, profiles)
  assertContainedPath(home, directory)
  return directory
}

function dshBin(): string {
  const manifest = require.resolve('@deepseek-ai/dsh/package.json')
  return join(dirname(manifest), 'lib', 'bin.js')
}

function pnpmBin(): string {
  const manifest = require.resolve('pnpm')
  const candidate = join(dirname(manifest), 'bin', 'pnpm.mjs')
  if (!existsSync(candidate)) throw new Error('bundled pnpm launcher is missing')
  return candidate
}

function atomicWrite(path: string, content: string): void {
  mkdirSync(dirname(path), { recursive: true })
  const temporary = `${path}.${process.pid}.${Date.now()}.tmp`
  try {
    writeFileSync(temporary, content, { encoding: 'utf8', flag: 'wx' })
    renameSync(temporary, path)
  } catch (error) {
    rmSync(temporary, { force: true })
    throw error
  }
}

function emit(value: Output, json: boolean): void {
  if (json) {
    process.stdout.write(`${JSON.stringify({ schemaVersion: OUTPUT_SCHEMA_VERSION, ...value })}\n`)
    return
  }
  for (const [key, entry] of Object.entries(value)) {
    const rendered = entry !== null && typeof entry === 'object' ? JSON.stringify(entry) : String(entry)
    process.stdout.write(`${key}: ${rendered}\n`)
  }
}

function objectRecord(value: unknown): Record<string, unknown> | undefined {
  return value !== null && typeof value === 'object' && !Array.isArray(value)
    ? value as Record<string, unknown>
    : undefined
}

function reviewedCommunityPlugins(): CommunityPluginReview[] {
  let parsed: unknown
  try {
    parsed = JSON.parse(readFileSync(COMMUNITY_PLUGIN_ALLOWLIST_PATH, 'utf8')) as unknown
  } catch (error) {
    throw new Error(`community plugin allowlist is unreadable: ${boundedReason(error)}`)
  }
  const root = objectRecord(parsed)
  const rawPlugins = root?.plugins
  if (root?.schemaVersion !== 1 || !Array.isArray(rawPlugins)) {
    throw new Error('community plugin allowlist must be schemaVersion 1 with a plugins array')
  }
  const reserved: Record<string, true> = {
    '@deepseek-ai/dsh-base': true,
    '@deepseek-ai/dsh-headless': true,
    '@deepseek-ai/dsh-web-app': true,
    '@deepseek-ai/dsh-lsp': true,
    '@deepseek-ai/dsh-lsp-stdio': true,
    '@deepseek-ai/dsh-tool-lsp': true,
    '@deepseek-ai/dsh-mcp-client': true,
    '@playwright/mcp': true,
    'pimp-my-dsh': true,
    'pnpm': true,
  }
  const names = new Set<string>()
  return rawPlugins.map((raw, index) => {
    const record = objectRecord(raw)
    const permissions = objectRecord(record?.permissions)
    const windows = objectRecord(record?.windows)
    const filesystem = permissions?.filesystem
    const network = permissions?.network
    const process = permissions?.process
    if (
      record === undefined
      || typeof record.name !== 'string'
      || !/^(?:@[a-z0-9][a-z0-9._-]*\/)?[a-z0-9][a-z0-9._-]*$/.test(record.name)
      || reserved[record.name] === true
      || names.has(record.name)
      || typeof record.version !== 'string'
      || !/^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(-[0-9A-Za-z.-]+)?$/.test(record.version)
      || typeof record.integrity !== 'string'
      || !/^sha512-[A-Za-z0-9+/]+=*$/.test(record.integrity)
      || typeof record.source !== 'string'
      || record.source.length === 0
      || typeof record.license !== 'string'
      || record.license.length === 0
      || permissions === undefined
      || (filesystem !== 'none' && filesystem !== 'workspace' && filesystem !== 'broad')
      || (network !== 'none' && network !== 'public' && network !== 'broad')
      || (process !== 'none' && process !== 'child')
      || filesystem === 'broad'
      || network === 'broad'
      || windows === undefined
      || windows.reviewed !== true
      || typeof windows.notes !== 'string'
      || windows.notes.length === 0
      || typeof record.reviewedBy !== 'string'
      || record.reviewedBy.length === 0
      || typeof record.reviewedAt !== 'string'
      || !REVIEWED_AT_PATTERN.test(record.reviewedAt)
      || !Number.isFinite(Date.parse(record.reviewedAt))
    ) {
      throw new Error(`community plugin allowlist entry ${index} is incomplete or not admissible`)
    }
    names.add(record.name)
    return {
      name: record.name,
      version: record.version,
      integrity: record.integrity,
      source: record.source,
      license: record.license,
      permissions: {
        filesystem: filesystem as CommunityPluginReview['permissions']['filesystem'],
        network: network as CommunityPluginReview['permissions']['network'],
        process: process as CommunityPluginReview['permissions']['process'],
      },
      windows: { reviewed: true, notes: windows.notes },
      reviewedBy: record.reviewedBy,
      reviewedAt: record.reviewedAt,
    }
  })
}


function templateBundles(profile: string): string[] {
  const bundles = ['@deepseek-ai/dsh-base']
  if (profile === 'web') bundles.push('@deepseek-ai/dsh-web-app')
  else bundles.push('@deepseek-ai/dsh-headless')
  return bundles
}

function profileManifest(profile: string): ProfileManifest {
  const reviewed = reviewedCommunityPlugins()
  const dependencies: Record<string, string> = {
    'pimp-my-dsh': `link:${packageRoot.replaceAll('\\', '/')}`,
    '@deepseek-ai/dsh-lsp': UPSTREAM_VERSION,
    '@deepseek-ai/dsh-lsp-stdio': UPSTREAM_VERSION,
    '@deepseek-ai/dsh-tool-lsp': UPSTREAM_VERSION,
    '@deepseek-ai/dsh-mcp-client': UPSTREAM_VERSION,
    '@playwright/mcp': PLAYWRIGHT_MCP_VERSION,
  }
  for (const plugin of reviewed) dependencies[plugin.name] = plugin.version
  return {
    name: `dsh-profile-${profile}`,
    private: true,
    packageManager: 'pnpm@11.7.0',
    dependencies,
    dsh: {
      profile: {
        bundles: [...templateBundles(profile), ...reviewed.map((plugin) => plugin.name), 'pimp-my-dsh'],
      },
    },
  }
}

function marker(profile: string): string {
  return `${JSON.stringify({
    schemaVersion: 1,
    bundleVersion: VERSION,
    upstreamVersion: UPSTREAM_VERSION,
    profile,
  }, null, 2)}\n`
}

function packageManagerEnvironment(userConfig: string): Environment {
  const allowed: Record<string, true> = {
    PATH: true, PATHEXT: true, SYSTEMROOT: true, WINDIR: true, COMSPEC: true,
    TEMP: true, TMP: true, TMPDIR: true, HOME: true, USERPROFILE: true,
    HOMEDRIVE: true, HOMEPATH: true, APPDATA: true, LOCALAPPDATA: true,
    PROGRAMDATA: true, LANG: true, LC_ALL: true, CI: true,
    HTTP_PROXY: true, HTTPS_PROXY: true, ALL_PROXY: true, NO_PROXY: true,
    NPM_CONFIG_REGISTRY: true,
    NODE_EXTRA_CA_CERTS: true, SSL_CERT_FILE: true, SSL_CERT_DIR: true,
  }
  const environment: Environment = {}
  for (const [name, value] of Object.entries(process.env)) {
    if (allowed[name.toUpperCase()] === true && value !== undefined) environment[name] = value
  }
  environment.npm_config_userconfig = userConfig
  environment.NPM_CONFIG_USERCONFIG = userConfig
  environment.npm_config_ignore_scripts = 'true'
  environment.NPM_CONFIG_IGNORE_SCRIPTS = 'true'
  environment.npm_config_ignore_pnpmfile = 'true'
  environment.NPM_CONFIG_IGNORE_PNPMFILE = 'true'
  return environment
}

function harnessEnvironment(): Environment {
  const environment: Environment = { ...process.env }
  const promotions = [
    ['PIMP_DSH_API_KEY', 'DSH_PIMP_API_KEY'],
    ['PIMP_DSH_BASE_URL', 'DSH_PIMP_BASE_URL'],
    ['PIMP_DSH_MODEL', 'DSH_PIMP_MODEL'],
    ['PIMP_DSH_ENABLE_LSP', 'DSH_PIMP_ENABLE_LSP'],
    ['PIMP_DSH_ENABLE_WEB_SEARCH', 'DSH_PIMP_ENABLE_WEB_SEARCH'],
    ['PIMP_DSH_WEB_SEARCH_KEY', 'DSH_PIMP_WEB_SEARCH_KEY'],
    ['PIMP_DSH_ENABLE_BROWSER', 'DSH_PIMP_ENABLE_BROWSER'],
    ['PIMP_DSH_CONTEXT7_KEY', 'DSH_PIMP_CONTEXT7_KEY'],
    ['PIMP_DSH_ENABLE_CONTEXT7', 'DSH_PIMP_ENABLE_CONTEXT7'],
  ] as const
  for (const [publicName, protectedName] of promotions) {
    const value = environment[publicName]
    if (value !== undefined) environment[protectedName] = value
    delete environment[publicName]
  }
  environment.DSH_PIMP_DSH_CHILD = '1'
  environment.DSH_PIMP_BROWSER_CLI = playwrightMcpCli
  environment.DSH_TELEMETRY_DISABLED = '1'
  delete environment.DSH_TELEMETRY_MODE
  delete environment.DSH_TELEMETRY_OTLP_URL
  return environment
}

interface OwnershipMarker {
  schemaVersion: 1
  bundleVersion: string
  upstreamVersion: string
  profile: string
}

function assertOwnedProfile(directory: string, profile: string): OwnershipMarker {
  const markerPath = join(directory, '.pimp-my-dsh.json')
  if (!existsSync(markerPath)) throw new Error(`refusing to replace unmanaged profile: ${profile}`)
  const markerEntry = lstatSync(markerPath)
  if (markerEntry.isSymbolicLink() || !markerEntry.isFile() || markerEntry.nlink !== 1) {
    throw new Error(`refusing to replace profile with an unsafe ownership marker: ${profile}`)
  }
  const installed = JSON.parse(readFileSync(markerPath, 'utf8')) as Partial<OwnershipMarker>
  if (
    installed.schemaVersion !== 1
    || typeof installed.bundleVersion !== 'string'
    || typeof installed.upstreamVersion !== 'string'
    || installed.profile !== profile
  ) {
    throw new Error(`refusing to replace profile with an invalid ownership marker: ${profile}`)
  }
  return installed as OwnershipMarker
}

function assertManagedProfileDirectory(directory: string, profile: string): void {
  assertOwnedProfile(directory, profile)
  const installedMarker = JSON.parse(readFileSync(join(directory, '.pimp-my-dsh.json'), 'utf8')) as unknown
  const expectedMarker = JSON.parse(marker(profile)) as unknown
  if (JSON.stringify(installedMarker) !== JSON.stringify(expectedMarker)) {
    throw new Error(`profile was installed by a different distribution version: ${profile}`)
  }
  const installedManifest = JSON.parse(readFileSync(join(directory, 'package.json'), 'utf8')) as unknown
  if (JSON.stringify(installedManifest) !== JSON.stringify(profileManifest(profile))) {
    throw new Error(`profile manifest is not distribution-managed: ${profile}`)
  }
  const installedPatch = readFileSync(join(directory, 'cordis.patch.yml'), 'utf8')
  if (installedPatch !== readFileSync(profileSource(profile), 'utf8')) {
    throw new Error(`profile patch is not distribution-managed: ${profile}`)
  }
  const linkedBundle = join(directory, 'node_modules', 'pimp-my-dsh')
  if (!bundleTargetsPackageRoot(linkedBundle)) {
    throw new Error(`profile bundle link is missing or points at another installation: ${profile}`)
  }
}

function assertManagedProfile(profile: string): string {
  const directory = profileDirectory(profile)
  assertManagedProfileDirectory(directory, profile)
  return directory
}

function assertNoGlobalPatch(): void {
  const path = join(dshHome(), 'cordis.patch.yml')
  if (existsSync(path)) {
    throw new Error(`global harness patch is unsupported because it would outrank distribution hardening: ${path}`)
  }
}

function assertConfigurationOutsideWorkspace(profileDirectoryPath: string): void {
  const workspace = boundaryPath(process.cwd())
  for (const [label, path] of [
    ['harness home', boundaryPath(dshHome())],
    ['profile directory', boundaryPath(profileDirectoryPath)],
  ] as const) {
    const fromWorkspace = relative(workspace, path)
    if (fromWorkspace === '' || (!fromWorkspace.startsWith('..') && !isAbsolute(fromWorkspace))) {
      throw new Error(`${label} must be outside the writable workspace: ${path}`)
    }
  }
}

function installStagedProfile(directory: string, profile: string, source: string): void {
  mkdirSync(directory, { recursive: false })
  atomicWrite(join(directory, 'package.json'), `${JSON.stringify(profileManifest(profile), null, 2)}\n`)
  atomicWrite(join(directory, 'pnpm-workspace.yaml'), 'packages:\n  - .\n\nnodeLinker: hoisted\nautoInstallPeers: false\n')
  const userConfig = join(directory, '.pimp-my-dsh.npmrc')
  atomicWrite(userConfig, 'ignore-scripts=true\nignore-pnpmfile=true\n')
  const child = spawnSync(
    process.execPath,
    [pnpmBin(), 'install', '--ignore-scripts', '--ignore-pnpmfile', '--lockfile-only=false', '--frozen-lockfile=false'],
    {
      cwd: directory,
      encoding: 'utf8',
      env: packageManagerEnvironment(userConfig),
      shell: false,
      windowsHide: true,
    },
  )
  rmSync(userConfig, { force: true })
  if (child.error) throw child.error
  if (child.status !== 0) {
    const detail = child.stderr.trim() || child.stdout.trim() || `exit ${String(child.status)}`
    throw new Error(`profile dependency installation failed: ${detail}`)
  }
  atomicWrite(join(directory, 'cordis.patch.yml'), readFileSync(source, 'utf8'))
  atomicWrite(join(directory, '.pimp-my-dsh.json'), marker(profile))
  assertManagedProfileDirectory(directory, profile)
}

function installProfile(profile: string, force: boolean): void {
  const source = profileSource(profile)
  const directory = profileDirectory(profile)
  if (existsSync(directory)) {
    if (!force) {
      throw new Error(`profile directory already exists: ${directory}; pass --force to replace it`)
    }
    assertOwnedProfile(directory, profile)
  }

  const home = dshHome()
  const profiles = dirname(directory)
  mkdirSync(profiles, { recursive: true })
  assertContainedPath(home, profiles)
  const nonce = `${process.pid}.${Date.now()}`
  const staging = join(profiles, `.${profile}.${nonce}.tmp`)
  const backup = join(profiles, `.${profile}.${nonce}.backup`)
  try {
    installStagedProfile(staging, profile, source)
    assertContainedPath(home, profiles)
    profileDirectory(profile)
    const hadExisting = existsSync(directory)
    if (hadExisting) renameSync(directory, backup)
    try {
      renameSync(staging, directory)
      assertManagedProfile(profile)
    } catch (error) {
      rmSync(directory, { recursive: true, force: true })
      if (hadExisting && existsSync(backup)) renameSync(backup, directory)
      throw error
    }
    rmSync(backup, { recursive: true, force: true })
  } catch (error) {
    rmSync(staging, { recursive: true, force: true })
    throw error
  }
}

function setup(args: ParsedArgs): void {
  const profile = args.profile ?? ''
  installProfile(profile, args.force)
  emit({ command: 'setup', profile, installed: true, upstreamVersion: UPSTREAM_VERSION }, args.json)
}

function run(args: ParsedArgs): never {
  const profile = args.profile
  if (profile === undefined) throw new Error('run requires --profile <name>')
  const directory = assertManagedProfile(profile)
  assertNoGlobalPatch()
  assertConfigurationOutsideWorkspace(directory)
  const child = spawnSync(process.execPath, [dshBin(), '--profile', profile, '--', ...args.passthrough], {
    stdio: 'inherit',
    env: harnessEnvironment(),
    shell: false,
    windowsHide: false,
  })
  if (child.error) throw child.error
  process.exit(child.status ?? 1)
}

type SandboxCheck = {
  id: string
  status: 'ok' | 'warning' | 'error' | 'unavailable'
  message: string
}

const VOLUME_FILESYSTEM_SCRIPT = `
$letters = @($env:DSH_DOCTOR_TARGET_1, $env:DSH_DOCTOR_TARGET_2) | Where-Object { $_ }
$result = foreach ($letter in $letters) {
  try {
    $filesystem = Get-Volume -DriveLetter $letter -ErrorAction Stop | Select-Object -ExpandProperty FileSystem
    [PSCustomObject]@{ letter = $letter; filesystem = $filesystem; error = $null }
  } catch {
    [PSCustomObject]@{ letter = $letter; filesystem = $null; error = $_.Exception.Message }
  }
}
if ($result) { $result | ConvertTo-Json -Compress }
`

const EVERYONE_GRANTS_SCRIPT = `
$targets = @($env:DSH_DOCTOR_TARGET_1, $env:DSH_DOCTOR_TARGET_2, $env:DSH_DOCTOR_TARGET_3) | Where-Object { $_ }
$everyone = 'S-1-1-0'
$anonymous = 'S-1-5-7'
$names = @('Write', 'Modify', 'FullControl', 'ChangePermissions', 'TakeOwnership')
$flags = @{}
foreach ($name in $names) { $flags[$name] = [System.Security.AccessControl.FileSystemRights]::$name }
$result = foreach ($target in $targets) {
  $grants = @()
  $probeError = $null
  try {
    $acl = Get-Acl -LiteralPath $target -ErrorAction Stop
    foreach ($ace in $acl.Access) {
      $sid = $ace.IdentityReference.Translate([System.Security.Principal.SecurityIdentifier]).Value
      $identity = $null
      if ($sid -eq $everyone) { $identity = 'Everyone' }
      elseif ($sid -eq $anonymous) { $identity = 'Anonymous' }
      if ($identity) {
        $rights = @()
        foreach ($name in $names) {
          if (([int]$ace.FileSystemRights -band [int]$flags[$name]) -eq [int]$flags[$name]) { $rights += $name }
        }
        if ($rights.Count -gt 0) { $grants += ($identity + ':' + ($rights -join ',')) }
      }
    }
  } catch {
    $probeError = $_.Exception.Message
  }
  [PSCustomObject]@{ path = $target; grants = $grants; error = $probeError }
}
if ($result) { $result | ConvertTo-Json -Compress }
`

function boundedReason(error: unknown): string {
  const reason = error instanceof Error ? error.message : String(error)
  return reason.length > 256 ? `${reason.slice(0, 253)}...` : reason
}

function doctorEnvironment(first?: string, second?: string, third?: string): Environment {
  const environment: Environment = { ...process.env }
  delete environment.DSH_DOCTOR_TARGET_1
  delete environment.DSH_DOCTOR_TARGET_2
  delete environment.DSH_DOCTOR_TARGET_3
  if (first !== undefined) environment.DSH_DOCTOR_TARGET_1 = first
  if (second !== undefined) environment.DSH_DOCTOR_TARGET_2 = second
  if (third !== undefined) environment.DSH_DOCTOR_TARGET_3 = third
  return environment
}

function runPowerShell(args: string[], env: Environment, executable = 'powershell.exe'): { status: number | null; stdout: string; stderr: string } {
  const result = spawnSync(executable, args, {
    encoding: 'utf8',
    env,
    shell: false,
    windowsHide: true,
    timeout: 15_000,
    maxBuffer: 16 * 1024,
  })
  if (result.error) throw result.error
  return { status: result.status, stdout: result.stdout ?? '', stderr: result.stderr ?? '' }
}

function runPowerShell7(args: string[], env: Environment): { status: number | null; stdout: string; stderr: string } {
  let lastError: unknown
  for (const executable of ['pwsh.exe', 'pwsh']) {
    try {
      return runPowerShell(args, env, executable)
    } catch (error) {
      if ((error as NodeJS.ErrnoException).code !== 'ENOENT') throw error
      lastError = error
    }
  }
  return {
    status: null,
    stdout: '',
    stderr: lastError instanceof Error ? lastError.message : 'PowerShell 7 (pwsh) is unavailable',
  }
}

function probeFailureReason(status: number | null, stdout: string, stderr: string): string {
  return boundedReason(stderr.trim() || stdout.trim() || `exit ${String(status)}`)
}

function jsonRecords(output: string): Array<Record<string, unknown>> {
  const value: unknown = JSON.parse(output)
  if (value === null) return []
  return Array.isArray(value) ? value as Array<Record<string, unknown>> : [value as Record<string, unknown>]
}

function driveLetter(path: string): string | undefined {
  const match = /^([A-Za-z]):/.exec(path)
  return match?.[1]?.toUpperCase()
}

function sandboxTargets(workspace: string, home: string): string[] {
  const targets = [workspace, home]
  const memory = join(home, 'pimp-my-dsh', 'memory.jsonl')
  if (existsSync(memory)) targets.push(memory)
  return [...new Set(targets)]
}

function volumeFilesystemCheck(workspace: string, home: string): SandboxCheck {
  const id = 'volume-filesystem'
  try {
    const workspaceDrive = driveLetter(workspace)
    const homeDrive = driveLetter(home)
    if (workspaceDrive === undefined || homeDrive === undefined) {
      return { id, status: 'warning', message: 'check unavailable: no drive letter resolved' }
    }
    const { status, stdout, stderr } = runPowerShell(
      ['-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-Command', VOLUME_FILESYSTEM_SCRIPT],
      doctorEnvironment(workspaceDrive, homeDrive === workspaceDrive ? undefined : homeDrive),
    )
    if (status !== 0 || stdout.trim() === '') {
      return { id, status: 'warning', message: `check unavailable: ${probeFailureReason(status, stdout, stderr)}` }
    }
    const volumes = jsonRecords(stdout)
    const failedVolume = volumes.find((entry) => typeof entry.error === 'string' && entry.error.length > 0)
    if (failedVolume !== undefined) {
      return { id, status: 'warning', message: `check unavailable: ${boundedReason(failedVolume.error)}` }
    }
    const unsupported = volumes.filter((entry) => entry.filesystem !== 'NTFS' && entry.filesystem !== 'ReFS')
    if (unsupported.length > 0) {
      const affectedVolumes = unsupported.map((entry) => `${String(entry.letter ?? '?')}:${String(entry.filesystem || 'Unknown')}`).join(', ')
      return { id, status: 'warning', message: `unsupported filesystem(s): ${affectedVolumes}; ACL sandboxing does not exist on FAT-family volumes` }
    }
    return { id, status: 'ok', message: 'all checked volumes use NTFS/ReFS' }
  } catch (error) {
    return { id, status: 'warning', message: `check unavailable: ${boundedReason(error)}` }
  }
}

function hardLinkAliasesCheck(workspace: string, home: string): SandboxCheck {
  const id = 'hard-link-aliases'
  try {
    const aliases: Array<{ path: string; links: number }> = []
    for (const path of sandboxTargets(workspace, home)) {
      const entry = lstatSync(path)
      if (!entry.isFile()) continue
      if (entry.nlink > 1) aliases.push({ path, links: entry.nlink })
    }
    if (aliases.length > 0) {
      const paths = aliases.map(({ path, links }) => `${path} (${links} links)`).join(', ')
      return { id, status: 'error', message: `hard links break the canonical-path boundary: ${paths}` }
    }
    return { id, status: 'ok', message: 'no hard-link aliases found' }
  } catch (error) {
    return { id, status: 'warning', message: `check unavailable: ${boundedReason(error)}` }
  }
}

function everyoneGrantsCheck(workspace: string, home: string): SandboxCheck {
  const id = 'everyone-grants'
  try {
    const [first, second, third] = sandboxTargets(workspace, home)
    const { status, stdout, stderr } = runPowerShell(
      ['-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-Command', EVERYONE_GRANTS_SCRIPT],
      doctorEnvironment(first, second, third),
    )
    if (status !== 0 || stdout.trim() === '') {
      return { id, status: 'warning', message: `check unavailable: ${probeFailureReason(status, stdout, stderr)}` }
    }
    const records = jsonRecords(stdout)
    const failedAcl = records.find((entry) => typeof entry.error === 'string' && entry.error.length > 0)
    if (failedAcl !== undefined) {
      return { id, status: 'warning', message: `check unavailable: ${boundedReason(failedAcl.error)}` }
    }
    const grants = records.filter((entry) => Array.isArray(entry.grants) && entry.grants.length > 0)
    if (grants.length > 0) {
      const paths = grants.map((entry) => `${String(entry.path)} (${(entry.grants as string[]).join(', ')})`).join('; ')
      return { id, status: 'error', message: `Everyone/Anonymous write access: ${paths}` }
    }
    return { id, status: 'ok', message: 'no Everyone/Anonymous write grants' }
  } catch (error) {
    return { id, status: 'warning', message: `check unavailable: ${boundedReason(error)}` }
  }
}

function readSideConfinementCheck(): SandboxCheck {
  return {
    id: 'read-side-confinement',
    status: 'unavailable',
    message: 'unavailable for direct CLI runs: packaged desktop-supervised web runs use a zero-capability AppContainer; standalone CLI runs retain the upstream write-only token',
  }
}

function browserConfinementCheck(): SandboxCheck {
  const id = 'browser-confinement'
  try {
    const { status } = runPowerShell7(
      ['-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', join(packageRoot, 'scripts', 'confine-browser.ps1'), '-Verify'],
      doctorEnvironment(),
    )
    if (status === 0) return { id, status: 'ok', message: 'browser egress confined' }
    if (status === null) return { id, status: 'unavailable', message: 'unavailable: PowerShell 7 (pwsh) is required to verify browser confinement' }
    return { id, status: 'warning', message: 'browser automation enabled but egress not confined (run scripts/confine-browser.ps1 -Apply)' }
  } catch (error) {
    return { id, status: 'warning', message: `check unavailable: ${boundedReason(error)}` }
  }
}

function sandboxChecks(): SandboxCheck[] | null {
  if (process.platform !== 'win32') return null
  const workspace = process.cwd()
  const home = dshHome()
  const checks = [
    volumeFilesystemCheck(workspace, home),
    hardLinkAliasesCheck(workspace, home),
    everyoneGrantsCheck(workspace, home),
    readSideConfinementCheck(),
  ]
  if (process.env.PIMP_DSH_ENABLE_BROWSER === '1') checks.push(browserConfinementCheck())
  return checks
}

function doctor(args: ParsedArgs): void {
  let executable: string | undefined
  let executableError: string | undefined
  try {
    executable = dshBin()
  } catch (error) {
    executableError = error instanceof Error ? error.message : String(error)
  }
  const profile = args.profile
  let profileReady: boolean | undefined
  if (profile !== undefined) {
    try {
      assertManagedProfile(profile)
      profileReady = true
    } catch {
      profileReady = false
    }
  }
  emit({
    command: 'doctor',
    version: VERSION,
    upstreamVersion: UPSTREAM_VERSION,
    node: process.versions.node,
    platform: process.platform,
    architecture: process.arch,
    dshAvailable: executable !== undefined && existsSync(executable),
    dshError: executableError,
    profile,
    profileReady,
    apiKeyConfigured: Boolean(process.env.PIMP_DSH_API_KEY),
    baseUrlConfigured: Boolean(process.env.PIMP_DSH_BASE_URL),
    modelConfigured: Boolean(process.env.PIMP_DSH_MODEL),
    lspEnabled: process.env.PIMP_DSH_ENABLE_LSP === '1',
    telemetryEnabled: false,
    sandboxChecks: args.runtimeOnly ? null : sandboxChecks(),
  }, args.json)
}

function compareVersions(left: string, right: string): number {
  const pattern = /^(\d+)\.(\d+)\.(\d+)(?:-([0-9A-Za-z.-]+))?(?:\+[0-9A-Za-z.-]+)?$/
  const a = pattern.exec(left)
  const b = pattern.exec(right)
  if (!a || !b) throw new Error('registry returned an invalid semantic version')
  for (let index = 1; index <= 3; index += 1) {
    const difference = Number(a[index]) - Number(b[index])
    if (difference !== 0) return difference < 0 ? -1 : 1
  }

  const aPre = a[4]
  const bPre = b[4]
  if (aPre === undefined || bPre === undefined) {
    if (aPre === bPre) return 0
    return aPre === undefined ? 1 : -1
  }

  const aParts = aPre.split('.')
  const bParts = bPre.split('.')
  for (let index = 0; index < Math.max(aParts.length, bParts.length); index += 1) {
    const av = aParts[index]
    const bv = bParts[index]
    if (av === bv) continue
    if (av === undefined) return -1
    if (bv === undefined) return 1
    const aNumeric = /^\d+$/.test(av)
    const bNumeric = /^\d+$/.test(bv)
    if (aNumeric && bNumeric) return Number(av) < Number(bv) ? -1 : 1
    if (aNumeric !== bNumeric) return aNumeric ? -1 : 1
    return av.localeCompare(bv)
  }
  return 0
}

async function updateCheck(args: ParsedArgs): Promise<void> {
  const registry = (process.env.NPM_CONFIG_REGISTRY ?? process.env.npm_config_registry ?? 'https://registry.npmjs.org').replace(/\/$/, '')
  const endpoint = new URL(`${registry}/pimp-my-dsh/latest`)
  if (!['http:', 'https:'].includes(endpoint.protocol) || endpoint.username || endpoint.password) {
    throw new Error('registry must be an HTTP(S) URL without credentials')
  }
  const response = await fetch(endpoint, {
    headers: { accept: 'application/json' },
    signal: AbortSignal.timeout(3_000),
  })
  if (!response.ok) throw new Error(`registry returned HTTP ${response.status}`)
  const metadata = await response.json() as { version?: unknown }
  if (typeof metadata.version !== 'string') throw new Error('registry response has no version')
  emit({
    command: 'update-check',
    current: VERSION,
    latest: metadata.version,
    updateAvailable: compareVersions(VERSION, metadata.version) < 0,
  }, args.json)
}

function migrate(args: ParsedArgs): void {
  const profile = args.profile ?? ''
  const directory = profileDirectory(profile)
  if (!existsSync(directory)) throw new Error(`profile is not installed: ${profile}`)
  const installed = assertOwnedProfile(directory, profile)
  if (compareVersions(installed.bundleVersion, VERSION) > 0) {
    throw new Error(`refusing to downgrade profile from ${installed.bundleVersion} to ${VERSION}`)
  }
  const required = installed.bundleVersion !== VERSION || installed.upstreamVersion !== UPSTREAM_VERSION
  const applied = required && args.apply
  if (applied) installProfile(profile, true)
  emit({
    command: 'migrate',
    profile,
    fromSchemaVersion: installed.schemaVersion,
    toSchemaVersion: 1,
    fromBundleVersion: installed.bundleVersion,
    toBundleVersion: VERSION,
    required,
    applied,
  }, args.json)
}

function usage(): void {
  process.stdout.write('pimp-dsh <setup|run|doctor|update-check|migrate> [options]\n')
}

async function main(): Promise<void> {
  let parsed: ParsedArgs | undefined
  try {
    parsed = parseArgs(process.argv.slice(2))
    switch (parsed.command) {
      case 'setup': setup(parsed); break
      case 'run': run(parsed); break
      case 'doctor': doctor(parsed); break
      case 'update-check': await updateCheck(parsed); break
      case 'migrate': migrate(parsed); break
      case 'help':
      case '--help':
      case '-h': usage(); break
      default: throw new Error(`unknown command: ${parsed.command || '(missing)'}`)
    }
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error)
    if (parsed?.json || process.argv.slice(2).includes('--json')) process.stderr.write(`${JSON.stringify({ schemaVersion: OUTPUT_SCHEMA_VERSION, error: message })}\n`)
    else process.stderr.write(`pimp-dsh: ${message}\n`)
    process.exitCode = 1
  }
}

await main()

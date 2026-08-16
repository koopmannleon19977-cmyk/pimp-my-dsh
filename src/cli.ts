#!/usr/bin/env node

import { spawnSync } from 'node:child_process'
import {
  existsSync,
  lstatSync,
  mkdirSync,
  readFileSync,
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
const UPSTREAM_VERSION = '0.1.0-rc.6'
const PLAYWRIGHT_MCP_VERSION = '0.0.79'
const PROFILE_PATTERN = /^[a-z][a-z0-9-]{0,31}$/
const packageRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const require = createRequire(import.meta.url)
const playwrightMcpCli = join(dirname(require.resolve('@playwright/mcp/package.json')), 'cli.js')

type Output = Record<string, unknown>
type Environment = NodeJS.ProcessEnv

interface ParsedArgs {
  command: string
  profile?: string
  force: boolean
  json: boolean
  apply: boolean
  passthrough: string[]
}

interface ProfileManifest {
  name: string
  private: true
  packageManager: string
  dependencies: Record<string, string>
  dsh: { profile: { bundles: string[] } }
}

function parseArgs(argv: readonly string[]): ParsedArgs {
  const command = argv[0] ?? ''
  let profile: string | undefined
  let force = false
  let json = false
  let apply = false
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
    } else if (command === 'run') {
      passthrough.push(arg)
    } else {
      throw new Error(`unknown argument: ${arg}`)
    }
  }

  return { command, profile, force, json, apply, passthrough }
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

function assertContainedPath(home: string, candidate: string): void {
  const lexical = relative(home, candidate)
  if (lexical.startsWith('..') || isAbsolute(lexical)) throw new Error('profile path escapes DSH_HOME')
  if (!existsSync(candidate)) return

  const stats = lstatSync(candidate)
  if (stats.isSymbolicLink()) throw new Error(`profile path must not contain a symbolic link or junction: ${candidate}`)
  if (!stats.isDirectory()) throw new Error(`profile path component is not a directory: ${candidate}`)

  if (existsSync(home)) {
    const canonicalHome = realpathSync(home)
    const canonicalCandidate = realpathSync(candidate)
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
    process.stdout.write(`${JSON.stringify(value)}\n`)
    return
  }
  for (const [key, entry] of Object.entries(value)) process.stdout.write(`${key}: ${String(entry)}\n`)
}

function templateBundles(profile: string): string[] {
  const bundles = ['@deepseek-ai/dsh-base']
  if (profile === 'web') bundles.push('@deepseek-ai/dsh-web-app')
  else bundles.push('@deepseek-ai/dsh-headless')
  return bundles
}

function profileManifest(profile: string): ProfileManifest {
  return {
    name: `dsh-profile-${profile}`,
    private: true,
    packageManager: 'pnpm@11.7.0',
    dependencies: {
      'pimp-my-dsh': `link:${packageRoot.replaceAll('\\', '/')}`,
      '@deepseek-ai/dsh-lsp': UPSTREAM_VERSION,
      '@deepseek-ai/dsh-lsp-stdio': UPSTREAM_VERSION,
      '@deepseek-ai/dsh-tool-lsp': UPSTREAM_VERSION,
      '@deepseek-ai/dsh-mcp-client': UPSTREAM_VERSION,
      '@playwright/mcp': PLAYWRIGHT_MCP_VERSION,
    },
    dsh: { profile: { bundles: [...templateBundles(profile), 'pimp-my-dsh'] } },
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
    ['PIMP_DSH_ENABLE_BROWSER', 'DSH_PIMP_ENABLE_BROWSER'],
  ] as const
  for (const [publicName, protectedName] of promotions) {
    const value = environment[publicName]
    if (value !== undefined) environment[protectedName] = value
    delete environment[publicName]
  }
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
  if (!existsSync(linkedBundle) || realpathSync(linkedBundle) !== realpathSync(packageRoot)) {
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
  const workspace = realpathSync(process.cwd())
  for (const [label, path] of [
    ['harness home', realpathSync(dshHome())],
    ['profile directory', realpathSync(profileDirectoryPath)],
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
    if (parsed?.json) process.stderr.write(`${JSON.stringify({ error: message })}\n`)
    else process.stderr.write(`pimp-dsh: ${message}\n`)
    process.exitCode = 1
  }
}

await main()

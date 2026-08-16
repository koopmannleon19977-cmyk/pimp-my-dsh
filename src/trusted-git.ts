import { accessSync, constants, realpathSync, statSync } from 'node:fs'
import { delimiter, isAbsolute, join, relative, sep } from 'node:path'

const SAFE_GIT_ENVIRONMENT = [
  'PATH',
  'PATHEXT',
  'SystemRoot',
  'WINDIR',
  'COMSPEC',
  'TEMP',
  'TMP',
  'HOME',
  'USERPROFILE',
  'HOMEDRIVE',
  'HOMEPATH',
  'APPDATA',
  'LOCALAPPDATA',
  'PROGRAMDATA',
  'LANG',
  'LC_ALL',
  'NO_COLOR',
] as const

const executableCache = new Map<string, string>()

export function trustedExecutable(baseName: string): string {
  const name = process.platform === 'win32' ? `${baseName}.exe` : baseName
  const cached = executableCache.get(name)
  if (cached !== undefined) return cached
  const workspace = realpathSync(process.cwd())
  for (const rawDirectory of (process.env.PATH ?? '').split(delimiter)) {
    const directory = rawDirectory.replace(/^"(.*)"$/, '$1')
    if (!isAbsolute(directory)) continue
    const candidate = join(directory, name)
    try {
      accessSync(candidate, constants.X_OK)
      if (!statSync(candidate).isFile()) continue
      const resolved = realpathSync(candidate)
      const boundary = relative(workspace, resolved)
      if (boundary === '' || (!isAbsolute(boundary) && boundary !== '..'
        && !boundary.startsWith(`..${sep}`))) continue
      executableCache.set(name, resolved)
      return resolved
    } catch {
      // Never consult the repository working directory during executable lookup.
    }
  }
  throw new Error(`cannot find ${name} on an absolute PATH entry outside the workspace`)
}

export function gitExecutable(): string {
  return trustedExecutable('git')
}

export function trustedGitEnvironment(): NodeJS.ProcessEnv {
  const environment: NodeJS.ProcessEnv = {}
  const workspace = realpathSync(process.cwd())
  for (const name of SAFE_GIT_ENVIRONMENT) {
    const value = process.env[name]
    if (value === undefined) continue
    if (name === 'PATH') {
      environment.PATH = value.split(delimiter).filter((directory) => {
        const unquoted = directory.replace(/^"(.*)"$/, '$1')
        if (!isAbsolute(unquoted)) return false
        try {
          const resolved = realpathSync(unquoted)
          const boundary = relative(workspace, resolved)
          return boundary === '..' || boundary.startsWith(`..${sep}`)
            || isAbsolute(boundary)
        } catch {
          return false
        }
      }).join(delimiter)
    } else {
      environment[name] = value
    }
  }
  environment.GIT_CONFIG_NOSYSTEM = '1'
  environment.GIT_CONFIG_GLOBAL = process.platform === 'win32' ? 'NUL' : '/dev/null'
  environment.GIT_NO_LAZY_FETCH = '1'
  environment.GIT_OPTIONAL_LOCKS = '0'
  environment.GIT_TERMINAL_PROMPT = '0'
  environment.GIT_ASKPASS = ''
  return environment
}

export function processFailure(
  stderr: string | Buffer | null | undefined,
  fallback: string,
  maxLength = 16_000,
): Error {
  const detail = typeof stderr === 'string'
    ? stderr.trim()
    : Buffer.isBuffer(stderr) ? stderr.toString('utf8').trim() : ''
  return new Error((detail || fallback).slice(0, maxLength))
}

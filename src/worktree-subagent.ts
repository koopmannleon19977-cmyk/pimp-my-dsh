import { isUtf8 } from 'node:buffer'
import { spawnSync } from 'node:child_process'
import { randomUUID } from 'node:crypto'
import {
  chmodSync,
  copyFileSync,
  lstatSync,
  mkdirSync,
  readlinkSync,
  realpathSync,
  symlinkSync,
  type Stats,
} from 'node:fs'
import { resolveDshHome } from '@deepseek-ai/dsh-home-paths'
import { dirname, isAbsolute, join, relative, resolve, sep } from 'node:path'
import type { Context } from '@deepseek-ai/cordis'
import type {
  ResolvedSubagentStartRequest,
  SubagentCapabilities,
  SubagentProvider,
  SubagentResult,
  SubagentRun,
} from '@deepseek-ai/dsh-subagent'
import { startInProcessRun } from '@deepseek-ai/dsh-subagent-in-process-driver'
import { gitExecutable, processFailure, trustedGitEnvironment } from './trusted-git.js'

const MAX_GIT_OUTPUT = 16_000
const MAX_INDEX_BYTES = 67_108_864
const PROVIDER_NAME = 'worktree'

export interface Worktree {
  branch: string
  path: string
  root: string
  hooks: string
}

function runGit(root: string, args: string[], maxBuffer = MAX_GIT_OUTPUT * 4): Buffer {
  const child = spawnSync(gitExecutable(), [
    '--no-pager',
    '-c', 'core.fsmonitor=false',
    '-c', 'core.untrackedCache=false',
    ...args,
  ], {
    cwd: root,
    env: trustedGitEnvironment(),
    shell: false,
    windowsHide: true,
    timeout: 30_000,
    maxBuffer,
  })
  if (child.error) throw new Error(child.error.message.slice(0, MAX_GIT_OUTPUT))
  if (child.status !== 0) throw processFailure(child.stderr, `git exited ${String(child.status)}`, MAX_GIT_OUTPUT)
  return child.stdout
}

function repositoryRoot(cwd: string): string {
  const workspace = realpathSync(cwd)
  const stdout = runGit(workspace, ['rev-parse', '--show-toplevel'])
  const root = realpathSync(resolve(workspace, stdout.toString('utf8').trim()))
  if (root !== workspace) throw new Error('worktree subagents require the session workspace to be the repository root')
  return root
}

function lstatIfExists(path: string): Stats | undefined {
  try {
    return lstatSync(path)
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code === 'ENOENT') return undefined
    throw error
  }
}

function ensurePrivateDirectory(path: string, parent: string): string {
  let entry = lstatIfExists(path)
  if (entry === undefined) {
    try {
      mkdirSync(path, { mode: 0o700 })
    } catch (error) {
      if ((error as NodeJS.ErrnoException).code !== 'EEXIST') throw error
    }
    entry = lstatSync(path)
  }
  if (entry.isSymbolicLink() || !entry.isDirectory()) {
    throw new Error('worktree storage must use private, non-linked directories')
  }
  const canonical = realpathSync(path)
  if (dirname(canonical) !== parent) throw new Error('worktree storage escapes the harness home')
  return canonical
}

function privateWorktreeRoot(): { directory: string; hooks: string } {
  const requestedHome = resolveDshHome()
  mkdirSync(requestedHome, { recursive: true })
  const home = realpathSync(requestedHome)
  const privateRoot = ensurePrivateDirectory(join(home, 'pimp-my-dsh'), home)
  const directory = ensurePrivateDirectory(join(privateRoot, 'worktrees'), privateRoot)
  const hooks = ensurePrivateDirectory(join(privateRoot, 'empty-git-hooks'), privateRoot)
  return { directory, hooks }
}
function gitUtf8(output: Buffer): string {
  if (!isUtf8(output)) throw new Error('worktree subagents require UTF-8 Git index paths')
  return output.toString('utf8')
}

function trackedEntries(root: string): Array<{ mode: string; path: string }> {
  const flags = gitUtf8(runGit(root, ['ls-files', '-v', '-z'], MAX_INDEX_BYTES))
  if (flags.split('\0').some(record => /^[Ss] /.test(record))) {
    throw new Error('worktree subagents do not support sparse or skip-worktree indexes')
  }
  const output = gitUtf8(runGit(root, ['ls-files', '--stage', '-z'], MAX_INDEX_BYTES))
  return output.split('\0').filter(Boolean).map((record) => {
    const match = /^([0-7]{6}) [0-9a-f]+ 0\t([\s\S]+)$/.exec(record)
    if (match === null) throw new Error('git returned an invalid stage-0 index record')
    return { mode: match[1]!, path: match[2]! }
  })
}

function boundedTrackedPath(root: string, path: string): string {
  if (path.length === 0 || path.includes('\0') || isAbsolute(path)) {
    throw new Error('git returned an unsafe tracked path')
  }
  const destination = resolve(root, path)
  const boundary = relative(root, destination)
  if (boundary === '..' || boundary.startsWith(`..${sep}`) || isAbsolute(boundary)
    || boundary.split(sep).includes('.git')) {
    throw new Error('git returned a tracked path outside the worktree')
  }
  return destination
}

function isWithin(root: string, path: string): boolean {
  const boundary = relative(root, path)
  return boundary !== '..' && !boundary.startsWith(`..${sep}`) && !isAbsolute(boundary)
}

function safeSourceParents(root: string, path: string): boolean {
  const boundary = relative(root, dirname(path))
  let current = root
  for (const segment of boundary === '' ? [] : boundary.split(sep)) {
    current = join(current, segment)
    const entry = lstatIfExists(current)
    if (entry === undefined) return false
    if (entry.isSymbolicLink() || !entry.isDirectory()) {
      throw new Error('tracked path traverses a linked or non-directory workspace parent')
    }
  }
  return true
}

function ensureDestinationParents(root: string, path: string): void {
  const boundary = relative(root, dirname(path))
  let current = root
  for (const segment of boundary === '' ? [] : boundary.split(sep)) {
    current = join(current, segment)
    let entry = lstatIfExists(current)
    if (entry === undefined) {
      mkdirSync(current, { mode: 0o700 })
      entry = lstatSync(current)
    }
    if (entry.isSymbolicLink() || !entry.isDirectory()) {
      throw new Error('worktree destination traverses a linked or non-directory parent')
    }
  }
}

function copyTrackedSnapshot(sourceRoot: string, destinationRoot: string): void {
  for (const entry of trackedEntries(sourceRoot)) {
    const source = boundedTrackedPath(sourceRoot, entry.path)
    const destination = boundedTrackedPath(destinationRoot, entry.path)
    if (entry.mode === '160000') {
      throw new Error('worktree subagents do not support repositories with submodules')
    }
    if (!safeSourceParents(sourceRoot, source)) continue
    const sourceEntry = lstatIfExists(source)
    if (sourceEntry === undefined) continue
    ensureDestinationParents(destinationRoot, destination)
    if (sourceEntry.isSymbolicLink()) {
      const target = readlinkSync(source)
      const lexicalTarget = resolve(dirname(source), target)
      let canonicalTarget: string
      try {
        canonicalTarget = realpathSync(source)
      } catch {
        throw new Error(`tracked symbolic link has no resolvable in-repository target: ${entry.path}`)
      }
      if (isAbsolute(target) || !isWithin(sourceRoot, lexicalTarget) || !isWithin(sourceRoot, canonicalTarget)) {
        throw new Error(`tracked symbolic link escapes the repository: ${entry.path}`)
      }
      symlinkSync(target, destination)
    } else if (sourceEntry.isFile()) {
      copyFileSync(source, destination)
      if (process.platform !== 'win32') chmodSync(destination, sourceEntry.mode)
    } else {
      throw new Error(`tracked path is neither a file nor a symbolic link: ${entry.path}`)
    }
  }
}

function runWorktreeGit(worktree: Worktree, cwd: string, args: string[]): Buffer {
  return runGit(cwd, ['-c', `core.hooksPath=${worktree.hooks}`, ...args])
}

function removeFailedWorktree(worktree: Worktree): void {
  try {
    runWorktreeGit(worktree, worktree.root, ['worktree', 'remove', '--force', worktree.path])
  } catch {
    // Preserve the primary startup error; `git worktree prune` can repair residue.
  }
  try {
    runWorktreeGit(worktree, worktree.root, ['branch', '-D', worktree.branch])
  } catch {
    // The branch may not have been created.
  }
}

export function createWorktree(cwd: string): Worktree {
  const root = repositoryRoot(cwd)
  const { directory, hooks } = privateWorktreeRoot()
  const id = randomUUID()
  const branch = `pimp-agent/${id}`
  const path = join(directory, id)
  const worktree = { branch, path, root, hooks }
  try {
    runWorktreeGit(worktree, root, [
      'worktree', 'add', '--no-checkout', '-b', branch, path, 'HEAD',
    ])
    runWorktreeGit(worktree, path, ['read-tree', 'HEAD'])
    copyTrackedSnapshot(root, path)
    return worktree
  } catch (error) {
    removeFailedWorktree(worktree)
    throw error
  }
}

function parentAtWorktree(
  parent: ResolvedSubagentStartRequest['parent'],
  cwd: string,
): ResolvedSubagentStartRequest['parent'] {
  const session = new Proxy(parent.session, {
    get(target, property) {
      if (property === 'header') return { ...target.header, cwd }
      const value = Reflect.get(target, property, target) as unknown
      return typeof value === 'function' ? value.bind(target) : value
    },
  })
  return new Proxy(parent, {
    get(target, property) {
      if (property === 'session') return session
      const value = Reflect.get(target, property, target) as unknown
      return typeof value === 'function' ? value.bind(target) : value
    },
  })
}

function appendWorktreeResult(result: SubagentResult, worktree: Worktree): SubagentResult {
  return {
    ...result,
    output: [
      ...result.output,
      {
        type: 'text',
        text: `\n\nIsolated worktree retained for review.\nPath: ${worktree.path}\nBranch: ${worktree.branch}`,
      },
    ],
  }
}

function retainedWorktreeFailure(error: unknown, worktree: Worktree): Error {
  const message = error instanceof Error ? error.message : String(error)
  return new Error(
    `${message}\n\nIsolated worktree retained for review.\nPath: ${worktree.path}\nBranch: ${worktree.branch}`,
    { cause: error },
  )
}

class WorktreeSubagentProvider implements SubagentProvider {
  readonly name = PROVIDER_NAME
  readonly inheritsParentContext = false
  readonly capabilities: SubagentCapabilities = {
    outputSchema: true,
    depthLimit: true,
    toolFilter: true,
    persona: true,
  }

  async start(request: ResolvedSubagentStartRequest): Promise<SubagentRun> {
    const cwd = request.parent.session.header.cwd
    if (cwd === undefined) throw new Error('worktree subagents require a session workspace')
    const worktree = createWorktree(cwd)
    try {
      const run = await startInProcessRun({
        ...request,
        parent: parentAtWorktree(request.parent, worktree.path),
      }, {})
      return {
        ...run,
        result: run.result.then(
          result => appendWorktreeResult(result, worktree),
          error => { throw retainedWorktreeFailure(error, worktree) },
        ),
        async dispose() {
          try {
            await run.dispose()
          } catch (error) {
            throw retainedWorktreeFailure(error, worktree)
          }
        },
      }
    } catch (error) {
      removeFailedWorktree(worktree)
      throw error
    }
  }
}

export function registerWorktreeSubagent(ctx: Context): void {
  ctx.subagents.registerProvider(new WorktreeSubagentProvider())
}

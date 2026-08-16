import { createConnection, type Socket } from 'node:net'
import type { Context } from '@deepseek-ai/cordis'

/**
 * Child side of the desktop supervisor bridge. The Rust supervisor owns the
 * named pipe and authenticates this child by the per-run token it placed in
 * the environment; this module only ever connects to that pipe as a client
 * and reports lifecycle state back. Every secret (token, pipe name, run id)
 * is read once from the process environment, held in memory, and never
 * logged or persisted.
 */

export const PROTOCOL_VERSION = 1
export const MAX_FRAME_BYTES = 64 * 1024
export const SEQUENCE_BASE = 1

const DISTRIBUTION_VERSION = '0.1.0'
const DSH_VERSION = '0.1.0-rc.6'
const LENGTH_PREFIX_BYTES = 4
const TOKEN_PATTERN = /^[0-9a-f]{64}$/
const CONNECT_TIMEOUT_MS = 5_000
const HEALTH_INTERVAL_MS = 30_000
const LOOPBACK_HOST = '127.0.0.1'

const ENV_PIPE = 'DSH_PIMP_SUPERVISOR_PIPE'
const ENV_TOKEN = 'DSH_PIMP_SUPERVISOR_TOKEN'
const ENV_RUN_ID = 'DSH_PIMP_SUPERVISOR_RUN_ID'

type ChildFrameType = 'hello' | 'ready' | 'health' | 'stopping' | 'stopped' | 'error'

interface SupervisorEnvironment {
  pipe: string
  token: string
  runId: string
}

/**
 * Frame one child-to-supervisor message as a length-prefixed UTF-8 JSON body.
 * The 4-byte little-endian unsigned prefix carries the body byte count, which
 * is bounded by MAX_FRAME_BYTES (64 KiB).
 */
export function encodeFrame(frame: Record<string, unknown>): Buffer {
  const body = Buffer.from(JSON.stringify(frame), 'utf8')
  if (body.length > MAX_FRAME_BYTES) throw new Error('supervisor frame exceeds 64 KiB')
  const header = Buffer.allocUnsafe(LENGTH_PREFIX_BYTES)
  header.writeUInt32LE(body.length, 0)
  return Buffer.concat([header, body])
}

/**
 * Read the supervisor bridge environment, validating every field. Returns
 * undefined (fail closed) when the process was not launched by the supervisor
 * or when any field is missing or malformed.
 */
function readEnvironment(env: NodeJS.ProcessEnv): SupervisorEnvironment | undefined {
  const pipe = env[ENV_PIPE]
  const token = env[ENV_TOKEN]
  const runId = env[ENV_RUN_ID]
  if (pipe === undefined || token === undefined || runId === undefined) return undefined
  if (pipe.length === 0) return undefined
  if (!TOKEN_PATTERN.test(token)) return undefined
  if (runId.length === 0 || /\s/.test(runId)) return undefined
  // The three bridge values are secrets (or run identity) that must not leak
  // to grandchild processes or later logs. Remove them from the environment
  // the moment they have been captured and validated; the ChildBridge holds
  // them in memory only.
  delete env[ENV_PIPE]
  delete env[ENV_TOKEN]
  delete env[ENV_RUN_ID]
  return { pipe, token, runId }
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value)
}

/**
 * Validate an incoming supervisor frame. The only Rust-to-child frame is
 * `shutdown`, carrying exactly protocolVersion, type, and sequence.
 */
function isShutdownFrame(value: unknown): value is { sequence: number } {
  if (!isRecord(value)) return false
  if (Object.keys(value).length !== 3) return false
  return value.protocolVersion === PROTOCOL_VERSION
    && value.type === 'shutdown'
    && typeof value.sequence === 'number'
    && Number.isInteger(value.sequence)
    && value.sequence >= 0
}

class ChildBridge {
  private readonly environment: SupervisorEnvironment
  private sequence = SEQUENCE_BASE
  private socket: Socket | null = null
  private receiveBuffer: Buffer<ArrayBufferLike> = Buffer.alloc(0)
  private writePending = false
  private healthTimer: NodeJS.Timeout | null = null
  private readySent = false
  private shutdownStarted = false
  private tornDown = false

  constructor(
    private readonly ctx: Context,
    environment: SupervisorEnvironment,
  ) {
    this.environment = environment
  }

  connect(): void {
    const socket = createConnection(this.environment.pipe)
    this.socket = socket
    socket.setNoDelay(true)

    const connectTimer = setTimeout(() => {
      if (!socket.connecting) return
      socket.destroy()
    }, CONNECT_TIMEOUT_MS)

    socket.once('connect', () => clearTimeout(connectTimer))
    socket.once('error', () => clearTimeout(connectTimer))
    socket.once('close', () => clearTimeout(connectTimer))

    socket.on('connect', () => this.onConnect())
    socket.on('data', (chunk: Buffer) => this.onData(chunk))
    socket.on('error', () => this.onSocketError())
    socket.on('close', () => this.onClose())
  }

  private onConnect(): void {
    this.sendFrame('hello')
    this.awaitWebServer()
    this.healthTimer = setInterval(() => this.sendFrame('health'), HEALTH_INTERVAL_MS)
  }

  private awaitWebServer(): void {
    this.ctx.inject(['webServer'], (serverCtx) => {
      try {
        const server = serverCtx.get('webServer') as { port?: unknown; host?: unknown } | undefined
        const port = server?.port
        const host = server?.host
        if (typeof port !== 'number' || !Number.isInteger(port) || port < 1 || port > 65535) {
          this.sendFrame('error', { message: 'web server did not report a valid bound port' })
          return
        }
        if (host !== LOOPBACK_HOST) {
          this.sendFrame('error', { message: 'web server did not bind to loopback' })
          return
        }
        this.sendReady(port)
      } catch {
        this.sendFrame('error', { message: 'web server readiness failed' })
      }
    })
  }

  private sendReady(port: number): void {
    if (this.readySent) return
    this.readySent = true
    this.sendFrame('ready', {
      profile: 'web',
      host: LOOPBACK_HOST,
      port,
      url: `http://${LOOPBACK_HOST}:${String(port)}`,
      distributionVersion: DISTRIBUTION_VERSION,
      dshVersion: DSH_VERSION,
    })
  }

  private sendFrame(type: ChildFrameType, extra?: Record<string, unknown>): void {
    const socket = this.socket
    if (socket === null || socket.destroyed) return
    const frame: Record<string, unknown> = {
      protocolVersion: PROTOCOL_VERSION,
      type,
      runId: this.environment.runId,
      token: this.environment.token,
      sequence: this.sequence,
      ...extra,
    }
    this.sequence += 1
    this.writePending = socket.write(encodeFrame(frame)) === false
  }

  private flush(): Promise<void> {
    const socket = this.socket
    if (socket === null || socket.destroyed) return Promise.resolve()
    if (!this.writePending) return Promise.resolve()
    return new Promise((resolve) => {
      const settle = (): void => {
        this.writePending = false
        resolve()
      }
      socket.once('drain', settle)
      socket.once('close', settle)
      socket.once('error', settle)
    })
  }

  private onData(chunk: Buffer): void {
    this.receiveBuffer = this.receiveBuffer.length === 0 ? chunk : Buffer.concat([this.receiveBuffer, chunk])
    this.drainFrames()
  }

  private drainFrames(): void {
    for (;;) {
      if (this.receiveBuffer.length < LENGTH_PREFIX_BYTES) return
      const length = this.receiveBuffer.readUInt32LE(0)
      if (length > MAX_FRAME_BYTES) {
        this.sendFrame('error', { message: 'received an oversized frame' })
        this.closeSocket()
        return
      }
      if (this.receiveBuffer.length < LENGTH_PREFIX_BYTES + length) return
      const body = this.receiveBuffer.subarray(LENGTH_PREFIX_BYTES, LENGTH_PREFIX_BYTES + length)
      this.receiveBuffer = this.receiveBuffer.subarray(LENGTH_PREFIX_BYTES + length)
      this.handleFrame(body)
    }
  }

  private handleFrame(body: Buffer): void {
    let value: unknown
    try {
      value = JSON.parse(body.toString('utf8'))
    } catch {
      this.sendFrame('error', { message: 'received a malformed frame' })
      return
    }
    if (!isShutdownFrame(value)) {
      this.sendFrame('error', { message: 'received an invalid frame' })
      return
    }
    void this.handleShutdown()
  }

  private async handleShutdown(): Promise<void> {
    if (this.shutdownStarted) return
    this.shutdownStarted = true
    this.sendFrame('stopping')
    await this.flush()

    // Enter the pinned DSH process-shutdown controller. It owns the same
    // whole-app disposer plus the upstream five-second forced-exit bound used
    // for a real SIGTERM; calling the fiber directly would lose that bound.
    if (!process.emit('SIGTERM')) {
      this.sendFrame('error', { message: 'bounded shutdown handler unavailable' })
      await this.flush().catch(() => {})
      this.closeSocket()
    }
  }

  private onSocketError(): void {
    // Fail closed without disclosing any environment value.
    this.ctx.logger('supervisor-bridge').warn('supervisor bridge connection closed unexpectedly')
    this.closeSocket()
  }

  private onClose(): void {
    this.closeSocket()
  }

  private closeSocket(): void {
    const socket = this.socket
    this.socket = null
    if (socket !== null) socket.destroy()
  }

  private clearHealth(): void {
    if (this.healthTimer !== null) {
      clearInterval(this.healthTimer)
      this.healthTimer = null
    }
  }

  dispose(): void {
    if (this.tornDown) return
    this.tornDown = true
    this.clearHealth()
    // A bridge-initiated shutdown exits through DSH's SIGTERM controller;
    // process teardown closes the pipe after application disposal.
    if (!this.shutdownStarted) this.closeSocket()
  }
}

/**
 * Register the supervisor bridge. This is a no-op (fail closed) unless the
 * process was launched by the desktop supervisor with a complete, valid
 * bridge environment.
 */
export function registerSupervisorBridge(ctx: Context): void {
  const environment = readEnvironment(process.env)
  if (environment === undefined) return
  ctx.effect(() => {
    const bridge = new ChildBridge(ctx, environment)
    bridge.connect()
    return () => bridge.dispose()
  })
}

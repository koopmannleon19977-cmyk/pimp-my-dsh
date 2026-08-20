import { afterEach, describe, expect, it, vi } from 'vitest'
import { registerSupervisorBridge, buildHealthChecks, parseSupervisorFrame } from '../src/supervisor-bridge'

const ENV_PIPE = 'DSH_PIMP_SUPERVISOR_PIPE'
const ENV_TOKEN = 'DSH_PIMP_SUPERVISOR_TOKEN'
const ENV_RUN_ID = 'DSH_PIMP_SUPERVISOR_RUN_ID'

function setValidBridgeEnv(): void {
  process.env[ENV_PIPE] = '\\\\.\\pipe\\pimp-dsh-test'
  process.env[ENV_TOKEN] = 'f'.repeat(64)
  process.env[ENV_RUN_ID] = 'run123'
}

function clearBridgeEnv(): void {
  delete process.env[ENV_PIPE]
  delete process.env[ENV_TOKEN]
  delete process.env[ENV_RUN_ID]
}

describe('registerSupervisorBridge environment hygiene', () => {
  afterEach(() => {
    clearBridgeEnv()
    vi.clearAllMocks()
  })

  it('removes token/pipe/run env keys immediately after validated capture', () => {
    setValidBridgeEnv()
    const effect = vi.fn()
    registerSupervisorBridge({ effect } as unknown as Parameters<typeof registerSupervisorBridge>[0])

    expect(process.env[ENV_PIPE]).toBeUndefined()
    expect(process.env[ENV_TOKEN]).toBeUndefined()
    expect(process.env[ENV_RUN_ID]).toBeUndefined()
    expect(effect).toHaveBeenCalledTimes(1)
  })

  it('fails closed without registering when the environment is absent', () => {
    clearBridgeEnv()
    const effect = vi.fn()
    registerSupervisorBridge({ effect } as unknown as Parameters<typeof registerSupervisorBridge>[0])
    expect(effect).not.toHaveBeenCalled()
  })

  it('does not mutate the environment when a field is malformed', () => {
    process.env[ENV_PIPE] = '\\\\.\\pipe\\pimp-dsh-test'
    process.env[ENV_TOKEN] = 'not-hex'
    process.env[ENV_RUN_ID] = 'run123'
    const effect = vi.fn()
    registerSupervisorBridge({ effect } as unknown as Parameters<typeof registerSupervisorBridge>[0])

    // Fail-closed: invalid token, no capture, no deletion, no effect.
    expect(process.env[ENV_TOKEN]).toBe('not-hex')
    expect(effect).not.toHaveBeenCalled()
  })
})

describe('buildHealthChecks', () => {
  it('reports no checks before the web server reports its port', () => {
    expect(buildHealthChecks(null)).toEqual([])
  })

  it('reports the loopback web server with its port once ready', () => {
    expect(buildHealthChecks(58581)).toEqual([
      { id: 'web-server', status: 'ok', message: 'listening on 127.0.0.1:58581' },
    ])
  })
})

describe('Rust-to-child control frames', () => {
  const environment = { runId: 'run123', token: 'f'.repeat(64) }
  const shutdown = {
    protocolVersion: 1,
    type: 'shutdown',
    runId: environment.runId,
    token: environment.token,
    sequence: 1,
  }
  const webAccept = {
    ...shutdown,
    type: 'web-accept',
    pipeName: '\\\\.\\pipe\\pimp-dsh-web-' + 'a'.repeat(32),
    connectionToken: 'b'.repeat(64),
  }

  it('authenticates and independently sequences both accepted frame types', () => {
    expect(parseSupervisorFrame(shutdown, environment, 1)).toEqual({ type: 'shutdown', sequence: 1 })
    expect(parseSupervisorFrame({ ...webAccept, sequence: 2 }, environment, 2)).toEqual({
      type: 'web-accept',
      sequence: 2,
      pipeName: webAccept.pipeName,
      connectionToken: webAccept.connectionToken,
    })
  })

  it.each([
    ['wrong run', { ...shutdown, runId: 'other' }],
    ['wrong token', { ...shutdown, token: 'e'.repeat(64) }],
    ['wrong sequence', { ...shutdown, sequence: 2 }],
    ['extra field', { ...shutdown, extra: true }],
    ['malformed pipe', { ...webAccept, pipeName: '\\\\.\\pipe\\other' }],
    ['malformed connection token', { ...webAccept, connectionToken: 'A'.repeat(64) }],
  ])('rejects %s', (_label, frame) => {
    expect(parseSupervisorFrame(frame, environment, 1)).toBeUndefined()
  })
})

import { afterEach, describe, expect, it, vi } from 'vitest'
import { registerSupervisorBridge } from '../src/supervisor-bridge'

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

import { EventEmitter } from 'node:events'
import { beforeEach, describe, expect, it, vi } from 'vitest'

const mocked = vi.hoisted(() => ({ createConnection: vi.fn() }))
vi.mock('node:net', () => ({ createConnection: mocked.createConnection }))
vi.mock('../src/confined-web-transport', () => ({
  acceptConfinedWebConnection: vi.fn(),
}))

import { encodeFrame, registerSupervisorBridge } from '../src/supervisor-bridge'

class FakeSocket extends EventEmitter {
  connecting = false
  destroyed = false
  writes: Buffer[] = []

  setNoDelay(): void {}
  write(frame: Buffer): boolean {
    this.writes.push(frame)
    return true
  }
  end(): void {}
  destroy(): void {
    this.destroyed = true
  }
}

describe('supervisor shutdown dispatch', () => {
  beforeEach(() => {
    delete process.env.DSH_PIMP_CONFINED_WEB
    delete process.env.DSH_PIMP_WEB_PROXY_PORT
    process.env.DSH_PIMP_SUPERVISOR_PIPE = '\\\\.\\pipe\\pimp-dsh-test'
    process.env.DSH_PIMP_SUPERVISOR_TOKEN = 'f'.repeat(64)
    process.env.DSH_PIMP_SUPERVISOR_RUN_ID = 'run123'
  })

  it('routes an authenticated shutdown frame through the bounded SIGTERM controller', async () => {
    const socket = new FakeSocket()
    mocked.createConnection.mockReturnValue(socket)
    let start: (() => () => void) | undefined
    const ctx = {
      effect(callback: () => () => void) { start = callback },
      inject: vi.fn(),
      logger: vi.fn(() => ({ warn: vi.fn() })),
    }
    registerSupervisorBridge(ctx as unknown as Parameters<typeof registerSupervisorBridge>[0])
    const dispose = start!()
    const emit = vi.spyOn(process, 'emit').mockReturnValue(true)
    socket.emit('data', encodeFrame({
      protocolVersion: 1,
      type: 'shutdown',
      runId: 'run123',
      token: 'f'.repeat(64),
      sequence: 1,
    }))
    await Promise.resolve()

    expect(emit).toHaveBeenCalledWith('SIGTERM')
    dispose()
    emit.mockRestore()
  })
})

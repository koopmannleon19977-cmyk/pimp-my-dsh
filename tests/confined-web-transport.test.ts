import { Server as HttpServer } from 'node:http'
import { EventEmitter } from 'node:events'
import { Server } from 'node:net'
import type { Socket } from 'node:net'
import { describe, expect, it, vi } from 'vitest'
import { acceptConfinedWebConnection, activateConfinedWebTransport } from '../src/confined-web-transport'

const PIPE = '\\\\.\\pipe\\pimp-dsh-web-' + 'a'.repeat(32)
const TOKEN = 'b'.repeat(64)
const ANCHOR = '\\\\.\\pipe\\LOCAL\\pimp-dsh-anchor-' + 'c'.repeat(32)

class FakeSocket extends EventEmitter {
  destroyed = false
  readonly writes: Array<[string, string]> = []

  constructor(private readonly writeError?: Error) {
    super()
  }

  write(chunk: string, encoding: string, callback: (error?: Error | null) => void): boolean {
    this.writes.push([chunk, encoding])
    queueMicrotask(() => callback(this.writeError))
  }

  destroy(): this {
    if (!this.destroyed) {
      this.destroyed = true
      this.emit('close')
    }
    return this
  }
}

function stubOriginalListen(prototype: Pick<Server, 'listen'>) {
  const listen = vi.fn(function (this: Server, ...args: unknown[]): Server {
    const callback = args[1]
    if (typeof callback === 'function') this.once('listening', callback as () => void)
    process.nextTick(() => this.emit('listening'))
    return this
  })
  Object.defineProperty(prototype, 'listen', { configurable: true, writable: true, value: listen })
  return listen
}

function environment(port = '43123'): NodeJS.ProcessEnv {
  return {
    DSH_PIMP_DSH_CHILD: '1',
    DSH_PIMP_CONFINED_WEB: '1',
    DSH_PIMP_WEB_PROXY_PORT: port,
    DSH_PIMP_WEB_ANCHOR_PIPE: ANCHOR,
  }
}

describe('confined web preload', () => {
  it('leaves pending transport state untouched in the distribution wrapper', () => {
    const listen = vi.fn()
    const prototype = { listen } as unknown as Pick<Server, 'listen'>
    const env = {
      DSH_PIMP_CONFINED_WEB: '1',
      DSH_PIMP_WEB_PROXY_PORT: '43123',
      DSH_PIMP_WEB_ANCHOR_PIPE: ANCHOR,
    }

    expect(activateConfinedWebTransport(env, prototype)).toBeUndefined()
    expect(env).toEqual({
      DSH_PIMP_CONFINED_WEB: '1',
      DSH_PIMP_WEB_PROXY_PORT: '43123',
      DSH_PIMP_WEB_ANCHOR_PIPE: ANCHOR,
    })
    expect(prototype.listen).toBe(listen)
  })

  it('is an inactive no-op in an unconfined DSH child', () => {
    const listen = vi.fn()
    const prototype = { listen } as unknown as Pick<Server, 'listen'>
    const env = { DSH_PIMP_DSH_CHILD: '1' }

    expect(activateConfinedWebTransport(env, prototype)).toBeUndefined()
    expect(env.DSH_PIMP_DSH_CHILD).toBeUndefined()
    expect(prototype.listen).toBe(listen)
  })

  it('intercepts one exact HTTP listen and exposes only its synthetic address', async () => {
    class ConfinedServer extends HttpServer {}
    const originalListen = stubOriginalListen(ConfinedServer.prototype)
    const env = environment()
    activateConfinedWebTransport(env, ConfinedServer.prototype)
    const server = new ConfinedServer()
    const listening = vi.fn()

    expect(server.listen(43123, '127.0.0.1', listening)).toBe(server)
    expect(server.address()).toEqual({ address: '127.0.0.1', family: 'IPv4', port: 43123 })
    expect(originalListen).toHaveBeenCalledWith(ANCHOR, listening)
    expect(env.DSH_PIMP_DSH_CHILD).toBeUndefined()
    expect(env.DSH_PIMP_CONFINED_WEB).toBeUndefined()
    expect(env.DSH_PIMP_WEB_PROXY_PORT).toBeUndefined()
    expect(env.DSH_PIMP_WEB_ANCHOR_PIPE).toBeUndefined()
    await new Promise<void>((resolve) => process.nextTick(resolve))
    expect(listening).toHaveBeenCalledOnce()
    expect(() => server.listen(43123, '127.0.0.1')).toThrow(/only be called once/)
  })

  it('rejects wrong listen targets and duplicate preload installation', () => {
    class WrongServer extends HttpServer {}
    stubOriginalListen(WrongServer.prototype)
    activateConfinedWebTransport(environment(), WrongServer.prototype)
    const server = new WrongServer()

    expect(() => server.listen(43124, '127.0.0.1')).toThrow(/must listen/)
    expect(() => server.listen(43123, '0.0.0.0')).toThrow(/must listen/)
    expect(() => activateConfinedWebTransport(environment(), WrongServer.prototype)).toThrow(/already installed/)
  })

  it('proves the authenticated control command before injecting the socket', async () => {
    class InjectedServer extends HttpServer {}
    stubOriginalListen(InjectedServer.prototype)
    const fake = new FakeSocket()
    const socket = fake as unknown as Socket
    const connect = vi.fn(() => socket)
    const accept = activateConfinedWebTransport(environment(), InjectedServer.prototype, connect)
    const server = new InjectedServer()
    server.removeAllListeners('connection')
    const connection = vi.fn()
    server.on('connection', connection)
    server.listen(43123, '127.0.0.1')

    await accept!(PIPE, TOKEN)

    expect(connect).toHaveBeenCalledWith(PIPE)
    expect(connection).toHaveBeenCalledWith(socket)
    expect(fake.destroyed).toBe(false)
    expect(fake.writes).toEqual([[TOKEN, 'ascii']])
  })

  it('rejects a failed connection proof without injecting the socket', async () => {
    class RejectedServer extends HttpServer {}
    stubOriginalListen(RejectedServer.prototype)
    const fake = new FakeSocket(new Error('write failed'))
    const socket = fake as unknown as Socket
    const accept = activateConfinedWebTransport(environment(), RejectedServer.prototype, () => socket)
    const server = new RejectedServer()
    server.removeAllListeners('connection')
    const connection = vi.fn()
    server.on('connection', connection)
    server.listen(43123, '127.0.0.1')

    await expect(accept!(PIPE, TOKEN)).rejects.toThrow(/proof failed/)
    expect(fake.destroyed).toBe(true)
    expect(connection).not.toHaveBeenCalled()
  })
  it('shares the active acceptor across duplicate physical module instances', async () => {
    const fake = new FakeSocket()
    const socket = fake as unknown as Socket
    stubOriginalListen(Server.prototype)
    activateConfinedWebTransport(environment(), Server.prototype, () => socket)
    const server = new HttpServer()
    server.removeAllListeners('connection')
    const injected = vi.fn()
    server.on('connection', injected)
    server.listen(43123, '127.0.0.1')

    await acceptConfinedWebConnection(PIPE, TOKEN)

    expect(injected).toHaveBeenCalledWith(socket)
    expect(fake.destroyed).toBe(false)
    expect(fake.writes).toEqual([[TOKEN, 'ascii']])
  })

})

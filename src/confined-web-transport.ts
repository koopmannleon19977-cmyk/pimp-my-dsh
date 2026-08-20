import { Server as HttpServer } from 'node:http'
import { createConnection, Server, type AddressInfo, type Socket } from 'node:net'

const ENV_ENABLED = 'DSH_PIMP_CONFINED_WEB'
const ENV_PROXY_PORT = 'DSH_PIMP_WEB_PROXY_PORT'
const ENV_DSH_CHILD = 'DSH_PIMP_DSH_CHILD'
const ENV_ANCHOR_PIPE = 'DSH_PIMP_WEB_ANCHOR_PIPE'
const LOOPBACK_HOST = '127.0.0.1'
const CONNECTION_TOKEN_PATTERN = /^[0-9a-f]{64}$/
const PIPE_PATTERN = /^\\\\\.\\pipe\\pimp-dsh-web-[0-9a-f]{32}$/
const ANCHOR_PIPE_PATTERN = /^\\\\\.\\pipe\\LOCAL\\pimp-dsh-anchor-[0-9a-f]{32}$/
const INSTALLED = Symbol.for('pimp-my-dsh.confined-web-transport')
const SHARED_ACCEPTOR = Symbol.for('pimp-my-dsh.confined-web-acceptor')

type Connect = (path: string) => Socket
type Acceptor = (pipeName: string, connectionToken: string) => Promise<void>
type Listen = (this: Server, ...args: unknown[]) => Server

function proveConnection(socket: Socket, token: string): Promise<void> {
  const { promise, resolve, reject } = Promise.withResolvers<void>()
  let settled = false
  const cleanup = (): void => {
    socket.off('error', fail)
    socket.off('close', fail)
  }
  const fail = (): void => {
    if (settled) return
    settled = true
    cleanup()
    socket.destroy()
    reject(new Error('confined web connection proof failed'))
  }
  socket.once('error', fail)
  socket.once('close', fail)
  try {
    socket.write(token, 'ascii', (error?: Error | null) => {
      if (error || socket.destroyed) {
        fail()
        return
      }
      settled = true
      cleanup()
      resolve()
    })
  } catch {
    fail()
  }
  return promise
}

/** Install the environment-gated preload transport. Optional arguments are test seams. */
export function activateConfinedWebTransport(
  env: NodeJS.ProcessEnv = process.env,
  serverPrototype: Pick<Server, 'listen'> = Server.prototype,
  connect: Connect = createConnection,
): Acceptor | undefined {
  const child = env[ENV_DSH_CHILD]
  if (child === undefined) return undefined
  delete env[ENV_DSH_CHILD]
  if (child !== '1') throw new Error('invalid confined web transport child marker')

  const enabled = env[ENV_ENABLED]
  const rawPort = env[ENV_PROXY_PORT]
  const anchorPipe = env[ENV_ANCHOR_PIPE]
  if (enabled === undefined && rawPort === undefined && anchorPipe === undefined) return undefined

  delete env[ENV_ENABLED]
  delete env[ENV_PROXY_PORT]
  delete env[ENV_ANCHOR_PIPE]

  if (enabled !== '1'
    || rawPort === undefined
    || !/^[1-9][0-9]{0,4}$/.test(rawPort)
    || anchorPipe === undefined
    || !ANCHOR_PIPE_PATTERN.test(anchorPipe)) {
    throw new Error('invalid confined web transport environment')
  }
  const proxyPort = Number(rawPort)
  if (proxyPort > 65535) throw new Error('invalid confined web transport environment')
  if (Object.prototype.hasOwnProperty.call(serverPrototype, INSTALLED)) {
    throw new Error('confined web transport is already installed')
  }
  const originalListen = serverPrototype.listen as unknown as Listen

  let webServer: HttpServer | undefined
  const listen: Listen = function (this: Server, ...args: unknown[]): Server {
    if (webServer !== undefined) throw new Error('confined web server listen may only be called once')
    const callback = args[2]
    if (!(this instanceof HttpServer)
      || args.length < 2
      || args.length > 3
      || args[0] !== proxyPort
      || args[1] !== LOOPBACK_HOST
      || (callback !== undefined && typeof callback !== 'function')) {
      throw new Error(`confined web server must listen on ${LOOPBACK_HOST}:${String(proxyPort)}`)
    }

    webServer = this
    Object.defineProperty(this, 'address', {
      configurable: true,
      value: (): AddressInfo => ({ address: LOOPBACK_HOST, family: 'IPv4', port: proxyPort }),
    })
    return callback === undefined
      ? originalListen.call(this, anchorPipe)
      : originalListen.call(this, anchorPipe, callback)
  }

  const acceptor: Acceptor = async (pipeName: string, connectionToken: string): Promise<void> => {
    if (!PIPE_PATTERN.test(pipeName) || !CONNECTION_TOKEN_PATTERN.test(connectionToken)) {
      throw new Error('invalid confined web connection parameters')
    }
    const server = webServer
    if (server === undefined) throw new Error('confined web server is not listening')

    let socket: Socket
    try {
      socket = connect(pipeName)
    } catch {
      throw new Error('confined web pipe connection failed')
    }
    await proveConnection(socket, connectionToken)
    server.emit('connection', socket)
  }

  Object.defineProperty(serverPrototype, INSTALLED, { value: true })
  Object.defineProperty(serverPrototype, 'listen', { configurable: true, writable: true, value: listen })
  if (serverPrototype === Server.prototype) {
    Object.defineProperty(globalThis, SHARED_ACCEPTOR, { value: acceptor })
  }
  return acceptor
}

const confinedWebAcceptor = activateConfinedWebTransport()

export function acceptConfinedWebConnection(pipeName: string, connectionToken: string): Promise<void> {
  const shared = (globalThis as unknown as Record<symbol, unknown>)[SHARED_ACCEPTOR]
  const acceptor = confinedWebAcceptor ?? (typeof shared === 'function' ? shared as Acceptor : undefined)
  if (acceptor === undefined) return Promise.reject(new Error('confined web transport is inactive'))
  return acceptor(pipeName, connectionToken)
}

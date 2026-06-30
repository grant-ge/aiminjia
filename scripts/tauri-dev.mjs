#!/usr/bin/env node
// Tauri dev 启动封装：保留 Cargo 增量编译，并按端口隔离 dev 身份。

import { spawn } from 'node:child_process'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const scriptDir = dirname(fileURLToPath(import.meta.url))
const projectDir = dirname(scriptDir)
const tauriCliScript = resolve(projectDir, 'node_modules/@tauri-apps/cli/tauri.js')
const DEFAULT_PORT = 5173

function parsePort(argv) {
  let port = DEFAULT_PORT
  const passthrough = []

  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index]
    if (arg === '--port' || arg === '-p') {
      const value = argv[index + 1]
      if (!value) {
        throw new Error(`${arg} requires a port value`)
      }
      port = parsePortValue(value)
      index += 1
      continue
    }

    if (arg.startsWith('--port=')) {
      port = parsePortValue(arg.slice('--port='.length))
      continue
    }

    passthrough.push(arg)
  }

  return { port, passthrough }
}

function parsePortValue(value) {
  if (!/^[0-9]+$/.test(value)) {
    throw new Error(`Invalid port "${value}"`)
  }

  const port = Number(value)
  if (!Number.isSafeInteger(port) || port < 1 || port > 65535) {
    throw new Error(`Port must be between 1 and 65535, got "${value}"`)
  }

  return port
}

const env = { ...process.env }

if (env.PKG_CONFIG_PATH) {
  const normalizedPkgConfigPath = [
    ...new Set(env.PKG_CONFIG_PATH.split(':').filter(Boolean)),
  ].join(':')
  if (normalizedPkgConfigPath !== env.PKG_CONFIG_PATH) {
    console.log('[tauri-dev] 已归一化 PKG_CONFIG_PATH，避免 Cargo 重复判定依赖过期。')
  }
  env.PKG_CONFIG_PATH = normalizedPkgConfigPath
}

if (!env.CARGO_INCREMENTAL) {
  env.CARGO_INCREMENTAL = '1'
  console.log('[tauri-dev] 已启用 CARGO_INCREMENTAL=1；如需节省 target 目录空间，可用 CARGO_INCREMENTAL=0 关闭。')
}

const tauriArgs = process.argv.slice(2)
if (tauriArgs[0] === '--') {
  tauriArgs.shift()
}

const { port, passthrough } = parsePort(tauriArgs)
const productName = String(port)
const identifier = `com.aijia.app.dev.${port}`
const devUrl = `http://127.0.0.1:${port}`
const runnerPath =
  process.platform === 'win32'
    ? resolve(projectDir, 'scripts/tauri-dev-runner.cmd')
    : resolve(projectDir, 'scripts/tauri-dev-runner.mjs')
const devCsp = [
  "default-src 'self'",
  `connect-src 'self' http://localhost:${port} ws://localhost:${port} http://127.0.0.1:${port} ws://127.0.0.1:${port} ipc: http://ipc.localhost https://*`,
  "style-src 'self' 'unsafe-inline'",
  "script-src 'self' 'unsafe-inline'",
  `img-src 'self' data: asset: http://localhost:${port} http://127.0.0.1:${port} https:`,
].join('; ')

const portOverride = JSON.stringify({
  productName,
  identifier,
  build: {
    devUrl,
    beforeDevCommand: `node scripts/vite-dev.mjs --port ${port}`,
  },
  app: {
    security: {
      devCsp,
    },
  },
})

const args = [
  'dev',
  '--config',
  'src-tauri/tauri.dev.conf.json',
  '--config',
  portOverride,
  '--runner',
  runnerPath,
  ...passthrough,
]

const command = process.platform === 'win32' ? process.execPath : 'tauri'
const commandArgs = process.platform === 'win32' ? [tauriCliScript, ...args] : args

if (process.env.AIJIA_TAURI_DEV_DRY_RUN === '1') {
  console.log(JSON.stringify({ command, args: commandArgs, port, productName, identifier }, null, 2))
  process.exit(0)
}

env.AIJIA_DEV_APP_NAME = productName
env.AIJIA_DEV_NODE = process.execPath

const child = spawn(command, commandArgs, {
  cwd: projectDir,
  env,
  stdio: 'inherit',
})

child.on('error', (error) => {
  console.error(`[tauri-dev] 启动 tauri dev 失败：${error.message}`)
  process.exit(1)
})

const forwardSignal = (signal) => {
  if (!child.killed) {
    child.kill(signal)
  }
}

for (const signal of ['SIGINT', 'SIGTERM', 'SIGHUP']) {
  process.on(signal, () => forwardSignal(signal))
}

child.on('exit', (code, signal) => {
  if (signal) {
    const signalExitCodes = {
      SIGHUP: 129,
      SIGINT: 130,
      SIGTERM: 143,
    }
    process.exit(signalExitCodes[signal] ?? 1)
    return
  }
  process.exit(code ?? 1)
})

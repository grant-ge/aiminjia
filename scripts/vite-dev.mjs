#!/usr/bin/env node
import { spawn } from 'node:child_process'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const DEFAULT_PORT = 5173
const scriptDir = dirname(fileURLToPath(import.meta.url))
const projectDir = resolve(scriptDir, '..')

function parsePort(argv) {
  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index]
    if (arg === '--port' || arg === '-p') {
      return parsePortValue(argv[index + 1])
    }
    if (arg.startsWith('--port=')) {
      return parsePortValue(arg.slice('--port='.length))
    }
  }
  return DEFAULT_PORT
}

function parsePortValue(value) {
  if (!value || !/^[0-9]+$/.test(value)) {
    throw new Error(`Invalid port "${value ?? ''}"`)
  }

  const port = Number(value)
  if (!Number.isSafeInteger(port) || port < 1 || port > 65535) {
    throw new Error(`Port must be between 1 and 65535, got "${value}"`)
  }

  return port
}

const port = parsePort(process.argv.slice(2))
const pnpmCommand = process.platform === 'win32' ? 'pnpm.cmd' : 'pnpm'
const viteArgs = ['--host', '0.0.0.0', '--port', String(port), '--strictPort']
const command = process.platform === 'win32' ? process.execPath : pnpmCommand
const commandArgs =
  process.platform === 'win32'
    ? [resolve(projectDir, 'node_modules/vite/bin/vite.js'), ...viteArgs]
    : ['exec', 'vite', ...viteArgs]

if (process.env.AIJIA_VITE_DEV_DRY_RUN === '1') {
  console.log(JSON.stringify({ command, args: commandArgs, port }, null, 2))
  process.exit(0)
}

const child = spawn(
  command,
  commandArgs,
  {
    cwd: process.cwd(),
    env: {
      ...process.env,
      VITE_AIJIA_DEV_APP_NAME: String(port),
    },
    stdio: 'inherit',
  },
)

child.on('error', (error) => {
  console.error(`Failed to start Vite dev server: ${error.message}`)
  process.exit(1)
})

child.on('exit', (code, signal) => {
  if (signal) {
    process.kill(process.pid, signal)
    return
  }
  process.exit(code ?? 1)
})

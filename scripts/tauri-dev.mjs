#!/usr/bin/env node
// Tauri dev 启动封装：保留内置 runtime 自检，同时默认启用 Cargo 增量编译来缩短日常重启时间。

import { spawn, spawnSync } from 'node:child_process'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

const scriptDir = dirname(fileURLToPath(import.meta.url))
const projectDir = dirname(scriptDir)
const ensureRuntimeScript = join(scriptDir, 'ensure-bundled-runtime.mjs')

const ensureResult = spawnSync(process.execPath, [ensureRuntimeScript], {
  cwd: projectDir,
  env: process.env,
  stdio: 'inherit',
})

if (ensureResult.error) {
  console.error(`[tauri-dev] 启动 runtime 自检失败：${ensureResult.error.message}`)
  process.exit(1)
}

if ((ensureResult.status ?? 1) !== 0) {
  process.exit(ensureResult.status ?? 1)
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

const child = spawn('tauri', ['dev', ...tauriArgs], {
  cwd: projectDir,
  env,
  shell: process.platform === 'win32',
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

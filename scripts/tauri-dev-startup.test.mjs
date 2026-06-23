import assert from 'node:assert/strict'
import { existsSync } from 'node:fs'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { spawnSync } from 'node:child_process'
import test from 'node:test'

const scriptDir = dirname(fileURLToPath(import.meta.url))
const projectDir = resolve(scriptDir, '..')
const tauriDevScript = resolve(scriptDir, 'tauri-dev.mjs')
const viteDevScript = resolve(scriptDir, 'vite-dev.mjs')

function parseDryRunJson(stdout) {
  const start = stdout.lastIndexOf('\n{')
  const jsonText = (start === -1 ? stdout.slice(stdout.indexOf('{')) : stdout.slice(start + 1)).trim()
  return JSON.parse(jsonText)
}

test('tauri-dev dry-run uses the project Tauri CLI and platform runner', () => {
  const result = spawnSync(process.execPath, [tauriDevScript, '--features', 'e2e', '--port', '5174'], {
    cwd: projectDir,
    env: {
      ...process.env,
      AIJIA_TAURI_DEV_DRY_RUN: '1',
    },
    encoding: 'utf8',
  })

  assert.equal(result.status, 0, result.stderr || result.stdout)

  const dryRun = parseDryRunJson(result.stdout)
  if (process.platform === 'win32') {
    assert.equal(dryRun.command, process.execPath)
    assert.match(dryRun.args[0], /node_modules[\\/]@tauri-apps[\\/]cli[\\/]tauri\.js$/)
  } else {
    assert.equal(dryRun.command, 'tauri')
  }
  const tauriArgs = process.platform === 'win32' ? dryRun.args.slice(1) : dryRun.args
  assert.equal(tauriArgs[0], 'dev')
  assert.equal(dryRun.port, 5174)
  assert.equal(dryRun.productName, '5174')
  assert.equal(dryRun.identifier, 'com.aijia.app.dev.5174')

  const runnerIndex = tauriArgs.indexOf('--runner')
  assert.notEqual(runnerIndex, -1)
  const runnerPath = tauriArgs[runnerIndex + 1]
  const expectedRunnerSuffix =
    process.platform === 'win32'
      ? /scripts[\\/]tauri-dev-runner\.cmd$/
      : /scripts[\\/]tauri-dev-runner\.mjs$/
  assert.match(runnerPath, expectedRunnerSuffix)
})

test('Windows runner wrapper exists when the platform requires it', () => {
  if (process.platform !== 'win32') return

  assert.equal(existsSync(resolve(scriptDir, 'tauri-dev-runner.cmd')), true)
})

test('vite-dev dry-run avoids the pnpm command shim on Windows', () => {
  const result = spawnSync(process.execPath, [viteDevScript, '--port', '5174'], {
    cwd: projectDir,
    env: {
      ...process.env,
      AIJIA_VITE_DEV_DRY_RUN: '1',
    },
    encoding: 'utf8',
  })

  assert.equal(result.status, 0, result.stderr || result.stdout)

  const dryRun = parseDryRunJson(result.stdout)
  assert.equal(dryRun.port, 5174)
  if (process.platform === 'win32') {
    assert.equal(dryRun.command, process.execPath)
    assert.match(dryRun.args[0], /node_modules[\\/]vite[\\/]bin[\\/]vite\.js$/)
  } else {
    assert.equal(dryRun.command, 'pnpm')
    assert.deepEqual(dryRun.args.slice(0, 2), ['exec', 'vite'])
  }
})

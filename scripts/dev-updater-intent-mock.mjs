#!/usr/bin/env node
import { spawn } from 'node:child_process'
import { mkdir, readFile, writeFile } from 'node:fs/promises'
import http from 'node:http'
import path from 'node:path'
import process from 'node:process'
import { fileURLToPath } from 'node:url'

const __filename = fileURLToPath(import.meta.url)
const __dirname = path.dirname(__filename)
const root = path.resolve(__dirname, '..')
const baseTauriConfigPath = path.join(root, 'src-tauri', 'tauri.conf.json')
const testConfigPath = path.join(root, 'update.test.json')
const tempDir = path.join(root, '.runtime-cache')
const tempTauriConfigPath = path.join(tempDir, 'tauri.updater-intent.conf.json')

const PROD_UPDATE_URL = process.env.AIJIA_UPDATER_PROD_URL
  || 'https://lotus.renlijia.com/aijia/update.json'
const DEFAULT_PORT = Number(process.env.AIJIA_UPDATER_TEST_PORT || 18088)
const DEFAULT_POLL_MS = Number(process.env.AIJIA_UPDATER_TEST_POLL_MS || 30000)

const args = new Set(process.argv.slice(2))
const modeOnce = args.has('--once')
const modeTauri = args.has('--tauri')
const modeServeOnly = args.has('--serve-only') || (!modeOnce && !modeTauri)

function readArgNumber(prefix, fallback) {
  const arg = process.argv.slice(2).find((item) => item.startsWith(`${prefix}=`))
  if (!arg) return fallback
  const value = Number(arg.slice(prefix.length + 1))
  return Number.isFinite(value) && value > 0 ? value : fallback
}

const port = readArgNumber('--port', DEFAULT_PORT)
const pollMs = readArgNumber('--poll-ms', DEFAULT_POLL_MS)
const localBaseUrl = `http://127.0.0.1:${port}`
const localUpdateUrl = `${localBaseUrl}/update.json`
const noProxyValue = '127.0.0.1,localhost'

const validModes = ['old-ok', 'old-fail', 'new-ok', 'new-fail']
let mode = process.env.AIJIA_UPDATER_TEST_MODE || 'old-ok'
if (!validModes.includes(mode)) {
  throw new Error(`AIJIA_UPDATER_TEST_MODE must be one of ${validModes.join(', ')}`)
}

async function readJson(filePath) {
  return JSON.parse(await readFile(filePath, 'utf8'))
}

async function fetchProductionManifest() {
  const response = await fetch(PROD_UPDATE_URL, { cache: 'no-store' })
  if (!response.ok) {
    throw new Error(`fetch production update.json failed: ${response.status} ${response.statusText}`)
  }
  return response.json()
}

async function readTestConfig() {
  const raw = await readJson(testConfigPath)
  const config = {
    ...raw,
    oldVersion: process.env.AIJIA_UPDATER_OLD_VERSION || raw.oldVersion,
    newVersion: process.env.AIJIA_UPDATER_NEW_VERSION || raw.newVersion,
  }
  if (!config.oldVersion || !config.newVersion) {
    throw new Error('update.test.json must provide oldVersion and newVersion')
  }
  return config
}

function currentVersion(config) {
  return mode.startsWith('new') ? config.newVersion : config.oldVersion
}

function shouldFailDownload() {
  return mode.endsWith('fail')
}

async function buildManifest() {
  const [production, config] = await Promise.all([
    fetchProductionManifest(),
    readTestConfig(),
  ])

  if (!production.platforms || typeof production.platforms !== 'object') {
    throw new Error('production update.json has no platforms')
  }

  const version = currentVersion(config)
  const manifest = {
    ...production,
    version,
    notes: config.notes || production.notes || `AIjia updater mock ${version}`,
    pub_date: new Date().toISOString().replace(/\.\d{3}Z$/, 'Z'),
    platforms: Object.fromEntries(
      Object.entries(production.platforms).map(([key, value]) => [
        key,
        {
          ...value,
          url: shouldFailDownload() ? `${localBaseUrl}/packages/${version}/missing.tar.gz` : value.url,
        },
      ]),
    ),
  }

  return { production, config, manifest }
}

async function writeTauriConfig() {
  const base = await readJson(baseTauriConfigPath)
  const updater = base?.plugins?.updater
  if (!updater?.pubkey) {
    throw new Error('base tauri.conf.json has no updater pubkey')
  }

  await mkdir(tempDir, { recursive: true })
  await writeFile(
    tempTauriConfigPath,
    `${JSON.stringify({
      plugins: {
        updater: {
          ...updater,
          endpoints: [localUpdateUrl],
        },
      },
    }, null, 2)}\n`,
  )
  return tempTauriConfigPath
}

function sendJson(res, statusCode, value) {
  const body = `${JSON.stringify(value, null, 2)}\n`
  res.writeHead(statusCode, {
    'content-type': 'application/json; charset=utf-8',
    'cache-control': 'no-store',
    'access-control-allow-origin': '*',
  })
  res.end(body)
}

function parseControlMode(url) {
  const parsed = new URL(url, localBaseUrl)
  if (parsed.pathname.startsWith('/control/')) {
    return parsed.pathname.slice('/control/'.length)
  }
  if (parsed.pathname === '/control') {
    return parsed.searchParams.get('mode')
  }
  return null
}

async function startServer() {
  const initial = await buildManifest()
  console.log(`[updater-mock] prod version: ${initial.production.version}`)
  console.log(`[updater-mock] old version: ${initial.config.oldVersion}`)
  console.log(`[updater-mock] new version: ${initial.config.newVersion}`)
  console.log(`[updater-mock] mode: ${mode}`)

  const server = http.createServer(async (req, res) => {
    try {
      const parsed = new URL(req.url || '/', localBaseUrl)
      console.log(`[updater-mock] ${req.method || 'GET'} ${parsed.pathname}${parsed.search}`)
      const controlMode = parseControlMode(req.url || '/')
      if (controlMode) {
        if (!validModes.includes(controlMode)) {
          sendJson(res, 400, {
            ok: false,
            error: `mode must be one of ${validModes.join(', ')}`,
          })
          return
        }
        mode = controlMode
        const { production, manifest } = await buildManifest()
        sendJson(res, 200, statusPayload(production, manifest))
        return
      }

      if (parsed.pathname === '/' || parsed.pathname === '/status') {
        const { production, manifest } = await buildManifest()
        sendJson(res, 200, statusPayload(production, manifest))
        return
      }

      if (parsed.pathname === '/update.json') {
        const { manifest } = await buildManifest()
        sendJson(res, 200, manifest)
        return
      }

      if (parsed.pathname.startsWith('/packages/')) {
        res.writeHead(404, { 'content-type': 'text/plain; charset=utf-8' })
        res.end('mock package missing\n')
        return
      }

      res.writeHead(404, { 'content-type': 'text/plain; charset=utf-8' })
      res.end('not found\n')
    } catch (error) {
      sendJson(res, 500, { ok: false, error: String(error?.message ?? error) })
    }
  })

  await new Promise((resolve, reject) => {
    server.once('error', reject)
    server.listen(port, '127.0.0.1', resolve)
  })

  console.log(`[updater-mock] serving ${localUpdateUrl}`)
  console.log(`[updater-mock] controls:`)
  console.log(`  curl -sS ${localBaseUrl}/control/old-ok`)
  console.log(`  curl -sS ${localBaseUrl}/control/old-fail`)
  console.log(`  curl -sS ${localBaseUrl}/control/new-ok`)
  console.log(`  curl -sS ${localBaseUrl}/control/new-fail`)
  console.log(`[updater-mock] initial mode env: AIJIA_UPDATER_TEST_MODE=${mode}`)
  return server
}

function statusPayload(production, manifest) {
  return {
    ok: true,
    mode,
    productionVersion: production.version,
    servedVersion: manifest.version,
    updateUrl: localUpdateUrl,
    pollMs,
    hmrSafe: 'control endpoints change in-memory mock state only; no source file is modified during a scenario',
  }
}

async function runOnce() {
  const { production, manifest } = await buildManifest()
  console.log(JSON.stringify(statusPayload(production, manifest), null, 2))
}

async function runServeOnly() {
  await startServer()
  const configPath = await writeTauriConfig()
  console.log(`[updater-mock] tauri config: ${configPath}`)
  console.log('[updater-mock] to run app against this mock:')
  console.log(`  NO_PROXY=${noProxyValue} no_proxy=${noProxyValue} VITE_AIJIA_UPDATER_POLL_INTERVAL_MS=${pollMs} pnpm tauri dev --features e2e --config ${configPath}`)
}

async function runTauri() {
  const server = await startServer()
  const configPath = await writeTauriConfig()
  console.log(`[updater-mock] tauri config: ${configPath}`)
  console.log(`[updater-mock] poll interval: ${pollMs}ms`)

  const env = {
    ...process.env,
    NO_PROXY: noProxyValue,
    no_proxy: noProxyValue,
    VITE_AIJIA_UPDATER_POLL_INTERVAL_MS: String(pollMs),
  }
  const child = spawn(
    process.platform === 'win32' ? 'pnpm.cmd' : 'pnpm',
    ['tauri', 'dev', '--features', 'e2e', '--config', configPath],
    { cwd: root, env, stdio: 'inherit' },
  )

  const shutdown = () => {
    child.kill('SIGINT')
    server.close()
  }
  process.once('SIGINT', shutdown)
  process.once('SIGTERM', shutdown)

  await new Promise((resolve) => {
    child.once('exit', (code, signal) => {
      server.close()
      if (signal) console.log(`[updater-mock] tauri exited by ${signal}`)
      else console.log(`[updater-mock] tauri exited with code ${code}`)
      resolve()
    })
  })
}

if (modeOnce) {
  await runOnce()
} else if (modeTauri) {
  await runTauri()
} else if (modeServeOnly) {
  await runServeOnly()
}

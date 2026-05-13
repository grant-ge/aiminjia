#!/usr/bin/env node
// Upload unsigned Windows installer to OSS staging area where
// release-windows.ps1 pulls it from. Called by build-desktop.yml.
//
// Usage: node scripts/ci-upload-staging.mjs <version> <exe-path> <sig-path>
//
// Required env: OSS_ACCESS_KEY_ID / OSS_ACCESS_KEY_SECRET

import { existsSync, statSync } from 'node:fs'
import { spawnSync } from 'node:child_process'

const [, , version, exePath, sigPath] = process.argv
if (!version || !exePath) {
  console.error('Usage: node scripts/ci-upload-staging.mjs <version> <exe-path> <sig-path>')
  process.exit(1)
}
if (!existsSync(exePath)) {
  console.error(`ERROR: exe not found: ${exePath}`)
  process.exit(1)
}
if (!process.env.OSS_ACCESS_KEY_ID || !process.env.OSS_ACCESS_KEY_SECRET) {
  console.error('ERROR: OSS_ACCESS_KEY_ID / OSS_ACCESS_KEY_SECRET not set')
  process.exit(1)
}

let OSS
try {
  ({ default: OSS } = await import('ali-oss'))
} catch {
  console.log('[setup] installing ali-oss...')
  const r = spawnSync('npm', ['install', '--no-save', '--no-audit', '--no-fund', 'ali-oss@6'], {
    stdio: 'inherit', shell: process.platform === 'win32',
  })
  if (r.status !== 0) {
    console.error('ERROR: failed to install ali-oss')
    process.exit(1)
  }
  ;({ default: OSS } = await import('ali-oss'))
}

const client = new OSS({
  region: 'oss-cn-beijing',
  bucket: 'lotus-releases',
  accessKeyId: process.env.OSS_ACCESS_KEY_ID,
  accessKeySecret: process.env.OSS_ACCESS_KEY_SECRET,
  timeout: 3600 * 1000,
  secure: true,
})

const exeName = exePath.replace(/\\/g, '/').split('/').pop()
const stagingPrefix = `aijia/staging/unsigned/v${version}`
const exeKey = `${stagingPrefix}/${exeName}`
const sigKey = `${exeKey}.sig`

async function upload(localPath, remoteKey) {
  const size = statSync(localPath).size
  console.log(`[staging] ${(size / 1024 / 1024).toFixed(1)} MB -> ${remoteKey}`)
  if (size > 10 * 1024 * 1024) {
    await client.multipartUpload(remoteKey, localPath, {
      partSize: 5 * 1024 * 1024,
      parallel: 4,
    })
  } else {
    await client.put(remoteKey, localPath)
  }
}

try {
  await upload(exePath, exeKey)
  if (sigPath && existsSync(sigPath)) {
    await upload(sigPath, sigKey)
  }
  console.log(`\n[ok] staging URLs:`)
  console.log(`  https://lotus.renlijia.com/${exeKey}`)
  if (sigPath && existsSync(sigPath)) console.log(`  https://lotus.renlijia.com/${sigKey}`)
} catch (err) {
  console.error(`\nERROR: staging upload failed: ${err.message}`)
  if (err.code) console.error(`  OSS error code: ${err.code}`)
  process.exit(1)
}

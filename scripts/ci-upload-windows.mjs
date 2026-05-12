#!/usr/bin/env node
// Upload signed Windows installer + Tauri updater .sig to Aliyun OSS.
//
// Pure Node — no Python dependency. Uses the official `ali-oss` SDK.
//
// Usage:
//   node scripts/ci-upload-windows.mjs <version> <release|beta> <exe-path>
//
// Required env vars:
//   OSS_ACCESS_KEY_ID
//   OSS_ACCESS_KEY_SECRET
//
// The .sig file is expected at <exe-path>.sig and uploaded alongside.
// For release type, also copies to aijia/latest/windows-x64.

import { readFileSync, statSync, existsSync } from 'node:fs'
import { basename } from 'node:path'
import { spawnSync } from 'node:child_process'

const BUCKET = 'lotus-releases'
const REGION = 'oss-cn-beijing'
const PREFIX = 'aijia'

const [, , version, releaseType, exePath] = process.argv
if (!version || !releaseType || !exePath) {
  console.error('Usage: node scripts/ci-upload-windows.mjs <version> <beta|release> <exe-path>')
  process.exit(1)
}
if (releaseType !== 'beta' && releaseType !== 'release') {
  console.error(`ERROR: release type must be 'beta' or 'release', got: ${releaseType}`)
  process.exit(1)
}
if (!existsSync(exePath)) {
  console.error(`ERROR: exe not found: ${exePath}`)
  process.exit(1)
}
const sigPath = `${exePath}.sig`
if (!existsSync(sigPath)) {
  console.error(`ERROR: .sig not found: ${sigPath}`)
  console.error('  Generate it with: tauri signer sign -k ~/.tauri/aijia.key <exe>')
  process.exit(1)
}

const keyId = process.env.OSS_ACCESS_KEY_ID
const keySecret = process.env.OSS_ACCESS_KEY_SECRET
if (!keyId || !keySecret) {
  console.error('ERROR: OSS_ACCESS_KEY_ID / OSS_ACCESS_KEY_SECRET not set')
  process.exit(1)
}

// Lazy-install ali-oss into a local node_modules if not present.
// Avoids polluting the repo's package.json with a release-only dep.
let OSS
try {
  ({ default: OSS } = await import('ali-oss'))
} catch {
  console.log('[setup] ali-oss not installed in this project — installing locally...')
  const r = spawnSync('npm', ['install', '--no-save', '--no-audit', '--no-fund', 'ali-oss@6'], {
    stdio: 'inherit',
    shell: process.platform === 'win32',
  })
  if (r.status !== 0) {
    console.error('ERROR: failed to install ali-oss')
    process.exit(1)
  }
  ;({ default: OSS } = await import('ali-oss'))
}

const client = new OSS({
  region: REGION,
  bucket: BUCKET,
  accessKeyId: keyId,
  accessKeySecret: keySecret,
  // 1 hour timeout for large uploads on slow networks
  timeout: 3600 * 1000,
  secure: true,
})

const ossPrefix = releaseType === 'beta'
  ? `${PREFIX}/beta/v${version}`
  : `${PREFIX}/v${version}`
const exeKey = `${ossPrefix}/AIjia_${version}_x64-setup.exe`
const sigKey = `${exeKey}.sig`

async function uploadOne(localPath, remoteKey) {
  const size = statSync(localPath).size
  const sizeMB = (size / 1024 / 1024).toFixed(1)
  console.log(`[upload] ${basename(localPath)} (${sizeMB} MB) -> ${remoteKey}`)

  if (size > 10 * 1024 * 1024) {
    // Multipart for large files (resumable, parallel)
    await client.multipartUpload(remoteKey, localPath, {
      partSize: 5 * 1024 * 1024,
      parallel: 4,
      progress: (p) => process.stdout.write(`\r  progress ${(p * 100).toFixed(0)}%   `),
    })
    process.stdout.write('\n')
  } else {
    await client.put(remoteKey, localPath)
  }
}

try {
  await uploadOne(exePath, exeKey)
  await uploadOne(sigPath, sigKey)

  if (releaseType === 'release') {
    const latestKey = `${PREFIX}/latest/windows-x64`
    console.log(`[copy ] ${exeKey} -> ${latestKey}`)
    await client.copy(latestKey, exeKey, {
      headers: {
        'x-oss-metadata-directive': 'REPLACE',
        'Content-Type': 'application/octet-stream',
        'Content-Disposition': `attachment; filename="AIjia_${version}_x64-setup.exe"`,
      },
    })
  }

  console.log(`\n[ok] Windows v${version} (${releaseType}) uploaded to OSS`)
  console.log(`     https://lotus.renlijia.com/${exeKey}`)
} catch (err) {
  console.error(`\nERROR: upload failed: ${err.message}`)
  if (err.code) console.error(`  OSS error code: ${err.code}`)
  if (err.requestId) console.error(`  request id: ${err.requestId}`)
  process.exit(1)
}

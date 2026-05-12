#!/usr/bin/env node
// Delete OSS staging artifacts after a successful Windows release.
//
// Usage:  node scripts/ci-cleanup-staging.mjs <version>
//
// Required env: OSS_ACCESS_KEY_ID / OSS_ACCESS_KEY_SECRET

import { spawnSync } from 'node:child_process'

const [, , version] = process.argv
if (!version) {
  console.error('Usage: node scripts/ci-cleanup-staging.mjs <version>')
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
  spawnSync('npm', ['install', '--no-save', '--no-audit', '--no-fund', 'ali-oss@6'], {
    stdio: 'inherit', shell: process.platform === 'win32',
  })
  ;({ default: OSS } = await import('ali-oss'))
}

const client = new OSS({
  region: 'oss-cn-beijing',
  bucket: 'lotus-releases',
  accessKeyId: process.env.OSS_ACCESS_KEY_ID,
  accessKeySecret: process.env.OSS_ACCESS_KEY_SECRET,
  secure: true,
})

const prefix = `aijia/staging/unsigned/v${version}/`
const list = await client.list({ prefix, 'max-keys': 1000 })
const objects = list.objects ?? []
if (objects.length === 0) {
  console.log(`[ok] no staging files at ${prefix}`)
  process.exit(0)
}
for (const obj of objects) {
  console.log(`  delete ${obj.name}`)
  await client.delete(obj.name)
}
console.log(`[ok] removed ${objects.length} staging file(s)`)

#!/usr/bin/env node
// CI-only: strip the updater pubkey + disable updater artifacts so
// `tauri build` does not require TAURI_SIGNING_PRIVATE_KEY. Used by
// .github/workflows/release.yml for unsigned cross-platform build
// verification. Once the signing secret is configured in GitHub, remove
// this step from the workflow.

import { readFileSync, writeFileSync } from 'node:fs'

const path = 'src-tauri/tauri.conf.json'
const cfg = JSON.parse(readFileSync(path, 'utf8'))

if (cfg.plugins?.updater?.pubkey) {
  delete cfg.plugins.updater.pubkey
  console.log('[ci] removed plugins.updater.pubkey')
}
if (cfg.bundle) {
  cfg.bundle.createUpdaterArtifacts = false
  console.log('[ci] set bundle.createUpdaterArtifacts = false')
}

writeFileSync(path, JSON.stringify(cfg, null, 2) + '\n')
console.log('[ci] updater signing disabled for this build')

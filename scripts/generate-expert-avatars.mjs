// scripts/generate-expert-avatars.mjs
// One-shot generator: read team-team mapping from
// src/features/expert-teams/teams.ts, dump one SVG per (team, expert)
// into public/expert-avatars/<team>/<safeName>.svg.
//
// Run with: node scripts/generate-expert-avatars.mjs
//
// Output is committed to git so production runtime never needs network
// or DiceBear at all. Re-run only when adding new experts.

import { createAvatar } from '@dicebear/core'
import * as lorelei from '@dicebear/lorelei'
import { mkdirSync, writeFileSync, readFileSync } from 'node:fs'
import { resolve, dirname } from 'node:path'
import { fileURLToPath } from 'node:url'

const __dirname = dirname(fileURLToPath(import.meta.url))
const root = resolve(__dirname, '..')
const teamsPath = resolve(root, 'src/features/expert-teams/teams.ts')
const outRoot = resolve(root, 'public/expert-avatars')

// Cheap parser: extract expert `name` rows from EXPERT_TEAMS, grouping by
// surrounding `id: '<team>'` block.
// Not bulletproof but the file shape is stable.
const src = readFileSync(teamsPath, 'utf8')
const teamBlocks = src
  .split(/\{\s*\n\s*id: '/)
  .slice(1)
  .map((chunk) => {
    const id = chunk.split("'", 1)[0]
    const experts = []
    const expertsBody = chunk.match(/experts:\s*\[([\s\S]*?)\],\n\s*examples:/)?.[1] ?? ''
    const re = /\{\s*name:\s*'([^']+)'[\s\S]*?\}/g
    let m
    while ((m = re.exec(expertsBody))) experts.push({ name: m[1] })
    return { id, experts }
  })
  .filter((t) => t.experts.length > 0)

const roundtablePlaceholders = [
  { name: '动态专家一' },
  { name: '动态专家二' },
  { name: '动态专家三' },
]

function safe(name) {
  return name.replace(/[\\/<>:"|?*\s]/g, '_').replace(/^[._]+|[._]+$/g, '') || 'unnamed'
}

let total = 0
for (const team of teamBlocks) {
  const dir = resolve(outRoot, team.id)
  mkdirSync(dir, { recursive: true })
  for (const exp of team.experts) {
    const svg = createAvatar(lorelei, {
      seed: exp.name,
      size: 96,
      backgroundColor: ['transparent'],
    }).toString()
    const file = resolve(dir, `${safe(exp.name)}.svg`)
    writeFileSync(file, svg, 'utf8')
    total += 1
  }
}

const roundtableDir = resolve(outRoot, 'roundtable')
mkdirSync(roundtableDir, { recursive: true })
for (const exp of roundtablePlaceholders) {
  const svg = createAvatar(lorelei, {
    seed: exp.name,
    size: 96,
    backgroundColor: ['transparent'],
  }).toString()
  const file = resolve(roundtableDir, `${safe(exp.name)}.svg`)
  writeFileSync(file, svg, 'utf8')
  total += 1
}

console.log(`generated ${total} avatars across ${teamBlocks.length} teams → public/expert-avatars/`)

// Generate static SVG portraits for digital employee personas.
// Uses the same DiceBear lorelei style as expert team avatars.

import { createAvatar } from '@dicebear/core'
import * as lorelei from '@dicebear/lorelei'
import { mkdirSync, writeFileSync } from 'node:fs'
import { resolve, dirname } from 'node:path'
import { fileURLToPath } from 'node:url'

const __dirname = dirname(fileURLToPath(import.meta.url))
const root = resolve(__dirname, '..')
const outRoot = resolve(root, 'public/employee-avatars')

const names = [
  '林知远',
  '陈景律',
  '周思齐',
  '许嘉宁',
  '丁若安',
  '赵明川',
  '何予周',
  '沈柏川',
  '顾承远',
  '韩可欣',
  '程砚舟',
  '方予衡',
  '陆时安',
  '秦砚知',
  '温嘉言',
  '梁承序',
  '何远策',
  '唐识衡',
]

function safe(name) {
  return name.replace(/[\\/<>:"|?*\s]/g, '_').replace(/^[._]+|[._]+$/g, '') || 'unnamed'
}

mkdirSync(outRoot, { recursive: true })
for (const name of names) {
  const svg = createAvatar(lorelei, {
    seed: name,
    size: 96,
    backgroundColor: ['transparent'],
  }).toString()
  writeFileSync(resolve(outRoot, `${safe(name)}.svg`), svg, 'utf8')
}

console.log(`generated ${names.length} employee avatars -> public/employee-avatars/`)

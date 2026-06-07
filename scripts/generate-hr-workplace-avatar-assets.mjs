// Generate HR workplace avatar assets using the same DiceBear personas style
// as the existing digital employee and expert-team avatars.

import { createAvatar } from '@dicebear/core'
import * as personas from '@dicebear/personas'
import { mkdirSync, writeFileSync } from 'node:fs'
import { resolve, dirname } from 'node:path'
import { fileURLToPath } from 'node:url'

const __dirname = dirname(fileURLToPath(import.meta.url))
const root = resolve(__dirname, '..')

const employeeOutRoot = resolve(root, 'public/employee-avatars')
const expertOutRoot = resolve(root, 'public/expert-avatars/hr-workplace')

export const HR_EMPLOYEE_AVATARS = [
  { key: 'organization-expert', name: '梁承序' },
  { key: 'salary-expert', name: '方予衡' },
  { key: 'workforce-planning-expert', name: '何远策' },
  { key: 'attendance-expert', name: '陆时安' },
  { key: 'performance-expert', name: '秦砚知' },
  { key: 'employee-relations-expert', name: '温嘉言' },
  { key: 'talent-review-expert', name: '唐识衡' },
]

export const HR_EXPERT_AVATARS = [
  { stableName: 'recruiting-lead', name: '宋知澜' },
  { stableName: 'hiring-manager', name: '陆承川' },
  { stableName: 'interview-coach', name: '唐砚宁' },
  { stableName: 'talent-researcher', name: '赵明川' },
  { stableName: 'compensation-expert', name: '方予衡' },
  { stableName: 'performance-advisor', name: '秦砚知' },
  { stableName: 'hrbp-care', name: '温嘉言' },
  { stableName: 'legal-advisor', name: '陈景律' },
  { stableName: 'od-advisor', name: '梁承序' },
  { stableName: 'hrbp-planning', name: '何远策' },
  { stableName: 'talent-reviewer', name: '唐识衡' },
  { stableName: 'people-analyst', name: '周思齐' },
]

const TILE = 96
const COLUMNS = 4

function safe(name) {
  return name.replace(/[\\/<>:"|?*\s]/g, '_').replace(/^[._]+|[._]+$/g, '') || 'unnamed'
}

function avatarSvg(seed) {
  return createAvatar(personas, {
    seed,
    size: TILE,
    backgroundColor: ['transparent'],
  }).toString()
}

mkdirSync(employeeOutRoot, { recursive: true })
for (const item of HR_EMPLOYEE_AVATARS) {
  writeFileSync(resolve(employeeOutRoot, `${safe(item.name)}.svg`), avatarSvg(item.name), 'utf8')
}

mkdirSync(expertOutRoot, { recursive: true })
const rows = Math.ceil(HR_EXPERT_AVATARS.length / COLUMNS)
const atlasWidth = COLUMNS * TILE
const atlasHeight = rows * TILE
const tiles = HR_EXPERT_AVATARS.map((item, index) => {
  const x = (index % COLUMNS) * TILE
  const y = Math.floor(index / COLUMNS) * TILE
  const svg = avatarSvg(item.name)
    .replace(/<svg\b/, `<svg x="${x}" y="${y}"`)
    .replace(/\swidth="96"\sheight="96"/, ` width="${TILE}" height="${TILE}"`)
  return `  ${svg}`
})

const atlas = [
  `<svg xmlns="http://www.w3.org/2000/svg" width="${atlasWidth}" height="${atlasHeight}" viewBox="0 0 ${atlasWidth} ${atlasHeight}" fill="none">`,
  ...tiles,
  '</svg>',
  '',
].join('\n')

writeFileSync(resolve(expertOutRoot, 'avatar-atlas.svg'), atlas, 'utf8')
writeFileSync(
  resolve(expertOutRoot, 'avatar-atlas.json'),
  JSON.stringify(
    {
      tile: TILE,
      columns: COLUMNS,
      atlasWidth,
      atlasHeight,
      avatars: HR_EXPERT_AVATARS.map((item, index) => ({
        ...item,
        x: (index % COLUMNS) * TILE,
        y: Math.floor(index / COLUMNS) * TILE,
        w: TILE,
        h: TILE,
      })),
    },
    null,
    2,
  ) + '\n',
  'utf8',
)

console.log(`generated ${HR_EMPLOYEE_AVATARS.length} HR employee avatars`)
console.log(`generated ${HR_EXPERT_AVATARS.length} HR expert avatars -> public/expert-avatars/hr-workplace/avatar-atlas.svg`)

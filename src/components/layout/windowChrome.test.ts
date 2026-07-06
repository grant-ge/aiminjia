import fs from 'node:fs'
import path from 'node:path'

import { describe, expect, it } from 'vitest'

const ROOTS = ['src/components', 'src/features']
const EXEMPT_FILES = new Set([
  path.normalize('src/components/layout/TitleBar.tsx'),
])

function walkSourceFiles(dir: string): string[] {
  const files: string[] = []
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    const fullPath = path.join(dir, entry.name)
    if (entry.isDirectory()) {
      if (entry.name === '__tests__') continue
      files.push(...walkSourceFiles(fullPath))
      continue
    }
    if (!/\.(tsx|ts)$/.test(entry.name)) continue
    if (/\.test\.[^.]+$/.test(entry.name)) continue
    files.push(fullPath)
  }
  return files
}

describe('window chrome drag regions', () => {
  it('uses the shared double-click handler for content drag regions', () => {
    const offenders: string[] = []

    for (const root of ROOTS) {
      for (const file of walkSourceFiles(root)) {
        const normalized = path.normalize(file)
        if (EXEMPT_FILES.has(normalized)) continue

        const source = fs.readFileSync(file, 'utf8')
        if (!source.includes('data-tauri-drag-region')) continue
        if (source.includes('handleChromeDragRegionMouseDown')) continue

        offenders.push(path.relative(process.cwd(), file))
      }
    }

    expect(offenders).toEqual([])
  })
})

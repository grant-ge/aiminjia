import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { describe, expect, it } from 'vitest'

const APP_SOURCE = readFileSync(resolve(process.cwd(), 'src/App.tsx'), 'utf8')
const SIDEBAR_SOURCE = readFileSync(resolve(process.cwd(), 'src/components/sidebar/AppSidebar.tsx'), 'utf8')

describe('App shell layout', () => {
  it('keeps the main/sider separator owned by the rounded main surface', () => {
    expect(APP_SOURCE).toContain('flex min-h-0 flex-1 bg-sidebar')
    expect(APP_SOURCE).toContain('rounded-l-lg border-l border-t border-border bg-background')
    expect(APP_SOURCE).not.toContain('shadow-sidebar-edge')
    expect(SIDEBAR_SOURCE).not.toContain('border-r border-sidebar-border')
  })
})

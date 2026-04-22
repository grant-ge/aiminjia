import { describe, expect, it } from 'vitest'
import fs from 'node:fs'
import path from 'node:path'

const CSS = fs.readFileSync(
  path.resolve(__dirname, '../globals.css'),
  'utf8',
)

function tokenValue(name: string): string | null {
  const m = CSS.match(new RegExp(`${name}\\s*:\\s*([^;]+);`))
  return m ? m[1].trim() : null
}

describe('design.pen token alignment', () => {
  it.each([
    ['--background', '#fafafa'],
    ['--foreground', '#0a0a0a'],
    ['--card', '#fafafa'],
    ['--border', '#e5e5e5'],
    ['--input', '#e5e5e5'],
    ['--muted', '#f5f5f5'],
    ['--muted-foreground', '#737373'],
    ['--popover', '#fafafa'],
    ['--secondary', '#f5f5f5'],
    ['--primary', '#DBAA22'],
    ['--primary-foreground', '#FFFFFF'],
    ['--brand-primary-subtle', '#FBF3DC'],
    ['--brand-secondary', '#3F3F46'],
    ['--brand-secondary-subtle', '#F3F4F6'],
    ['--ring', '#DBAA22'],
    ['--sidebar', '#F4F0E6'],
    ['--sidebar-accent', '#E1DAC6'],
    ['--sidebar-border', '#E1DAC6'],
    ['--sidebar-primary', '#DBAA22'],
    ['--sidebar-primary-foreground', '#FFFFFF'],
    ['--destructive', '#e7000b'],
  ])('token %s equals %s (design.pen)', (name, expected) => {
    const value = tokenValue(name)
    expect(value?.toLowerCase()).toBe(expected.toLowerCase())
  })
})

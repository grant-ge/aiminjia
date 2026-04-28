import { describe, expect, it } from 'vitest'
import fs from 'node:fs'
import path from 'node:path'

const CSS = fs.readFileSync(
  path.resolve(__dirname, '../globals.css'),
  'utf8',
)

function tokenValue(name: string): string | null {
  const m = CSS.match(new RegExp(`${name}\\s*:\\s*([^;]+);`))
  if (!m) return null
  const raw = m[1].trim()
  const varRef = raw.match(/^var\((--[\w-]+)\)$/)
  if (varRef) return tokenValue(varRef[1])
  return raw
}

describe('design.pen token alignment', () => {
  it('sets the root rem baseline to 16px', () => {
    expect(CSS).toMatch(/html\s*\{[^}]*font-size:\s*16px;/s)
  })

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
    ['--sidebar', '#f7f7f7'],
    ['--sidebar-accent', '#ebebeb'],
    ['--sidebar-border', '#ebebeb'],
    ['--sidebar-primary', '#DBAA22'],
    ['--sidebar-primary-foreground', '#FFFFFF'],
    ['--sidebar-accent-foreground', '#18181b'],
    ['--destructive', '#e7000b'],
  ])('token %s equals %s (design.pen)', (name, expected) => {
    const value = tokenValue(name)
    expect(value?.toLowerCase()).toBe(expected.toLowerCase())
  })

  it('sets pointer cursor for enabled buttons globally', () => {
    expect(CSS).toMatch(/button:not\(\s*:disabled\s*\)\s*\{[^}]*cursor:\s*pointer;/s)
  })

  it('keeps font sizing on rem-compatible scales instead of fixed px rules', () => {
    const cssWithoutRootBaseline = CSS.replace(/html\s*\{[^}]*\}/s, '')
    expect(cssWithoutRootBaseline).not.toMatch(/(?<!-)font-size:\s*[0-9.]+px;/)
  })
})


describe('assistant markdown typography', () => {
  it('restores markdown heading, list, and rich text styles inside the assistant scope', () => {
    expect(CSS).toMatch(/\.assistant-markdown h1[\s\S]*font-size:\s*1\.45em;/)
    expect(CSS).toMatch(/\.assistant-markdown h2[\s\S]*font-size:\s*1\.28em;/)
    expect(CSS).toMatch(/\.assistant-markdown ul\s*\{[\s\S]*list-style-type:\s*disc;/)
    expect(CSS).toMatch(/\.assistant-markdown ol\s*\{[\s\S]*list-style-type:\s*decimal;/)
    expect(CSS).toMatch(/\.assistant-markdown blockquote\s*\{[\s\S]*border-left:/)
    expect(CSS).toMatch(/\.assistant-markdown :not\(pre\) > code\s*\{[\s\S]*background:\s*var\(--color-bg-code\);/)
    expect(CSS).toMatch(/\.assistant-markdown \.markdown-table-wrap,[\s\S]*margin-top:\s*20px;[\s\S]*margin-bottom:\s*12px;/)
    expect(CSS).toMatch(/\.assistant-markdown \.markdown-table-copy\s*\{[\s\S]*display:\s*inline-flex;/)
    expect(CSS).toMatch(/\.assistant-markdown \.markdown-table-copy\s*\{[\s\S]*font-size:\s*inherit;/)
    expect(CSS).not.toMatch(/\.assistant-markdown \.markdown-table-copy\s*\{[\s\S]*font-size:\s*15px;/)
    expect(CSS).toMatch(/\.assistant-markdown \.markdown-table-scroll > table\s*\{[\s\S]*border-collapse:\s*collapse;/)
    expect(CSS).toMatch(new RegExp('\\.assistant-markdown \\.markdown-table-scroll th,\\s*\\.assistant-markdown \\.markdown-table-scroll td\\s*\\{[\\s\\S]*height:\\s*40px;'))
    expect(CSS).not.toMatch(/^\.assistant-markdown table\s*\{/m)
    expect(CSS).not.toContain(':has(')
  })
})

import '@testing-library/jest-dom'
import { fireEvent, render, screen } from '@testing-library/react'
import { Search } from 'lucide-react'
import fs from 'node:fs'
import path from 'node:path'
import * as ts from 'typescript'
import { describe, expect, it, vi } from 'vitest'

import { Button } from '../button'

describe('Button', () => {
  it('uses the compact md size by default', () => {
    render(<Button>派活</Button>)

    const button = screen.getByRole('button', { name: '派活' })
    expect(button).toHaveClass('inline-flex')
    expect(button).toHaveClass('h-8')
    expect(button).not.toHaveClass('h-[30px]')
    expect(button).toHaveClass('px-[15px]')
    expect(button).toHaveClass('rounded-md')
    expect(button).toHaveClass('transition-colors')
    expect(button).not.toHaveClass('hover:opacity-90')
    expect(button).not.toHaveClass('active:scale-[0.98]')
    expect(button).not.toHaveClass('w-full')
  })

  it('supports lg and sm sizing', () => {
    render(
      <>
        <Button size="lg">大</Button>
        <Button size="sm">小</Button>
      </>,
    )

    expect(screen.getByRole('button', { name: '大' })).toHaveClass('h-10', 'px-[15px]')
    expect(screen.getByRole('button', { name: '小' })).toHaveClass('h-6', 'px-[7px]', 'rounded')
    expect(screen.getByRole('button', { name: '小' })).not.toHaveClass('rounded-md')
  })

  it('renders block buttons as a full row', () => {
    render(<Button block>整行</Button>)

    const button = screen.getByRole('button', { name: '整行' })
    expect(button).toHaveClass('flex')
    expect(button).toHaveClass('w-full')
    expect(button).not.toHaveClass('inline-flex')
  })

  it('renders an icon before text and icon-only buttons as squares', () => {
    render(
      <>
        <Button icon={<Search data-testid="search-icon" />}>搜索</Button>
        <Button suffixIcon={<Search data-testid="suffix-search-icon" />}>新建</Button>
        <Button aria-label="仅搜索" icon={<Search data-testid="only-icon" />} />
        <Button size="lg" aria-label="大搜索" icon={<Search />} />
        <Button size="sm" aria-label="小搜索" icon={<Search />} />
      </>,
    )

    expect(screen.getByRole('button', { name: '搜索' })).toHaveClass('gap-1.5')
    expect(screen.getByRole('button', { name: '新建' })).toHaveClass('gap-1.5')
    expect(screen.getByTestId('search-icon')).toHaveClass('h-4', 'w-4')
    expect(screen.getByTestId('suffix-search-icon')).toHaveClass('h-4', 'w-4')
    expect(screen.getByRole('button', { name: '仅搜索' })).toHaveClass('h-8', 'w-8', 'p-0')
    expect(screen.getByRole('button', { name: '仅搜索' })).not.toHaveClass('h-[30px]', 'w-[30px]')
    expect(screen.getByRole('button', { name: '大搜索' })).toHaveClass('h-10', 'w-10', 'p-0')
    expect(screen.getByRole('button', { name: '小搜索' })).toHaveClass('h-6', 'w-6', 'p-0')
  })

  it('supports danger, disabled, loading, and link modes', () => {
    const onClick = vi.fn()
    render(
      <>
        <Button danger>删除</Button>
        <Button disabled onClick={onClick}>禁用</Button>
        <Button loading onClick={onClick}>保存</Button>
        <Button link>查看详情</Button>
      </>,
    )

    expect(screen.getByRole('button', { name: '删除' })).toHaveClass('bg-destructive', 'text-destructive-foreground')

    const disabled = screen.getByRole('button', { name: '禁用' })
    fireEvent.click(disabled)
    expect(disabled).toBeDisabled()
    expect(onClick).not.toHaveBeenCalled()

    const loading = screen.getByRole('button', { name: '保存' })
    fireEvent.click(loading)
    expect(loading).toBeDisabled()
    expect(loading).toHaveAttribute('aria-busy', 'true')
    expect(onClick).not.toHaveBeenCalled()

    const link = screen.getByRole('button', { name: '查看详情' })
    expect(link).toHaveClass('h-auto', 'p-0', 'border-transparent', 'bg-transparent')
    expect(link).not.toHaveClass('h-8')
    expect(link).not.toHaveClass('hover:opacity-90')
  })

  it('supports unstyled mode for custom interactive surfaces', () => {
    render(<Button unstyled className="flex h-12 rounded-xl px-4">自定义卡片</Button>)

    const button = screen.getByRole('button', { name: '自定义卡片' })
    expect(button).toHaveClass('flex', 'h-12', 'rounded-xl', 'px-4')
    expect(button).not.toHaveClass('h-8')
    expect(button).not.toHaveClass('border-primary')
  })

  it('keeps standard Button icons in explicit icon props', () => {
    const roots = ['src/components', 'src/features']
    const files: string[] = []
    const visit = (dir: string) => {
      for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
        const fullPath = path.join(dir, entry.name)
        if (entry.isDirectory()) {
          visit(fullPath)
          continue
        }
        if (/\.tsx$/.test(entry.name) && !/(\.test|__tests__)/.test(fullPath)) files.push(fullPath)
      }
    }
    roots.forEach(visit)

    const getJsxName = (name: ts.JsxTagNameExpression): string => {
      if (ts.isIdentifier(name)) return name.text
      if (ts.isPropertyAccessExpression(name)) return name.name.text
      return ''
    }
    const hasJsxAttr = (attrs: ts.JsxAttributes, attrName: string): boolean =>
      attrs.properties.some((prop) => ts.isJsxAttribute(prop) && ts.isIdentifier(prop.name) && prop.name.text === attrName)
    const lucideIconImports = (sourceFile: ts.SourceFile): Set<string> => {
      const icons = new Set<string>()
      for (const statement of sourceFile.statements) {
        if (
          !ts.isImportDeclaration(statement) ||
          !ts.isStringLiteral(statement.moduleSpecifier) ||
          statement.moduleSpecifier.text !== 'lucide-react'
        ) {
          continue
        }
        const elements = statement.importClause?.namedBindings
        if (!elements || !ts.isNamedImports(elements)) continue
        for (const element of elements.elements) {
          icons.add((element.propertyName ?? element.name).text)
        }
      }
      return icons
    }
    const containsLucideIcon = (node: ts.Node, icons: Set<string>): boolean => {
      if (ts.isJsxElement(node)) return icons.has(getJsxName(node.openingElement.tagName))
      if (ts.isJsxSelfClosingElement(node)) return icons.has(getJsxName(node.tagName))
      let found = false
      ts.forEachChild(node, (child) => {
        if (containsLucideIcon(child, icons)) found = true
      })
      return found
    }

    const offenders: string[] = []
    for (const file of files) {
      const source = fs.readFileSync(file, 'utf8')
      const sourceFile = ts.createSourceFile(file, source, ts.ScriptTarget.Latest, true, ts.ScriptKind.TSX)
      const icons = lucideIconImports(sourceFile)
      if (icons.size === 0) continue

      const visitNode = (node: ts.Node) => {
        if (ts.isJsxElement(node) && getJsxName(node.openingElement.tagName) === 'Button') {
          const attrs = node.openingElement.attributes
          if (!hasJsxAttr(attrs, 'unstyled')) {
            for (const child of node.children) {
              if (!containsLucideIcon(child, icons)) continue
              const pos = sourceFile.getLineAndCharacterOfPosition(child.getStart(sourceFile))
              offenders.push(`${file}:${pos.line + 1}`)
            }
          }
        }
        ts.forEachChild(node, visitNode)
      }
      visitNode(sourceFile)
    }

    expect(offenders).toEqual([])
  })

  it('keeps app source from using raw button elements outside the Button primitive', () => {
    const roots = ['src/components', 'src/features']
    const files: string[] = []
    const visit = (dir: string) => {
      for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
        const fullPath = path.join(dir, entry.name)
        if (entry.isDirectory()) {
          visit(fullPath)
          continue
        }
        if (/\.(tsx|ts)$/.test(entry.name)) files.push(fullPath)
      }
    }
    roots.forEach(visit)

    const allowRawButtonFiles = new Set([
      path.normalize('src/components/ui/button.tsx'),
    ])
    const offenders = files
      .filter((file) => !/(\.test|__tests__)/.test(file))
      .filter((file) => !allowRawButtonFiles.has(path.normalize(file)))
      .flatMap((file) => {
        const source = fs.readFileSync(file, 'utf8')
        return [...source.matchAll(/<button\b/g)].map((match) => `${file}:${source.slice(0, match.index).split('\n').length}`)
      })

    expect(offenders).toEqual([])
  })

  it('keeps call sites from overriding Button-owned sizing and shape styles', () => {
    const roots = ['src/components', 'src/features']
    const files: string[] = []
    const visit = (dir: string) => {
      for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
        const fullPath = path.join(dir, entry.name)
        if (entry.isDirectory()) {
          visit(fullPath)
          continue
        }
        if (/\.(tsx|ts)$/.test(entry.name)) files.push(fullPath)
      }
    }
    roots.forEach(visit)

    const readOpeningTag = (source: string, start: number): string => {
      let quote: string | null = null
      let braceDepth = 0
      for (let i = start; i < source.length; i += 1) {
        const char = source[i]
        const prev = source[i - 1]
        if (quote) {
          if (char === quote && prev !== '\\') quote = null
          continue
        }
        if (char === '"' || char === "'" || char === '`') {
          quote = char
          continue
        }
        if (char === '{') {
          braceDepth += 1
          continue
        }
        if (char === '}') {
          braceDepth = Math.max(0, braceDepth - 1)
          continue
        }
        if (char === '>' && braceDepth === 0) {
          return source.slice(start, i + 1)
        }
      }
      return source.slice(start)
    }

    const classNameSources = (tag: string): string[] => {
      const values: string[] = []
      let attrIndex = -1
      let quote: string | null = null
      let braceDepth = 0
      for (let i = 0; i < tag.length; i += 1) {
        const char = tag[i]
        const prev = tag[i - 1]
        if (quote) {
          if (char === quote && prev !== '\\') quote = null
          continue
        }
        if (char === '"' || char === "'" || char === '`') {
          quote = char
          continue
        }
        if (char === '{') {
          braceDepth += 1
          continue
        }
        if (char === '}') {
          braceDepth = Math.max(0, braceDepth - 1)
          continue
        }
        if (braceDepth === 0 && tag.startsWith('className=', i)) {
          attrIndex = i
          break
        }
      }
      if (attrIndex < 0) return values
      const rest = tag.slice(attrIndex + 'className='.length)
      const first = rest.trimStart()[0]
      if (first === '"' || first === "'") {
        const quote = first
        const raw = rest.trimStart().slice(1)
        values.push(raw.slice(0, raw.indexOf(quote)))
        return values
      }
      if (first === '{') {
        const raw = rest.trimStart().slice(1)
        let quote: string | null = null
        let depth = 0
        for (let i = 0; i < raw.length; i += 1) {
          const char = raw[i]
          const prev = raw[i - 1]
          if (quote) {
            if (char === quote && prev !== '\\') quote = null
            continue
          }
          if (char === '"' || char === "'" || char === '`') {
            quote = char
            continue
          }
          if (char === '{' || char === '(' || char === '[') depth += 1
          if (char === '}' || char === ')' || char === ']') {
            if (depth === 0 && char === '}') {
              values.push(raw.slice(0, i))
              return values
            }
            depth = Math.max(0, depth - 1)
          }
        }
      }
      return values
    }

    const offenders: string[] = []
    const forbidden = /\b(?:h-\d+|min-h-\d+|px-\S+|py-\S+|p-\d+|rounded-\S+|text-xs|text-sm|font-semibold|font-medium)\b/
    for (const file of files) {
      const source = fs.readFileSync(file, 'utf8')
      const matches = source.matchAll(/<Button\b/g)
      for (const match of matches) {
        const tag = readOpeningTag(source, match.index ?? 0)
        const classSource = classNameSources(tag).join(' ')
        if (!/\bunstyled\b/.test(tag) && forbidden.test(classSource)) {
          offenders.push(`${file}: ${classSource}`)
        }
      }
    }

    expect(offenders).toEqual([])
  })

  it('keeps small unstyled Button surfaces on the 4px radius tier', () => {
    const roots = ['src/components', 'src/features']
    const files: string[] = []
    const visit = (dir: string) => {
      for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
        const fullPath = path.join(dir, entry.name)
        if (entry.isDirectory()) {
          visit(fullPath)
          continue
        }
        if (/\.(tsx|ts)$/.test(entry.name)) files.push(fullPath)
      }
    }
    roots.forEach(visit)

    const readOpeningTag = (source: string, start: number): string => {
      let quote: string | null = null
      let braceDepth = 0
      for (let i = start; i < source.length; i += 1) {
        const char = source[i]
        const prev = source[i - 1]
        if (quote) {
          if (char === quote && prev !== '\\') quote = null
          continue
        }
        if (char === '"' || char === "'" || char === '`') {
          quote = char
          continue
        }
        if (char === '{') {
          braceDepth += 1
          continue
        }
        if (char === '}') {
          braceDepth = Math.max(0, braceDepth - 1)
          continue
        }
        if (char === '>' && braceDepth === 0) {
          return source.slice(start, i + 1)
        }
      }
      return source.slice(start)
    }

    const offenders: string[] = []
    const smallButtonClass = /\b(?:h-[4567]|w-[4567]|px-1\.5|py-0\.5|p-0\.5|text-xs|text-\[11px\]|text-\[10px\])\b|px-2 py-1(?!\.)/
    for (const file of files) {
      const source = fs.readFileSync(file, 'utf8')
      for (const match of source.matchAll(/<Button\b/g)) {
        const tag = readOpeningTag(source, match.index ?? 0)
        if (/\bunstyled\b/.test(tag) && /\brounded-md\b/.test(tag) && smallButtonClass.test(tag)) {
          offenders.push(`${file}:${source.slice(0, match.index).split('\n').length}`)
        }
      }
    }

    expect(offenders).toEqual([])
  })
})

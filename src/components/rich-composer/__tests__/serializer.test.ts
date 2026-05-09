// src/components/rich-composer/__tests__/serializer.test.ts
import { describe, expect, it } from 'vitest'
import { serializeComposerDoc } from '../serializer'
import type { ComposerJsonNode } from '../types'

const doc = (...content: ComposerJsonNode[]): ComposerJsonNode => ({ type: 'doc', content })
const p = (...content: ComposerJsonNode[]): ComposerJsonNode => ({ type: 'paragraph', content })
const t = (text: string, marks?: ComposerJsonNode['marks']): ComposerJsonNode => ({
  type: 'text',
  text,
  marks,
})

describe('serializeComposerDoc — 空 / 纯文本', () => {
  it('空 doc → markdown=""，isEmpty=true', () => {
    const result = serializeComposerDoc(doc())
    expect(result).toEqual({ markdown: '', attachments: [], isEmpty: true })
  })

  it('空 paragraph → markdown=""，isEmpty=true', () => {
    const result = serializeComposerDoc(doc(p()))
    expect(result).toEqual({ markdown: '', attachments: [], isEmpty: true })
  })

  it('单段纯文本 → 原样输出，isEmpty=false', () => {
    const result = serializeComposerDoc(doc(p(t('hello world'))))
    expect(result).toEqual({ markdown: 'hello world', attachments: [], isEmpty: false })
  })

  it('多段段落用 \\n\\n 分隔', () => {
    const result = serializeComposerDoc(doc(p(t('first')), p(t('second'))))
    expect(result.markdown).toBe('first\n\nsecond')
  })

  it('hardBreak → 行尾两空格 + \\n', () => {
    const result = serializeComposerDoc(
      doc(p(t('line1'), { type: 'hardBreak' }, t('line2')))
    )
    expect(result.markdown).toBe('line1  \nline2')
  })
})

describe('serializeComposerDoc — inline marks', () => {
  it('bold → **text**', () => {
    const result = serializeComposerDoc(doc(p(t('hi', [{ type: 'bold' }]))))
    expect(result.markdown).toBe('**hi**')
  })

  it('italic → *text*', () => {
    const result = serializeComposerDoc(doc(p(t('hi', [{ type: 'italic' }]))))
    expect(result.markdown).toBe('*hi*')
  })

  it('strike → ~~text~~', () => {
    const result = serializeComposerDoc(doc(p(t('hi', [{ type: 'strike' }]))))
    expect(result.markdown).toBe('~~hi~~')
  })

  it('inline code → `text`，且 code 内不被 italic 包', () => {
    const result = serializeComposerDoc(
      doc(p(t('x', [{ type: 'code' }, { type: 'italic' }])))
    )
    // code 是最内层，italic 在外
    expect(result.markdown).toBe('*`x`*')
  })

  it('link → [text](url)', () => {
    const result = serializeComposerDoc(
      doc(p(t('点这里', [{ type: 'link', attrs: { href: 'https://example.com' } }])))
    )
    expect(result.markdown).toBe('[点这里](https://example.com)')
  })

  it('link 包 bold：bold 在内、link 在外', () => {
    const result = serializeComposerDoc(
      doc(
        p(
          t('hi', [
            { type: 'bold' },
            { type: 'link', attrs: { href: 'https://example.com' } },
          ])
        )
      )
    )
    expect(result.markdown).toBe('[**hi**](https://example.com)')
  })

  it('混合 marks：code + bold + italic + strike + link', () => {
    const result = serializeComposerDoc(
      doc(
        p(
          t('x', [
            { type: 'code' },
            { type: 'bold' },
            { type: 'italic' },
            { type: 'strike' },
            { type: 'link', attrs: { href: 'https://e.com' } },
          ])
        )
      )
    )
    // 由内到外：code → bold → italic → strike → link
    expect(result.markdown).toBe('[~~***`x`***~~](https://e.com)')
  })

  it('text containing ~~ is escaped so it does not render as strike', () => {
    const result = serializeComposerDoc(doc(p(t('hello ~~world~~'))))
    expect(result.markdown).toBe('hello \\~\\~world\\~\\~')
  })
})

describe('serializeComposerDoc — markdown 特殊字符 escape', () => {
  it('escape * _ ` [ ] \\ < >', () => {
    const result = serializeComposerDoc(
      doc(p(t('a*b_c`d[e]f\\g<h>')))
    )
    expect(result.markdown).toBe('a\\*b\\_c\\`d\\[e\\]f\\\\g\\<h\\>')
  })

  it('inline code mark 内部不 escape markdown 特殊字符', () => {
    const result = serializeComposerDoc(
      doc(p(t('a*b', [{ type: 'code' }])))
    )
    expect(result.markdown).toBe('`a*b`')
  })
})

describe('serializeComposerDoc — 块级节点', () => {
  const blockquote = (...content: ComposerJsonNode[]): ComposerJsonNode => ({
    type: 'blockquote',
    content,
  })
  const codeBlock = (text: string, language?: string): ComposerJsonNode => ({
    type: 'codeBlock',
    attrs: language ? { language } : undefined,
    content: [{ type: 'text', text }],
  })
  const ul = (...items: ComposerJsonNode[]): ComposerJsonNode => ({
    type: 'bulletList',
    content: items,
  })
  const ol = (...items: ComposerJsonNode[]): ComposerJsonNode => ({
    type: 'orderedList',
    content: items,
  })
  const li = (...content: ComposerJsonNode[]): ComposerJsonNode => ({
    type: 'listItem',
    content,
  })

  it('blockquote 单段 → 行首 > ', () => {
    const result = serializeComposerDoc(doc(blockquote(p(t('hello')))))
    expect(result.markdown).toBe('> hello')
  })

  it('blockquote 多段 → 每行 > 前缀，段间 > 空行', () => {
    const result = serializeComposerDoc(doc(blockquote(p(t('a')), p(t('b')))))
    expect(result.markdown).toBe('> a\n>\n> b')
  })

  it('codeBlock 带 language', () => {
    const result = serializeComposerDoc(doc(codeBlock('let x = 1', 'ts')))
    expect(result.markdown).toBe('```ts\nlet x = 1\n```')
  })

  it('codeBlock 无 language', () => {
    const result = serializeComposerDoc(doc(codeBlock('plain')))
    expect(result.markdown).toBe('```\nplain\n```')
  })

  it('codeBlock 内的 markdown 特殊字符不 escape', () => {
    const result = serializeComposerDoc(doc(codeBlock('a*b_c[d]', 'ts')))
    expect(result.markdown).toBe('```ts\na*b_c[d]\n```')
  })

  it('bulletList 多项', () => {
    const result = serializeComposerDoc(
      doc(ul(li(p(t('a'))), li(p(t('b'))), li(p(t('c')))))
    )
    expect(result.markdown).toBe('- a\n- b\n- c')
  })

  it('orderedList 多项 → 1. 2. 3.', () => {
    const result = serializeComposerDoc(
      doc(ol(li(p(t('a'))), li(p(t('b'))), li(p(t('c')))))
    )
    expect(result.markdown).toBe('1. a\n2. b\n3. c')
  })

  it('listItem 多段 → 续段缩进 4 空格', () => {
    const result = serializeComposerDoc(
      doc(ul(li(p(t('first line')), p(t('second line')))))
    )
    expect(result.markdown).toBe('- first line\n\n    second line')
  })
})

describe('serializeComposerDoc — attachmentToken', () => {
  const tokenAttrs = (overrides: Partial<ComposerJsonNode['attrs']> = {}): ComposerJsonNode['attrs'] => ({
    id: 'a1',
    fileName: 'report.pdf',
    path: '/abs/report.pdf',
    kind: 'file',
    fileType: 'pdf',
    fileSize: 1024,
    source: 'picker',
    ...overrides,
  })
  const at = (overrides: Partial<ComposerJsonNode['attrs']> = {}): ComposerJsonNode => ({
    type: 'attachmentToken',
    attrs: tokenAttrs(overrides),
  })

  it('单个非图片 token → [附件: name](file://path)', () => {
    const result = serializeComposerDoc(doc(p(at())))
    expect(result.markdown).toBe('[附件: report.pdf](<file:///abs/report.pdf>)')
    expect(result.attachments).toHaveLength(1)
    expect(result.attachments[0]).toMatchObject({
      id: 'a1',
      fileName: 'report.pdf',
      path: '/abs/report.pdf',
      kind: 'file',
      fileType: 'pdf',
      fileSize: 1024,
      source: 'picker',
    })
    expect(result.isEmpty).toBe(false)
  })

  it('image token → ![name](file://path)', () => {
    const result = serializeComposerDoc(
      doc(
        p(
          at({
            id: 'img1',
            fileName: 'a.png',
            path: '/abs/a.png',
            kind: 'image',
            fileType: 'image',
          })
        )
      )
    )
    expect(result.markdown).toBe('![a.png](<file:///abs/a.png>)')
  })

  it('folder token → kind=folder 仍走非图片占位符', () => {
    const result = serializeComposerDoc(
      doc(
        p(
          at({
            id: 'f1',
            fileName: 'docs',
            path: '/abs/docs',
            kind: 'folder',
            fileType: 'folder',
          })
        )
      )
    )
    expect(result.markdown).toBe('[附件: docs](<file:///abs/docs>)')
  })

  it('文本 + token + 文本，按文档顺序', () => {
    const result = serializeComposerDoc(
      doc(p(t('请分析 '), at(), t(' 谢谢')))
    )
    expect(result.markdown).toBe('请分析 [附件: report.pdf](<file:///abs/report.pdf>) 谢谢')
    expect(result.attachments.map((a) => a.id)).toEqual(['a1'])
  })

  it('多个不同 token 按出现顺序收集', () => {
    const result = serializeComposerDoc(
      doc(p(at({ id: 'a' }), at({ id: 'b' }), at({ id: 'c' })))
    )
    expect(result.attachments.map((a) => a.id)).toEqual(['a', 'b', 'c'])
  })

  it('同 id token 出现多次：markdown 保留多处占位符，attachments 去重', () => {
    const result = serializeComposerDoc(
      doc(p(at({ id: 'dup' }), t(' 和 '), at({ id: 'dup' })))
    )
    expect(result.markdown.match(/附件: report\.pdf/g)).toHaveLength(2)
    expect(result.attachments).toHaveLength(1)
    expect(result.attachments[0].id).toBe('dup')
  })

  it('只附件提交：markdown 是占位符串联，isEmpty=false', () => {
    const result = serializeComposerDoc(doc(p(at({ id: 'a' }), t(' '), at({ id: 'b' }))))
    expect(result.isEmpty).toBe(false)
    expect(result.attachments.map((a) => a.id)).toEqual(['a', 'b'])
  })

  it('文件名含 [ ] \\ 时 escape', () => {
    const result = serializeComposerDoc(
      doc(p(at({ id: 'x', fileName: 'a[1]\\b.pdf', path: '/p/a[1]\\b.pdf' })))
    )
    // Filename: link text escapes \ [ ]; URL is angle-bracketed so [ ] inside don't need escape
    expect(result.markdown).toBe('[附件: a\\[1\\]\\\\b.pdf](<file:///p/a[1]\\\\b.pdf>)')
  })

  it('路径含空格 → angle-bracketed URL 直接保留空格', () => {
    const result = serializeComposerDoc(
      doc(p(at({ id: 'x', fileName: 'note.pdf', path: '/Users/me/Desktop/钉钉 skill/note.pdf' })))
    )
    expect(result.markdown).toBe('[附件: note.pdf](<file:///Users/me/Desktop/钉钉 skill/note.pdf>)')
  })

  it('路径含 ( ) 不需要 escape（angle-bracketed URL 内只 escape < > \\）', () => {
    const result = serializeComposerDoc(
      doc(p(at({ id: 'x', fileName: 'name', path: '/a (b)/c.pdf' })))
    )
    expect(result.markdown).toBe('[附件: name](<file:///a (b)/c.pdf>)')
  })

  it('Windows 绝对路径 → file:///C:/... 三斜线 + 反斜线转正斜线', () => {
    const result = serializeComposerDoc(
      doc(p(at({ id: 'w1', fileName: 'foo.pdf', path: 'C:\\Users\\me\\foo.pdf' })))
    )
    expect(result.markdown).toBe('[附件: foo.pdf](<file:///C:/Users/me/foo.pdf>)')
  })

  it('Windows 路径正斜线写法也支持', () => {
    const result = serializeComposerDoc(
      doc(p(at({ id: 'w2', fileName: 'foo.pdf', path: 'D:/data/foo.pdf' })))
    )
    expect(result.markdown).toBe('[附件: foo.pdf](<file:///D:/data/foo.pdf>)')
  })
})

describe('serializeComposerDoc — 综合 / isEmpty', () => {
  it('只有空白文本 → isEmpty=true', () => {
    const result = serializeComposerDoc(
      doc(p({ type: 'text', text: '   ' }))
    )
    expect(result.isEmpty).toBe(true)
  })

  it('只有 hardBreak → isEmpty=true', () => {
    const result = serializeComposerDoc(doc(p({ type: 'hardBreak' })))
    expect(result.isEmpty).toBe(true)
  })

  it('hardBreak + token → isEmpty=false', () => {
    const result = serializeComposerDoc(
      doc(
        p(
          { type: 'hardBreak' },
          {
            type: 'attachmentToken',
            attrs: {
              id: 'x',
              fileName: 'a.pdf',
              path: '/p/a.pdf',
              kind: 'file',
              fileType: 'pdf',
              fileSize: 1,
              source: 'picker',
            },
          }
        )
      )
    )
    expect(result.isEmpty).toBe(false)
    expect(result.attachments).toHaveLength(1)
  })

  it('端到端：富文本 + 附件 + 图片 + 列表 + 引��', () => {
    const result = serializeComposerDoc(
      doc(
        p(
          t('请帮我看看 ', undefined),
          {
            type: 'attachmentToken',
            attrs: {
              id: 'pdf1',
              fileName: 'plan.pdf',
              path: '/p/plan.pdf',
              kind: 'file',
              fileType: 'pdf',
              fileSize: 1,
              source: 'picker',
            },
          },
          t(' 和 ', undefined),
          {
            type: 'attachmentToken',
            attrs: {
              id: 'img1',
              fileName: 'chart.png',
              path: '/p/chart.png',
              kind: 'image',
              fileType: 'image',
              fileSize: 1,
              source: 'paste',
            },
          },
          t(' 的关系'),
        ),
        {
          type: 'bulletList',
          content: [
            { type: 'listItem', content: [p(t('重点 1'))] },
            { type: 'listItem', content: [p(t('重点 2'))] },
          ],
        },
        {
          type: 'blockquote',
          content: [p(t('备注'))],
        },
      ),
    )
    expect(result.markdown).toBe(
      '请帮我看看 [附件: plan.pdf](<file:///p/plan.pdf>) 和 ![chart.png](<file:///p/chart.png>) 的关系\n\n- 重点 1\n- 重点 2\n\n> 备注'
    )
    expect(result.attachments.map((a) => a.id)).toEqual(['pdf1', 'img1'])
    expect(result.isEmpty).toBe(false)
  })
})

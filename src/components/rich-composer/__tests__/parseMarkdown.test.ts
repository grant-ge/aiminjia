import { describe, expect, it } from 'vitest'
import { parseMarkdownToComposerJson } from '../parseMarkdown'

describe('parseMarkdownToComposerJson', () => {
  it('空字符串 → 单空 paragraph 文档', () => {
    const json = parseMarkdownToComposerJson('')
    expect(json).toEqual({
      type: 'doc',
      content: [{ type: 'paragraph' }],
    })
  })

  it('单段纯文本', () => {
    const json = parseMarkdownToComposerJson('hello world')
    expect(json).toEqual({
      type: 'doc',
      content: [
        { type: 'paragraph', content: [{ type: 'text', text: 'hello world' }] },
      ],
    })
  })

  it('\\n\\n → 两段', () => {
    const json = parseMarkdownToComposerJson('a\n\nb')
    expect(json.content).toEqual([
      { type: 'paragraph', content: [{ type: 'text', text: 'a' }] },
      { type: 'paragraph', content: [{ type: 'text', text: 'b' }] },
    ])
  })

  it('行尾两空格 + \\n → hardBreak', () => {
    const json = parseMarkdownToComposerJson('line1  \nline2')
    expect(json.content).toEqual([
      {
        type: 'paragraph',
        content: [
          { type: 'text', text: 'line1' },
          { type: 'hardBreak' },
          { type: 'text', text: 'line2' },
        ],
      },
    ])
  })

  it('单换行 \\n → 同样作为 hardBreak（容错）', () => {
    const json = parseMarkdownToComposerJson('a\nb')
    expect(json.content).toEqual([
      {
        type: 'paragraph',
        content: [
          { type: 'text', text: 'a' },
          { type: 'hardBreak' },
          { type: 'text', text: 'b' },
        ],
      },
    ])
  })

  it('image attachment: ![name](file:///abs/x.png) → image attachmentToken', () => {
    const json = parseMarkdownToComposerJson('![chart.png](file:///abs/chart.png)')
    const para = json.content?.[0]
    expect(para?.content?.[0]).toMatchObject({
      type: 'attachmentToken',
      attrs: {
        fileName: 'chart.png',
        path: '/abs/chart.png',
        kind: 'image',
        fileType: 'image',
        source: 'paste',
      },
    })
    expect(
      typeof (para?.content?.[0] as unknown as { attrs: { id: string } }).attrs.id,
    ).toBe('string')
  })

  it('file attachment: [附件: r.pdf](file:///p/r.pdf) → pdf attachmentToken', () => {
    const json = parseMarkdownToComposerJson('[附件: r.pdf](file:///p/r.pdf)')
    const para = json.content?.[0]
    expect(para?.content?.[0]).toMatchObject({
      type: 'attachmentToken',
      attrs: {
        fileName: 'r.pdf',
        path: '/p/r.pdf',
        kind: 'file',
        fileType: 'pdf',
      },
    })
  })

  it('文本 + token + 文本', () => {
    const json = parseMarkdownToComposerJson('请分析 [附件: r.pdf](file:///p/r.pdf) 谢谢')
    const para = json.content?.[0]
    expect(para?.content?.map((n: { type: string }) => n.type)).toEqual([
      'text',
      'attachmentToken',
      'text',
    ])
  })
})

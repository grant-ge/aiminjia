import { describe, expect, it } from 'vitest'
import { Editor } from '@tiptap/core'
import { buildComposerExtensions } from '../composerSchema'
import { serializeComposerDoc } from '../serializer'
import type { ComposerAttachmentToken, ComposerJsonNode } from '../types'

const mkToken = (overrides: Partial<ComposerAttachmentToken> = {}): ComposerAttachmentToken => ({
  id: 'a1',
  fileName: 'plan.pdf',
  path: '/p/plan.pdf',
  kind: 'file',
  fileType: 'pdf',
  fileSize: 1,
  source: 'picker',
  ...overrides,
})

function makeEditor(content = '<p></p>') {
  return new Editor({ extensions: buildComposerExtensions(), content })
}

describe('composerSchema 端到端：editor → P0 serializer', () => {
  it('插入 plain text + 调用 serializer 输出 markdown', () => {
    const editor = makeEditor()
    editor.commands.setContent('<p>hello world</p>')
    const json = editor.getJSON() as unknown as ComposerJsonNode
    expect(serializeComposerDoc(json).markdown).toBe('hello world')
    editor.destroy()
  })

  it('粗体 HTML → markdown **text**', () => {
    const editor = makeEditor()
    editor.commands.setContent('<p>hi <strong>there</strong></p>')
    const json = editor.getJSON() as unknown as ComposerJsonNode
    expect(serializeComposerDoc(json).markdown).toBe('hi **there**')
    editor.destroy()
  })

  it('bullet list → markdown - item', () => {
    const editor = makeEditor()
    editor.commands.setContent('<ul><li><p>a</p></li><li><p>b</p></li></ul>')
    const json = editor.getJSON() as unknown as ComposerJsonNode
    expect(serializeComposerDoc(json).markdown).toBe('- a\n- b')
    editor.destroy()
  })

  it('blockquote → markdown > line', () => {
    const editor = makeEditor()
    editor.commands.setContent('<blockquote><p>note</p></blockquote>')
    const json = editor.getJSON() as unknown as ComposerJsonNode
    expect(serializeComposerDoc(json).markdown).toBe('> note')
    editor.destroy()
  })

  it('codeBlock → 输出 fenced code block', () => {
    const editor = makeEditor()
    editor.commands.setContent('<pre><code class="language-ts">let x = 1</code></pre>')
    const json = editor.getJSON() as unknown as ComposerJsonNode
    const md = serializeComposerDoc(json).markdown
    // StarterKit 默认 codeBlock 不一定保留 language attr — 接受任一形态
    expect(md.startsWith('```')).toBe(true)
    expect(md).toContain('let x = 1')
    expect(md.endsWith('```')).toBe(true)
    editor.destroy()
  })

  it('link mark → markdown [txt](url)', () => {
    const editor = makeEditor()
    editor.commands.setContent('<p><a href="https://example.com">click</a></p>')
    const json = editor.getJSON() as unknown as ComposerJsonNode
    expect(serializeComposerDoc(json).markdown).toBe('[click](https://example.com)')
    editor.destroy()
  })

  it('插入 attachmentToken + 文本 → markdown 占位符 + 文本', () => {
    const editor = makeEditor()
    editor.commands.insertAttachmentTokens([mkToken()])
    editor.commands.insertContent(' 你好')
    const json = editor.getJSON() as unknown as ComposerJsonNode
    const result = serializeComposerDoc(json)
    expect(result.markdown).toBe('[附件: plan.pdf](<file:///p/plan.pdf>) 你好')
    expect(result.attachments[0].id).toBe('a1')
    editor.destroy()
  })

  it('禁用 heading：粘贴 h1 → 退化为段落（markdown 不以 # 开头）', () => {
    const editor = makeEditor()
    editor.commands.setContent('<h1>Big</h1>')
    const json = editor.getJSON() as unknown as ComposerJsonNode
    const md = serializeComposerDoc(json).markdown
    expect(md.startsWith('#')).toBe(false)
    expect(md).toContain('Big')
    editor.destroy()
  })
})

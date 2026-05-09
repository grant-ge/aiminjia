import { describe, expect, it } from 'vitest'
import { Editor } from '@tiptap/core'
import StarterKit from '@tiptap/starter-kit'
import { AttachmentTokenExtension } from '../attachmentTokenExtension'
import type { ComposerAttachmentToken } from '../types'

const mkToken = (overrides: Partial<ComposerAttachmentToken> = {}): ComposerAttachmentToken => ({
  id: 'a1',
  fileName: 'plan.pdf',
  path: '/abs/plan.pdf',
  kind: 'file',
  fileType: 'pdf',
  fileSize: 2048,
  source: 'picker',
  ...overrides,
})

function makeEditor() {
  return new Editor({
    extensions: [StarterKit, AttachmentTokenExtension],
    content: '<p></p>',
  })
}

describe('attachmentTokenExtension', () => {
  it('insertAttachmentTokens 单 token → JSON 含 attachmentToken 节点', () => {
    const editor = makeEditor()
    editor.commands.insertAttachmentTokens([mkToken()])
    const json = editor.getJSON()
    const para = (json.content as Array<{ content?: unknown[] }>)[0]
    const tokenNode = (para.content as Array<{ type: string; attrs: ComposerAttachmentToken }>).find(
      (n) => n.type === 'attachmentToken'
    )
    expect(tokenNode).toBeDefined()
    expect(tokenNode?.attrs.id).toBe('a1')
    expect(tokenNode?.attrs.fileName).toBe('plan.pdf')
    editor.destroy()
  })

  it('insertAttachmentTokens 多 token → 节点之间用空格 text 分隔', () => {
    const editor = makeEditor()
    editor.commands.insertAttachmentTokens([
      mkToken({ id: 'a' }),
      mkToken({ id: 'b' }),
      mkToken({ id: 'c' }),
    ])
    const json = editor.getJSON()
    const para = (json.content as Array<{ content: Array<{ type: string; attrs?: { id?: string }; text?: string }> }>)[0]
    const types = para.content.map((n) => n.type)
    // expect alternating: token, text(' '), token, text(' '), token
    expect(types).toEqual(['attachmentToken', 'text', 'attachmentToken', 'text', 'attachmentToken'])
    expect(para.content[1].text).toBe(' ')
    expect(para.content[3].text).toBe(' ')
    editor.destroy()
  })

  it('insertAttachmentTokens 空数组 → 不修改文档，命令返回 false', () => {
    const editor = makeEditor()
    const beforeJson = JSON.stringify(editor.getJSON())
    const result = editor.commands.insertAttachmentTokens([])
    expect(result).toBe(false)
    expect(JSON.stringify(editor.getJSON())).toBe(beforeJson)
    editor.destroy()
  })

  it('HTML round-trip：getHTML 输出 data-* 属性，setContent 还原 attrs', () => {
    const editor = makeEditor()
    const token = mkToken({
      id: 'rt1',
      fileName: 'a (b).pdf',
      path: '/p/a (b).pdf',
      mimeType: 'application/pdf',
    })
    editor.commands.insertAttachmentTokens([token])
    const html = editor.getHTML()
    expect(html).toContain('data-rich-composer-attachment-token')
    expect(html).toContain('data-id="rt1"')
    expect(html).toContain('data-file-name="a (b).pdf"')
    expect(html).toContain('data-mime-type="application/pdf"')

    const editor2 = new Editor({ extensions: [StarterKit, AttachmentTokenExtension], content: html })
    const json = editor2.getJSON()
    const para = (json.content as Array<{ content: Array<{ type: string; attrs?: ComposerAttachmentToken }> }>)[0]
    const node = para.content.find((n) => n.type === 'attachmentToken')
    expect(node?.attrs?.id).toBe('rt1')
    expect(node?.attrs?.fileName).toBe('a (b).pdf')
    expect(node?.attrs?.path).toBe('/p/a (b).pdf')
    expect(node?.attrs?.mimeType).toBe('application/pdf')
    editor2.destroy()
    editor.destroy()
  })

  it('attrs 不全（缺 id）的 HTML → parseHTML 拒绝，节点不出现', () => {
    const html = '<p><span data-rich-composer-attachment-token data-file-name="x.pdf"></span></p>'
    const editor = new Editor({ extensions: [StarterKit, AttachmentTokenExtension], content: html })
    const json = editor.getJSON()
    const para = (json.content as Array<{ content?: Array<{ type: string }> }>)[0]
    const hasToken = (para.content ?? []).some((n) => n.type === 'attachmentToken')
    expect(hasToken).toBe(false)
    editor.destroy()
  })

  it('fileType 不在白名单 → parseHTML 拒绝', () => {
    const html =
      '<p><span data-rich-composer-attachment-token data-id="x" data-file-name="x.bin" data-path="/p/x.bin" data-kind="file" data-file-type="malware" data-file-size="1" data-source="picker"></span></p>'
    const editor = new Editor({ extensions: [StarterKit, AttachmentTokenExtension], content: html })
    const json = editor.getJSON()
    const para = (json.content as Array<{ content?: Array<{ type: string }> }>)[0]
    const hasToken = (para.content ?? []).some((n) => n.type === 'attachmentToken')
    expect(hasToken).toBe(false)
    editor.destroy()
  })
})

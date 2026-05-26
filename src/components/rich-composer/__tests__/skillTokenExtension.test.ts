import { describe, expect, it } from 'vitest'
import { Editor } from '@tiptap/core'
import StarterKit from '@tiptap/starter-kit'
import { SkillTokenExtension } from '../skillTokenExtension'
import type { ComposerSkillToken } from '../types'

const skill: ComposerSkillToken = {
  id: 'dingtalk-workspace',
  label: '玩转钉钉',
  command: '/dingtalk-workspace',
}

function makeEditor() {
  return new Editor({
    extensions: [StarterKit, SkillTokenExtension.configure({ skills: [skill] })],
    content: '<p></p>',
  })
}

describe('skillTokenExtension', () => {
  it('insertSkillToken inserts a skillToken node', () => {
    const editor = makeEditor()
    editor.commands.insertSkillToken(skill)
    const json = editor.getJSON()
    const para = (json.content as Array<{ content?: unknown[] }>)[0]
    const tokenNode = (para.content as Array<{ type: string; attrs: ComposerSkillToken }>).find(
      (node) => node.type === 'skillToken',
    )
    expect(tokenNode?.attrs).toMatchObject(skill)
    editor.destroy()
  })

  it('input rule converts slash id followed by a space into a skillToken', () => {
    const editor = makeEditor()
    editor.commands.insertContent('/dingtalk-workspace ')
    const json = editor.getJSON()
    const para = (json.content as Array<{ content?: Array<{ type: string; attrs?: ComposerSkillToken; text?: string }> }>)[0]
    expect((para.content ?? []).some((node) => node.type === 'skillToken' && node.attrs?.label === '玩转钉钉')).toBe(true)
    expect(editor.getText()).not.toContain('/dingtalk-workspace')
    editor.destroy()
  })

  it('unknown slash id stays as text', () => {
    const editor = makeEditor()
    editor.commands.insertContent('/unknown-skill ')
    expect(editor.getText()).toContain('/unknown-skill')
    editor.destroy()
  })

  it('HTML round-trip preserves skill attrs', () => {
    const editor = makeEditor()
    editor.commands.insertSkillToken(skill)
    const html = editor.getHTML()
    expect(html).toContain('data-rich-composer-skill-token')
    expect(html).toContain('data-id="dingtalk-workspace"')
    const editor2 = new Editor({
      extensions: [StarterKit, SkillTokenExtension.configure({ skills: [skill] })],
      content: html,
    })
    const para = (editor2.getJSON().content as Array<{ content?: Array<{ type: string; attrs?: ComposerSkillToken }> }>)[0]
    const node = (para.content ?? []).find((item) => item.type === 'skillToken')
    expect(node?.attrs?.label).toBe('玩转钉钉')
    editor2.destroy()
    editor.destroy()
  })

  it('removes the stranded U+200B caret boundary after the chip is deleted', () => {
    const editor = makeEditor()
    // insertSkillToken at offset 0 injects a U+200B caret boundary before the chip.
    editor.commands.insertSkillToken(skill)
    type Node = { type: string; text?: string; content?: Node[] }
    const paraBefore = (editor.getJSON().content as Node[])[0]
    expect(paraBefore.content?.[0]?.text).toBe('​')
    expect(paraBefore.content?.[1]?.type).toBe('skillToken')

    // Delete the chip — the zero-width is now orphaned.
    const tokenPos = (() => {
      let found = -1
      editor.state.doc.descendants((node, pos) => {
        if (node.type.name === 'skillToken') {
          found = pos
          return false
        }
        return true
      })
      return found
    })()
    expect(tokenPos).toBeGreaterThanOrEqual(0)
    editor
      .chain()
      .focus()
      .setNodeSelection(tokenPos)
      .deleteSelection()
      .run()

    const paraAfter = (editor.getJSON().content as Node[])[0]
    const hasZeroWidth = (paraAfter.content ?? []).some((n) => n.text?.includes('​'))
    expect(hasZeroWidth).toBe(false)
    editor.destroy()
  })
})

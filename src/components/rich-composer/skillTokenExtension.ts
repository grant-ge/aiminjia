import { InputRule, Node, mergeAttributes } from '@tiptap/core'
import { ReactNodeViewRenderer } from '@tiptap/react'
import { Plugin } from '@tiptap/pm/state'
import type { ReactNodeViewProps } from '@tiptap/react'
import type { ComponentType } from 'react'
import { SkillTokenView } from './SkillTokenView'
import type { ComposerSkillToken } from './types'

declare module '@tiptap/core' {
  interface Commands<ReturnType> {
    skillToken: {
      insertSkillToken: (token: ComposerSkillToken) => ReturnType
    }
  }
}

const DATA_ATTR = 'data-rich-composer-skill-token'
const CARET_BOUNDARY = '\u200B'

export interface SkillTokenExtensionOptions {
  skills: ComposerSkillToken[]
}

function normalizeCommand(value: string) {
  return value.startsWith('/') ? value : `/${value}`
}

function findSkill(skills: ComposerSkillToken[], slashText: string): ComposerSkillToken | null {
  const command = normalizeCommand(slashText.trim())
  return skills.find((skill) => skill.command === command || `/${skill.id}` === command) ?? null
}

export const SkillTokenExtension = Node.create<SkillTokenExtensionOptions>({
  name: 'skillToken',
  group: 'inline',
  inline: true,
  atom: true,
  selectable: false,
  draggable: true,

  addOptions() {
    return { skills: [] }
  },

  addAttributes() {
    return {
      id: { default: null },
      label: { default: null },
      command: { default: null },
    }
  },

  parseHTML() {
    return [
      {
        tag: `span[${DATA_ATTR}]`,
        getAttrs: (el) => {
          if (!(el instanceof HTMLElement)) return false
          const id = el.getAttribute('data-id')
          const label = el.getAttribute('data-label')
          const command = el.getAttribute('data-command')
          if (!id || !label || !command) return false
          return { id, label, command }
        },
      },
    ]
  },

  renderHTML({ HTMLAttributes, node }) {
    const attrs = node.attrs as Partial<ComposerSkillToken>
    if (!attrs.id || !attrs.label || !attrs.command) {
      return ['span', mergeAttributes(HTMLAttributes, { [DATA_ATTR]: '' })]
    }
    return [
      'span',
      mergeAttributes(HTMLAttributes, {
        [DATA_ATTR]: '',
        'data-id': attrs.id,
        'data-label': attrs.label,
        'data-command': attrs.command,
      }),
      attrs.label,
    ]
  },

  addNodeView() {
    return ReactNodeViewRenderer(
      SkillTokenView as unknown as ComponentType<ReactNodeViewProps>,
    )
  },

  addCommands() {
    return {
      insertSkillToken:
        (token: ComposerSkillToken) =>
        ({ chain, state }) => {
          let c = chain()
          if (state.selection.$from.parentOffset === 0) {
            c = c.insertContent({ type: 'text', text: CARET_BOUNDARY })
          }
          return c.insertContent({ type: 'skillToken', attrs: token }).run()
        },
    }
  },


  addProseMirrorPlugins() {
    return [
      new Plugin({
        appendTransaction: (transactions, _oldState, newState) => {
          if (!transactions.some((transaction) => transaction.docChanged)) return null
          const tokenType = newState.schema.nodes.skillToken
          if (!tokenType) return null
          let tr = newState.tr
          let changed = false
          // 1) Slash-shortcut → token replacement (typing "/foo ").
          newState.doc.descendants((node, pos) => {
            if (!node.isTextblock) return true
            const text = node.textBetween(0, node.content.size, undefined, undefined)
            const match = /(?:^|\s)(\/[a-z0-9][a-z0-9_-]{1,63})\s$/.exec(text)
            if (!match) return true
            const skill = findSkill(this.options.skills, match[1])
            if (!skill) return true
            const leading = match[0].startsWith(' ') ? 1 : 0
            const from = pos + 1 + match.index + leading
            const to = pos + 1 + match.index + match[0].length
            tr = tr.replaceWith(from, to, tokenType.create(skill))
            changed = true
            return false
          })
          if (changed) return tr

          // 2) Stranded caret-boundary cleanup. `insertSkillToken` injects a
          // U+200B before the chip when inserted at parentOffset 0 so the
          // caret has somewhere to land. After the chip is removed the
          // zero-width lingers, making the chip feel like it needs two
          // backspaces to clear. Strip any U+200B run that no longer sits
          // next to a skillToken in the same textblock.
          let cleaned = newState.tr
          let didClean = false
          newState.doc.descendants((node, pos) => {
            if (!node.isTextblock) return true
            // Build a flat list of (kind, fromOffset, toOffset) for this block.
            type Span = { kind: 'text' | 'skill'; from: number; to: number; text?: string }
            const spans: Span[] = []
            node.forEach((child, offset) => {
              const childFrom = offset
              const childTo = offset + child.nodeSize
              if (child.type.name === 'skillToken') {
                spans.push({ kind: 'skill', from: childFrom, to: childTo })
              } else if (child.isText && typeof child.text === 'string') {
                spans.push({ kind: 'text', from: childFrom, to: childTo, text: child.text })
              } else {
                spans.push({ kind: 'text', from: childFrom, to: childTo })
              }
            })
            spans.forEach((span, idx) => {
              if (span.kind !== 'text' || !span.text) return
              if (!span.text.includes('​')) return
              const prevIsSkill = idx > 0 && spans[idx - 1].kind === 'skill'
              const nextIsSkill = idx < spans.length - 1 && spans[idx + 1].kind === 'skill'
              if (prevIsSkill || nextIsSkill) return
              // No adjacent skill token — the zero-width is orphaned. Strip it.
              const cleanedText = span.text.replace(/​+/g, '')
              const blockFrom = pos + 1 + span.from
              const blockTo = pos + 1 + span.to
              if (cleanedText.length === 0) {
                cleaned = cleaned.delete(blockFrom, blockTo)
              } else {
                cleaned = cleaned.replaceWith(
                  blockFrom,
                  blockTo,
                  newState.schema.text(cleanedText),
                )
              }
              didClean = true
            })
            return false
          })
          return didClean ? cleaned : null
        },
      }),
    ]
  },

  addInputRules() {
    return [
      new InputRule({
        find: /(?:^|\s)(\/[a-z0-9][a-z0-9_-]{1,63})\s$/,
        handler: ({ range, match, chain }) => {
          const skill = findSkill(this.options.skills, match[1])
          if (!skill) return
          const leading = match[0].startsWith(' ') ? ' ' : ''
          const from = range.from + leading.length
          chain()
            .deleteRange({ from, to: range.to })
            .insertContentAt(from, { type: 'skillToken', attrs: skill })
            .run()
        },
      }),
    ]
  },
})

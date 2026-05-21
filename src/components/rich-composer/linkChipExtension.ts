import { Node, mergeAttributes } from '@tiptap/core'
import { Fragment, Slice } from '@tiptap/pm/model'
import { Plugin, PluginKey } from '@tiptap/pm/state'
import { ReactNodeViewRenderer } from '@tiptap/react'
import type { ReactNodeViewProps } from '@tiptap/react'
import type { ComponentType } from 'react'

import { LinkChipView } from './LinkChipView'

declare module '@tiptap/core' {
  interface Commands<ReturnType> {
    linkChip: {
      insertLinkChip: (url: string) => ReturnType
    }
  }
}

const DATA_ATTR = 'data-rich-composer-link-chip'

export const LinkChipExtension = Node.create({
  name: 'linkChip',
  group: 'inline',
  inline: true,
  atom: true,
  // selectable=false matches AttachmentTokenExtension: arrow keys slide past
  // the chip into the surrounding text rather than selecting the chip itself.
  selectable: false,
  draggable: true,

  addAttributes() {
    return {
      url: { default: null },
    }
  },

  parseHTML() {
    return [
      {
        tag: `span[${DATA_ATTR}]`,
        getAttrs: (el) => {
          if (!(el instanceof HTMLElement)) return false
          const url = el.getAttribute('data-url')
          if (!url) return false
          return { url }
        },
      },
    ]
  },

  renderHTML({ HTMLAttributes, node }) {
    const url = typeof node.attrs.url === 'string' ? node.attrs.url : ''
    if (!url) {
      return ['span', mergeAttributes(HTMLAttributes, { [DATA_ATTR]: '' })]
    }
    return [
      'span',
      mergeAttributes(HTMLAttributes, {
        [DATA_ATTR]: '',
        'data-url': url,
      }),
    ]
  },

  addNodeView() {
    return ReactNodeViewRenderer(
      LinkChipView as unknown as ComponentType<ReactNodeViewProps>,
    )
  },

  addCommands() {
    return {
      insertLinkChip:
        (url: string) =>
        ({ chain }) => {
          if (!url) return false
          return chain()
            .insertContent({ type: 'linkChip', attrs: { url } })
            .run()
        },
    }
  },

  addProseMirrorPlugins() {
    return [
      new Plugin({
        key: new PluginKey('linkChipPaste'),
        props: {
          handlePaste(view, event) {
            const cd = event.clipboardData
            if (!cd) return false
            // If the clipboard carries file references, let the attachment
            // paste flow handle it (see useComposerAttachmentPaste).
            const types = Array.from(cd.types ?? [])
            const hasFiles = types.some(
              (t) =>
                t === 'Files' ||
                t === 'text/uri-list' ||
                t.startsWith('public.file-url'),
            )
            if (hasFiles) return false

            const text = cd.getData('text/plain')
            if (!text) return false

            const matches = extractUrls(text)
            if (matches.length === 0) return false

            const { state, dispatch } = view
            const schema = state.schema
            const linkChipType = schema.nodes.linkChip
            if (!linkChipType) return false

            // Build inline node array: alternating text segments and chip nodes,
            // with newlines mapped to hardBreak (paragraph-flattening pastes,
            // which matches the chat-input UX better than spawning new paragraphs).
            const inline = [] as ReturnType<typeof schema.text>[]
            const pushText = (s: string) => {
              if (!s) return
              const lines = s.split('\n')
              lines.forEach((line, idx) => {
                if (line.length > 0) inline.push(schema.text(line))
                if (idx < lines.length - 1 && schema.nodes.hardBreak) {
                  inline.push(schema.nodes.hardBreak.create() as never)
                }
              })
            }

            let cursor = 0
            for (const m of matches) {
              pushText(text.slice(cursor, m.start))
              inline.push(linkChipType.create({ url: m.url }) as never)
              cursor = m.end
            }
            pushText(text.slice(cursor))

            if (inline.length === 0) return false

            const slice = new Slice(Fragment.fromArray(inline), 0, 0)
            dispatch(state.tr.replaceSelection(slice).scrollIntoView())
            return true
          },
        },
      }),
    ]
  },
})

// Match http(s) URLs anywhere in pasted text. The character class excludes
// whitespace and the markdown/HTML punctuation that virtually never legally
// appears mid-URL; trailing soft punctuation is trimmed afterwards.
const URL_RE = /\bhttps?:\/\/[^\s<>"'`{}|\\^[\]]+/gi
const TRAILING_PUNCT_RE = /[.,;:!?)\]}'"」』、，。；：！？]+$/

interface UrlMatch {
  start: number
  end: number
  url: string
}

export function extractUrls(text: string): UrlMatch[] {
  const out: UrlMatch[] = []
  for (const m of text.matchAll(URL_RE)) {
    if (m.index === undefined) continue
    const trimmed = m[0].replace(TRAILING_PUNCT_RE, '')
    if (trimmed.length === 0) continue
    // Require a host character after `://`.
    if (!/^https?:\/\/[A-Za-z0-9._~%-]/i.test(trimmed)) continue
    out.push({ start: m.index, end: m.index + trimmed.length, url: trimmed })
  }
  return out
}

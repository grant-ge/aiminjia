import StarterKit from '@tiptap/starter-kit'
import Placeholder from '@tiptap/extension-placeholder'
import { AttachmentTokenExtension } from './attachmentTokenExtension'

export interface BuildComposerExtensionsOptions {
  placeholder?: string
}

export function buildComposerExtensions(options: BuildComposerExtensionsOptions = {}) {
  return [
    StarterKit.configure({
      // Spec disallows headings and horizontal rules; everything else from StarterKit
      // (paragraph, text, hardBreak, blockquote, codeBlock, lists, bold, italic,
      // strike, code, link, history) stays enabled.
      heading: false,
      horizontalRule: false,
      link: {
        openOnClick: false,
        autolink: true,
        linkOnPaste: true,
      },
    }),
    Placeholder.configure({
      placeholder: options.placeholder ?? '',
    }),
    AttachmentTokenExtension,
  ]
}

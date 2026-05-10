import { useEffect, useMemo, useState } from 'react'
import ReactMarkdown from 'react-markdown'
import remarkGfm from 'remark-gfm'
import { getLocalFilePreview, openLocalFile } from '@/lib/tauri'
import { isPreviewableFileType } from '@/components/chat/generatedFileActions'
import { useGeneratedFilePreviewStore } from '@/stores/generatedFilePreviewStore'
import type { FileAttachment } from '@/types/message'

const FILE_TYPE_LABEL: Record<FileAttachment['fileType'], string> = {
  excel: 'XLS',
  csv: 'CSV',
  word: 'DOC',
  pdf: 'PDF',
  json: 'JSON',
  image: 'IMG',
  folder: 'DIR',
}

const EXT_TO_LABEL: Record<string, string> = {
  png: 'IMG', jpg: 'IMG', jpeg: 'IMG', gif: 'IMG', webp: 'IMG', bmp: 'IMG', svg: 'IMG',
  xls: 'XLS', xlsx: 'XLS',
  doc: 'DOC', docx: 'DOC',
  pdf: 'PDF',
  csv: 'CSV',
  json: 'JSON',
}

function inferLabelFromName(name: string): string {
  const m = /\.([A-Za-z0-9]+)$/.exec(name)
  if (!m) return 'FILE'
  return EXT_TO_LABEL[m[1].toLowerCase()] ?? 'FILE'
}

interface UserBubbleMarkdownProps {
  text: string
  conversationId?: string
  files?: FileAttachment[]
}

function fileUrlToPath(href: string | undefined | null): string | null {
  if (!href || !href.startsWith('file://')) return null
  let stripped = href.slice('file://'.length)
  // react-markdown / mdast normalize URL-encoded chars (e.g. CJK or spaces in
  // bare file:// URLs become `%E8%96%AA…` / `%20`). Decode so we hand the OS
  // back the actual path it stores on disk.
  try {
    stripped = decodeURI(stripped)
  } catch {
    // Malformed escape — fall through with the raw string.
  }
  if (/^\/[A-Za-z]:/.test(stripped)) return stripped.slice(1)
  return stripped
}

// Cache the dataURL per path so re-renders / re-mounts skip the IPC.
const localImageCache = new Map<string, string>()

function useLocalImageThumb(path: string) {
  const [url, setUrl] = useState<string | null>(() => localImageCache.get(path) ?? null)
  useEffect(() => {
    if (!path || localImageCache.has(path)) return
    let cancelled = false
    void (async () => {
      try {
        const preview = await getLocalFilePreview(path)
        if (cancelled) return
        if (preview.kind === 'image') {
          localImageCache.set(path, preview.dataUrl)
          setUrl(preview.dataUrl)
        }
      } catch {
        // swallow — caller falls back to chip rendering on null
      }
    })()
    return () => {
      cancelled = true
    }
  }, [path])
  return url
}

// Open the attachment using the in-app playground preview when the file type is
// supported, otherwise fall back to the OS default app. `files` (the structured
// attachment array, present for messages sent in newer versions) lets us route
// to the workspace-aware preview by fileId; legacy bubbles only have a
// raw `file://` href and rely on `localPath`.
function useOpenLocalAttachment() {
  const openPreview = useGeneratedFilePreviewStore((s) => s.openPreview)
  return ({
    path,
    fileName,
    files,
    conversationId,
  }: {
    path: string
    fileName: string
    files?: FileAttachment[]
    conversationId?: string
  }) => {
    if (!path) return
    const matched = files?.find((f) => f.filePath === path)
    if (matched?.kind === 'folder') {
      void openLocalFile(path)
      return
    }
    const fileType = matched?.fileType
    if (isPreviewableFileType(fileType, matched?.fileName ?? fileName)) {
      openPreview({
        fileId: matched?.id ?? `local:${path}`,
        conversationId: conversationId ?? '',
        fileName: matched?.fileName ?? fileName,
        fileType,
        localPath: path,
      })
      return
    }
    void openLocalFile(path)
  }
}

function FileLinkChip({
  href,
  text,
  files,
  conversationId,
}: {
  href: string
  text: string
  files?: FileAttachment[]
  conversationId?: string
}) {
  const openAttachment = useOpenLocalAttachment()
  const path = fileUrlToPath(href) ?? ''
  const matched = useMemo(() => files?.find((f) => f.filePath === path), [path, files])
  const fileName = matched?.fileName ?? path.split(/[\\/]/).pop() ?? text
  const label = matched?.fileType
    ? FILE_TYPE_LABEL[matched.fileType]
    : inferLabelFromName(fileName)

  return (
    <button
      type="button"
      onClick={() => openAttachment({ path, fileName, files, conversationId })}
      aria-label={text}
      className="mx-0.5 my-0.5 inline-flex h-6 items-center gap-1.5 rounded-md bg-primary-foreground/15 px-2 align-middle text-xs leading-none text-primary-foreground transition-opacity hover:opacity-80"
      title={text}
    >
      <span aria-hidden="true" className="rounded bg-primary-foreground/15 px-1 text-xs font-bold">{label}</span>
      <span className="max-w-[200px] truncate">{text}</span>
    </button>
  )
}

function FileImage({
  href,
  alt,
  files,
  conversationId,
}: {
  href: string
  alt: string
  files?: FileAttachment[]
  conversationId?: string
}) {
  const openAttachment = useOpenLocalAttachment()
  const path = fileUrlToPath(href) ?? ''
  const url = useLocalImageThumb(path)
  // Use the path's basename (which carries the extension) for routing decisions
  // — `alt` is purely the markdown image's user-visible label and may not have
  // a recognizable extension, but isPreviewableFileType keys on extension.
  const fileName = path.split(/[\\/]/).pop() || alt || 'image'
  const ariaLabel = alt || fileName

  if (!url) {
    return (
      <FileLinkChip href={href} text={ariaLabel} files={files} conversationId={conversationId} />
    )
  }
  return (
    <button
      type="button"
      onClick={() => openAttachment({ path, fileName, files, conversationId })}
      aria-label={ariaLabel}
      title={ariaLabel}
      className="mx-0.5 my-0.5 inline-block align-middle"
    >
      <img
        src={url}
        alt={alt}
        className="max-h-40 max-w-[200px] rounded-lg object-cover transition-opacity hover:opacity-90"
      />
    </button>
  )
}

function allowFileUrl(url: string): string {
  // react-markdown v10 default urlTransform blocks file:// (not in safe protocol list).
  // We need file:// to pass through so our custom a/img components can render chips.
  if (url.startsWith('file://')) return url
  // For all other URLs, use the default safe-protocol check (https/http/mailto/etc.)
  const colon = url.indexOf(':')
  const slash = url.indexOf('/')
  const question = url.indexOf('?')
  const hash = url.indexOf('#')
  const safeProtocol = /^(https?|ircs?|mailto|xmpp)$/i
  if (
    colon === -1 ||
    (slash !== -1 && colon > slash) ||
    (question !== -1 && colon > question) ||
    (hash !== -1 && colon > hash) ||
    safeProtocol.test(url.slice(0, colon))
  ) {
    return url
  }
  return ''
}

// Older messages (before serializer commit 68f76c1f) stored bare `file://` URLs
// without angle-bracket wrapping. CommonMark's bare-URL form forbids spaces, so
// `[附件: 钉钉 skill](file:///.../钉钉 skill)` won't parse and falls back to raw
// text. We wrap any unwrapped file:// URL that contains whitespace in `<...>`
// so CommonMark accepts it. Idempotent: already-wrapped URLs are ignored, and
// URLs without whitespace are left alone (preserves CommonMark's native
// balanced-paren handling for cases like `v260407(1).pdf`).
function wrapBareFileUrlsWithSpaces(text: string): string {
  return text.replace(
    /(!?)\[((?:\\.|[^\]])*)\]\((file:\/\/[^>)]*\s[^>)]*)\)/g,
    (_match, bang, label, url) => `${bang}[${label}](<${url}>)`,
  )
}

export function UserBubbleMarkdown({ text, conversationId, files }: UserBubbleMarkdownProps) {
  if (!text.trim()) return null
  const normalized = useMemo(() => wrapBareFileUrlsWithSpaces(text), [text])
  return (
    <div className="user-bubble-markdown text-sm leading-relaxed">
      <ReactMarkdown
        remarkPlugins={[remarkGfm]}
        urlTransform={allowFileUrl}
        skipHtml
        components={{
          p: ({ children }) => <p className="leading-relaxed [&:not(:first-child)]:mt-2">{children}</p>,
          a: ({ href, children }) => {
            const hrefStr = typeof href === 'string' ? href : ''
            if (hrefStr.startsWith('file://')) {
              const text = String(children)
              return (
                <FileLinkChip href={hrefStr} text={text} files={files} conversationId={conversationId} />
              )
            }
            return (
              <a
                href={hrefStr}
                target="_blank"
                rel="noopener noreferrer"
                className="underline underline-offset-2"
              >
                {children}
              </a>
            )
          },
          img: ({ src, alt }) => {
            const srcStr = typeof src === 'string' ? src : ''
            const altStr = typeof alt === 'string' ? alt : ''
            if (srcStr.startsWith('file://')) {
              return <FileImage href={srcStr} alt={altStr} files={files} conversationId={conversationId} />
            }
            return (
              <img src={srcStr} alt={altStr} className="max-h-40 max-w-[200px] rounded-lg object-cover" />
            )
          },
          code: ({ className, children }) => {
            const isFenced = typeof className === 'string' && className.startsWith('language-')
            if (isFenced) {
              return <code className={className}>{children}</code>
            }
            return (
              <code className="rounded bg-primary-foreground/15 px-1 text-[0.8125em]">
                {children}
              </code>
            )
          },
          pre: ({ children }) => (
            <pre className="overflow-x-auto rounded bg-primary-foreground/10 p-2 text-xs">
              {children}
            </pre>
          ),
          ul: ({ children }) => <ul className="list-disc pl-5">{children}</ul>,
          ol: ({ children }) => <ol className="list-decimal pl-5">{children}</ol>,
          blockquote: ({ children }) => (
            <blockquote className="border-l-2 border-primary-foreground/40 pl-3 opacity-90">
              {children}
            </blockquote>
          ),
        }}
      >
        {normalized}
      </ReactMarkdown>
    </div>
  )
}

import { type MouseEvent, type ReactNode, useEffect, useMemo, useState } from 'react'
import { useGeneratedFilePreviewStore } from '@/stores/generatedFilePreviewStore'
import { getLocalFilePreview, openLocalFile } from '@/lib/tauri'
import { isPreviewableFileType } from '@/components/chat/generatedFileActions'

interface LocalMarkdownTarget {
  path: string
  fileName: string
}

interface FileLinkProps {
  href?: string
  children?: ReactNode
  conversationId?: string
  workspaceRoot?: string
}

interface FileImageProps {
  src?: string
  alt?: string
  conversationId?: string
  workspaceRoot?: string
}

function decodeUrlValue(value: string): string {
  try {
    return decodeURI(value)
  } catch {
    return value
  }
}

function basename(path: string): string {
  return path.split(/[\\/]/).filter(Boolean).pop() ?? path
}

function isExternalHref(href: string): boolean {
  return /^(https?:|mailto:|ircs?:|xmpp:)/i.test(href)
}

function isAbsoluteLocalPath(value: string): boolean {
  return value.startsWith('/') || /^[A-Za-z]:[\\/]/.test(value)
}

function joinWorkspacePath(root: string, relativePath: string): string | null {
  const rootValue = root.replace(/\/+$/, '')
  const parts: string[] = []
  for (const part of relativePath.replace(/\\/g, '/').split('/')) {
    if (!part || part === '.') continue
    if (part === '..') {
      if (parts.length === 0) return null
      parts.pop()
      continue
    }
    parts.push(part)
  }
  return `${rootValue}/${parts.join('/')}`
}

export function resolveMarkdownLocalTarget(
  href: string | undefined,
  workspaceRoot?: string,
): LocalMarkdownTarget | null {
  if (!href) return null
  const raw = href.trim()
  if (!raw || isExternalHref(raw)) return null

  if (raw.startsWith('file://')) {
    const stripped = decodeUrlValue(raw.slice('file://'.length))
    const path = /^\/[A-Za-z]:/.test(stripped) ? stripped.slice(1) : stripped
    return { path, fileName: basename(path) }
  }

  const decoded = decodeUrlValue(raw)
  if (isAbsoluteLocalPath(decoded)) {
    return { path: decoded, fileName: basename(decoded) }
  }

  if (!workspaceRoot) return null
  const path = joinWorkspacePath(workspaceRoot, decoded)
  return path ? { path, fileName: basename(path) } : null
}

export function allowMarkdownUrl(url: string): string {
  if (url.startsWith('file://')) return url
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

function useOpenMarkdownFile(conversationId?: string) {
  const openPreview = useGeneratedFilePreviewStore((s) => s.openPreview)
  return (target: LocalMarkdownTarget) => {
    if (isPreviewableFileType(undefined, target.fileName)) {
      openPreview({
        fileId: `local:${target.path}`,
        conversationId: conversationId ?? '',
        fileName: target.fileName,
        fileType: undefined,
        localPath: target.path,
      })
      return
    }
    void openLocalFile(target.path)
  }
}

export function FileLink({
  href,
  children,
  conversationId,
  workspaceRoot,
}: FileLinkProps) {
  const target = useMemo(
    () => resolveMarkdownLocalTarget(href, workspaceRoot),
    [href, workspaceRoot],
  )
  const openMarkdownFile = useOpenMarkdownFile(conversationId)

  const handleClick = (event: MouseEvent<HTMLAnchorElement>) => {
    if (!target) return
    event.preventDefault()
    openMarkdownFile(target)
  }

  if (!href) return <>{children}</>

  return (
    <a
      href={href}
      target={target ? undefined : '_blank'}
      rel={target ? undefined : 'noopener noreferrer'}
      onClick={handleClick}
    >
      {children}
    </a>
  )
}

export function FileImage({
  src,
  alt = '',
  conversationId,
  workspaceRoot,
}: FileImageProps) {
  const target = useMemo(
    () => resolveMarkdownLocalTarget(src, workspaceRoot),
    [src, workspaceRoot],
  )
  const openMarkdownFile = useOpenMarkdownFile(conversationId)
  const [dataUrl, setDataUrl] = useState<string | null>(null)
  const [checked, setChecked] = useState(false)

  useEffect(() => {
    setDataUrl(null)
    setChecked(false)
    if (!target) return
    let cancelled = false
    void (async () => {
      try {
        const preview = await getLocalFilePreview(target.path)
        if (cancelled) return
        if (preview.kind === 'image') {
          setDataUrl(preview.dataUrl)
        }
      } catch {
        // Fall back to a normal local file link below.
      } finally {
        if (!cancelled) setChecked(true)
      }
    })()
    return () => {
      cancelled = true
    }
  }, [target?.path])

  if (!src) return null

  if (!target) {
    return <img src={src} alt={alt} />
  }

  if (!checked) return null

  if (!dataUrl) {
    return (
      <FileLink href={src} conversationId={conversationId} workspaceRoot={workspaceRoot}>
        {alt || target.fileName}
      </FileLink>
    )
  }

  return (
    <button
      type="button"
      aria-label={alt || target.fileName}
      title={alt || target.fileName}
      className="my-1 inline-block align-middle"
      onClick={() => openMarkdownFile(target)}
    >
      <img src={dataUrl} alt={alt} className="h-40 max-w-[240px] rounded-lg object-cover" />
    </button>
  )
}

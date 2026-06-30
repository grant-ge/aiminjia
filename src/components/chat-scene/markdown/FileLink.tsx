import { type MouseEvent, type ReactNode, useEffect, useMemo, useState } from 'react'
import { useGeneratedFilePreviewStore } from '@/stores/generatedFilePreviewStore'
import { getLocalFilePreview, openGeneratedFile, openLocalFile, revealFileInFolder } from '@/lib/tauri'
import {
  getGeneratedFilePrimaryAction,
  isFileActionEnabled,
  isPreviewActionEnabledForFile,
  isPreviewableFileType,
  type PreviewTarget,
} from '@/components/chat/generatedFileActions'
import { savePreviewTargetToDisk } from '@/components/chat/fileDownload'
import { GeneratedFileCard } from '@/components/chat-scene/GeneratedFileCard'
import type { GeneratedFile } from '@/types/message'
import { Button } from '@/components/ui/button'
import {
  ARTIFACT_ALT,
  basename,
  decodeMarkdownUrlValue,
  findGeneratedFileForArtifactPath,
  inferArtifactFileType,
  isAbsoluteLocalPath,
  isExternalHref,
  normalizeComparablePath,
} from './artifactMarkdown'

interface LocalMarkdownTarget {
  path: string
  fileName: string
}

interface FileLinkProps {
  href?: string
  children?: ReactNode
  conversationId?: string
  workspaceRoot?: string
  generatedFiles?: GeneratedFile[]
}

interface FileImageProps {
  src?: string
  alt?: string
  conversationId?: string
  workspaceRoot?: string
  generatedFiles?: GeneratedFile[]
}

function resolveGeneratedFileTarget(
  href: string,
  generatedFiles: GeneratedFile[] | undefined,
): LocalMarkdownTarget | null {
  if (!generatedFiles?.length) return null
  const decoded = normalizeComparablePath(href.trim())
  if (!decoded || isExternalHref(decoded) || isAbsoluteLocalPath(decoded) || decoded.startsWith('file://')) {
    return null
  }
  const sourceName = basename(decoded)
  const isGeneratedReference = decoded.startsWith('generated/') || decoded.includes('/generated/')
  if (!isGeneratedReference && !sourceName) return null

  for (const file of generatedFiles) {
    const filePath = file.filePath?.trim()
    if (!filePath) continue
    const comparableFilePath = normalizeComparablePath(filePath)
    const fileName = file.fileName || basename(comparableFilePath)
    const pathMatches = comparableFilePath.endsWith(`/${decoded}`)
    const nameMatches = isGeneratedReference && sourceName === fileName
    if (pathMatches || nameMatches) {
      return { path: filePath, fileName }
    }
  }
  return null
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
  generatedFiles?: GeneratedFile[],
): LocalMarkdownTarget | null {
  if (!href) return null
  const raw = href.trim()
  if (!raw || isExternalHref(raw)) return null

  if (raw.startsWith('file://')) {
    const stripped = decodeMarkdownUrlValue(raw.slice('file://'.length))
    const path = /^\/[A-Za-z]:/.test(stripped) ? stripped.slice(1) : stripped
    return { path, fileName: basename(path) }
  }

  const decoded = decodeMarkdownUrlValue(raw)
  if (isAbsoluteLocalPath(decoded)) {
    return { path: decoded, fileName: basename(decoded) }
  }

  const generatedTarget = resolveGeneratedFileTarget(decoded, generatedFiles)
  if (generatedTarget) return generatedTarget

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
  generatedFiles,
}: FileLinkProps) {
  const target = useMemo(
    () => resolveMarkdownLocalTarget(href, workspaceRoot, generatedFiles),
    [href, workspaceRoot, generatedFiles],
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
  generatedFiles,
}: FileImageProps) {
  if (alt === ARTIFACT_ALT) {
    return (
      <MarkdownArtifactCard
        src={src}
        conversationId={conversationId}
        workspaceRoot={workspaceRoot}
        generatedFiles={generatedFiles}
      />
    )
  }

  return (
    <InlineFileImage
      src={src}
      alt={alt}
      conversationId={conversationId}
      workspaceRoot={workspaceRoot}
      generatedFiles={generatedFiles}
    />
  )
}

function InlineFileImage({
  src,
  alt = '',
  conversationId,
  workspaceRoot,
  generatedFiles,
}: FileImageProps) {
  const target = useMemo(
    () => resolveMarkdownLocalTarget(src, workspaceRoot, generatedFiles),
    [src, workspaceRoot, generatedFiles],
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
    <Button unstyled
      type="button"
      aria-label={alt || target.fileName}
      title={alt || target.fileName}
      className="my-1 inline-block align-middle"
      onClick={() => openMarkdownFile(target)}
    >
      <img src={dataUrl} alt={alt} className="h-40 max-w-[240px] rounded-md object-cover" />
    </Button>
  )
}

function formatArtifactSize(bytes: number | undefined): string | null {
  if (bytes == null || bytes <= 0) return null
  if (bytes < 1024) return `${bytes} B`
  if (bytes < 1024 * 1024) return `${Math.round(bytes / 1024)} KB`
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`
}

function artifactTypeLabel(fileType: string | undefined, fileName: string): string {
  const type = fileType?.trim().toLowerCase() || inferArtifactFileType(fileName)
  if (type === 'image') return '图片'
  if (type === 'markdown' || type === 'md') return 'Markdown'
  if (type === 'excel') return 'XLS'
  if (type === 'word') return 'DOC'
  if (type === 'ppt') return 'PPT'
  if (type === 'pdf') return 'PDF'
  const ext = fileName.includes('.') ? fileName.split('.').pop()?.toUpperCase() : undefined
  return ext || '文件'
}

function artifactSub(file: GeneratedFile | null, fileName: string): string {
  const size = formatArtifactSize(file?.fileSize)
  const type = artifactTypeLabel(file?.fileType, fileName)
  return [size, type].filter(Boolean).join(' · ')
}

function MarkdownArtifactCard({
  src,
  conversationId,
  workspaceRoot,
  generatedFiles,
}: FileImageProps) {
  const generatedFile = useMemo(
    () => findGeneratedFileForArtifactPath(src, generatedFiles),
    [src, generatedFiles],
  )
  const target = useMemo(
    () => resolveMarkdownLocalTarget(src, workspaceRoot, generatedFiles),
    [src, workspaceRoot, generatedFiles],
  )
  const openPreview = useGeneratedFilePreviewStore((s) => s.openPreview)

  if (!src || (!generatedFile && !target)) return null

  const fileName = generatedFile?.fileName || target?.fileName || basename(src)
  const filePath = generatedFile?.filePath || target?.path
  const fileType = generatedFile?.fileType || inferArtifactFileType(fileName)
  const title = generatedFile?.title || fileName
  const actions = generatedFile?.actions
  const canPreview = isPreviewActionEnabledForFile(actions, fileType, fileName)
  const canOpenExternal = isFileActionEnabled(actions, 'open')
  const canReveal = isFileActionEnabled(actions, 'reveal')
  const primaryAction = getGeneratedFilePrimaryAction({ fileType, fileName })
  const previewTarget: PreviewTarget | null = generatedFile && conversationId
    ? {
        fileId: generatedFile.id,
        conversationId,
        fileName,
        fileType,
      }
    : target && conversationId
      ? {
          fileId: `artifact:${target.path}`,
          conversationId,
          fileName,
          fileType,
          localPath: target.path,
        }
      : null

  const handlePreview = () => {
    if (!previewTarget) return
    openPreview(previewTarget)
  }
  const handleOpenExternal = () => {
    if (generatedFile && conversationId) {
      void openGeneratedFile(generatedFile.id, conversationId)
      return
    }
    if (target) void openLocalFile(target.path)
  }
  const handleReveal = () => {
    if (generatedFile && conversationId) {
      void revealFileInFolder(generatedFile.id, conversationId)
      return
    }
    if (!target) return
    const parent = target.path.replace(/[/\\][^/\\]+$/, '') || '/'
    void openLocalFile(parent)
  }
  const handleDownload = () => {
    if (!previewTarget) return
    void savePreviewTargetToDisk(previewTarget)
  }

  return (
    <div className="my-2">
      <GeneratedFileCard
        title={title}
        sub={artifactSub(generatedFile, fileName)}
        appName={primaryAction === 'preview' ? '预览' : '打开'}
        primaryAction={primaryAction}
        canPreview={canPreview}
        canOpenExternal={canOpenExternal}
        canDownload={Boolean(previewTarget)}
        canReveal={canReveal}
        filePath={filePath}
        onPreview={handlePreview}
        onOpenExternal={handleOpenExternal}
        onDownload={handleDownload}
        onReveal={handleReveal}
      />
    </div>
  )
}

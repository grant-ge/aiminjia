/**
 * @designSource design.pen#v46uG
 * @sizing height 64 r-14 border 1 bg card padding x16; clipped tilted monochrome file icon
 */
import type { ReactNode } from 'react'
import { useTranslation } from 'react-i18next'
import { ChevronDown, Download, ExternalLink, Eye, FolderOpen } from 'lucide-react'

import type { GeneratedFilePrimaryAction } from '@/components/chat/generatedFileActions'
import { AppDropdown, type AppDropdownItem } from '@/components/common/AppDropdown'
import { Button } from '@/components/ui/button'

interface GeneratedFileCardProps {
  title: string
  sub: string
  appName: string
  appIcon?: ReactNode
  primaryAction?: GeneratedFilePrimaryAction
  canPreview?: boolean
  canOpenExternal?: boolean
  canDownload?: boolean
  canReveal?: boolean
  /** 文件绝对路径，用于 e2e CLI 按 filePath 子串定位卡片 */
  filePath?: string
  onOpen?: () => void
  onPreview?: () => void
  onOpenExternal?: () => void
  onDownload?: () => void
  onReveal?: () => void
}

function normalizeFileLabel(raw: string | undefined): string | null {
  const value = raw?.trim().toUpperCase()
  if (!value) return null
  if (value === 'XLSX' || value === 'EXCEL') return 'XLS'
  if (value === 'PPTX' || value === 'POWERPOINT') return 'PPT'
  if (value === 'DOCX' || value === 'WORD') return 'DOC'
  if (value === 'JPEG') return 'JPG'
  return value.slice(0, 4)
}

function fileLabelFromTitle(title: string, sub: string): string {
  const ext = title.includes('.') ? title.split('.').pop() : undefined
  const subMatch = sub.match(/\b([A-Za-z0-9]{2,5})\b$/)
  return normalizeFileLabel(ext) ?? normalizeFileLabel(subMatch?.[1]) ?? 'FILE'
}

function useActionLabels() {
  const { t } = useTranslation()
  return {
    open: t('fileCard.open'),
    preview: t('fileCard.preview'),
    more: t('fileCard.more'),
    previewInside: t('fileCard.previewInside'),
    previewUnavailable: t('fileCard.previewUnavailable'),
    openExternal: t('fileCard.openExternal'),
    download: t('fileCard.download'),
    reveal: t('fileCard.reveal'),
  }
}

function TiltedFileIcon({ title, sub }: { title: string; sub: string }) {
  const label = fileLabelFromTitle(title, sub)

  return (
    <div
      aria-label={`${label} file icon`}
      className="relative -left-[5px] h-14 w-12 shrink-0 translate-y-2 rotate-[-8deg] text-muted-foreground"
    >
      <svg
        viewBox="0 0 48 56"
        className="absolute inset-0 h-full w-full overflow-visible drop-shadow-[0_8px_14px_rgba(0,0,0,0.08)]"
        fill="none"
        aria-hidden="true"
      >
        <path
          d="M8.5 1.5H33.5L46.5 14.5V49C46.5 52.0376 44.0376 54.5 41 54.5H8.5C4.63401 54.5 1.5 51.366 1.5 47.5V8.5C1.5 4.63401 4.63401 1.5 8.5 1.5Z"
          className="fill-background stroke-[rgba(var(--foreground-rgb),0.20)]"
          strokeWidth="1.5"
        />
        <path
          d="M33.5 2V10.5C33.5 12.7091 35.2909 14.5 37.5 14.5H46"
          className="stroke-[rgba(var(--foreground-rgb),0.20)]"
          strokeWidth="1.5"
        />
        <path d="M13 21H34" className="stroke-[rgba(var(--foreground-rgb),0.20)]" strokeWidth="3" strokeLinecap="round" />
        <path d="M13 29H29" className="stroke-[rgba(var(--foreground-rgb),0.15)]" strokeWidth="3" strokeLinecap="round" />
      </svg>
      <div className="absolute bottom-2 left-1/2 -translate-x-1/2 whitespace-nowrap text-[0.5625rem] font-semibold tracking-[0.14em] text-muted-foreground">
        {label}
      </div>
    </div>
  )
}

export function GeneratedFileCard({
  title,
  sub,
  appName,
  appIcon,
  primaryAction = 'open',
  canPreview = false,
  canOpenExternal = true,
  canDownload = true,
  canReveal = true,
  filePath,
  onOpen,
  onPreview,
  onOpenExternal,
  onDownload,
  onReveal,
}: GeneratedFileCardProps) {
  const ACTION_LABELS = useActionLabels()
  const openExternalAction = onOpenExternal ?? onOpen
  const previewAction = canPreview ? onPreview : undefined
  const enabledOpenExternalAction = canOpenExternal !== false ? openExternalAction : undefined
  const downloadAction = canDownload !== false ? onDownload : undefined
  const revealAction = canReveal !== false ? onReveal : undefined
  const previewEnabled = Boolean(previewAction)
  const openEnabled = Boolean(enabledOpenExternalAction)
  const downloadEnabled = Boolean(downloadAction)
  const revealEnabled = Boolean(revealAction)
  const isPreviewPrimary = primaryAction === 'preview'
  const primaryLabel = isPreviewPrimary ? ACTION_LABELS.preview : ACTION_LABELS.open
  const isPrimaryDisabled = isPreviewPrimary ? !previewEnabled : !openEnabled

  const handlePrimaryAction = () => {
    if (isPreviewPrimary) {
      previewAction?.()
      return
    }
    enabledOpenExternalAction?.()
  }

  const menuItems: AppDropdownItem[] = [
    {
      id: 'preview',
      label: previewEnabled ? ACTION_LABELS.previewInside : ACTION_LABELS.previewUnavailable,
      icon: <Eye className="h-4 w-4" />,
      disabled: !previewEnabled,
      onSelect: () => previewAction?.(),
    },
    {
      id: 'open-external',
      label: ACTION_LABELS.openExternal,
      icon: <ExternalLink className="h-4 w-4" />,
      disabled: !openEnabled,
      onSelect: () => enabledOpenExternalAction?.(),
    },
    {
      id: 'download',
      label: ACTION_LABELS.download,
      icon: <Download className="h-4 w-4" />,
      disabled: !downloadEnabled,
      onSelect: () => downloadAction?.(),
    },
    {
      id: 'reveal',
      label: ACTION_LABELS.reveal,
      icon: <FolderOpen className="h-4 w-4" />,
      disabled: !revealEnabled,
      onSelect: () => revealAction?.(),
    },
  ]

  return (
    <div data-testid="generated-file-card" data-aijia-file-path={filePath ?? ''} className="flex h-16 items-center justify-between gap-4 overflow-hidden rounded-md border border-border bg-card px-4">
      <div className="flex min-w-0 items-center gap-2">
        <div className="flex h-16 w-12 shrink-0 items-center justify-center">
          <TiltedFileIcon title={title} sub={sub} />
        </div>
        <div className="flex min-w-0 flex-col gap-0.5">
          <div className="truncate text-sm font-semibold leading-5 text-foreground">{title}</div>
          <div className="truncate text-xs leading-4 text-muted-foreground">{sub}</div>
        </div>
      </div>
      <div className="flex shrink-0 items-center rounded-md border border-border bg-background text-sm text-foreground">
        <Button unstyled
          type="button"
          onClick={handlePrimaryAction}
          disabled={isPrimaryDisabled}
          aria-label={`${primaryLabel} ${title}`}
          className="flex items-center gap-2 rounded-l-md py-1.5 pl-3 pr-2 transition-colors hover:bg-muted disabled:pointer-events-none disabled:opacity-50"
        >
          {appIcon}
          <span>{appName}</span>
        </Button>
        <span className="h-4 w-px bg-border" />
        <AppDropdown
          ariaLabel={`${ACTION_LABELS.more}：${title}`}
          contentClassName="min-w-48"
          trigger={
            <Button unstyled
              type="button"
              className="flex items-center rounded-r-md py-1.5 pl-2 pr-2 transition-colors hover:bg-muted"
            >
              <ChevronDown className="h-3.5 w-3.5 text-muted-foreground" />
            </Button>
          }
          items={menuItems}
        />
      </div>
    </div>
  )
}

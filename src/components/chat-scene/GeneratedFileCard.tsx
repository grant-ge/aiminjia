/**
 * @designSource design.pen#v46uG
 * @sizing height 64 r-14 border 1 bg card padding x16; clipped tilted monochrome file icon
 */
import type { ReactNode } from 'react'
import { ChevronDown } from 'lucide-react'

interface GeneratedFileCardProps {
  title: string
  sub: string
  appName: string
  appIcon?: ReactNode
  onOpen: () => void
}

function normalizeFileLabel(raw: string | undefined): string | null {
  const value = raw?.trim().toUpperCase()
  if (!value) return null
  if (value === 'XLSX' || value === 'EXCEL') return 'XLS'
  if (value === 'JPEG') return 'JPG'
  return value.slice(0, 4)
}

function fileLabelFromTitle(title: string, sub: string): string {
  const ext = title.includes('.') ? title.split('.').pop() : undefined
  const subMatch = sub.match(/\b([A-Za-z0-9]{2,5})\b$/)
  return normalizeFileLabel(ext) ?? normalizeFileLabel(subMatch?.[1]) ?? 'FILE'
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
          className="fill-background stroke-foreground/20"
          strokeWidth="1.5"
        />
        <path
          d="M33.5 2V10.5C33.5 12.7091 35.2909 14.5 37.5 14.5H46"
          className="stroke-foreground/20"
          strokeWidth="1.5"
        />
        <path d="M13 21H34" className="stroke-foreground/20" strokeWidth="3" strokeLinecap="round" />
        <path d="M13 29H29" className="stroke-foreground/15" strokeWidth="3" strokeLinecap="round" />
      </svg>
      <div className="absolute bottom-2 left-1/2 -translate-x-1/2 text-[0.5625rem] font-semibold tracking-[0.14em] text-muted-foreground">
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
  onOpen,
}: GeneratedFileCardProps) {
  return (
    <div data-testid="generated-file-card" className="flex h-16 items-center justify-between gap-4 overflow-hidden rounded-[14px] border border-border bg-card px-4">
      <div className="flex min-w-0 items-center gap-2">
        <div className="flex h-16 w-12 shrink-0 items-center justify-center">
          <TiltedFileIcon title={title} sub={sub} />
        </div>
        <div className="flex min-w-0 flex-col gap-0.5">
          <div className="truncate text-sm font-semibold leading-5 text-foreground">{title}</div>
          <div className="truncate text-xs leading-4 text-muted-foreground">{sub}</div>
        </div>
      </div>
      <button
        type="button"
        onClick={onOpen}
        aria-label={`${appName} open`}
        className="flex shrink-0 items-center gap-2 rounded-full border border-border bg-background py-1.5 pl-3 pr-1.5 text-[0.8125rem] text-foreground transition-colors hover:bg-muted"
      >
        {appIcon}
        <span>{appName}</span>
        <span className="mx-1 h-4 w-px bg-border" />
        <ChevronDown className="h-3.5 w-3.5 text-muted-foreground" />
      </button>
    </div>
  )
}

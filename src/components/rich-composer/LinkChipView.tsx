import { useState } from 'react'
import { Link2, X } from 'lucide-react'

import { NodeViewWrapper } from '@tiptap/react'

import { cn } from '@/lib/utils'
import { Button } from '@/components/ui/button'

interface LinkChipViewProps {
  node: { attrs: { url: string } }
  deleteNode: () => void
}

function hostFromUrl(url: string): string {
  try {
    return new URL(url).host.replace(/^www\./, '')
  } catch {
    return url
  }
}

function faviconUrlFor(url: string): string | null {
  try {
    const u = new URL(url)
    return `${u.protocol}//${u.host}/favicon.ico`
  } catch {
    return null
  }
}

export function LinkChipView({ node, deleteNode }: LinkChipViewProps) {
  const url = node.attrs.url
  const host = hostFromUrl(url)
  const favicon = faviconUrlFor(url)
  const [faviconBroken, setFaviconBroken] = useState(false)

  return (
    <NodeViewWrapper
      as="span"
      data-link-chip
      contentEditable={false}
      title={url}
      className={cn(
        'inline-flex max-w-[220px] items-center gap-1 rounded-md border border-border bg-muted px-1.5 py-0.5 align-middle text-xs leading-none text-foreground',
      )}
    >
      {favicon && !faviconBroken ? (
        <img
          src={favicon}
          alt=""
          className="h-3.5 w-3.5 shrink-0"
          referrerPolicy="no-referrer"
          onError={() => setFaviconBroken(true)}
        />
      ) : (
        <Link2 className="h-3.5 w-3.5 shrink-0 text-muted-foreground" />
      )}
      <span className="truncate">{host}</span>
      <Button unstyled
        type="button"
        aria-label="remove link"
        onMouseDown={(e) => e.preventDefault()}
        onClick={(e) => {
          e.preventDefault()
          e.stopPropagation()
          deleteNode()
        }}
        className="ml-0.5 inline-flex h-4 w-4 shrink-0 items-center justify-center rounded hover:bg-background"
      >
        <X className="h-3 w-3" />
      </Button>
    </NodeViewWrapper>
  )
}

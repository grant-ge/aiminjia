import { NodeViewWrapper } from '@tiptap/react'
import { Blocks, X } from 'lucide-react'
import { cn } from '@/lib/utils'
import type { ComposerSkillToken } from './types'

interface SkillTokenViewProps {
  node: { attrs: ComposerSkillToken }
  deleteNode: () => void
}

export function SkillTokenView({ node, deleteNode }: SkillTokenViewProps) {
  const attrs = node.attrs
  return (
    <NodeViewWrapper
      as="span"
      data-skill-chip
      contentEditable={false}
      className={cn(
        'inline-flex max-w-[180px] items-center gap-1 rounded-md border border-border bg-muted px-1.5 py-0.5 text-xs leading-none text-foreground',
      )}
      title={attrs.command}
    >
      <Blocks aria-label="skill" className="h-3.5 w-3.5 shrink-0 text-muted-foreground" />
      <span className="truncate">{attrs.label}</span>
      <button
        type="button"
        aria-label={`remove skill ${attrs.label}`}
        onMouseDown={(event) => event.preventDefault()}
        onClick={(event) => {
          event.preventDefault()
          event.stopPropagation()
          deleteNode()
        }}
        className="ml-0.5 inline-flex h-4 w-4 shrink-0 items-center justify-center rounded hover:bg-background"
      >
        <X className="h-3 w-3" />
      </button>
    </NodeViewWrapper>
  )
}

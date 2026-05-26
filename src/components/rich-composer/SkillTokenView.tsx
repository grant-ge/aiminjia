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
        // .skill-token-chip injects the animated gradient + breathing glow
        // (defined in globals.css). Keep tailwind classes for layout/typo only.
        'skill-token-chip relative inline-flex max-w-[200px] items-center gap-1.5 rounded-md px-2 py-1 text-xs font-semibold leading-none text-primary',
      )}
      title={attrs.command}
    >
      <Blocks
        aria-label="skill"
        className="h-3.5 w-3.5 shrink-0"
        style={{ filter: 'drop-shadow(0 0 4px rgba(var(--primary-rgb), 0.45))' }}
      />
      <span className="truncate tracking-tight">{attrs.label}</span>
      <button
        type="button"
        aria-label={`remove skill ${attrs.label}`}
        onMouseDown={(event) => event.preventDefault()}
        onClick={(event) => {
          event.preventDefault()
          event.stopPropagation()
          deleteNode()
        }}
        className="ml-0.5 inline-flex h-4 w-4 shrink-0 items-center justify-center rounded text-primary/60 transition-all hover:bg-primary/20 hover:text-primary"
      >
        <X className="h-3 w-3" />
      </button>
    </NodeViewWrapper>
  )
}

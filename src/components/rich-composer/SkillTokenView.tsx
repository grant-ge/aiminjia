import { NodeViewWrapper } from '@tiptap/react'
import { Blocks, X } from 'lucide-react'
import type { ComposerSkillToken } from './types'
import { Tag } from '@/components/common/Tag'
import { Button } from '@/components/ui/button'

interface SkillTokenViewProps {
  node: { attrs: ComposerSkillToken }
  deleteNode: () => void
}

export function SkillTokenView({ node, deleteNode }: SkillTokenViewProps) {
  const attrs = node.attrs
  return (
    <NodeViewWrapper
      as="span"
      contentEditable={false}
      title={attrs.command}
    >
      <Tag
        data-skill-chip
        size="sm"
        color="primary"
        className="skill-token-chip relative max-w-[200px] px-2 font-semibold text-primary"
        icon={
          <Blocks
            aria-label="skill"
            aria-hidden={false}
            style={{ filter: 'drop-shadow(0 0 4px rgba(var(--primary-rgb), 0.45))' }}
          />
        }
      >
        <span className="truncate">{attrs.label}</span>
        <Button unstyled
          type="button"
          aria-label={`remove skill ${attrs.label}`}
          onMouseDown={(event) => event.preventDefault()}
          onClick={(event) => {
            event.preventDefault()
            event.stopPropagation()
            deleteNode()
          }}
          className="ml-0.5 inline-flex h-4 w-4 shrink-0 items-center justify-center rounded text-[rgba(var(--primary-rgb),0.60)] transition-all hover:bg-[rgba(var(--primary-rgb),0.20)] hover:text-primary"
        >
          <X className="h-3 w-3" />
        </Button>
      </Tag>
    </NodeViewWrapper>
  )
}

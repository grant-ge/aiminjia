import { MarkdownCodeBlock, textFromNode } from '@/components/chat-scene/markdown/MarkdownCodeBlock'
import { parseAijiaCardPayload } from './aijiaCardPayload'
import { ScheduleCreatedCard } from './ScheduleCreatedCard'
import { SkillCreatedCard } from './SkillCreatedCard'

interface CodeProps {
  inline?: boolean
  className?: string
  children?: React.ReactNode
}

export function AijiaCardCodeBlock(props: CodeProps) {
  const rawCodeText = textFromNode(props.children).replace(/\n$/, '')
  const classNames = new Set((props.className ?? '').split(/\s+/).filter(Boolean))
  const isAijiaCard = !props.inline && classNames.has('language-aijia-card')

  if (!isAijiaCard) return <MarkdownCodeBlock {...props} />

  const payload = parseAijiaCardPayload(rawCodeText)
  if (!payload) return <MarkdownCodeBlock {...props} />

  if (payload.type === 'skill_created') {
    return <SkillCreatedCard payload={payload} />
  }

  return <ScheduleCreatedCard payload={payload} />
}

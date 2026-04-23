/**
 * @designSource design.pen#uq6ga ChatComposerCompact (home page variant)
 *
 * Home page wrapper around ChatComposerCompact.
 * Send/submit is a no-op here — actual chat starts when the user
 * navigates to the chat page. This component is purely visual.
 */
import { useState } from 'react'

import { ChatComposerCompact } from '@/components/chat-scene/ChatComposerCompact'

export function HomeTaskComposerCard() {
  const [value, setValue] = useState('')

  return (
    <ChatComposerCompact
      value={value}
      onChange={setValue}
      onSubmit={() => {}}
      placeholder="描述你的任务，或输入 / 选择技能来开始..."
    />
  )
}

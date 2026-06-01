import '@testing-library/jest-dom'
import { fireEvent, render, screen } from '@testing-library/react'
import { beforeEach, describe, expect, it } from 'vitest'

import i18n from '@/i18n'
import { ToolTraceIO } from '../ToolTraceIO'

describe('ToolTraceIO', () => {
  beforeEach(async () => {
    await i18n.changeLanguage('zh-CN')
  })

  it('localizes collapsible output controls in English', async () => {
    await i18n.changeLanguage('en-US')
    const output = Array.from({ length: 8 }, (_, i) => `line ${i + 1}`).join('\n')

    render(<ToolTraceIO output={output} />)

    fireEvent.click(screen.getByRole('button', { name: 'Show 3 more lines' }))
    expect(screen.getByRole('button', { name: 'Collapse' })).toBeInTheDocument()
    expect(screen.queryByRole('button', { name: '收起' })).not.toBeInTheDocument()
  })
})

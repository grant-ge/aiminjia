import '@testing-library/jest-dom'
import { fireEvent, render, screen } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

import { FilePreviewPane } from './FilePreviewPane'
import type { PreviewTarget } from './generatedFileActions'

const target: PreviewTarget = {
  fileId: 'gf-1',
  conversationId: 'conv-1',
  fileName: 'summary.md',
  fileType: 'markdown',
}

describe('FilePreviewPane', () => {
  it('shows an empty state when no target is selected', () => {
    render(<FilePreviewPane target={null} onOpenExternal={vi.fn()} />)

    expect(screen.getByText('选择一个产物进行预览')).toBeInTheDocument()
  })

  it('shows the target shell and opens it externally', () => {
    const onOpenExternal = vi.fn()
    render(<FilePreviewPane target={target} onOpenExternal={onOpenExternal} />)

    expect(screen.getByText('summary.md')).toBeInTheDocument()
    expect(screen.getByText('预览内容加载能力将在下一阶段接入')).toBeInTheDocument()

    fireEvent.click(screen.getByRole('button', { name: 'Open with default app' }))

    expect(onOpenExternal).toHaveBeenCalledWith(target)
  })
})

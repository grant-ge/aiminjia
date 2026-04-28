import '@testing-library/jest-dom'
import { fireEvent, render, screen } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

import type { GeneratedFile } from '@/types/message'
import { GeneratedFileCard } from './GeneratedFileCard'

const baseFile: GeneratedFile = {
  id: 'f-1',
  fileName: 'chart.png',
  filePath: '/tmp/chart.png',
  fileType: 'png',
  fileSize: 2048,
  category: 'chart',
  version: 1,
  isLatest: true,
  createdAt: '2026-04-19T10:00:00Z',
  description: '图表',
  actions: [],
}

describe('GeneratedFileCard inline preview', () => {
  it('renders inline preview for image files', () => {
    render(<GeneratedFileCard file={baseFile} />)

    const image = screen.getByRole('img', { name: 'chart.png 预览图' })
    expect(image).toBeInTheDocument()
    expect(image).toHaveAttribute('src', 'file:///tmp/chart.png')
  })

  it('does not render inline preview for non-image files', () => {
    render(<GeneratedFileCard file={{ ...baseFile, fileType: 'pdf', fileName: 'report.pdf', filePath: '/tmp/report.pdf' }} />)

    expect(screen.queryByRole('img', { name: /预览图/ })).toBeNull()
  })

  it('renders inline preview for image extensions beyond png', () => {
    render(
      <GeneratedFileCard
        file={{ ...baseFile, fileType: 'png', fileName: 'diagram.webp', filePath: '/tmp/diagram.webp' }}
      />,
    )

    expect(screen.getByRole('img', { name: 'diagram.webp 预览图' })).toHaveAttribute(
      'src',
      'file:///tmp/diagram.webp',
    )
  })

  it('renders safely when fileType is missing', () => {
    render(<GeneratedFileCard file={{ ...baseFile, fileType: undefined, fileName: 'report.md', filePath: '/tmp/report.md' }} />)

    expect(screen.getByText('report.md')).toBeInTheDocument()
    expect(screen.queryByRole('img', { name: /预览图/ })).toBeNull()
  })

  it('disables built-in open actions when file actions explicitly disable them', () => {
    const onAction = vi.fn()

    render(
      <GeneratedFileCard
        file={{
          ...baseFile,
          actions: [
            { type: 'open', label: 'Open', enabled: false },
            { type: 'reveal', label: 'Open Folder', enabled: false },
          ],
        }}
        onAction={onAction}
      />,
    )

    const openButton = screen.getByRole('button', { name: 'Open' })
    const revealButton = screen.getByRole('button', { name: 'Open Folder' })
    expect(openButton).toBeDisabled()
    expect(revealButton).toBeDisabled()

    fireEvent.click(openButton)
    fireEvent.click(revealButton)
    expect(onAction).not.toHaveBeenCalled()
  })

  it('keeps built-in open actions enabled when file actions are missing', () => {
    const onAction = vi.fn()
    const { actions: _actions, ...fileWithoutActions } = baseFile

    render(<GeneratedFileCard file={fileWithoutActions} onAction={onAction} />)

    fireEvent.click(screen.getByRole('button', { name: 'Open' }))
    fireEvent.click(screen.getByRole('button', { name: 'Open Folder' }))
    expect(onAction).toHaveBeenNthCalledWith(1, 'f-1', 'open')
    expect(onAction).toHaveBeenNthCalledWith(2, 'f-1', 'reveal')
  })
})

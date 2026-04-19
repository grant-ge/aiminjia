import '@testing-library/jest-dom'
import { render, screen } from '@testing-library/react'
import { describe, expect, it } from 'vitest'

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
        file={{ ...baseFile, fileType: 'other', fileName: 'diagram.webp', filePath: '/tmp/diagram.webp' }}
      />,
    )

    expect(screen.getByRole('img', { name: 'diagram.webp 预览图' })).toHaveAttribute(
      'src',
      'file:///tmp/diagram.webp',
    )
  })
})

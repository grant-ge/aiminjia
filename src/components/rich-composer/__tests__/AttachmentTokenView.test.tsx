import '@testing-library/jest-dom'
import { describe, expect, it, vi } from 'vitest'
import { render, screen, fireEvent } from '@testing-library/react'
import { AttachmentTokenView } from '../AttachmentTokenView'
import type { ComposerAttachmentToken } from '../types'

const mkAttrs = (overrides: Partial<ComposerAttachmentToken> = {}): ComposerAttachmentToken => ({
  id: 'a1',
  fileName: 'plan.pdf',
  path: '/abs/plan.pdf',
  kind: 'file',
  fileType: 'pdf',
  fileSize: 2048,
  source: 'picker',
  ...overrides,
})

describe('AttachmentTokenView', () => {
  it('显示文件名', () => {
    const node = { attrs: mkAttrs() } as never
    render(<AttachmentTokenView node={node} deleteNode={() => {}} />)
    expect(screen.getByText('plan.pdf')).toBeInTheDocument()
  })

  it('文件 kind → 显示 fileType 标签', () => {
    const node = { attrs: mkAttrs({ fileType: 'pdf' }) } as never
    render(<AttachmentTokenView node={node} deleteNode={() => {}} />)
    expect(screen.getByText('PDF')).toBeInTheDocument()
  })

  it('image kind → 显示 image 图标 (aria-label "image attachment")', () => {
    const node = { attrs: mkAttrs({ kind: 'image', fileType: 'image' }) } as never
    render(<AttachmentTokenView node={node} deleteNode={() => {}} />)
    expect(screen.getByLabelText('image attachment')).toBeInTheDocument()
  })

  it('folder kind → 显示 folder 图标 (aria-label "folder attachment")', () => {
    const node = { attrs: mkAttrs({ kind: 'folder', fileType: 'folder' }) } as never
    render(<AttachmentTokenView node={node} deleteNode={() => {}} />)
    expect(screen.getByLabelText('folder attachment')).toBeInTheDocument()
  })

  it('点击删除按钮 → 调用 deleteNode', () => {
    const deleteNode = vi.fn()
    const node = { attrs: mkAttrs() } as never
    render(<AttachmentTokenView node={node} deleteNode={deleteNode} />)
    fireEvent.click(screen.getByLabelText('remove attachment'))
    expect(deleteNode).toHaveBeenCalledTimes(1)
  })
})

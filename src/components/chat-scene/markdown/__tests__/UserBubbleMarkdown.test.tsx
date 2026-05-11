import '@testing-library/jest-dom'
import { describe, expect, it, vi, beforeEach } from 'vitest'
import { render, screen, fireEvent, waitFor } from '@testing-library/react'
import { UserBubbleMarkdown } from '../UserBubbleMarkdown'

const mockOpenLocalFile = vi.fn()
const mockOpenPreview = vi.fn()
const mockGetLocalFilePreview = vi.fn()

vi.mock('@/lib/tauri', () => ({
  openLocalFile: (...args: unknown[]) => mockOpenLocalFile(...args),
  getLocalFilePreview: (...args: unknown[]) => mockGetLocalFilePreview(...args),
}))

vi.mock('@/stores/generatedFilePreviewStore', () => ({
  useGeneratedFilePreviewStore: (selector: (s: { openPreview: typeof mockOpenPreview }) => unknown) =>
    selector({ openPreview: mockOpenPreview }),
}))

vi.mock('@/components/chat/generatedFileActions', () => ({
  isPreviewableFileType: (fileType: string | undefined, fileName?: string) => {
    if (fileType === 'pdf' || fileType === 'image') return true
    if (!fileName) return false
    return /\.(png|jpg|jpeg|gif|webp|bmp|svg|pdf|md|txt|json|csv|html)$/i.test(fileName)
  },
}))

beforeEach(() => {
  mockOpenLocalFile.mockReset()
  mockOpenPreview.mockReset()
  mockGetLocalFilePreview.mockReset()
  // Default: report image kind unavailable so FileImage falls back to chip and
  // tests stay deterministic. Image-thumbnail tests override per-case.
  mockGetLocalFilePreview.mockResolvedValue({
    kind: 'unsupported',
    fileName: '',
    reason: 'unsupported',
  })
})

describe('UserBubbleMarkdown', () => {
  it('renders plain text in a paragraph', () => {
    render(<UserBubbleMarkdown text="hello world" />)
    expect(screen.getByText('hello world')).toBeInTheDocument()
  })

  it('renders bold + italic + inline code', () => {
    const { container } = render(<UserBubbleMarkdown text="**a** *b* `c`" />)
    expect(container.querySelector('strong')).toHaveTextContent('a')
    expect(container.querySelector('em')).toHaveTextContent('b')
    expect(container.querySelector('code')).toHaveTextContent('c')
  })

  it('renders http link as plain anchor', () => {
    const { container } = render(<UserBubbleMarkdown text="[click](https://example.com)" />)
    const a = container.querySelector('a')
    expect(a).toHaveAttribute('href', 'https://example.com')
    expect(a).toHaveAttribute('target', '_blank')
  })

  it('matched file → openPreview routes through fileId', () => {
    const files = [
      {
        id: 'f1',
        fileName: 'plan.pdf',
        filePath: '/p/plan.pdf',
        kind: 'file' as const,
        fileType: 'pdf' as const,
        fileSize: 0,
        status: 'uploaded' as const,
      },
    ]
    render(
      <UserBubbleMarkdown
        text="[附件: plan.pdf](file:///p/plan.pdf)"
        files={files}
        conversationId="c1"
      />,
    )
    const btn = screen.getByRole('button', { name: '附件: plan.pdf' })
    fireEvent.click(btn)
    expect(mockOpenPreview).toHaveBeenCalled()
    const arg = mockOpenPreview.mock.calls[0][0]
    expect(arg.fileId).toBe('f1')
    expect(arg.localPath).toBe('/p/plan.pdf')
  })

  it('previewable file without match → openPreview with localPath', () => {
    render(<UserBubbleMarkdown text="[附件: x.pdf](file:///p/x.pdf)" />)
    fireEvent.click(screen.getByRole('button', { name: '附件: x.pdf' }))
    expect(mockOpenPreview).toHaveBeenCalled()
    const arg = mockOpenPreview.mock.calls[0][0]
    expect(arg.localPath).toBe('/p/x.pdf')
    expect(mockOpenLocalFile).not.toHaveBeenCalled()
  })

  it('non-previewable file → openLocalFile fallback', () => {
    render(<UserBubbleMarkdown text="[附件: setup.bat](file:///p/setup.bat)" />)
    fireEvent.click(screen.getByRole('button', { name: '附件: setup.bat' }))
    expect(mockOpenLocalFile).toHaveBeenCalledWith('/p/setup.bat')
    expect(mockOpenPreview).not.toHaveBeenCalled()
  })

  it('file:// image renders as <img> after preview pipeline returns dataUrl', async () => {
    mockGetLocalFilePreview.mockResolvedValueOnce({
      kind: 'image',
      fileName: 'thumb-render.png',
      mimeType: 'image/png',
      dataUrl: 'data:image/png;base64,FAKE',
    })
    const { container } = render(<UserBubbleMarkdown text="![chart](file:///p/thumb-render.png)" />)
    await waitFor(() => {
      expect(container.querySelector('img')).not.toBeNull()
    })
    const img = container.querySelector('img')!
    expect(img).toHaveAttribute('src', 'data:image/png;base64,FAKE')
    expect(img).toHaveAttribute('alt', 'chart')
  })

  it('clicking thumbnail opens preview', async () => {
    mockGetLocalFilePreview.mockResolvedValueOnce({
      kind: 'image',
      fileName: 'thumb-click.png',
      mimeType: 'image/png',
      dataUrl: 'data:image/png;base64,FAKE',
    })
    const { container } = render(<UserBubbleMarkdown text="![chart](file:///p/thumb-click.png)" />)
    await waitFor(() => expect(container.querySelector('img')).not.toBeNull())
    fireEvent.click(screen.getByRole('button', { name: 'chart' }))
    expect(mockOpenPreview).toHaveBeenCalled()
    expect(mockOpenPreview.mock.calls[0][0].localPath).toBe('/p/thumb-click.png')
  })

  it('file:// image with no thumbnail available falls back to chip', async () => {
    const { container } = render(<UserBubbleMarkdown text="![chart](file:///p/thumb-fallback.png)" />)
    // Wait for the async preview attempt to settle
    await waitFor(() => expect(mockGetLocalFilePreview).toHaveBeenCalled())
    expect(container.querySelector('img')).toBeNull()
    const btn = screen.getByRole('button', { name: 'chart' })
    expect(btn).toHaveTextContent('IMG')
  })

  it('renders https image directly', () => {
    const { container } = render(<UserBubbleMarkdown text="![](https://example.com/x.png)" />)
    const img = container.querySelector('img')
    expect(img).toHaveAttribute('src', 'https://example.com/x.png')
  })

  it('renders bullet list', () => {
    const { container } = render(<UserBubbleMarkdown text={'- a\n- b'} />)
    expect(container.querySelector('ul')).toBeInTheDocument()
    expect(container.querySelectorAll('li')).toHaveLength(2)
  })

  it('renders blockquote', () => {
    const { container } = render(<UserBubbleMarkdown text="> note" />)
    const bq = container.querySelector('blockquote')
    expect(bq).toBeInTheDocument()
    expect(bq?.className).toContain('border-l-2')
  })

  it('renders fenced code block', () => {
    const { container } = render(<UserBubbleMarkdown text={'```\nlet x = 1\n```'} />)
    const pre = container.querySelector('pre')
    expect(pre).toBeInTheDocument()
    expect(pre?.querySelector('code')).toHaveTextContent('let x = 1')
  })

  it('empty text → renders nothing', () => {
    const { container } = render(<UserBubbleMarkdown text="   " />)
    expect(container.firstChild).toBeNull()
  })

  it('rescues legacy bare file:// links containing spaces (pre-serializer-fix payloads)', () => {
    render(
      <UserBubbleMarkdown text="[附件: 钉钉 skill](file:///Users/x/Desktop/钉钉 skill)" />,
    )
    const btn = screen.getByRole('button', { name: '附件: 钉钉 skill' })
    expect(btn).toBeInTheDocument()
  })

  it('does not double-wrap already-angle-bracketed file:// links', () => {
    render(
      <UserBubbleMarkdown text="[附件: a b.pdf](<file:///p/a b.pdf>)" />,
    )
    const btn = screen.getByRole('button', { name: '附件: a b.pdf' })
    expect(btn).toBeInTheDocument()
  })
})

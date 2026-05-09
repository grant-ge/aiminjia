import '@testing-library/jest-dom'
import { useRef, useEffect } from 'react'
import { describe, expect, it, beforeEach, vi } from 'vitest'
import { render, waitFor } from '@testing-library/react'
import { RichComposer } from '../RichComposer'
import type { RichComposerHandle } from '../RichComposer'
import { useComposerDropInbox } from '../useComposerDropInbox'
import { useDropInbox } from '@/stores/dropInbox'
import type { PendingAttachment } from '@/hooks/useChatAttachments'

// ReactNodeViewRenderer requires NodeViewWrapper context which isn't available in jsdom.
// Stub it out so ProseMirror falls back to the extension's renderHTML for DOM output.
vi.mock('@tiptap/react', async (importOriginal) => {
  const mod = await importOriginal<typeof import('@tiptap/react')>()
  return {
    ...mod,
    ReactNodeViewRenderer: () => () => ({}),
  }
})

const mkPending = (overrides: Partial<PendingAttachment> = {}): PendingAttachment => ({
  id: '/abs/plan.pdf',
  fileName: 'plan.pdf',
  path: '/abs/plan.pdf',
  kind: 'file',
  fileType: 'pdf',
  fileSize: 1024,
  mimeType: undefined,
  source: 'drop',
  ...overrides,
})

interface HarnessProps {
  onReady?: () => void
}

function Harness({ onReady }: HarnessProps) {
  const ref = useRef<RichComposerHandle>(null)
  useComposerDropInbox(ref)
  useEffect(() => {
    onReady?.()
  }, [onReady])
  return <RichComposer ref={ref} onSubmit={() => {}} />
}

beforeEach(() => {
  useDropInbox.setState({ pending: [] })
})

describe('useComposerDropInbox', () => {
  it('push single attachment → editor inserts attachmentToken', async () => {
    const { unmount } = render(<Harness />)
    await waitFor(() => expect(document.querySelector('.ProseMirror')).toBeTruthy())
    useDropInbox.getState().push([mkPending()])
    await waitFor(() => {
      const html = document.querySelector('.ProseMirror')?.innerHTML ?? ''
      expect(html).toContain('plan.pdf')
    })
    expect(useDropInbox.getState().pending).toEqual([])
    unmount()
  })

  it('push multiple attachments → all inserted in order', async () => {
    const { unmount } = render(<Harness />)
    await waitFor(() => expect(document.querySelector('.ProseMirror')).toBeTruthy())
    useDropInbox.getState().push([
      mkPending({ id: 'a', fileName: 'a.pdf', path: '/p/a.pdf' }),
      mkPending({ id: 'b', fileName: 'b.pdf', path: '/p/b.pdf' }),
      mkPending({ id: 'c', fileName: 'c.pdf', path: '/p/c.pdf' }),
    ])
    await waitFor(() => {
      const html = document.querySelector('.ProseMirror')?.innerHTML ?? ''
      expect(html).toContain('a.pdf')
      expect(html).toContain('b.pdf')
      expect(html).toContain('c.pdf')
    })
    const html = document.querySelector('.ProseMirror')?.innerHTML ?? ''
    const idxA = html.indexOf('data-id="a"')
    const idxB = html.indexOf('data-id="b"')
    const idxC = html.indexOf('data-id="c"')
    expect(idxA).toBeGreaterThan(-1)
    expect(idxA).toBeLessThan(idxB)
    expect(idxB).toBeLessThan(idxC)
    unmount()
  })

  it('push twice → second batch appended, not lost', async () => {
    const { unmount } = render(<Harness />)
    await waitFor(() => expect(document.querySelector('.ProseMirror')).toBeTruthy())
    useDropInbox.getState().push([
      mkPending({ id: 'first', fileName: 'first.pdf', path: '/p/first.pdf' }),
    ])
    await waitFor(() => {
      expect(document.querySelector('.ProseMirror')?.innerHTML).toContain('first.pdf')
    })
    useDropInbox.getState().push([
      mkPending({ id: 'second', fileName: 'second.pdf', path: '/p/second.pdf' }),
    ])
    await waitFor(() => {
      const html = document.querySelector('.ProseMirror')?.innerHTML ?? ''
      expect(html).toContain('first.pdf')
      expect(html).toContain('second.pdf')
    })
    unmount()
  })

  it('push empty array → noop, store stays empty', async () => {
    const { unmount } = render(<Harness />)
    await waitFor(() => expect(document.querySelector('.ProseMirror')).toBeTruthy())
    useDropInbox.getState().push([])
    expect(useDropInbox.getState().pending).toEqual([])
    expect(document.querySelector('.ProseMirror')?.innerHTML).not.toContain('plan.pdf')
    unmount()
  })

  it('store is reset between tests (no leak)', () => {
    expect(useDropInbox.getState().pending).toEqual([])
  })
})

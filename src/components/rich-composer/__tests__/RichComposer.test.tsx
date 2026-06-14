import '@testing-library/jest-dom'
import { createRef } from 'react'
import { describe, expect, it, vi } from 'vitest'
import { render, screen, waitFor, fireEvent, act } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { RichComposer } from '../RichComposer'
import type { RichComposerHandle } from '../RichComposer'

// ReactNodeViewRenderer requires NodeViewWrapper context which is not available in jsdom.
// Stub it out so ProseMirror falls back to the extension's renderHTML for DOM output.
vi.mock('@tiptap/react', async (importOriginal) => {
  const mod = await importOriginal<typeof import('@tiptap/react')>()
  return {
    ...mod,
    ReactNodeViewRenderer: () => () => ({}),
  }
})

async function typeIntoEditor(user: ReturnType<typeof userEvent.setup>, text: string) {
  const editor = document.querySelector('.ProseMirror') as HTMLElement
  await user.click(editor)
  await user.type(editor, text)
}

async function pressEnter(user: ReturnType<typeof userEvent.setup>) {
  const editor = document.querySelector('.ProseMirror') as HTMLElement
  await user.click(editor)
  fireEvent.keyDown(editor, { key: 'Enter', code: 'Enter' })
}

describe('RichComposer — basic submit', () => {
  it('renders ProseMirror editor', async () => {
    render(<RichComposer placeholder="say something" onSubmit={() => {}} />)
    await waitFor(() => {
      expect(document.querySelector('.ProseMirror')).toBeTruthy()
    })
  })

  it('Enter submits payload with markdown', async () => {
    const onSubmit = vi.fn()
    const user = userEvent.setup()
    render(<RichComposer onSubmit={onSubmit} />)
    await waitFor(() => expect(document.querySelector('.ProseMirror')).toBeTruthy())
    await typeIntoEditor(user, 'hello')
    await pressEnter(user)
    expect(onSubmit).toHaveBeenCalledTimes(1)
    expect(onSubmit.mock.calls[0][0].markdown).toBe('hello')
    expect(onSubmit.mock.calls[0][0].isEmpty).toBe(false)
  })

  it('empty document Enter does not submit', async () => {
    const onSubmit = vi.fn()
    const user = userEvent.setup()
    render(<RichComposer onSubmit={onSubmit} />)
    await waitFor(() => expect(document.querySelector('.ProseMirror')).toBeTruthy())
    await pressEnter(user)
    expect(onSubmit).not.toHaveBeenCalled()
  })

  it('Shift+Enter does not submit', async () => {
    const onSubmit = vi.fn()
    const user = userEvent.setup()
    render(<RichComposer onSubmit={onSubmit} />)
    await waitFor(() => expect(document.querySelector('.ProseMirror')).toBeTruthy())
    await typeIntoEditor(user, 'hi')
    const editor = document.querySelector('.ProseMirror') as HTMLElement
    fireEvent.keyDown(editor, { key: 'Enter', code: 'Enter', shiftKey: true })
    expect(onSubmit).not.toHaveBeenCalled()
  })

  it('IME composition + Enter does not submit', async () => {
    const onSubmit = vi.fn()
    const user = userEvent.setup()
    render(<RichComposer onSubmit={onSubmit} />)
    await waitFor(() => expect(document.querySelector('.ProseMirror')).toBeTruthy())
    await typeIntoEditor(user, 'hi')
    const editor = document.querySelector('.ProseMirror') as HTMLElement
    fireEvent.compositionStart(editor)
    fireEvent.keyDown(editor, { key: 'Enter', code: 'Enter' })
    expect(onSubmit).not.toHaveBeenCalled()
    fireEvent.compositionEnd(editor)
  })
})

describe('RichComposer — disabled / streaming / clearOnSubmit', () => {
  it('disabled=true → Enter does not submit and send button is disabled', async () => {
    const onSubmit = vi.fn()
    render(<RichComposer disabled onSubmit={onSubmit} />)
    await waitFor(() => expect(document.querySelector('.ProseMirror')).toBeTruthy())
    const editor = document.querySelector('.ProseMirror') as HTMLElement
    fireEvent.keyDown(editor, { key: 'Enter', code: 'Enter' })
    expect(onSubmit).not.toHaveBeenCalled()
    const sendButton = screen.getByLabelText('发送')
    expect(sendButton).toBeDisabled()
    expect(sendButton).toHaveClass('h-8', 'w-8')
    expect(sendButton).not.toHaveClass('h-6', 'w-6')
  })

  it('isStreaming=true → shows stop button and clicking calls onStop', async () => {
    const onStop = vi.fn()
    const onSubmit = vi.fn()
    render(<RichComposer isStreaming onStop={onStop} onSubmit={onSubmit} />)
    const stopBtn = await screen.findByLabelText('停止')
    fireEvent.click(stopBtn)
    expect(onStop).toHaveBeenCalledTimes(1)
    expect(onSubmit).not.toHaveBeenCalled()
  })

  it('isStreaming=true → send button hidden, Enter still submits (queued via backend)', async () => {
    // Pending message queue: while a turn is streaming the visible button is
    // the stop button only. The user can still submit via Enter; the backend
    // PendingQueueManager buffers it for the next turn.
    const onSubmit = vi.fn().mockResolvedValue(undefined)
    const user = userEvent.setup()
    render(<RichComposer isStreaming onStop={vi.fn()} onSubmit={onSubmit} />)
    await waitFor(() => expect(document.querySelector('.ProseMirror')).toBeTruthy())
    // Stop button is the only one visible
    expect(screen.getByLabelText('停止')).toBeInTheDocument()
    expect(screen.queryByLabelText('发送')).not.toBeInTheDocument()
    // Enter key still works for submission
    const editor = document.querySelector('.ProseMirror') as HTMLElement
    await user.click(editor)
    await user.keyboard('queued message')
    await user.keyboard('{Enter}')
    await waitFor(() => expect(onSubmit).toHaveBeenCalledTimes(1))
  })

  it('clearOnSubmit=true → editor cleared after successful submit', async () => {
    const onSubmit = vi.fn().mockResolvedValue(undefined)
    const user = userEvent.setup()
    render(<RichComposer clearOnSubmit onSubmit={onSubmit} />)
    await waitFor(() => expect(document.querySelector('.ProseMirror')).toBeTruthy())
    const editor = document.querySelector('.ProseMirror') as HTMLElement
    await user.click(editor)
    await user.type(editor, 'hello')
    fireEvent.keyDown(editor, { key: 'Enter', code: 'Enter' })
    await waitFor(() => expect(onSubmit).toHaveBeenCalled())
    await waitFor(() => {
      expect(editor.textContent ?? '').toBe('')
    })
  })

  it('clearOnSubmit=true + onSubmit rejects → editor content preserved', async () => {
    const onSubmit = vi.fn().mockRejectedValue(new Error('boom'))
    const user = userEvent.setup()
    render(<RichComposer clearOnSubmit onSubmit={onSubmit} />)
    await waitFor(() => expect(document.querySelector('.ProseMirror')).toBeTruthy())
    const editor = document.querySelector('.ProseMirror') as HTMLElement
    await user.click(editor)
    await user.type(editor, 'keepme')
    fireEvent.keyDown(editor, { key: 'Enter', code: 'Enter' })
    await waitFor(() => expect(onSubmit).toHaveBeenCalled())
    expect(editor.textContent).toContain('keepme')
  })

  it('concurrent Enter while submitting → onSubmit is called only once', async () => {
    let resolveOuter: () => void = () => {}
    const onSubmit = vi.fn(
      () =>
        new Promise<void>((resolve) => {
          resolveOuter = resolve
        }),
    )
    const user = userEvent.setup()
    render(<RichComposer onSubmit={onSubmit} />)
    await waitFor(() => expect(document.querySelector('.ProseMirror')).toBeTruthy())
    const editor = document.querySelector('.ProseMirror') as HTMLElement
    await user.click(editor)
    await user.type(editor, 'first')
    fireEvent.keyDown(editor, { key: 'Enter', code: 'Enter' })
    fireEvent.keyDown(editor, { key: 'Enter', code: 'Enter' })
    fireEvent.keyDown(editor, { key: 'Enter', code: 'Enter' })
    await waitFor(() => expect(onSubmit).toHaveBeenCalledTimes(1))
    act(() => resolveOuter())
  })
})

describe('RichComposer — ref handle + attachment-only submit', () => {
  it('ref.insertAttachmentTokens inserts a token; subsequent Enter submits with attachments', async () => {
    const onSubmit = vi.fn()
    const handleRef = createRef<RichComposerHandle>()
    render(<RichComposer ref={handleRef} onSubmit={onSubmit} />)
    await waitFor(() => expect(document.querySelector('.ProseMirror')).toBeTruthy())
    act(() => {
      handleRef.current?.insertAttachmentTokens([
        {
          id: 'ref-1',
          fileName: 'a.pdf',
          path: '/p/a.pdf',
          kind: 'file',
          fileType: 'pdf',
          fileSize: 1,
          source: 'picker',
        },
      ])
    })
    const editor = document.querySelector('.ProseMirror') as HTMLElement
    fireEvent.keyDown(editor, { key: 'Enter', code: 'Enter' })
    await waitFor(() => expect(onSubmit).toHaveBeenCalledTimes(1))
    const payload = onSubmit.mock.calls[0][0]
    expect(payload.attachments).toHaveLength(1)
    expect(payload.attachments[0].id).toBe('ref-1')
    expect(payload.markdown).toContain('[附件: a.pdf](<file:///p/a.pdf>)')
  })



  it('ref.insertSkillToken inserts inline skill and submit includes skills without markdown command', async () => {
    const onSubmit = vi.fn()
    const handleRef = createRef<RichComposerHandle>()
    render(<RichComposer ref={handleRef} onSubmit={onSubmit} />)
    await waitFor(() => expect(document.querySelector('.ProseMirror')).toBeTruthy())
    act(() => {
      handleRef.current?.insertSkillToken({
        id: 'dingtalk-workspace',
        label: '玩转钉钉',
        command: '/dingtalk-workspace',
      })
    })
    const editor = document.querySelector('.ProseMirror') as HTMLElement
    expect(editor.textContent).toContain('玩转钉钉')
    fireEvent.keyDown(editor, { key: 'Enter', code: 'Enter' })
    await waitFor(() => expect(onSubmit).toHaveBeenCalledTimes(1))
    const payload = onSubmit.mock.calls[0][0]
    expect(payload.skills).toEqual([
      { id: 'dingtalk-workspace', label: '玩转钉钉', command: '/dingtalk-workspace' },
    ])
    expect(payload.markdown).not.toContain('/dingtalk-workspace')
  })

  it('typing slash command converts to inline skill token', async () => {
    const user = userEvent.setup()
    render(
      <RichComposer
        onSubmit={() => {}}
        skillTokens={[{ id: 'dingtalk-workspace', label: '玩转钉钉', command: '/dingtalk-workspace' }]}
      />,
    )
    await waitFor(() => expect(document.querySelector('.ProseMirror')).toBeTruthy())
    const editor = document.querySelector('.ProseMirror') as HTMLElement
    await user.click(editor)
    await user.type(editor, '/dingtalk-workspace ')
    await waitFor(() => expect(editor.textContent).toContain('玩转钉钉'))
    expect(editor.textContent).not.toContain('/dingtalk-workspace')
  })

  it('ref.clear empties editor', async () => {
    const handleRef = createRef<RichComposerHandle>()
    const user = userEvent.setup()
    render(<RichComposer ref={handleRef} onSubmit={() => {}} />)
    await waitFor(() => expect(document.querySelector('.ProseMirror')).toBeTruthy())
    const editor = document.querySelector('.ProseMirror') as HTMLElement
    await user.click(editor)
    await user.type(editor, 'hello')
    expect(editor.textContent).toContain('hello')
    act(() => handleRef.current?.clear())
    await waitFor(() => expect(editor.textContent).toBe(''))
  })
})

describe('RichComposer — getEditor handle', () => {
  it('ref.getEditor returns editor instance after mount', async () => {
    const handleRef = createRef<RichComposerHandle>()
    render(<RichComposer ref={handleRef} onSubmit={() => {}} />)
    await waitFor(() => expect(document.querySelector('.ProseMirror')).toBeTruthy())
    const ed = handleRef.current?.getEditor()
    expect(ed).toBeTruthy()
    expect(typeof ed?.view?.dom).toBe('object')
  })
})

describe('RichComposer — slash shortcut to open skill picker', () => {
  it('empty editor: pressing / calls onOpenSkill and the slash does not enter the editor', async () => {
    const onOpenSkill = vi.fn()
    const user = userEvent.setup()
    render(<RichComposer onSubmit={() => {}} onOpenSkill={onOpenSkill} />)
    await waitFor(() => expect(document.querySelector('.ProseMirror')).toBeTruthy())
    const editor = document.querySelector('.ProseMirror') as HTMLElement
    await user.click(editor)
    await user.keyboard('/')
    expect(onOpenSkill).toHaveBeenCalledTimes(1)
    expect(editor.textContent ?? '').toBe('')
  })

  it('after whitespace: pressing / opens picker and the slash is swallowed', async () => {
    const onOpenSkill = vi.fn()
    const user = userEvent.setup()
    render(<RichComposer onSubmit={() => {}} onOpenSkill={onOpenSkill} />)
    await waitFor(() => expect(document.querySelector('.ProseMirror')).toBeTruthy())
    const editor = document.querySelector('.ProseMirror') as HTMLElement
    await user.click(editor)
    await user.type(editor, 'hello ')
    await user.keyboard('/')
    expect(onOpenSkill).toHaveBeenCalledTimes(1)
    expect(editor.textContent).toBe('hello ')
  })

  it('after a non-space character: / falls through into the editor and onOpenSkill is not called', async () => {
    const onOpenSkill = vi.fn()
    const user = userEvent.setup()
    render(<RichComposer onSubmit={() => {}} onOpenSkill={onOpenSkill} />)
    await waitFor(() => expect(document.querySelector('.ProseMirror')).toBeTruthy())
    const editor = document.querySelector('.ProseMirror') as HTMLElement
    await user.click(editor)
    await user.type(editor, 'a')
    await user.keyboard('/')
    expect(onOpenSkill).not.toHaveBeenCalled()
    expect(editor.textContent).toBe('a/')
  })

  it('no onOpenSkill prop: / always falls through to the editor (preserves legacy /command input rule)', async () => {
    const user = userEvent.setup()
    render(<RichComposer onSubmit={() => {}} />)
    await waitFor(() => expect(document.querySelector('.ProseMirror')).toBeTruthy())
    const editor = document.querySelector('.ProseMirror') as HTMLElement
    await user.click(editor)
    await user.keyboard('/')
    expect(editor.textContent).toBe('/')
  })
})

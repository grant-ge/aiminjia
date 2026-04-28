import '@testing-library/jest-dom'
import { act, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

const chatState = vi.hoisted(() => ({
  isStreaming: false,
}))

const sendUserMessageMock = vi.hoisted(() => vi.fn(async () => true))
const stopCurrentStreamMock = vi.hoisted(() => vi.fn())
const selectAndPickAttachmentsMock = vi.hoisted(() => vi.fn(async () => []))
const resolvePastedPathsMock = vi.hoisted(() => vi.fn(async (paths: string[]) => paths.map((path) => ({
  id: path,
  fileName: path.split('/').pop() ?? path,
  path,
  kind: 'file' as const,
  fileType: 'csv' as const,
  fileSize: 0,
  source: 'paste' as const,
}))))
const readClipboardFilePathsMock = vi.hoisted(() => vi.fn(async () => [] as string[]))
const saveClipboardImageMock = vi.hoisted(() => vi.fn(async () => ({
  id: '/tmp/clipboard-1.png',
  fileName: 'clipboard-1.png',
  path: '/tmp/clipboard-1.png',
  kind: 'image',
  fileType: 'image',
  fileSize: 12,
  mimeType: 'image/png',
  source: 'clipboard-image',
})))

vi.mock('@/hooks/useChat', () => ({
  useChat: () => ({
    sendUserMessage: sendUserMessageMock,
    isStreaming: chatState.isStreaming,
    stopCurrentStream: stopCurrentStreamMock,
  }),
}))

vi.mock('@/hooks/useChatAttachments', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/hooks/useChatAttachments')>()
  return {
    ...actual,
    useChatAttachments: () => ({
      isPickingAttachments: false,
      pickAttachments: selectAndPickAttachmentsMock,
      resolvePastedPaths: resolvePastedPathsMock,
      saveClipboardImage: saveClipboardImageMock,
    }),
  }
})

vi.mock('@/components/chat/SlashCommandPopover', () => ({
  SlashCommandPopover: () => null,
}))

vi.mock('@/lib/tauri', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/lib/tauri')>()
  return {
    ...actual,
    readClipboardFilePaths: readClipboardFilePathsMock,
  }
})

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string) => {
      const map: Record<string, string> = {
        'inputBar.placeholder': '描述你的任务',
        'inputBar.placeholderWithFile': '结合附件继续输入',
        'inputBar.analyzeFile': '请分析附件',
        'inputBar.attachData': '添加附件',
        'inputBar.uploadFile': '上传文件',
      }
      return map[key] ?? key
    },
  }),
  initReactI18next: { type: '3rdParty', init: () => {} },
}))

import { useChatStore } from '@/stores/chatStore'
import { useSkillStore } from '@/stores/skillStore'
import { ChatBottomArea } from '../ChatBottomArea'

describe('ChatBottomArea', () => {
  beforeEach(() => {
    vi.useRealTimers()
    chatState.isStreaming = false
    vi.clearAllMocks()
    readClipboardFilePathsMock.mockResolvedValue([])
    resolvePastedPathsMock.mockImplementation(async (paths: string[]) => paths.map((path) => ({
      id: path,
      fileName: path.split('/').pop() ?? path,
      path,
      kind: 'file',
      fileType: 'csv',
      fileSize: 0,
      source: 'paste',
    })))
    useChatStore.setState({
      activeConversationId: 'conv-chat-bottom',
      conversations: [],
      messages: [],
    })
    useSkillStore.setState({
      skills: [
        { id: 'salary-query', displayName: '薪酬查询', description: '', source: 'local', hasWorkflow: true, icon: '', category: 'general', triggerText: '/salary-query', shortDescription: '', displayNameEn: 'Salary Query', shortDescriptionEn: '' },
      ],
      recommendedIds: [],
      isLoading: false,
    })
  })

  it('keeps the composer absolutely pinned inside a normal footer slot', () => {
    render(<ChatBottomArea />)

    const footer = screen.getByTestId('chat-bottom-area')
    expect(footer).toHaveClass('relative')
    expect(footer).toHaveClass('h-[148px]')
    expect(footer).toHaveClass('shrink-0')
    expect(footer.firstElementChild).toHaveClass('absolute')
    expect(footer.firstElementChild).toHaveClass('bottom-0')
    expect(footer.firstElementChild).toHaveClass('[scrollbar-gutter:stable_both-edges]')
    expect(footer.firstElementChild?.firstElementChild).toHaveClass('w-full')
    expect(footer.firstElementChild?.firstElementChild).toHaveClass('max-w-[736px]')
  })

  it('hides project button but keeps tips', () => {
    render(<ChatBottomArea />)

    expect(screen.queryByText('Desktop')).not.toBeInTheDocument()
    // 权限、模型选择、语音输入暂未实现
    expect(screen.queryByText('完全访问权限')).not.toBeInTheDocument()
    expect(screen.queryByRole('button', { name: '打开模型设置' })).not.toBeInTheDocument()
    expect(screen.queryByRole('button', { name: '语音输入' })).not.toBeInTheDocument()
    expect(screen.getByText('Enter 发送')).toBeInTheDocument()
    expect(screen.getByText('Shift+Enter 换行')).toBeInTheDocument()
  })

  it('does not render workspace authorization status copy in the composer', () => {
    render(<ChatBottomArea />)

    expect(screen.queryByText(/已连接本地目录：/)).not.toBeInTheDocument()
    expect(screen.queryByText('AI 当前可直接读取该目录，无需先上传文件')).not.toBeInTheDocument()
    expect(screen.queryByText(/workspace on/i)).not.toBeInTheDocument()
  })

  it('pastes absolute file and folder paths into attachment chips instead of raw text', async () => {
    render(<ChatBottomArea />)

    fireEvent.paste(screen.getByRole('textbox'), {
      clipboardData: {
        getData: (type: string) => type === 'text/plain' ? '/tmp/report.csv\n/tmp/reports' : '',
      },
    })

    expect(await screen.findByText('report.csv')).toBeInTheDocument()
    expect(await screen.findByText('reports')).toBeInTheDocument()
    expect(screen.getByRole('textbox')).toHaveValue('')
  })

  it('falls back to native clipboard file paths when pasted text has no absolute path', async () => {
    readClipboardFilePathsMock.mockResolvedValueOnce(['/tmp/Finder Copy/report.csv'])
    resolvePastedPathsMock.mockResolvedValueOnce([
      {
        id: '/tmp/Finder Copy/report.csv',
        fileName: 'report.csv',
        path: '/tmp/Finder Copy/report.csv',
        kind: 'file',
        fileType: 'csv',
        fileSize: 21,
        source: 'paste',
      },
    ] as Awaited<ReturnType<typeof resolvePastedPathsMock>>)

    render(<ChatBottomArea />)

    const textbox = screen.getByRole('textbox') as HTMLTextAreaElement
    fireEvent.change(textbox, {
      target: { value: '已有输入' },
    })

    fireEvent.paste(textbox, {
      clipboardData: {
        items: [],
        getData: (type: string) => type === 'text/plain' ? 'report.csv' : '',
      },
    })

    await waitFor(() => {
      expect(readClipboardFilePathsMock).toHaveBeenCalled()
    })
    await waitFor(() => {
      expect(resolvePastedPathsMock).toHaveBeenCalledWith(['/tmp/Finder Copy/report.csv'])
    })

    expect(await screen.findByText('report.csv')).toBeInTheDocument()
    expect(textbox).toHaveValue('已有输入')
  })

  it('resolves pasted paths with real path metadata instead of guessing from file names', async () => {
    resolvePastedPathsMock.mockResolvedValueOnce(([
      {
        id: '/tmp/README',
        fileName: 'README',
        path: '/tmp/README',
        kind: 'file',
        fileType: 'csv',
        fileSize: 12,
        source: 'paste',
      },
      {
        id: '/tmp/archive.v1',
        fileName: 'archive.v1',
        path: '/tmp/archive.v1',
        kind: 'folder',
        fileType: 'folder',
        fileSize: 0,
        source: 'paste',
      },
    ] as Awaited<ReturnType<typeof resolvePastedPathsMock>>))

    render(<ChatBottomArea />)

    fireEvent.paste(screen.getByRole('textbox'), {
      clipboardData: {
        getData: (type: string) => type === 'text/plain' ? '/tmp/README\n/tmp/archive.v1' : '',
      },
    })

    await waitFor(() => {
      expect(resolvePastedPathsMock).toHaveBeenCalledWith(['/tmp/README', '/tmp/archive.v1'])
    })

    const readmeChip = await screen.findByText('README')
    const archiveChip = await screen.findByText('archive.v1')

    expect(readmeChip.parentElement).toHaveTextContent('CSV')
    expect(archiveChip.parentElement).toHaveTextContent('DIR')
  })

  it('saves clipboard image blobs into attachment chips when no local path exists', async () => {
    render(<ChatBottomArea />)

    const imageFile = new File([new Uint8Array([1, 2, 3])], 'screenshot.png', { type: 'image/png' })

    fireEvent.paste(screen.getByRole('textbox'), {
      clipboardData: {
        items: [
          {
            kind: 'file',
            type: 'image/png',
            getAsFile: () => imageFile,
          },
        ],
        getData: () => '',
      },
    })

    await waitFor(() => {
      expect(saveClipboardImageMock).toHaveBeenCalledWith(
        'conv-chat-bottom',
        expect.any(Uint8Array),
        'image/png',
      )
    })

    expect(await screen.findByText('clipboard-1.png')).toBeInTheDocument()
  })

  it('clicking the plus button directly triggers file picking instead of opening a two-option menu', async () => {
    selectAndPickAttachmentsMock.mockResolvedValueOnce([])
    render(<ChatBottomArea />)

    fireEvent.click(screen.getByRole('button', { name: '添加附件' }))

    await waitFor(() => {
      expect(selectAndPickAttachmentsMock).toHaveBeenCalled()
    })

    expect(screen.queryByText('连接本地目录（不复制）')).not.toBeInTheDocument()
    expect(screen.queryByText('继续使用复制上传模式')).not.toBeInTheDocument()
  })

  it('sends message on Enter', async () => {
    render(<ChatBottomArea />)

    fireEvent.change(screen.getByRole('textbox'), {
      target: { value: 'hello' },
    })
    fireEvent.keyDown(screen.getByRole('textbox'), { key: 'Enter' })

    await waitFor(() => {
      expect(sendUserMessageMock).toHaveBeenCalledWith('hello', undefined)
    })
  })

  it('shows stop while streaming', () => {
    chatState.isStreaming = true
    render(<ChatBottomArea />)

    fireEvent.click(screen.getByRole('button', { name: '停止' }))
    expect(stopCurrentStreamMock).toHaveBeenCalled()
  })

  it('sends slash-prefixed text verbatim (skill id not in IPC)', async () => {
    render(<ChatBottomArea />)

    // Type a slash command that is NOT in the skill store (unknown skill)
    fireEvent.change(screen.getByRole('textbox'), {
      target: { value: '/not-a-skill hello' },
    })
    fireEvent.keyDown(screen.getByRole('textbox'), { key: 'Enter' })

    await waitFor(() => {
      expect(sendUserMessageMock).toHaveBeenCalledWith('/not-a-skill hello', undefined)
    })
  })

  it('typing /salary-query expands to triggerText in input via useSkillComposer', () => {
    render(<ChatBottomArea />)

    // Type the skill command with a space-separated tail — useSkillComposer will
    // replace the slash-command prefix with triggerText
    fireEvent.change(screen.getByRole('textbox'), {
      target: { value: '/salary-query 你好' },
    })

    // The input value should now be the triggerText + tail
    // '/salary-query' triggerText + ' 你好' tail
    expect(screen.getByRole('textbox')).toHaveValue('/salary-query 你好')
    // No selectedSkillCommands on the store
    expect((useChatStore.getState() as unknown as Record<string, unknown>)['selectedSkillCommands']).toBeUndefined()
  })

  it('clears input after successful send, no skill state involved', async () => {
    sendUserMessageMock.mockResolvedValueOnce(true)

    render(<ChatBottomArea />)

    fireEvent.change(screen.getByRole('textbox'), { target: { value: 'hello' } })
    fireEvent.keyDown(screen.getByRole('textbox'), { key: 'Enter' })

    await waitFor(() => {
      expect(screen.getByRole('textbox')).toHaveValue('')
    })

    expect(sendUserMessageMock).toHaveBeenCalledWith('hello', undefined)
  })

  it('clears input immediately while send is still in flight', async () => {
    let resolveSend: (value: boolean) => void = () => {}
    sendUserMessageMock.mockImplementationOnce(() => new Promise<boolean>((resolve) => {
      resolveSend = resolve
    }))

    render(<ChatBottomArea />)

    fireEvent.change(screen.getByRole('textbox'), { target: { value: 'hello' } })
    fireEvent.keyDown(screen.getByRole('textbox'), { key: 'Enter' })

    expect(screen.getByRole('textbox')).toHaveValue('')

    resolveSend(true)

    await waitFor(() => {
      expect(sendUserMessageMock).toHaveBeenCalledWith('hello', undefined, undefined, undefined)
    })
  })

  it('does not restore input just because the backend turn takes longer than 15 seconds', async () => {
    vi.useFakeTimers()
    sendUserMessageMock.mockImplementationOnce(() => new Promise<boolean>(() => {}))

    render(<ChatBottomArea />)

    fireEvent.change(screen.getByRole('textbox'), { target: { value: 'hello' } })
    fireEvent.keyDown(screen.getByRole('textbox'), { key: 'Enter' })

    expect(screen.getByRole('textbox')).toHaveValue('')

    await act(async () => {
      vi.advanceTimersByTime(15_001)
    })

    expect(screen.getByRole('textbox')).toHaveValue('')
  })

})

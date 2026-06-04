import { describe, expect, it } from 'vitest'

import { buildTurnsFromMessages } from '../useTurnRenderModel'
import type { AssistantToolCall, GeneratedFile, Message, ToolResultContent } from '@/types/message'
import type { ToolExecution } from '@/stores/streamingStore'

function userMsg(id: string, text: string): Message {
  return { id, conversationId: 'c1', role: 'user', createdAt: new Date().toISOString(), content: { text } }
}

function aiMsg(id: string, text: string): Message {
  return { id, conversationId: 'c1', role: 'assistant', createdAt: new Date().toISOString(), content: { text } }
}

function assistantMsgWithToolCalls(id: string, toolCalls: AssistantToolCall[]): Message {
  return { id, conversationId: 'c1', role: 'assistant', createdAt: new Date().toISOString(), content: { text: '' }, toolCalls }
}

function toolResultMsg(id: string, toolResult: ToolResultContent): Message {
  return { id, conversationId: 'c1', role: 'tool', createdAt: new Date().toISOString(), content: { text: '' }, toolResult }
}

describe('buildTurnsFromMessages', () => {
  it('groups messages into turns starting at each user message', () => {
    const msgs = [userMsg('u1', 'hi'), aiMsg('a1', 'hello'), userMsg('u2', 'again'), aiMsg('a2', 'hi!')]
    const turns = buildTurnsFromMessages(msgs, [])
    expect(turns.map((t) => t.userMessage?.id)).toEqual(['u1', 'u2'])
    expect(turns[0].aiSegments.map((s) => s.id)).toEqual(['a1'])
    expect(turns[1].aiSegments.map((s) => s.id)).toEqual(['a2'])
  })

  it('filters task notification XML user messages from the chat turns', () => {
    const notification = userMsg('task-notification', [
      '<task-notification>',
      '  <task-id>b7a56f590</task-id>',
      '  <status>completed</status>',
      '  <summary>Background command completed</summary>',
      '</task-notification>',
    ].join('\n'))
    const msgs = [notification, userMsg('u1', '好了'), aiMsg('a1', '登录成功')]

    const turns = buildTurnsFromMessages(msgs, [])

    expect(turns.map((t) => t.userMessage?.id)).toEqual(['u1'])
    expect(turns[0].userMessage?.text).toBe('好了')
    expect(turns[0].aiSegments.map((s) => s.id)).toEqual(['a1'])
  })

  it('attaches tool executions to the last turn as a single ToolGroup', () => {
    const msgs = [userMsg('u1', 'x'), aiMsg('a1', 'done')]
    const tools: ToolExecution[] = [
      { toolId: 't1', toolName: 'fetch_feedback', status: 'completed' },
      { toolId: 't2', toolName: 'cluster_topics', status: 'completed' },
    ]
    const turns = buildTurnsFromMessages(msgs, tools)
    expect(turns[0].toolGroup).toBeDefined()
    expect(turns[0].toolGroup?.steps.map((s) => s.name)).toEqual(['fetch_feedback', 'cluster_topics'])
    expect(turns[0].toolGroup?.status).toBe('done')
    // durationMs is 0 when no timestamps are available (test fixtures have no startedAt)
    expect(turns[0].toolGroup?.durationMs).toBe(0)
  })

  it('marks toolGroup as running when any tool is executing', () => {
    const tools: ToolExecution[] = [
      { toolId: 't1', toolName: 'fetch', status: 'completed' },
      { toolId: 't2', toolName: 'run', status: 'executing' },
    ]
    const turns = buildTurnsFromMessages([userMsg('u1', 'x')], tools)
    expect(turns[0].toolGroup?.status).toBe('running')
  })

  it('aiSegment carries the full message object', () => {
    const msg = aiMsg('a1', 'hello')
    const turns = buildTurnsFromMessages([userMsg('u1', 'hi'), msg], [])
    expect(turns[0].aiSegments[0].message).toBe(msg)
    expect(turns[0].aiSegments[0].id).toBe('a1')
  })

  it('maps inputJson from assistant.toolCalls by toolCallId', () => {
    const msgs = [
      userMsg('u1', 'go'),
      assistantMsgWithToolCalls('a1', [
        { id: 'tc-1', name: 'run_python', arguments: { code: 'print(1)' } },
      ]),
      toolResultMsg('t1', { toolCallId: 'tc-1', name: 'run_python', content: '1\n', isError: false }),
    ]
    const turns = buildTurnsFromMessages(msgs, [])
    const step = turns[0].toolGroup?.steps[0]
    expect(step?.toolCallId).toBe('tc-1')
    expect(step?.inputJson).toContain('print(1)')
    expect(step?.output).toContain('1')
  })

  it('backfills inputJson when tool result arrives before assistant.toolCalls snapshot', () => {
    const msgs = [
      userMsg('u1', 'go'),
      toolResultMsg('t1', { toolCallId: 'tc-1', name: 'run_python', content: '1\n', isError: false }),
      assistantMsgWithToolCalls('a1', [
        { id: 'tc-1', name: 'run_python', arguments: { code: 'print(1)' } },
      ]),
    ]
    const step = buildTurnsFromMessages(msgs, [])[0].toolGroup?.steps[0]
    expect(step?.inputJson).toContain('print(1)')
    expect(step?.output).toContain('1')
  })

  it('does not confuse same-name tools called twice', () => {
    const msgs = [
      userMsg('u1', 'go'),
      assistantMsgWithToolCalls('a1', [
        { id: 'tc-1', name: 'browse', arguments: { url: 'http://a.com' } },
        { id: 'tc-2', name: 'browse', arguments: { url: 'http://b.com' } },
      ]),
      toolResultMsg('t1', { toolCallId: 'tc-1', name: 'browse', content: 'page A', isError: false }),
      toolResultMsg('t2', { toolCallId: 'tc-2', name: 'browse', content: 'page B', isError: false }),
    ]
    const steps = buildTurnsFromMessages(msgs, [])[0].toolGroup?.steps ?? []
    expect(steps).toHaveLength(2)
    expect(steps.find((s) => s.toolCallId === 'tc-1')?.output).toContain('page A')
    expect(steps.find((s) => s.toolCallId === 'tc-2')?.output).toContain('page B')
  })

  it('error output preserved in step output', () => {
    const msgs = [
      userMsg('u1', 'go'),
      assistantMsgWithToolCalls('a1', [{ id: 'tc-1', name: 'run_python', arguments: {} }]),
      toolResultMsg('t1', { toolCallId: 'tc-1', name: 'run_python', content: 'Traceback...\nValueError: bad', isError: true }),
    ]
    const step = buildTurnsFromMessages(msgs, [])[0].toolGroup?.steps[0]
    expect(step?.status).toBe('error')
    expect(step?.output).toBeDefined()
  })

  it('preserves skill command metadata on user messages for the chat-scene UI', () => {
    const msg: Message = {
      ...userMsg('u1', '你可以做什么'),
      content: {
        text: '你可以做什么',
        commandText: '/salary-query 你可以做什么',
        skillCommand: { id: 'salary-query', label: 'salary-query', command: '/salary-query' },
      },
    }

    const turns = buildTurnsFromMessages([msg], [])

    expect(turns[0].userMessage).toMatchObject({
      id: 'u1',
      text: '你可以做什么',
      commandText: '/salary-query 你可以做什么',
      skillCommand: { id: 'salary-query', label: 'salary-query', command: '/salary-query' },
    })
  })

  it('formats generated file metadata for the compact file card subtitle', () => {
    const msg: Message = {
      ...aiMsg('a1', 'done'),
      content: {
        generatedFiles: [
          {
            id: 'file-1',
            fileName: 'mock-data-matrix.csv',
            filePath: '/tmp/mock-data-matrix.csv',
            fileType: 'csv',
            fileSize: 12_288,
            category: 'data',
            version: 1,
            isLatest: true,
            createdAt: '2026-04-28T00:00:00Z',
            description: 'exported matrix',
            actions: [],
          },
        ],
      },
    }

    const turns = buildTurnsFromMessages([userMsg('u1', 'export'), msg], [])

    expect(turns[0].generatedFiles[0]).toMatchObject({
      title: 'mock-data-matrix.csv',
      sub: '12 KB · 数据',
      appName: '打开',
    })
  })

  it('uses degradation notice as generated file metadata when available', () => {
    const msg: Message = {
      ...aiMsg('a1', 'done'),
      content: {
        generatedFiles: [
          {
            id: 'file-1',
            fileName: 'report.html',
            filePath: '/tmp/report.html',
            fileType: 'html',
            fileSize: 2048,
            category: 'report',
            version: 1,
            isLatest: true,
            createdAt: '2026-04-28T00:00:00Z',
            description: '',
            actions: [],
            isDegraded: true,
            requestedFormat: 'docx',
          },
        ],
      },
    }

    const turns = buildTurnsFromMessages([userMsg('u1', 'report'), msg], [])

    expect(turns[0].generatedFiles[0].sub).toBe('已降级为 HTML · 原请求 DOCX')
  })


  it('normalizes slash command user text into skill command metadata', () => {
    const turns = buildTurnsFromMessages([userMsg('u1', '/salary-query 看看你的技能能力')], [])

    expect(turns[0].userMessage).toMatchObject({
      id: 'u1',
      text: '看看你的技能能力',
      commandText: '/salary-query 看看你的技能能力',
      skillCommand: { id: 'salary-query', label: 'salary-query', command: '/salary-query' },
    })
  })


  it('preserves generated file action metadata for file card interactions', () => {
    const msg: Message = {
      ...aiMsg('a1', 'done'),
      content: {
        text: 'done',
        generatedFiles: [
          {
            id: 'file-1',
            fileName: 'report.md',
            filePath: '/tmp/report.md',
            fileType: 'markdown',
            fileSize: 128,
            category: 'report',
            version: 1,
            isLatest: true,
            createdAt: '2026-04-28T00:00:00Z',
            description: 'Report',
            actions: [{ type: 'preview', label: 'Preview', enabled: true }],
          },
        ],
      },
    }

    const generatedFile = buildTurnsFromMessages([userMsg('u1', 'go'), msg], [])[0].generatedFiles[0]

    expect(generatedFile).toEqual(
      expect.objectContaining({
        id: 'file-1',
        title: 'report.md',
        fileType: 'markdown',
        actions: [{ type: 'preview', label: 'Preview', enabled: true }],
        canPreview: true,
        canOpenExternal: false,
        primaryAction: 'preview',
        conversationId: 'c1',
      }),
    )
  })

  it('uses safe defaults for old generated file records without actions', () => {
    const oldFile = {
      id: 'file-2',
      fileName: 'book.xlsx',
      filePath: '/tmp/book.xlsx',
      fileType: 'xlsx',
      fileSize: 256,
      category: 'legacy-output',
      version: 1,
      isLatest: true,
      createdAt: '2026-04-28T00:00:00Z',
      description: 'Workbook',
    } satisfies GeneratedFile
    const msg: Message = {
      ...aiMsg('a1', 'done'),
      content: { text: 'done', generatedFiles: [oldFile] },
    }

    const generatedFile = buildTurnsFromMessages([userMsg('u1', 'go'), msg], [])[0].generatedFiles[0]

    expect(generatedFile).toEqual(
      expect.objectContaining({
        id: 'file-2',
        title: 'book.xlsx',
        fileType: 'xlsx',
        actions: [],
        canPreview: false,
        canOpenExternal: true,
        primaryAction: 'open',
      }),
    )
  })

  it('uses type-based preview even when preview action is disabled', () => {
    const msg: Message = {
      ...aiMsg('a1', 'done'),
      content: {
        text: 'done',
        generatedFiles: [
          {
            id: 'file-3',
            fileName: 'report.md',
            filePath: '/tmp/report.md',
            fileType: 'markdown',
            fileSize: 128,
            category: 'report',
            version: 1,
            isLatest: true,
            createdAt: '2026-04-28T00:00:00Z',
            description: 'Report',
            actions: [
              { type: 'preview', label: 'Preview', enabled: false },
              { type: 'open', label: 'Open', enabled: false },
            ],
          },
        ],
      },
    }

    const generatedFile = buildTurnsFromMessages([userMsg('u1', 'go'), msg], [])[0].generatedFiles[0]

    expect(generatedFile).toEqual(
      expect.objectContaining({
        canPreview: true,
        canOpenExternal: false,
        primaryAction: 'preview',
      }),
    )
  })

  it('marks external open unavailable for non-previewable generated files with disabled open action', () => {
    const msg: Message = {
      ...aiMsg('a1', 'done'),
      content: {
        text: 'done',
        generatedFiles: [
          {
            id: 'file-5',
            fileName: 'book.xlsx',
            filePath: '/tmp/book.xlsx',
            fileType: 'xlsx',
            fileSize: 256,
            category: 'workbook',
            version: 1,
            isLatest: true,
            createdAt: '2026-04-28T00:00:00Z',
            description: 'Workbook',
            actions: [{ type: 'open', label: 'Open', enabled: false }],
          },
        ],
      },
    }

    const generatedFile = buildTurnsFromMessages([userMsg('u1', 'go'), msg], [])[0].generatedFiles[0]

    expect(generatedFile).toEqual(
      expect.objectContaining({
        canPreview: false,
        canOpenExternal: false,
        primaryAction: 'open',
      }),
    )
  })

  it('uses image preview when legacy actions omit the preview action', () => {
    const msg: Message = {
      ...aiMsg('a1', 'done'),
      content: {
        text: 'done',
        generatedFiles: [
          {
            id: 'file-legacy-image',
            fileName: 'mock-status-chart.png',
            filePath: '/tmp/mock-status-chart.png',
            fileType: 'png',
            fileSize: 68,
            category: 'chart',
            version: 1,
            isLatest: true,
            createdAt: '2026-04-28T00:00:00Z',
            description: 'Chart',
            actions: [
              { type: 'open', label: 'Open', enabled: true },
              { type: 'reveal', label: 'Open Folder', enabled: true },
            ],
          },
        ],
      },
    }

    const generatedFile = buildTurnsFromMessages([userMsg('u1', 'go'), msg], [])[0].generatedFiles[0]

    expect(generatedFile).toEqual(
      expect.objectContaining({
        canPreview: true,
        canOpenExternal: true,
        primaryAction: 'preview',
      }),
    )
  })

  it('uses type-based preview for legacy HTML actions that omit preview', () => {
    const msg: Message = {
      ...aiMsg('a1', 'done'),
      content: {
        text: 'done',
        generatedFiles: [
          {
            id: 'file-legacy-html',
            fileName: 'mock-coverage-report.html',
            filePath: '/tmp/mock-coverage-report.html',
            fileType: 'html',
            fileSize: 8971,
            category: 'report',
            version: 1,
            isLatest: true,
            createdAt: '2026-04-28T00:00:00Z',
            description: 'Report',
            actions: [
              { type: 'open', label: 'Open', enabled: true },
              { type: 'reveal', label: 'Open Folder', enabled: true },
            ],
          },
        ],
      },
    }

    const generatedFile = buildTurnsFromMessages([userMsg('u1', 'go'), msg], [])[0].generatedFiles[0]

    expect(generatedFile).toEqual(
      expect.objectContaining({
        canPreview: true,
        primaryAction: 'preview',
      }),
    )
  })

  it('marks PNG generated artifacts as previewable in the app', () => {
    const msg: Message = {
      ...aiMsg('a1', 'done'),
      content: {
        text: 'done',
        generatedFiles: [
          {
            id: 'file-image',
            fileName: 'mock-status-chart.png',
            filePath: '/tmp/mock-status-chart.png',
            fileType: 'png',
            fileSize: 68,
            category: 'chart',
            version: 1,
            isLatest: true,
            createdAt: '2026-04-28T00:00:00Z',
            description: 'Chart',
            actions: [{ type: 'preview', label: 'Preview', enabled: true }],
          },
        ],
      },
    }

    const generatedFile = buildTurnsFromMessages([userMsg('u1', 'go'), msg], [])[0].generatedFiles[0]

    expect(generatedFile).toEqual(
      expect.objectContaining({
        id: 'file-image',
        title: 'mock-status-chart.png',
        fileType: 'png',
        canPreview: true,
        primaryAction: 'preview',
      }),
    )
  })

  it('keeps generated file display title while using fileName as preview fallback', () => {
    const msg: Message = {
      ...aiMsg('a1', 'done'),
      content: {
        text: 'done',
        generatedFiles: [
          {
            id: 'file-4',
            title: 'Readable Report',
            fileName: 'report.md',
            filePath: '/tmp/report.md',
            fileSize: 128,
            category: 'report',
            version: 1,
            isLatest: true,
            createdAt: '2026-04-28T00:00:00Z',
            description: 'Report',
          },
        ],
      },
    }

    const generatedFile = buildTurnsFromMessages([userMsg('u1', 'go'), msg], [])[0].generatedFiles[0]

    expect(generatedFile).toEqual(
      expect.objectContaining({
        title: 'Readable Report',
        fileName: 'report.md',
        canPreview: true,
        primaryAction: 'preview',
      }),
    )
  })

})

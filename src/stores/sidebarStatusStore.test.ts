import { beforeEach, describe, expect, it, vi } from 'vitest'

const tauriMocks = vi.hoisted(() => ({
  getSettings: vi.fn(),
  updateSettings: vi.fn(),
}))

vi.mock('@/lib/tauri', () => tauriMocks)

import { DEFAULT_SETTINGS } from '@/types/settings'
import { useSidebarStatusStore } from './sidebarStatusStore'

describe('sidebarStatusStore', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    tauriMocks.getSettings.mockResolvedValue({ ...DEFAULT_SETTINGS })
    tauriMocks.updateSettings.mockResolvedValue(undefined)
    useSidebarStatusStore.setState({ statuses: {} })
  })

  it('hydrates cached statuses from settings json', () => {
    useSidebarStatusStore.getState().hydrateFromSettings({
      ...DEFAULT_SETTINGS,
      uiSidebarConversationStatuses: JSON.stringify({
        'conv-1': {
          kind: 'permission-review',
          updatedAt: 1780000000000,
          runId: 'run-1',
          toolCallId: 'tool-1',
        },
      }),
    })

    expect(useSidebarStatusStore.getState().statuses).toMatchObject({
      'conv-1': {
        kind: 'permission-review',
        runId: 'run-1',
        toolCallId: 'tool-1',
      },
    })
  })

  it('persists permission and waiting-reply statuses into settings', async () => {
    await useSidebarStatusStore.getState().setStatus('conv-1', {
      kind: 'permission-review',
      runId: 'run-1',
      toolCallId: 'tool-1',
    })

    await useSidebarStatusStore.getState().setStatus('conv-2', {
      kind: 'waiting-reply',
      runId: 'run-2',
      interactionId: 'ask-1',
    })

    const lastUpdate = tauriMocks.updateSettings.mock.calls.at(-1)?.[0]
    expect(JSON.parse(lastUpdate.uiSidebarConversationStatuses)).toMatchObject({
      'conv-1': { kind: 'permission-review', toolCallId: 'tool-1' },
      'conv-2': { kind: 'waiting-reply', interactionId: 'ask-1' },
    })
  })

  it('removes a cached status from settings', async () => {
    useSidebarStatusStore.setState({
      statuses: {
        'conv-1': {
          kind: 'permission-review',
          updatedAt: 1780000000000,
          toolCallId: 'tool-1',
        },
      },
    })

    await useSidebarStatusStore.getState().clearStatus('conv-1')

    const lastUpdate = tauriMocks.updateSettings.mock.calls.at(-1)?.[0]
    expect(JSON.parse(lastUpdate.uiSidebarConversationStatuses)).toEqual({})
  })
})

import { beforeEach, describe, expect, it, vi } from 'vitest'

const tauriMocks = vi.hoisted(() => ({
  getSettings: vi.fn(),
  updateSettings: vi.fn(),
  pendingPermissionSnapshotForSession: vi.fn(),
  pendingInteractionSnapshotForSession: vi.fn(),
}))

vi.mock('@/lib/tauri', () => tauriMocks)

import { DEFAULT_SETTINGS } from '@/types/settings'
import { useSidebarStatusStore } from './sidebarStatusStore'

describe('sidebarStatusStore', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    tauriMocks.getSettings.mockResolvedValue({ ...DEFAULT_SETTINGS })
    tauriMocks.updateSettings.mockResolvedValue(undefined)
    tauriMocks.pendingPermissionSnapshotForSession.mockResolvedValue([])
    tauriMocks.pendingInteractionSnapshotForSession.mockResolvedValue([])
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

  it('clears hydrated permission status when runtime has no pending permission', async () => {
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

    await useSidebarStatusStore.getState().reconcileWithRuntimeSnapshots()

    expect(tauriMocks.pendingPermissionSnapshotForSession).toHaveBeenCalledWith('conv-1')
    expect(useSidebarStatusStore.getState().statuses).toEqual({})
    const lastUpdate = tauriMocks.updateSettings.mock.calls.at(-1)?.[0]
    expect(JSON.parse(lastUpdate.uiSidebarConversationStatuses)).toEqual({})
  })

  it('keeps hydrated permission status when runtime still has the pending permission', async () => {
    tauriMocks.pendingPermissionSnapshotForSession.mockResolvedValueOnce([
      {
        conversationId: 'conv-1',
        runId: 'run-1',
        toolCallId: 'tool-1',
        toolName: 'Read',
        message: 'Allow?',
        suggestions: null,
        mode: 'default',
        rememberOptions: null,
        defaultDestination: null,
      },
    ])
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

    await useSidebarStatusStore.getState().reconcileWithRuntimeSnapshots()

    expect(useSidebarStatusStore.getState().statuses).toMatchObject({
      'conv-1': { kind: 'permission-review', toolCallId: 'tool-1' },
    })
    expect(tauriMocks.updateSettings).not.toHaveBeenCalled()
  })

  it('clears hydrated waiting-reply status when runtime has no pending interaction', async () => {
    useSidebarStatusStore.getState().hydrateFromSettings({
      ...DEFAULT_SETTINGS,
      uiSidebarConversationStatuses: JSON.stringify({
        'conv-1': {
          kind: 'waiting-reply',
          updatedAt: 1780000000000,
          runId: 'run-1',
          interactionId: 'ask-1',
        },
      }),
    })

    await useSidebarStatusStore.getState().reconcileWithRuntimeSnapshots()

    expect(tauriMocks.pendingInteractionSnapshotForSession).toHaveBeenCalledWith('conv-1')
    expect(useSidebarStatusStore.getState().statuses).toEqual({})
  })
})

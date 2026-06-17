import { beforeEach, describe, expect, it } from 'vitest'

import {
  SIDEBAR_STATUS_SESSION_KEY,
  useSidebarStatusStore,
} from './sidebarStatusStore'

describe('sidebarStatusStore', () => {
  beforeEach(() => {
    useSidebarStatusStore.setState({ statuses: {} })
    window.sessionStorage.clear()
  })

  it('hydrates cached statuses from the current browser session', () => {
    window.sessionStorage.setItem(
      SIDEBAR_STATUS_SESSION_KEY,
      JSON.stringify({
        'conv-1': {
          kind: 'permission-review',
          updatedAt: 1780000000000,
          runId: 'run-1',
          toolCallId: 'tool-1',
        },
      }),
    )

    useSidebarStatusStore.getState().hydrateFromSession()

    expect(useSidebarStatusStore.getState().statuses).toMatchObject({
      'conv-1': {
        kind: 'permission-review',
        runId: 'run-1',
        toolCallId: 'tool-1',
      },
    })
  })

  it('persists permission and waiting-reply statuses into session storage', async () => {
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

    const stored = window.sessionStorage.getItem(SIDEBAR_STATUS_SESSION_KEY)
    expect(JSON.parse(stored ?? '{}')).toMatchObject({
      'conv-1': { kind: 'permission-review', toolCallId: 'tool-1' },
      'conv-2': { kind: 'waiting-reply', interactionId: 'ask-1' },
    })
  })

  it('removes a cached status from session storage', async () => {
    useSidebarStatusStore.setState({
      statuses: {
        'conv-1': {
          kind: 'permission-review',
          updatedAt: 1780000000000,
          toolCallId: 'tool-1',
        },
      },
    })
    window.sessionStorage.setItem(
      SIDEBAR_STATUS_SESSION_KEY,
      JSON.stringify(useSidebarStatusStore.getState().statuses),
    )

    await useSidebarStatusStore.getState().clearStatus('conv-1')

    expect(
      JSON.parse(
        window.sessionStorage.getItem(SIDEBAR_STATUS_SESSION_KEY) ?? '{}',
      ),
    ).toEqual({})
  })

  it('clears the session cached statuses on reset', () => {
    useSidebarStatusStore.setState({
      statuses: {
        'conv-1': {
          kind: 'waiting-reply',
          updatedAt: 1780000000000,
          interactionId: 'ask-1',
        },
      },
    })
    window.sessionStorage.setItem(
      SIDEBAR_STATUS_SESSION_KEY,
      JSON.stringify(useSidebarStatusStore.getState().statuses),
    )

    useSidebarStatusStore.getState().reset()

    expect(useSidebarStatusStore.getState().statuses).toEqual({})
    expect(window.sessionStorage.getItem(SIDEBAR_STATUS_SESSION_KEY)).toBeNull()
  })
})

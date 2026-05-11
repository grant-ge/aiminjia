import { beforeEach, describe, expect, it } from 'vitest'
import {
  useUiStore,
  getActiveConversationId,
  getActiveChannelSessionId,
} from '@/stores/uiStore'

beforeEach(() => {
  useUiStore.setState({ route: { kind: 'home' } })
})

describe('uiStore derived selectors', () => {
  it('getActiveConversationId returns conversationId when route is chat', () => {
    useUiStore.getState().setRoute({ kind: 'chat', conversationId: 'c1' })
    expect(getActiveConversationId()).toBe('c1')
  })

  it('getActiveConversationId returns null for non-chat routes', () => {
    useUiStore.getState().setRoute({ kind: 'channel', sessionId: 's1' })
    expect(getActiveConversationId()).toBeNull()
    useUiStore.getState().setRoute({ kind: 'home' })
    expect(getActiveConversationId()).toBeNull()
  })

  it('getActiveChannelSessionId returns sessionId when route is channel with sessionId', () => {
    useUiStore.getState().setRoute({ kind: 'channel', sessionId: 's1' })
    expect(getActiveChannelSessionId()).toBe('s1')
  })

  it('getActiveChannelSessionId returns null for channel without sessionId', () => {
    useUiStore.getState().setRoute({ kind: 'channel' })
    expect(getActiveChannelSessionId()).toBeNull()
  })

  it('getActiveChannelSessionId returns null for non-channel routes', () => {
    useUiStore.getState().setRoute({ kind: 'chat', conversationId: 'c1' })
    expect(getActiveChannelSessionId()).toBeNull()
  })
})

import { beforeEach, describe, expect, it, vi } from 'vitest'

import { useUiStore } from '../uiStore'

describe('uiStore.settingsModal', () => {
  beforeEach(() => {
    useUiStore.getState().closeSettings()
  })

  it('opens only implemented settings keys', () => {
    const keys = ['account', 'archived', 'about'] as const
    for (const k of keys) {
      useUiStore.getState().openSettings(k)
      expect(useUiStore.getState().settingsModal).toBe(k)
    }
  })

  it('falls back to account for unimplemented settings keys', () => {
    const keys = ['usage', 'permissions', 'mcp', 'sso', 'shortcuts'] as const
    for (const k of keys) {
      useUiStore.getState().openSettings(k)
      expect(useUiStore.getState().settingsModal).toBe('account')
    }
  })
})

describe('uiStore.route persistence', () => {
  beforeEach(() => {
    localStorage.clear()
    useUiStore.setState({ route: { kind: 'home' }, settingsModal: null })
  })

  it('persists chat route when route changes', () => {
    useUiStore.getState().setRoute({ kind: 'chat', conversationId: 'conv-1' })

    expect(JSON.parse(localStorage.getItem('aijia-ui-route') ?? '{}')).toEqual({
      kind: 'chat',
      conversationId: 'conv-1',
    })
  })

  it('restores persisted chat route on store initialization', async () => {
    localStorage.setItem(
      'aijia-ui-route',
      JSON.stringify({ kind: 'chat', conversationId: 'conv-restored' }),
    )
    vi.resetModules()

    const { useUiStore: freshStore } = await import('../uiStore')

    expect(freshStore.getState().route).toEqual({
      kind: 'chat',
      conversationId: 'conv-restored',
    })
  })

  it('falls back to home when persisted route is invalid', async () => {
    localStorage.setItem('aijia-ui-route', JSON.stringify({ kind: 'chat' }))
    vi.resetModules()

    const { useUiStore: freshStore } = await import('../uiStore')

    expect(freshStore.getState().route).toEqual({ kind: 'home' })
  })
})

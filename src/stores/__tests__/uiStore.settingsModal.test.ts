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
    const keys = ['mcp', 'sso', 'shortcuts'] as const
    for (const k of keys) {
      useUiStore.getState().openSettings(k)
      expect(useUiStore.getState().settingsModal).toBe('account')
    }
  })

  it('routes legacy usage settings entry to account billing', () => {
    useUiStore.getState().openSettings('usage')

    expect(useUiStore.getState().settingsModal).toBe('account-billing')
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


it('accepts persisted employees route', async () => {
  localStorage.setItem('aijia-ui-route', JSON.stringify({ kind: 'employees' }))

  vi.resetModules()
  const { useUiStore: freshStore } = await import('../uiStore')

  expect(freshStore.getState().route).toEqual({ kind: 'employees' })
})

describe('uiStore sidebar visibility', () => {
  beforeEach(() => {
    localStorage.clear()
    useUiStore.setState({ sidebarHidden: false })
  })

  it('toggles and persists sidebar hidden state', () => {
    useUiStore.getState().toggleSidebarHidden()

    expect(useUiStore.getState().sidebarHidden).toBe(true)
    expect(localStorage.getItem('aijia-sidebar-hidden')).toBe('true')

    useUiStore.getState().setSidebarHidden(false)

    expect(useUiStore.getState().sidebarHidden).toBe(false)
    expect(localStorage.getItem('aijia-sidebar-hidden')).toBe('false')
  })

  it('restores persisted sidebar hidden state on store initialization', async () => {
    localStorage.setItem('aijia-sidebar-hidden', 'true')

    vi.resetModules()
    const { useUiStore: freshStore } = await import('../uiStore')

    expect(freshStore.getState().sidebarHidden).toBe(true)
  })
})

describe('uiStore reasoning mode persistence', () => {
  beforeEach(() => {
    localStorage.clear()
    useUiStore.setState({ reasoningModesBySession: {} })
  })

  it('persists reasoning mode per session', () => {
    useUiStore.getState().setReasoningModeForSession('conv-1', 'deep')

    expect(useUiStore.getState().reasoningModesBySession['conv-1']).toBe('deep')
    expect(JSON.parse(localStorage.getItem('aijia-reasoning-modes-by-session') ?? '{}')).toEqual({
      'conv-1': 'deep',
    })
  })

  it('restores persisted reasoning modes on store initialization', async () => {
    localStorage.setItem(
      'aijia-reasoning-modes-by-session',
      JSON.stringify({ 'conv-restored': 'deep' }),
    )

    vi.resetModules()
    const { useUiStore: freshStore } = await import('../uiStore')

    expect(freshStore.getState().reasoningModesBySession['conv-restored']).toBe('deep')
  })

  it('drops unknown persisted reasoning modes', async () => {
    localStorage.setItem(
      'aijia-reasoning-modes-by-session',
      JSON.stringify({ good: 'auto', bad: 'xhigh' }),
    )

    vi.resetModules()
    const { useUiStore: freshStore } = await import('../uiStore')

    expect(freshStore.getState().reasoningModesBySession).toEqual({ good: 'auto' })
  })
})

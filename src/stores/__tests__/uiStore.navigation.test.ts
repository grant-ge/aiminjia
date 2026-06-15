import { beforeEach, describe, expect, it } from 'vitest'

import { useUiStore } from '../uiStore'

describe('uiStore route history', () => {
  beforeEach(() => {
    localStorage.clear()
    useUiStore.setState({
      route: { kind: 'home' },
      backStack: [],
      forwardStack: [],
    })
  })

  it('pushes the current route when navigating to a different route', () => {
    useUiStore.getState().setRoute({ kind: 'skill-center' })
    useUiStore.getState().setRoute({ kind: 'skill-detail', skillId: 'sales-followup' })

    expect(useUiStore.getState().route).toEqual({
      kind: 'skill-detail',
      skillId: 'sales-followup',
    })
    expect(useUiStore.getState().backStack).toEqual([
      { kind: 'home' },
      { kind: 'skill-center' },
    ])
    expect(useUiStore.getState().forwardStack).toEqual([])
  })

  it('does not push duplicate route entries', () => {
    useUiStore.getState().setRoute({ kind: 'skill-center' })
    useUiStore.getState().setRoute({ kind: 'skill-center' })

    expect(useUiStore.getState().backStack).toEqual([{ kind: 'home' }])
  })

  it('replaces the current route without changing history', () => {
    useUiStore.getState().setRoute({ kind: 'chat', conversationId: 'optimistic' })
    useUiStore.getState().replaceRoute({ kind: 'chat', conversationId: 'backend' })

    expect(useUiStore.getState().route).toEqual({ kind: 'chat', conversationId: 'backend' })
    expect(useUiStore.getState().backStack).toEqual([{ kind: 'home' }])
    expect(useUiStore.getState().forwardStack).toEqual([])
  })

  it('goes back and forward through app routes', () => {
    useUiStore.getState().setRoute({ kind: 'chat', conversationId: 'conv-1' })
    useUiStore.getState().setRoute({ kind: 'skill-detail', skillId: 'sales-followup' })

    expect(useUiStore.getState().canGoBack()).toBe(true)
    expect(useUiStore.getState().canGoForward()).toBe(false)

    useUiStore.getState().goBack()

    expect(useUiStore.getState().route).toEqual({ kind: 'chat', conversationId: 'conv-1' })
    expect(useUiStore.getState().backStack).toEqual([{ kind: 'home' }])
    expect(useUiStore.getState().forwardStack).toEqual([
      { kind: 'skill-detail', skillId: 'sales-followup' },
    ])

    useUiStore.getState().goForward()

    expect(useUiStore.getState().route).toEqual({
      kind: 'skill-detail',
      skillId: 'sales-followup',
    })
    expect(useUiStore.getState().backStack).toEqual([
      { kind: 'home' },
      { kind: 'chat', conversationId: 'conv-1' },
    ])
    expect(useUiStore.getState().forwardStack).toEqual([])
  })

  it('keeps the current route when there is no history in that direction', () => {
    useUiStore.getState().goBack()
    expect(useUiStore.getState().route).toEqual({ kind: 'home' })

    useUiStore.getState().goForward()
    expect(useUiStore.getState().route).toEqual({ kind: 'home' })
  })
})

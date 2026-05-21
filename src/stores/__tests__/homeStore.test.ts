import { beforeEach, describe, expect, it, vi } from 'vitest'

const { mockUpdateSettings, mockGetSettings } = vi.hoisted(() => {
  const mockUpdateSettings = vi.fn().mockResolvedValue(undefined)
  const mockGetSettings = vi.fn().mockResolvedValue({})
  return { mockUpdateSettings, mockGetSettings }
})

vi.mock('@/lib/tauri', () => ({
  updateSettings: mockUpdateSettings,
  getSettings: mockGetSettings,
}))

import { hydrateHomeStore, useHomeStore } from '../homeStore'

beforeEach(() => {
  useHomeStore.setState({ selectedWorkspace: null, recentWorkspaces: [] })
  mockUpdateSettings.mockClear()
  mockGetSettings.mockClear()
  mockGetSettings.mockResolvedValue({})
})

describe('homeStore', () => {
  it('hydrates from empty settings to empty state', () => {
    hydrateHomeStore({ uiHomeSelectedWorkspace: '', uiHomeRecentWorkspaces: '' } as any)
    expect(useHomeStore.getState().selectedWorkspace).toBe(null)
    expect(useHomeStore.getState().recentWorkspaces).toEqual([])
  })

  it('hydrates selectedWorkspace and recentWorkspaces from JSON strings', () => {
    hydrateHomeStore({
      uiHomeSelectedWorkspace: '{"id":"ws-1","rootPath":"/x","displayName":"x"}',
      uiHomeRecentWorkspaces:
        '[{"id":"ws-1","rootPath":"/x","displayName":"x"},{"id":"ws-2","rootPath":"/y","displayName":"y"}]',
    } as any)
    expect(useHomeStore.getState().selectedWorkspace?.id).toBe('ws-1')
    expect(useHomeStore.getState().recentWorkspaces).toHaveLength(2)
  })

  it('hydrates gracefully when JSON is malformed', () => {
    hydrateHomeStore({
      uiHomeSelectedWorkspace: 'not-json',
      uiHomeRecentWorkspaces: '[invalid',
    } as any)
    expect(useHomeStore.getState().selectedWorkspace).toBe(null)
    expect(useHomeStore.getState().recentWorkspaces).toEqual([])
  })

  it('setSelectedWorkspace pushes to recentWorkspaces head and persists', async () => {
    const ws = { id: 'ws-1', rootPath: '/x', displayName: 'x' }
    useHomeStore.getState().setSelectedWorkspace(ws as any)
    expect(useHomeStore.getState().selectedWorkspace).toEqual(ws)
    expect(useHomeStore.getState().recentWorkspaces[0]).toEqual(ws)
    // persist is fire-and-forget; await one microtask
    await Promise.resolve()
    await Promise.resolve()
    expect(mockUpdateSettings).toHaveBeenCalled()
  })

  it('recentWorkspaces is capped at 10', () => {
    for (let i = 0; i < 15; i++) {
      useHomeStore.getState().setSelectedWorkspace({
        id: `ws-${i}`,
        rootPath: `/x${i}`,
        displayName: `x${i}`,
      } as any)
    }
    expect(useHomeStore.getState().recentWorkspaces).toHaveLength(10)
    // newest at head
    expect(useHomeStore.getState().recentWorkspaces[0].id).toBe('ws-14')
  })

  it('removeRecentWorkspace removes by rootPath', () => {
    useHomeStore.getState().setSelectedWorkspace({
      id: 'ws-1',
      rootPath: '/x',
      displayName: 'x',
    } as any)
    useHomeStore.getState().setSelectedWorkspace({
      id: 'ws-2',
      rootPath: '/y',
      displayName: 'y',
    } as any)
    expect(useHomeStore.getState().recentWorkspaces).toHaveLength(2)
    useHomeStore.getState().removeRecentWorkspace('/x')
    expect(useHomeStore.getState().recentWorkspaces).toHaveLength(1)
    expect(useHomeStore.getState().recentWorkspaces[0].id).toBe('ws-2')
  })

  it('does not call localStorage.setItem', () => {
    const spy = vi.spyOn(window.localStorage.__proto__ as any, 'setItem')
    useHomeStore.getState().setSelectedWorkspace({
      id: 'ws-1',
      rootPath: '/x',
      displayName: 'x',
    } as any)
    expect(spy).not.toHaveBeenCalled()
    spy.mockRestore()
  })
})

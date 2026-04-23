import { beforeEach, describe, expect, it } from 'vitest'

import { useUiStore } from '../uiStore'

describe('uiStore.settingsModal', () => {
  beforeEach(() => {
    useUiStore.getState().closeSettings()
  })

  it('accepts all 7 plan-C keys', () => {
    const keys = ['account', 'usage', 'permissions', 'mcp', 'sso', 'shortcuts', 'about'] as const
    for (const k of keys) {
      useUiStore.getState().openSettings(k)
      expect(useUiStore.getState().settingsModal).toBe(k)
    }
  })
})

import { beforeEach, describe, expect, it, vi } from 'vitest'

import { DEV_SETTINGS_STORAGE_KEY } from '../devSettingsStore'

describe('devSettingsStore', () => {
  beforeEach(() => {
    localStorage.removeItem(DEV_SETTINGS_STORAGE_KEY)
    vi.resetModules()
  })

  it('默认关闭工具失败诊断显示', async () => {
    const { useDevSettingsStore } = await import('../devSettingsStore')

    expect(useDevSettingsStore.getState().showToolErrorIcon).toBe(false)
  })
})

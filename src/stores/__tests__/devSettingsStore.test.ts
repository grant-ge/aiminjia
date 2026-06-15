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

  it('默认关闭技能原始内容显示', async () => {
    const { useDevSettingsStore } = await import('../devSettingsStore')

    expect(useDevSettingsStore.getState().showRawSkillContent).toBe(false)
  })

  it('持久化技能原始内容显示开关', async () => {
    const { useDevSettingsStore } = await import('../devSettingsStore')

    useDevSettingsStore.getState().setShowRawSkillContent(true)

    expect(useDevSettingsStore.getState().showRawSkillContent).toBe(true)
    expect(JSON.parse(localStorage.getItem(DEV_SETTINGS_STORAGE_KEY) ?? '{}')).toMatchObject({
      showRawSkillContent: true,
    })
  })
})

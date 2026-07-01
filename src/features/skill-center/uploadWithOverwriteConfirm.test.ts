import { beforeEach, describe, expect, it, vi } from 'vitest'

import { useBrandingStore } from '@/stores/brandingStore'
import { SkillAlreadyExistsError } from '@/stores/skillStore'

import { uploadWithOverwriteConfirm } from './uploadWithOverwriteConfirm'

const askMock = vi.hoisted(() => vi.fn())

vi.mock('@tauri-apps/plugin-dialog', () => ({
  ask: askMock,
}))

describe('uploadWithOverwriteConfirm', () => {
  beforeEach(() => {
    askMock.mockReset()
    useBrandingStore.getState().reset()
  })

  it('uses tenant product name in overwrite confirmation title', async () => {
    useBrandingStore.setState({ productName: '小新助手' })
    askMock.mockResolvedValueOnce(false)
    const upload = vi.fn(async (force: boolean) => {
      if (!force) throw new SkillAlreadyExistsError('report-writer')
    })

    await expect(uploadWithOverwriteConfirm(upload)).resolves.toBe('cancelled')

    expect(askMock).toHaveBeenCalledWith(
      '技能 "report-writer" 已存在，是否覆盖？',
      { title: '小新助手', kind: 'warning' },
    )
  })
})

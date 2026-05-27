import { beforeEach, describe, expect, it, vi } from 'vitest'

const tauriMock = vi.hoisted(() => ({
  listSkills: vi.fn().mockResolvedValue([
    { id: 'write-plan', displayName: '写计划', description: 'desc', source: 'builtin', hasWorkflow: true, icon: 'file-text', category: 'general', triggerText: '', shortDescription: 'short', displayNameEn: 'Plan', shortDescriptionEn: 'short' },
    { id: 'shop-report', displayName: '店铺日报', description: 'desc', source: 'user', hasWorkflow: false, icon: 'store', category: 'ops', triggerText: '', shortDescription: 'short', displayNameEn: 'Ops', shortDescriptionEn: 'short' },
  ]),
  installCustomSkill: vi.fn().mockResolvedValue('installed'),
  uninstallCustomSkill: vi.fn().mockResolvedValue(undefined),
}))

vi.mock('@/lib/tauri', () => tauriMock)
vi.mock('@/i18n', () => ({
  default: { language: 'en-US' },
}))

import { useSkillStore } from '@/stores/skillStore'

describe('skillStore', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    useSkillStore.setState({ skills: [], recommendedIds: ['write-plan'], isLoading: false })
  })

  it('reload 后可按分类过滤', async () => {
    await useSkillStore.getState().reload()

    expect(tauriMock.listSkills).toHaveBeenCalledWith('en-US')
    expect(useSkillStore.getState().listByCategory('general')).toHaveLength(1)
    expect(useSkillStore.getState().listByCategory('recommended')).toHaveLength(1)
    expect(useSkillStore.getState().getById('shop-report')?.displayName).toBe('店铺日报')
  })

  it('初始状态不再使用 mock 技能，等待后端 reload', () => {
    expect(useSkillStore.getState().skills).toEqual([])
  })

  it('upload 调用后端安装本地技能目录并刷新列表', async () => {
    await useSkillStore.getState().upload('/tmp/my-skill')

    expect(tauriMock.installCustomSkill).toHaveBeenCalledWith('/tmp/my-skill', false)
    expect(tauriMock.listSkills).toHaveBeenCalled()
  })

  it('upload 将结构化 alreadyExists 错误转换为 SkillAlreadyExistsError', async () => {
    tauriMock.installCustomSkill.mockRejectedValueOnce({ kind: 'alreadyExists', detail: 'dup-skill' })

    await expect(useSkillStore.getState().upload('/tmp/dup-skill')).rejects.toMatchObject({
      name: 'SkillAlreadyExistsError',
      skillId: 'dup-skill',
    })
  })

  it('upload 将结构化校验错误转换为 SkillValidationError', async () => {
    tauriMock.installCustomSkill.mockRejectedValueOnce({ kind: 'missingSkillMd' })

    await expect(useSkillStore.getState().upload('/tmp/bad-skill')).rejects.toMatchObject({
      name: 'SkillValidationError',
      kind: 'missingSkillMd',
    })
  })

  it('upload 透传 parseFailed 的 detail', async () => {
    tauriMock.installCustomSkill.mockRejectedValueOnce({ kind: 'parseFailed', detail: 'yaml 错误' })

    await expect(useSkillStore.getState().upload('/tmp/bad-skill')).rejects.toMatchObject({
      name: 'SkillValidationError',
      kind: 'parseFailed',
      detail: 'yaml 错误',
    })
  })

  it('upload 支持强制覆盖重复技能并刷新列表', async () => {
    await useSkillStore.getState().upload('/tmp/dup-skill', true)

    expect(tauriMock.installCustomSkill).toHaveBeenCalledWith('/tmp/dup-skill', true)
    expect(tauriMock.listSkills).toHaveBeenCalled()
  })
})

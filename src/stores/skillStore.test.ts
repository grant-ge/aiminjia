import { beforeEach, describe, expect, it, vi } from 'vitest'

const tauriMock = vi.hoisted(() => ({
  listSkills: vi.fn().mockResolvedValue([
    { id: 'write-plan', displayName: '写计划', description: 'desc', source: 'builtin', hasWorkflow: true, icon: 'file-text', category: 'dev', triggerText: '', shortDescription: 'short', displayNameEn: 'Plan', shortDescriptionEn: 'short' },
    { id: 'shop-report', displayName: '店铺日报', description: 'desc', source: 'user', hasWorkflow: false, icon: 'store', category: 'ops', triggerText: '', shortDescription: 'short', displayNameEn: 'Ops', shortDescriptionEn: 'short' },
  ]),
}))

vi.mock('@/lib/tauri', () => tauriMock)

import { useSkillStore } from '@/stores/skillStore'

describe('skillStore', () => {
  beforeEach(() => {
    useSkillStore.setState({ skills: [], recommendedIds: ['write-plan'], isLoading: false })
  })

  it('reload 后可按分类过滤', async () => {
    await useSkillStore.getState().reload()

    expect(useSkillStore.getState().listByCategory('dev')).toHaveLength(1)
    expect(useSkillStore.getState().listByCategory('recommended')).toHaveLength(1)
    expect(useSkillStore.getState().getById('shop-report')?.displayName).toBe('店铺日报')
  })
})

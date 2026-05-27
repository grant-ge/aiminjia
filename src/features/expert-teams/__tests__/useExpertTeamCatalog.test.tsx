import { act, renderHook, waitFor } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import i18n from '@/i18n'
import type { ExpertTeamSnapshot } from '@/lib/tauri'

const mocks = vi.hoisted(() => ({
  expertTeamTemplateCatalog: vi.fn(),
}))
vi.mock('@/lib/tauri', () => ({
  expertTeamTemplateCatalog: mocks.expertTeamTemplateCatalog,
}))

import { BUILTIN_EXPERT_TEAMS } from '../teams'
import { getCachedExpertTeam, seedExpertTeamCatalog, useExpertTeamCatalog } from '../useExpertTeamCatalog'

function makeSnapshot(): ExpertTeamSnapshot {
  return {
    teamId: 'remote-growth-council',
    version: '1.0.0',
    facilitationStyle: 'open',
    displayI18n: {
      'zh-CN': {
        name: '增长议事团',
        tagline: '中文副标题',
        examples: ['中文案例'],
        composerPlaceholder: '中文占位',
      },
      'en-US': {
        name: 'Growth Council',
        tagline: 'English tagline',
        examples: ['English example'],
        composerPlaceholder: 'English placeholder',
      },
    },
    experts: [
      {
        stableName: 'growth-lead',
        emoji: 'G',
        displayI18n: {
          'zh-CN': { name: '增长负责人' },
          'en-US': { name: 'Growth Lead' },
        },
        promptI18n: {
          'zh-CN': { persona: '关注中文增长策略' },
          'en-US': { persona: 'Focuses on growth strategy' },
        },
      },
    ],
    directorPromptI18n: {},
  }
}

describe('useExpertTeamCatalog', () => {
  let consoleWarn: ReturnType<typeof vi.spyOn>

  beforeEach(async () => {
    consoleWarn = vi.spyOn(console, 'warn').mockImplementation(() => {})
    mocks.expertTeamTemplateCatalog.mockReset()
    seedExpertTeamCatalog(BUILTIN_EXPERT_TEAMS)
    await act(async () => {
      await i18n.changeLanguage('zh-CN')
    })
  })

  afterEach(async () => {
    consoleWarn.mockRestore()
    await act(async () => {
      await i18n.changeLanguage('zh-CN')
    })
  })

  it('falls back to builtin expert teams when IPC fails', async () => {
    mocks.expertTeamTemplateCatalog.mockRejectedValue(new Error('ipc unavailable'))
    const { result } = renderHook(() => useExpertTeamCatalog())
    await waitFor(() => expect(result.current.isLoading).toBe(false))
    expect(result.current.teams.length).toBeGreaterThan(0)
    expect(result.current.source).toBe('bootstrap')
  })

  it('remaps loaded snapshots and cached lookup when language changes', async () => {
    mocks.expertTeamTemplateCatalog.mockResolvedValue([makeSnapshot()])

    const { result } = renderHook(() => useExpertTeamCatalog())

    await waitFor(() => expect(result.current.source).toBe('remote'))
    expect(result.current.teams[0].name).toBe('增长议事团')
    expect(getCachedExpertTeam('remote-growth-council')?.name).toBe('增长议事团')

    await act(async () => {
      await i18n.changeLanguage('en-US')
    })

    await waitFor(() => expect(result.current.teams[0].name).toBe('Growth Council'))
    expect(result.current.teams[0].experts[0].name).toBe('Growth Lead')
    expect(getCachedExpertTeam('remote-growth-council')?.name).toBe('Growth Council')
  })

  it('maps production expert persona fields and OSS atlas avatars', async () => {
    mocks.expertTeamTemplateCatalog.mockResolvedValue([
      {
        teamId: 'remote-marketing-council',
        version: '1.0.0',
        facilitationStyle: 'rounds',
        displayI18n: {
          'zh-CN': {
            name: '营销策划团',
            tagline: '中文副标题',
          },
        },
        experts: [
          {
            stableName: 'brand-lead',
            agentName: 'brand-lead',
            name: '品牌负责人',
            persona: '关注定位、调性、长期心智占领',
            emoji: '🎨',
            avatar: {
              kind: 'atlas',
              url: 'https://lotus-releases.oss-cn-beijing.aliyuncs.com/desktop-resources/expert-team-avatars/v1/avatar-atlas.svg',
              x: 96,
              y: 0,
              w: 96,
              h: 96,
              atlasWidth: 672,
              atlasHeight: 384,
            },
          },
        ],
        directorPromptI18n: {},
      } as ExpertTeamSnapshot,
    ])

    const { result } = renderHook(() => useExpertTeamCatalog())

    await waitFor(() => expect(result.current.source).toBe('remote'))
    expect(result.current.teams[0].experts[0]).toMatchObject({
      name: '品牌负责人',
      agentName: 'brand-lead',
      persona: '关注定位、调性、长期心智占领',
      avatar: {
        kind: 'atlas',
        url: 'https://lotus-releases.oss-cn-beijing.aliyuncs.com/desktop-resources/expert-team-avatars/v1/avatar-atlas.svg',
      },
    })
  })
})

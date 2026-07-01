import '@testing-library/jest-dom'
import { render, screen, waitFor } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import { useBrandingStore } from '@/stores/brandingStore'
import { useDevSettingsStore } from '@/stores/devSettingsStore'

import { SkillDetailDialog } from './SkillDetailDialog'

const tauriMock = vi.hoisted(() => ({
  getSkillDetail: vi.fn(),
  previewMarketplaceSkill: vi.fn(),
}))

vi.mock('@/lib/tauri', () => tauriMock)

const MARKET_ITEM = {
  id: 101,
  pluginId: 'deep-research',
  name: '深入研究',
  description: '深度研究助手',
  category: 'general',
  icon: 'search',
  version: '1.0',
  scope: 'public',
  status: 'published',
  downloads: 1,
  featured: false,
  packageSize: 128,
  tenantName: '',
  createdAt: '2026-06-10T00:00:00Z',
}

const INSTALLED_SKILL = {
  id: 'local-report',
  displayName: '本地日报',
  displayNameEn: 'Local Report',
  description: '本地日报技能',
  source: 'user',
  enabled: true,
  hasWorkflow: false,
  icon: 'file-text',
  shortDescription: '日报',
  shortDescriptionEn: 'report',
  triggerText: '/local-report',
  category: 'ops',
  updatedAt: null,
  version: '0.1.0',
}

describe('SkillDetailDialog', () => {
  beforeEach(() => {
    tauriMock.getSkillDetail.mockReset()
    tauriMock.previewMarketplaceSkill.mockReset()
    useBrandingStore.getState().reset()
    useDevSettingsStore.setState({ showRawSkillContent: false })
  })

  it('does not request raw detail when raw skill content is hidden', () => {
    render(
      <SkillDetailDialog
        open
        skill={INSTALLED_SKILL}
        onOpenChange={() => {}}
        onUse={() => {}}
      />,
    )

    expect(screen.getByTestId('skill-detail-dialog-title')).toHaveTextContent('本地日报')
    expect(screen.queryByText('正在加载技能详情...')).toBeNull()
    expect(tauriMock.getSkillDetail).not.toHaveBeenCalled()
    expect(tauriMock.previewMarketplaceSkill).not.toHaveBeenCalled()
  })

  it('renders usage instructions and notes in the dialog', () => {
    render(
      <SkillDetailDialog
        open
        skill={INSTALLED_SKILL}
        onOpenChange={() => {}}
        onUse={() => {}}
      />,
    )

    expect(screen.getByText('使用说明')).toBeInTheDocument()
    expect(screen.getByText(/点击“使用”后/)).toBeInTheDocument()
    expect(screen.getByText('注意事项')).toBeInTheDocument()
    expect(screen.getByText(/技能 chip 只对当前这一轮消息生效/)).toBeInTheDocument()
  })

  it('uses tenant product name in usage instructions', () => {
    useBrandingStore.setState({ productName: '小新助手' })

    render(
      <SkillDetailDialog
        open
        skill={INSTALLED_SKILL}
        onOpenChange={() => {}}
        onUse={() => {}}
      />,
    )

    expect(screen.getByText('发送后，小新助手会按该技能的规则处理本轮请求。')).toBeInTheDocument()
    expect(screen.queryByText(/AI 小家/)).not.toBeInTheDocument()
  })

  it('previews raw SKILL.md from remote marketplace package without installing', async () => {
    useDevSettingsStore.setState({ showRawSkillContent: true })
    tauriMock.previewMarketplaceSkill.mockResolvedValueOnce({
      rawContent: '---\nname: deep-research\ndescription: deep\n---\n\n# Deep Research',
    })

    render(
      <SkillDetailDialog
        open
        marketplaceItem={MARKET_ITEM}
        onOpenChange={() => {}}
        onInstall={() => {}}
      />,
    )

    expect(tauriMock.previewMarketplaceSkill).toHaveBeenCalledWith(101, 'deep-research')
    await waitFor(() => expect(screen.getByText('原始技能内容')).toBeInTheDocument())
    expect(screen.getByText(/name: deep-research/)).toBeInTheDocument()
    expect(tauriMock.getSkillDetail).not.toHaveBeenCalled()
  })
})

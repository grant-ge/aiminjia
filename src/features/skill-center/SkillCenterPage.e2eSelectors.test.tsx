import '@testing-library/jest-dom'
import { fireEvent, render, waitFor } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import { SkillCenterPage } from '@/features/skill-center/SkillCenterPage'
import i18n from '@/i18n'
import { useAuthStore } from '@/stores/authStore'
import { useSkillStore } from '@/stores/skillStore'
import { useUiStore } from '@/stores/uiStore'

const openDialogMock = vi.hoisted(() => vi.fn())
const askDialogMock = vi.hoisted(() => vi.fn())
const tauriMock = vi.hoisted(() => ({
  listMarketplaceSkills: vi.fn(),
  refreshSkillRegistry: vi.fn().mockResolvedValue(undefined),
  syncBuiltinSkills: vi.fn().mockResolvedValue({ installed: [], updated: [], skipped: [], changed: [] }),
  listSkills: vi.fn().mockResolvedValue([]),
  installCustomSkill: vi.fn().mockResolvedValue('installed'),
  installMarketplaceSkill: vi.fn().mockResolvedValue('installed'),
  setSkillEnabled: vi.fn().mockResolvedValue(undefined),
  uninstallCustomSkill: vi.fn().mockResolvedValue('uninstalled'),
}))

vi.mock('@tauri-apps/plugin-dialog', () => ({
  open: openDialogMock,
  ask: askDialogMock,
}))

vi.mock('@/lib/tauri', () => tauriMock)

const BUILTIN_SKILL = {
  id: 'create-skill',
  displayName: 'Create Skill',
  displayNameEn: 'Create Skill',
  description: 'Create new skills',
  shortDescription: 'Create new skills',
  shortDescriptionEn: 'Create new skills',
  source: 'global',
  hasWorkflow: true,
  icon: 'tool',
  category: 'general',
  triggerText: '/create-skill',
  updatedAt: null,
  enabled: true,
}

const INSTALLED_SKILL = {
  id: 'local-report',
  displayName: 'Local Report',
  displayNameEn: 'Local Report',
  description: 'User installed skill',
  shortDescription: 'User installed skill',
  shortDescriptionEn: 'User installed skill',
  source: 'user',
  hasWorkflow: false,
  icon: 'file-text',
  category: 'ops',
  triggerText: '/local-report',
  updatedAt: null,
  enabled: false,
}

const MARKET_SKILL_ITEM = {
  id: 101,
  pluginId: 'tenant-policy',
  name: 'Tenant Policy',
  description: 'Tenant distributed skill',
  category: 'general',
  icon: 'building',
  version: '1.0',
  scope: 'tenant',
  status: 'published',
  downloads: 10,
  featured: false,
  packageSize: 128,
  tenantName: 'ACME',
  createdAt: '2026-06-10T00:00:00Z',
}

function seedStore() {
  useSkillStore.setState({
    skills: [BUILTIN_SKILL, INSTALLED_SKILL],
    recommendedIds: [],
    isLoading: false,
    reload: vi.fn().mockResolvedValue(undefined),
    upload: vi.fn().mockResolvedValue(undefined),
    installMarketplace: vi.fn().mockResolvedValue(undefined),
    uninstall: vi.fn().mockResolvedValue(undefined),
    setSkillEnabled: vi.fn().mockResolvedValue(undefined),
  })
}

describe('SkillCenterPage e2e selectors', () => {
  beforeEach(async () => {
    await i18n.changeLanguage('en-US')
    openDialogMock.mockReset()
    askDialogMock.mockReset()
    tauriMock.listMarketplaceSkills.mockReset()
    tauriMock.listMarketplaceSkills.mockResolvedValue({
      items: [MARKET_SKILL_ITEM],
      total: 1,
      page: 1,
      size: 100,
    })
    seedStore()
    useAuthStore.setState({ isLoggedIn: true })
    useUiStore.setState({ route: { kind: 'skill-center' }, settingsModal: null, pendingSkill: null })
  })

  it('exposes tabs, cards, market action and enablement toggles', async () => {
    const { container } = render(<SkillCenterPage />)

    expect(container.querySelector('[data-aijia-skill-tab="market"]')).toBeInTheDocument()
    expect(container.querySelector('[data-aijia-skill-tab="builtin"]')).toBeInTheDocument()
    expect(container.querySelector('[data-aijia-skill-tab="installed"]')).toBeInTheDocument()
    expect(container.querySelector('[data-aijia-skill-sync-trigger]')).toBeInTheDocument()

    await waitFor(() => expect(tauriMock.listMarketplaceSkills).toHaveBeenCalled())
    const marketCard = container.querySelector('[data-aijia-skill-card][data-aijia-skill-id="tenant-policy"]')
    expect(marketCard).toHaveAttribute('data-aijia-skill-source', 'tenant')
    expect(marketCard).not.toHaveAttribute('data-aijia-skill-enabled')
    expect(marketCard).toHaveAttribute('data-aijia-skill-market-card', 'true')
    expect(marketCard).toHaveAttribute('data-aijia-skill-installed', 'false')
    expect(marketCard?.querySelector('[data-aijia-skill-market-action="add"]')).toBeInTheDocument()

    fireEvent.click(container.querySelector('[data-aijia-skill-tab="installed"]') as HTMLElement)
    const installedCard = container.querySelector('[data-aijia-skill-card][data-aijia-skill-id="local-report"]')
    expect(installedCard).toHaveAttribute('data-aijia-skill-source', 'user')
    expect(installedCard).toHaveAttribute('data-aijia-skill-enabled', 'false')
    expect(container.querySelector('[data-aijia-skill-toggle="local-report"]')).toHaveAttribute('aria-checked', 'false')
  })
})

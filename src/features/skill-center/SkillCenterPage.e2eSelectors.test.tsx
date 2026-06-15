import '@testing-library/jest-dom'
import { fireEvent, render } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import { SkillCenterPage } from '@/features/skill-center/SkillCenterPage'
import i18n from '@/i18n'
import { useAuthStore } from '@/stores/authStore'
import { useSkillStore } from '@/stores/skillStore'
import { useUiStore } from '@/stores/uiStore'

const openDialogMock = vi.hoisted(() => vi.fn())
const askDialogMock = vi.hoisted(() => vi.fn())

vi.mock('@tauri-apps/plugin-dialog', () => ({
  open: openDialogMock,
  ask: askDialogMock,
}))

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

const MARKET_SKILL = {
  id: 'tenant-policy',
  displayName: 'Tenant Policy',
  displayNameEn: 'Tenant Policy',
  description: 'Tenant distributed skill',
  shortDescription: 'Tenant distributed skill',
  shortDescriptionEn: 'Tenant distributed skill',
  source: 'tenant',
  hasWorkflow: false,
  icon: 'building',
  category: 'general',
  triggerText: '/tenant-policy',
  updatedAt: null,
  enabled: false,
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

function seedStore() {
  useSkillStore.setState({
    skills: [BUILTIN_SKILL, MARKET_SKILL, INSTALLED_SKILL],
    recommendedIds: [],
    isLoading: false,
    reload: vi.fn().mockResolvedValue(undefined),
    upload: vi.fn().mockResolvedValue(undefined),
    uninstall: vi.fn().mockResolvedValue(undefined),
    setSkillEnabled: vi.fn().mockResolvedValue(undefined),
  })
}

describe('SkillCenterPage e2e selectors', () => {
  beforeEach(async () => {
    await i18n.changeLanguage('en-US')
    openDialogMock.mockReset()
    askDialogMock.mockReset()
    seedStore()
    useAuthStore.setState({ isLoggedIn: true })
    useUiStore.setState({ route: { kind: 'skill-center' }, settingsModal: null })
  })

  it('exposes tabs, cards, market action and enablement toggles', () => {
    const { container } = render(<SkillCenterPage />)

    expect(container.querySelector('[data-aijia-skill-tab="market"]')).toBeInTheDocument()
    expect(container.querySelector('[data-aijia-skill-tab="builtin"]')).toBeInTheDocument()
    expect(container.querySelector('[data-aijia-skill-tab="installed"]')).toBeInTheDocument()
    expect(container.querySelector('[data-aijia-skill-sync-trigger]')).toBeInTheDocument()

    const marketCard = container.querySelector('[data-aijia-skill-card][data-aijia-skill-id="tenant-policy"]')
    expect(marketCard).toHaveAttribute('data-aijia-skill-source', 'tenant')
    expect(marketCard).toHaveAttribute('data-aijia-skill-enabled', 'false')
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

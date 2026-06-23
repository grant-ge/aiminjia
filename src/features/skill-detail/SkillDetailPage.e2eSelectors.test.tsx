import '@testing-library/jest-dom'
import { render } from '@testing-library/react'
import { beforeEach, describe, expect, it } from 'vitest'

import i18n from '@/i18n'
import { useSkillStore } from '@/stores/skillStore'
import { useUiStore } from '@/stores/uiStore'

import { SkillDetailPage } from './SkillDetailPage'

const INSTALLED_SKILL = {
  id: 'biz-proposal',
  displayName: 'Business Proposal',
  displayNameEn: 'Business Proposal',
  description: 'Write business proposals',
  shortDescription: 'Write business proposals',
  shortDescriptionEn: 'Write business proposals',
  source: 'user',
  hasWorkflow: true,
  icon: 'sparkles',
  category: 'general',
  triggerText: '/biz-proposal',
  updatedAt: null,
  enabled: true,
}

describe('SkillDetailPage e2e selectors', () => {
  beforeEach(async () => {
    await i18n.changeLanguage('en-US')
    useSkillStore.setState({
      skills: [INSTALLED_SKILL],
      setSkillEnabled: async () => undefined,
    })
    useUiStore.setState({
      route: { kind: 'skill-detail', skillId: 'biz-proposal' },
      settingsModal: null,
    })
  })

  it('exposes detail state, primary action, secondary action and toggle selectors', () => {
    const { container } = render(<SkillDetailPage skillId="biz-proposal" />)

    const detail = container.querySelector('[data-aijia-skill-detail]')
    expect(detail).toHaveAttribute('data-aijia-skill-id', 'biz-proposal')
    expect(detail).toHaveAttribute('data-aijia-skill-enabled', 'true')
    expect(container.querySelector('[data-aijia-skill-detail-action="primary"]')).toBeInTheDocument()
    expect(container.querySelector('[data-aijia-skill-detail-action="disable"]')).toBeInTheDocument()
    expect(container.querySelector('[data-aijia-skill-toggle="biz-proposal"]')).toHaveAttribute('aria-checked', 'true')
  })
})

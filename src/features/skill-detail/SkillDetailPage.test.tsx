import '@testing-library/jest-dom'
import { act, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import i18n from '@/i18n'
import { useDevSettingsStore } from '@/stores/devSettingsStore'
import { useBrandingStore } from '@/stores/brandingStore'
import { useSkillStore } from '@/stores/skillStore'
import { useUiStore } from '@/stores/uiStore'

import { SkillDetailPage } from './SkillDetailPage'

const createConversationFromSkillMock = vi.hoisted(() => vi.fn())
const getSkillDetailMock = vi.hoisted(() => vi.fn())

vi.mock('@/hooks/useChat', () => ({
  useChat: () => ({ createConversationFromSkill: createConversationFromSkillMock }),
}))

vi.mock('@/lib/tauri', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/lib/tauri')>()),
  getSkillDetail: getSkillDetailMock,
}))

const enabledSkill = {
  id: 'biz-proposal',
  displayName: '商业方案撰写',
  displayNameEn: 'Business Proposal',
  description: '依据业务数据生成商业方案。',
  source: 'user',
  hasWorkflow: true,
  icon: 'sparkles',
  shortDescription: '商业方案撰写',
  shortDescriptionEn: 'Business proposal writing',
  triggerText: '/biz-proposal',
  category: 'general',
  updatedAt: null,
  enabled: true,
}

describe('SkillDetailPage', () => {
  beforeEach(() => {
    void i18n.changeLanguage('zh-CN')
    createConversationFromSkillMock.mockClear()
    getSkillDetailMock.mockReset()
    getSkillDetailMock.mockResolvedValue(null)
    useBrandingStore.getState().reset()
    useDevSettingsStore.setState({
      showToolErrorIcon: false,
      showRawSkillContent: false,
    })
    useSkillStore.setState({
      skills: [enabledSkill],
    })
    useUiStore.setState({
      route: { kind: 'skill-detail', skillId: 'biz-proposal' },
      settingsModal: null,
    })
  })

  it('uses the skill via the action bar without auto-running it', () => {
    render(<SkillDetailPage skillId="biz-proposal" />)

    expect(screen.getAllByText('商业方案撰写').length).toBeGreaterThan(0)
    fireEvent.click(screen.getByRole('button', { name: '使用' }))

    expect(createConversationFromSkillMock).not.toHaveBeenCalled()
    expect(useUiStore.getState().pendingSkill).toEqual({
      id: 'biz-proposal',
      label: '商业方案撰写',
      trigger: '/biz-proposal',
    })
    expect(useUiStore.getState().route).toEqual({ kind: 'home' })
  })

  it('returns to the previous route when opened from a chat card', () => {
    useUiStore.setState({
      route: { kind: 'skill-detail', skillId: 'biz-proposal' },
      backStack: [{ kind: 'chat', conversationId: 'conv-1' }],
      forwardStack: [],
    })

    render(<SkillDetailPage skillId="biz-proposal" />)

    fireEvent.click(screen.getByRole('button', { name: '返回' }))

    expect(useUiStore.getState().route).toEqual({ kind: 'chat', conversationId: 'conv-1' })
  })

  it('falls back to skill center when there is no previous route', () => {
    useUiStore.setState({
      route: { kind: 'skill-detail', skillId: 'biz-proposal' },
      backStack: [],
      forwardStack: [],
    })

    render(<SkillDetailPage skillId="biz-proposal" />)

    fireEvent.click(screen.getByRole('button', { name: '返回' }))

    expect(useUiStore.getState().route).toEqual({ kind: 'skill-center' })
  })

  it('renders the English skill name and description when language is English', async () => {
    await i18n.changeLanguage('en-US')

    render(<SkillDetailPage skillId="biz-proposal" />)

    expect(screen.getAllByText('Business Proposal').length).toBeGreaterThan(0)
    expect(screen.getByText('Business proposal writing')).toBeInTheDocument()
    expect(screen.queryByText('商业方案撰写')).toBeNull()
  })

  it('uses tenant product name in built-in source and usage copy', () => {
    useBrandingStore.setState({ productName: '小新助手' })
    useSkillStore.setState({
      skills: [{ ...enabledSkill, source: 'builtin' }],
    })

    render(<SkillDetailPage skillId="biz-proposal" />)

    expect(screen.getByText('小新助手内置')).toBeInTheDocument()
    expect(screen.getByText('发送后，小新助手会按该技能的规则处理本轮请求。')).toBeInTheDocument()
    expect(screen.queryByText(/AI 小家/)).not.toBeInTheDocument()
  })

  it('disabled installed skill must be enabled before use', async () => {
    const setSkillEnabled = vi.fn().mockImplementation(async (skillId: string, enabled: boolean) => {
      useSkillStore.setState({
        skills: useSkillStore.getState().skills.map((skill) =>
          skill.id === skillId ? { ...skill, enabled } : skill,
        ),
      })
    })
    useSkillStore.setState({
      skills: [{ ...enabledSkill, enabled: false }],
      setSkillEnabled,
    })

    render(<SkillDetailPage skillId="biz-proposal" />)
    await act(async () => {
      fireEvent.click(screen.getByRole('button', { name: '开启并使用' }))
    })

    expect(setSkillEnabled).toHaveBeenCalledWith('biz-proposal', true)
    await waitFor(() => {
      expect(useUiStore.getState().route).toEqual({ kind: 'home' })
    })
  })

  it('renders user-facing skill details with clipped previews while keeping the generic usage guide', async () => {
    getSkillDetailMock.mockResolvedValueOnce({
      id: 'biz-proposal',
      whenToUse: '用户需要商业计划书、项目建议书或融资 BP 时使用。',
      allowedTools: ['Read', 'Write', 'Bash'],
      argumentHint: '请提供业务目标、受众和期望交付格式。',
      arguments: ['业务背景', '目标受众'],
      context: 'inline',
      userInvocable: true,
      disableModelInvocation: false,
      rawContent: '---\nallowed-tools: Read, Write, Bash\n---\n## 核心原则\n先澄清目标。',
      body: [
        '## 输入约定',
        '用户需要提供业务目标。',
        '用户需要说明目标受众。',
        '用户需要说明交付格式。',
        '用户也可以补充品牌语气。',
        '',
        '## 执行步骤',
        '1. 读取资料',
        '2. 生成方案',
        '3. 校验内容',
        '4. 输出结果',
      ].join('\n'),
    })

    render(<SkillDetailPage skillId="biz-proposal" />)

    expect(await screen.findByText('适用场景')).toBeInTheDocument()
    expect(screen.getByText('用户需要商业计划书、项目建议书或融资 BP 时使用。')).toBeInTheDocument()
    expect(screen.getByText('开始前准备')).toBeInTheDocument()
    expect(screen.getByText(/校验内容\.\.\./)).toBeInTheDocument()
    expect(screen.getByText(/读取资料/).closest('p')?.textContent).not.toContain('\n')
    expect(screen.queryByText('Read')).not.toBeInTheDocument()
    expect(screen.queryByText('能力参数')).not.toBeInTheDocument()
    expect(screen.queryByText('原始技能内容')).not.toBeInTheDocument()
    expect(screen.getByText('请提供业务目标、受众和期望交付格式。')).toBeInTheDocument()
    expect(screen.getByText('使用说明')).toBeInTheDocument()
    expect(screen.getByText(/点击右上角“使用”后/)).toBeInTheDocument()
  })

  it('shows raw skill content only when the dev control switch is enabled', async () => {
    useDevSettingsStore.setState({
      showToolErrorIcon: false,
      showRawSkillContent: true,
    })
    getSkillDetailMock.mockResolvedValueOnce({
      id: 'biz-proposal',
      whenToUse: '用户需要商业计划书、项目建议书或融资 BP 时使用。',
      allowedTools: ['Read', 'Write', 'Bash'],
      argumentHint: null,
      arguments: [],
      context: 'inline',
      userInvocable: true,
      disableModelInvocation: false,
      body: '## 输入约定\n提供业务目标。',
      rawContent: '---\nallowed-tools: Read, Write, Bash\n---\n# 原始 SKILL.md 正文\n\n- 第一项',
    })

    render(<SkillDetailPage skillId="biz-proposal" />)

    expect(await screen.findByText('原始技能内容')).toBeInTheDocument()
    expect(screen.getByRole('heading', { level: 1, name: '原始 SKILL.md 正文' })).toBeInTheDocument()
    expect(screen.getByText('第一项')).toBeInTheDocument()
    expect(screen.queryByRole('heading', { level: 2, name: /allowed-tools/ })).toBeNull()
    expect(screen.getByText(/allowed-tools: Read, Write, Bash/)).toBeInTheDocument()
    expect(screen.getByText(/allowed-tools: Read, Write, Bash/).closest('code')).toBeInTheDocument()
    expect(screen.getByText('调试信息')).toBeInTheDocument()
    expect(screen.getByText('Read')).toBeInTheDocument()
  })
})

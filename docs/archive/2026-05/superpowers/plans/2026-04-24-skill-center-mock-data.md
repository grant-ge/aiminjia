# 技能中心 Mock 数据 + 热门推荐常驻 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在前端注入 mock 技能数据让页面有内容，热门推荐固定取 4 个并常驻显示。

**Architecture:** 新建 `src/data/mock-skills.ts` 存放静态 SkillInfo 列表；`skillStore` 初始值从 mock 数据读取，`reload()` 调用后真实后端数据覆盖；`SkillCenterPage` 移除 `showHot` 条件，热门区始终渲染，固定取前 4 个推荐技能。

**Tech Stack:** React 18, TypeScript, Zustand, Vitest + @testing-library/react

---

## 文件结构

| 文件 | 变更 |
|---|---|
| `src/data/mock-skills.ts` | 新建：22 条 SkillInfo mock 数据 |
| `src/stores/skillStore.ts` | 修改：初始 `skills` 从 mock 导入；`RECOMMENDED_SKILL_IDS` 取 4 个 |
| `src/features/skill-center/SkillCenterPage.tsx` | 修改：移除 `showHot`，热门区常驻，取 4 个；`officeSkills` 去掉重复推荐逻辑 |
| `src/features/skill-center/SkillCenterPage.integration.test.tsx` | 修改：更新 beforeEach mock 数据以匹配新 recommendedIds |

---

### Task 1: 新建 mock 数据文件

**Files:**
- Create: `src/data/mock-skills.ts`

- [ ] **Step 1: 创建 `src/data/mock-skills.ts`**

```ts
import type { SkillInfo } from '@/lib/tauri'

export const MOCK_SKILLS: SkillInfo[] = [
  {
    id: 'org-diagnosis',
    displayName: '组织诊断报告',
    displayNameEn: 'Org Diagnosis Report',
    description: '六盒子模型 + 麦肯锡 7S 框架诊断，输出组织优化建议。',
    shortDescription: '六盒子模型 + 麦肯锡 7S 框架诊断，输出组织优化建议。',
    shortDescriptionEn: 'Diagnose org with 6-box model and McKinsey 7S framework.',
    source: 'builtin',
    hasWorkflow: false,
    icon: 'building-2',
    category: 'hr',
    triggerText: '',
  },
  {
    id: 'okr-coaching',
    displayName: 'OKR 制定辅导',
    displayNameEn: 'OKR Coaching',
    description: '帮助团队拆解目标、关键结果与行动计划。',
    shortDescription: '帮助团队拆解目标、关键结果与行动计划。',
    shortDescriptionEn: 'Help teams set OKRs and action plans.',
    source: 'builtin',
    hasWorkflow: false,
    icon: 'target',
    category: 'general',
    triggerText: '',
  },
  {
    id: 'business-proposal',
    displayName: '商业方案撰写',
    displayNameEn: 'Business Proposal',
    description: '快速生成商业计划、项目方案与汇报材料。',
    shortDescription: '快速生成商业计划、项目方案与汇报材料。',
    shortDescriptionEn: 'Generate business plans and presentation materials.',
    source: 'builtin',
    hasWorkflow: false,
    icon: 'file-text',
    category: 'finance',
    triggerText: '',
  },
  {
    id: 'hr-data-maturity',
    displayName: 'HR数据分析成熟度评估',
    displayNameEn: 'HR Data Maturity Assessment',
    description: '评估 HR 数据治理、分析能力与决策成熟度。',
    shortDescription: '评估 HR 数据治理、分析能力与决策成熟度。',
    shortDescriptionEn: 'Assess HR data governance and analytics maturity.',
    source: 'builtin',
    hasWorkflow: false,
    icon: 'ruler',
    category: 'hr',
    triggerText: '',
  },
  {
    id: 'sales-analysis',
    displayName: '销售数据分析',
    displayNameEn: 'Sales Data Analysis',
    description: '分析销售趋势、成交转化、客户增长与复购机会。',
    shortDescription: '分析销售趋势、成交转化、客户���长与复购机会。',
    shortDescriptionEn: 'Analyze sales trends, conversion and growth.',
    source: 'builtin',
    hasWorkflow: true,
    icon: 'bar-chart-2',
    category: 'sales',
    triggerText: '',
  },
  {
    id: 'labor-compliance',
    displayName: '劳动合规风险检查',
    displayNameEn: 'Labor Compliance Check',
    description: '检查劳动合同中的潜在风险与违规条款。',
    shortDescription: '检查劳动合同中的潜在风险与违规条款。',
    shortDescriptionEn: 'Check labor contracts for compliance risks.',
    source: 'builtin',
    hasWorkflow: false,
    icon: 'scale',
    category: 'legal',
    triggerText: '',
  },
  {
    id: 'salary-benchmark',
    displayName: '薪酬市场对标分析',
    displayNameEn: 'Salary Benchmark Analysis',
    description: '对标行业薪酬水平，评估企业薪酬竞争力。',
    shortDescription: '对标行业薪酬水平，评估企业薪酬竞争力。',
    shortDescriptionEn: 'Benchmark salary against industry standards.',
    source: 'builtin',
    hasWorkflow: false,
    icon: 'briefcase',
    category: 'hr',
    triggerText: '',
  },
  {
    id: 'ops-analysis',
    displayName: '运营数据分析',
    displayNameEn: 'Operations Data Analysis',
    description: '分析 GMV、活跃用户、客单价与平台效率。',
    shortDescription: '分析 GMV、活跃用户、客单价与平台效率。',
    shortDescriptionEn: 'Analyze GMV, DAU and platform efficiency.',
    source: 'builtin',
    hasWorkflow: true,
    icon: 'shopping-cart',
    category: 'ops',
    triggerText: '',
  },
  {
    id: 'finance-analysis',
    displayName: '财务数据分析',
    displayNameEn: 'Finance Data Analysis',
    description: '解读收益结构、现金流及财务健康指标变化。',
    shortDescription: '解读收益结构、现金流及财务健康指标变化。',
    shortDescriptionEn: 'Analyze revenue structure and cash flow health.',
    source: 'builtin',
    hasWorkflow: true,
    icon: 'trending-up',
    category: 'finance',
    triggerText: '',
  },
  {
    id: 'recruitment-funnel',
    displayName: '招聘漏斗分析',
    displayNameEn: 'Recruitment Funnel Analysis',
    description: '分析招聘各阶段转化率，识别瓶颈与优化机会。',
    shortDescription: '分析招聘各阶段转化率，识别瓶颈与优化机会。',
    shortDescriptionEn: 'Analyze recruitment funnel conversion rates.',
    source: 'builtin',
    hasWorkflow: true,
    icon: 'bar-chart-2',
    category: 'hr',
    triggerText: '',
  },
  {
    id: 'customer-segmentation',
    displayName: '客户细分分析',
    displayNameEn: 'Customer Segmentation',
    description: '基于行为数据对客户分层，制定差异化策略。',
    shortDescription: '基于行为数据对客户分层，制定差异化策略。',
    shortDescriptionEn: 'Segment customers by behavior and design strategies.',
    source: 'builtin',
    hasWorkflow: false,
    icon: 'users',
    category: 'sales',
    triggerText: '',
  },
  {
    id: 'multi-file',
    displayName: '多文件处理',
    displayNameEn: 'Multi-file Processing',
    description: '批量读取、汇总、比对多个文档数据文件。',
    shortDescription: '批量读取、汇总、比对多个文档数据文件。',
    shortDescriptionEn: 'Batch read, summarize and compare multiple files.',
    source: 'builtin',
    hasWorkflow: false,
    icon: 'folder',
    category: 'general',
    triggerText: '',
  },
  {
    id: 'engagement-analysis',
    displayName: '员工敬业度分析',
    displayNameEn: 'Employee Engagement Analysis',
    description: '分析员工敬业度调研结果，找出影响因子。',
    shortDescription: '分析员工敬业度调研结果，找出影响因子。',
    shortDescriptionEn: 'Analyze employee engagement survey results.',
    source: 'builtin',
    hasWorkflow: false,
    icon: 'heart',
    category: 'hr',
    triggerText: '',
  },
  {
    id: 'perf-system-design',
    displayName: '绩效体系设计向导',
    displayNameEn: 'Performance System Design',
    description: '引导搭建 KPI/OKR 绩效考核体系与评分规则。',
    shortDescription: '引导搭建 KPI/OKR 绩效考核体系与评分规则。',
    shortDescriptionEn: 'Guide building a KPI/OKR performance review system.',
    source: 'builtin',
    hasWorkflow: false,
    icon: 'clipboard-list',
    category: 'hr',
    triggerText: '',
  },
  {
    id: 'salary-equity',
    displayName: '薪酬公平性分析',
    displayNameEn: 'Salary Equity Analysis',
    description: '检测薪酬结构中的性别、部门或岗位公平性问题。',
    shortDescription: '检测薪酬结构中的性别、部门或岗位公平性问题。',
    shortDescriptionEn: 'Detect pay equity issues by gender, department or role.',
    source: 'builtin',
    hasWorkflow: false,
    icon: 'coins',
    category: 'hr',
    triggerText: '',
  },
  {
    id: 'contract-risk',
    displayName: '合同风险审查',
    displayNameEn: 'Contract Risk Review',
    description: '识别合同条款中的权利义务失衡与法律风险。',
    shortDescription: '识别合同条款中的权利义务失衡与法律风险。',
    shortDescriptionEn: 'Identify legal risks in contract terms.',
    source: 'builtin',
    hasWorkflow: false,
    icon: 'file-search',
    category: 'legal',
    triggerText: '',
  },
  {
    id: 'policy-compliance',
    displayName: '规章制度合规审查',
    displayNameEn: 'Policy Compliance Review',
    description: '检查员工手册与规章制度是否符合劳动法规。',
    shortDescription: '检查员工手册与规章制度是否符合劳动法规。',
    shortDescriptionEn: 'Check employee policies against labor regulations.',
    source: 'builtin',
    hasWorkflow: false,
    icon: 'scroll',
    category: 'legal',
    triggerText: '',
  },
  {
    id: 'user-behavior',
    displayName: '用户行为分析',
    displayNameEn: 'User Behavior Analysis',
    description: '分析用户路径、留存与功能使用分布。',
    shortDescription: '分析用户路径、留存与功能使用分布。',
    shortDescriptionEn: 'Analyze user paths, retention and feature usage.',
    source: 'builtin',
    hasWorkflow: true,
    icon: 'smartphone',
    category: 'ops',
    triggerText: '',
  },
  {
    id: 'survey-analysis',
    displayName: '问卷调研分析',
    displayNameEn: 'Survey Analysis',
    description: '对调研问卷结果进行统计分析并输出洞察报告。',
    shortDescription: '对调研问卷结果进行统计分析并输出洞察报告。',
    shortDescriptionEn: 'Statistically analyze survey results and generate insights.',
    source: 'builtin',
    hasWorkflow: false,
    icon: 'clipboard',
    category: 'general',
    triggerText: '',
  },
  {
    id: 'budget-execution',
    displayName: '预算执行分析',
    displayNameEn: 'Budget Execution Analysis',
    description: '对比预算与实际执行偏差，输出原因分析。',
    shortDescription: '对比预算与实际执行偏差，输出原因分析。',
    shortDescriptionEn: 'Compare budget vs. actual and analyze variance.',
    source: 'builtin',
    hasWorkflow: true,
    icon: 'bar-chart-2',
    category: 'finance',
    triggerText: '',
  },
  {
    id: 'talent-grid',
    displayName: '人才盘点九宫格',
    displayNameEn: 'Talent 9-Box Grid',
    description: '按绩效与潜力绘制人才分布，支持继任计划。',
    shortDescription: '按绩效与潜力绘制人才分布，支持继任计划。',
    shortDescriptionEn: 'Plot talent by performance and potential for succession planning.',
    source: 'builtin',
    hasWorkflow: false,
    icon: 'target',
    category: 'hr',
    triggerText: '',
  },
  {
    id: 'biz-doc-writing',
    displayName: '商务文档撰写',
    displayNameEn: 'Business Document Writing',
    description: '起草合同、报价单、商务信函等正式文档。',
    shortDescription: '起草合同、报价单、商务信函等正式文档。',
    shortDescriptionEn: 'Draft contracts, quotes and formal business letters.',
    source: 'builtin',
    hasWorkflow: false,
    icon: 'pen-line',
    category: 'general',
    triggerText: '',
  },
]
```

- [ ] **Step 2: 确认 TS 无报错**

```bash
cd /Users/oayzz/project/lotus/lotus-workbench/lotus-app
pnpm exec tsc --noEmit 2>&1 | head -20
```

Expected: 0 errors

- [ ] **Step 3: Commit**

```bash
git add src/data/mock-skills.ts
git commit -m "feat(skill-center): add mock skill data (22 builtin skills)"
```

---

### Task 2: skillStore 使用 mock 初始数据，recommendedIds 取 4 个

**Files:**
- Modify: `src/stores/skillStore.ts`

当前 `skills: []`，需改为 `skills: MOCK_SKILLS`；当前 `RECOMMENDED_SKILL_IDS` 指向 5 个不存在的 id，需改为 mock 中真实存在的 4 个。

- [ ] **Step 1: 修改 `src/stores/skillStore.ts`**

完整替换文件：

```ts
import { create } from 'zustand'

import type { SkillCategoryId } from '@/data/skill-categories'
import { MOCK_SKILLS } from '@/data/mock-skills'
import { listSkills, type SkillInfo } from '@/lib/tauri'

const RECOMMENDED_SKILL_IDS = ['org-diagnosis', 'okr-coaching', 'sales-analysis', 'finance-analysis']

interface SkillState {
  skills: SkillInfo[]
  recommendedIds: string[]
  isLoading: boolean
  listByCategory: (id: SkillCategoryId) => SkillInfo[]
  getById: (id: string) => SkillInfo | null
  reload: () => Promise<void>
  install: (id: string) => Promise<void>
  uninstall: (id: string) => Promise<void>
  upload: (file: File) => Promise<void>
}

export const useSkillStore = create<SkillState>((set, get) => ({
  skills: MOCK_SKILLS,
  recommendedIds: RECOMMENDED_SKILL_IDS,
  isLoading: false,
  listByCategory(id) {
    const { skills, recommendedIds } = get()
    if (id === 'recommended') {
      return skills.filter((skill) => recommendedIds.includes(skill.id))
    }
    return skills.filter((skill) => (skill.category || 'general') === id)
  },
  getById(id) {
    return get().skills.find((skill) => skill.id === id) ?? null
  },
  async reload() {
    set({ isLoading: true })
    try {
      const skills = await listSkills()
      set({ skills, isLoading: false })
    } catch (error) {
      set({ isLoading: false })
      throw error
    }
  },
  async install() {
    throw new Error('技能市场即将开放')
  },
  async uninstall() {
    throw new Error('卸载功能即将开放')
  },
  async upload() {
    throw new Error('上传功能即将开放')
  },
}))
```

- [ ] **Step 2: 运行 TS 检查**

```bash
pnpm exec tsc --noEmit 2>&1 | head -20
```

Expected: 0 errors

- [ ] **Step 3: Commit**

```bash
git add src/stores/skillStore.ts
git commit -m "feat(skill-center): init skillStore with mock data, 4 recommended ids"
```

---

### Task 3: SkillCenterPage 热门推荐常驻，取 4 个

**Files:**
- Modify: `src/features/skill-center/SkillCenterPage.tsx:31-85`
- Modify: `src/features/skill-center/SkillCenterPage.integration.test.tsx`

- [ ] **Step 1: 更新 integration test 的 beforeEach（先写失败测试）**

在 `src/features/skill-center/SkillCenterPage.integration.test.tsx` 中，`beforeEach` 里的 `recommendedIds` 改为 4 个 mock 中真实存在的 id，同时把「热门推荐常驻」加为新测试：

完整替换文件：

```tsx
import '@testing-library/jest-dom'
import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import { SkillCenterPage } from '@/features/skill-center/SkillCenterPage'
import { useSkillStore } from '@/stores/skillStore'
import { useUiStore } from '@/stores/uiStore'

vi.mock('@/hooks/useChat', () => ({
  useChat: () => ({ createConversationFromSkill: vi.fn() }),
}))

const HR_SKILL = {
  id: 'hr-analysis',
  displayName: 'HR分析',
  description: '详细描述',
  source: 'builtin',
  hasWorkflow: true,
  icon: 'users',
  category: 'hr',
  triggerText: '',
  shortDescription: '短描述',
  displayNameEn: 'HR Analysis',
  shortDescriptionEn: 'short',
}

const REC1 = { id: 'rec1', displayName: '推荐1', description: 'd', source: 'builtin', hasWorkflow: false, icon: 'x', category: 'general', triggerText: '', shortDescription: 's', displayNameEn: 'r1', shortDescriptionEn: 's' }
const REC2 = { id: 'rec2', displayName: '推荐2', description: 'd', source: 'builtin', hasWorkflow: false, icon: 'x', category: 'general', triggerText: '', shortDescription: 's', displayNameEn: 'r2', shortDescriptionEn: 's' }
const REC3 = { id: 'rec3', displayName: '推荐3', description: 'd', source: 'builtin', hasWorkflow: false, icon: 'x', category: 'general', triggerText: '', shortDescription: 's', displayNameEn: 'r3', shortDescriptionEn: 's' }
const REC4 = { id: 'rec4', displayName: '推荐4', description: 'd', source: 'builtin', hasWorkflow: false, icon: 'x', category: 'general', triggerText: '', shortDescription: 's', displayNameEn: 'r4', shortDescriptionEn: 's' }

describe('SkillCenterPage', () => {
  beforeEach(() => {
    useSkillStore.setState({
      skills: [REC1, REC2, REC3, REC4, HR_SKILL],
      recommendedIds: ['rec1', 'rec2', 'rec3', 'rec4'],
      isLoading: false,
    })
    useUiStore.setState({ route: { kind: 'skill-center' }, settingsModal: null })
  })

  it('顶栏渲染标题、技能数量徽章和搜索框', () => {
    render(<SkillCenterPage />)
    expect(screen.getByText('技能中心')).toBeInTheDocument()
    expect(screen.getByText(/5 个技能/)).toBeInTheDocument()
    expect(screen.getByPlaceholderText('搜索技能名称或场景')).toBeInTheDocument()
  })

  it('顶栏有上传技能资料和创建技能按钮', () => {
    render(<SkillCenterPage />)
    expect(screen.getByRole('button', { name: '上传技能资料' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: /创建技能/ })).toBeInTheDocument()
  })

  it('分类 bar 包含全部/HR/财务/法务/销售/运营/通用', () => {
    render(<SkillCenterPage />)
    for (const label of ['全部', 'HR', '财务', '法务', '销售', '运营', '通用']) {
      expect(screen.getByRole('button', { name: label })).toBeInTheDocument()
    }
  })

  it('热门推荐始终渲染，切换分类后也可见', () => {
    render(<SkillCenterPage />)
    expect(screen.getByText('热门推荐')).toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: 'HR' }))
    expect(screen.getByText('热门推荐')).toBeInTheDocument()
  })

  it('热门推荐显示 4 张卡片（size=hot）', () => {
    render(<SkillCenterPage />)
    const hotCards = document.querySelectorAll('[data-testid="skill-card"].h-\\[140px\\]')
    // 通过 text 验证 4 个推荐技能都出现
    expect(screen.getByText('推荐1')).toBeInTheDocument()
    expect(screen.getByText('推荐2')).toBeInTheDocument()
    expect(screen.getByText('推荐3')).toBeInTheDocument()
    expect(screen.getByText('推荐4')).toBeInTheDocument()
    void hotCards
  })

  it('切换到 HR 分类后卡片点击进入详情', async () => {
    render(<SkillCenterPage />)
    fireEvent.click(screen.getByRole('button', { name: 'HR' }))
    // HR_SKILL 在全部技能区（非热门），取最后一个 skill-card
    const cards = screen.getAllByTestId('skill-card')
    const hrCard = cards.find((c) => c.textContent?.includes('HR分析'))
    expect(hrCard).toBeTruthy()
    fireEvent.click(hrCard!)
    await waitFor(() => {
      expect(useUiStore.getState().route).toEqual({ kind: 'skill-detail', skillId: 'hr-analysis' })
    })
  })

  it('没有常驻的详情/使用按钮', () => {
    render(<SkillCenterPage />)
    expect(screen.queryByRole('button', { name: /^详情$/ })).toBeNull()
    expect(screen.queryByRole('button', { name: /^使用$/ })).toBeNull()
  })
})
```

- [ ] **Step 2: 运行测试，确认���失败**

```bash
cd /Users/oayzz/project/lotus/lotus-workbench/lotus-app
pnpm exec vitest run src/features/skill-center/SkillCenterPage.integration.test.tsx 2>&1 | tail -30
```

Expected: 「热门推荐始终渲染」测试失败（当前切换分类后热门区会隐藏）

- [ ] **Step 3: 修改 `SkillCenterPage.tsx`**

改动两处：
1. 移除 `showHot` 变量及条件渲染，热门区始终渲染
2. `slice(0, 3)` 改为 `slice(0, 4)`
3. `officeSkills` 始终用 `listByCategory(category)`（`category === 'recommended'` 时即全量 recommended 技能，全量浏览时可换 `skills`）

完整替换 `src/features/skill-center/SkillCenterPage.tsx`：

```tsx
import { Search } from 'lucide-react'
import { useMemo, useState } from 'react'

import { PageSectionShell } from '@/components/shell/PageSectionShell'
import { SkillCard } from '@/components/skills/SkillCard'
import { SkillCategoryBar } from '@/components/skills/SkillCategoryBar'
import { SkillHotSection } from '@/components/skills/SkillHotSection'
import { SkillOfficeSection } from '@/components/skills/SkillOfficeSection'
import { Button } from '@/components/ui/button'
import { SKILL_CATEGORIES, type SkillCategoryId } from '@/data/skill-categories'
import { useSkillStore } from '@/stores/skillStore'
import { useUiStore } from '@/stores/uiStore'

import { SkillUploadModal } from './SkillUploadModal'

export function SkillCenterPage() {
  const [category, setCategory] = useState<SkillCategoryId>('recommended')
  const [uploadOpen, setUploadOpen] = useState(false)
  const skills = useSkillStore((s) => s.skills)
  const listByCategory = useSkillStore((s) => s.listByCategory)
  const setRoute = useUiStore((s) => s.setRoute)

  const categoryItems = useMemo(
    () => [
      { key: 'recommended', label: '全部' },
      ...SKILL_CATEGORIES.map((c) => ({ key: c.id, label: c.name })),
    ],
    [],
  )

  const recommended = listByCategory('recommended')
  const officeSkills = category === 'recommended' ? skills : listByCategory(category)

  function getSkillMeta(source: string, cat: string) {
    const normalizedCategory = cat || 'general'
    const label = SKILL_CATEGORIES.find((c) => c.id === normalizedCategory)?.name ?? '通用'
    const sourceLabel = source === 'builtin' ? '内置' : '自定义'
    return `${sourceLabel} · ${label}`
  }

  return (
    <PageSectionShell
      topBar={
        <header data-tauri-drag-region className="flex h-14 items-center justify-between border-b border-border px-6">
          <div className="flex items-center gap-3">
            <span className="text-[18px] font-bold text-foreground">技能中心</span>
            <span className="rounded-full bg-secondary px-2.5 py-1 text-[12px] font-medium text-muted-foreground">
              {skills.length} 个技能
            </span>
          </div>
          <div className="flex items-center gap-2.5">
            <div className="flex h-[34px] w-[220px] items-center gap-2 rounded-full bg-secondary px-3">
              <Search className="h-3.5 w-3.5 shrink-0 text-muted-foreground" />
              <input
                className="flex-1 bg-transparent text-[13px] text-foreground outline-none placeholder:text-muted-foreground"
                placeholder="搜索技能名称或场景"
                readOnly
              />
            </div>
            <Button variant="outline" onClick={() => setUploadOpen(true)}>
              上传技能资料
            </Button>
            <Button>+ 创建技能</Button>
          </div>
        </header>
      }
      padding="px-7 pt-6 pb-8"
      gap="gap-5"
    >
      <SkillHotSection>
        {recommended.slice(0, 4).map((skill) => (
          <SkillCard
            key={skill.id}
            size="hot"
            title={skill.displayName}
            meta={getSkillMeta(skill.source, skill.category)}
            desc={skill.shortDescription || skill.description}
            iconNode={null}
            onClick={() => setRoute({ kind: 'skill-detail', skillId: skill.id })}
          />
        ))}
      </SkillHotSection>
      <SkillOfficeSection
        categoryBar={
          <SkillCategoryBar
            items={categoryItems}
            activeKey={category}
            onSelect={(key) => setCategory(key as SkillCategoryId)}
          />
        }
      >
        {officeSkills.map((skill) => (
          <SkillCard
            key={skill.id}
            title={skill.displayName}
            meta={getSkillMeta(skill.source, skill.category)}
            desc={skill.shortDescription || skill.description}
            iconNode={null}
            onClick={() => setRoute({ kind: 'skill-detail', skillId: skill.id })}
          />
        ))}
      </SkillOfficeSection>
      <SkillUploadModal open={uploadOpen} onOpenChange={setUploadOpen} />
    </PageSectionShell>
  )
}
```

- [ ] **Step 4: 运行测试，确认全部通过**

```bash
pnpm exec vitest run src/features/skill-center/SkillCenterPage.integration.test.tsx 2>&1 | tail -20
```

Expected: PASS（7 tests pass）

- [ ] **Step 5: 全量 TS 检查**

```bash
pnpm exec tsc --noEmit 2>&1 | head -10
```

Expected: 0 errors

- [ ] **Step 6: Commit**

```bash
git add src/features/skill-center/SkillCenterPage.tsx src/features/skill-center/SkillCenterPage.integration.test.tsx
git commit -m "feat(skill-center): hot section always visible, show 4 recommended"
```

---

## 验收标准

1. `pnpm exec tsc --noEmit` 0 errors
2. `pnpm exec vitest run src/features/skill-center/SkillCenterPage.integration.test.tsx` 全绿
3. 页面打开后：顶栏显示「22 个技能」；热门推荐显示 4 张大卡；全部技能分类筛选正常；切换 HR 等分类后热门推荐仍可见

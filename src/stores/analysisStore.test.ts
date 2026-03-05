import { describe, it, expect, beforeEach } from 'vitest'
import { useAnalysisStore } from './analysisStore'
import type {
  FieldMapping,
  JobFamily,
  LevelFramework,
  DiagnosisResults,
  ActionPlan,
} from '@/types/analysis'

// Reset store between tests
beforeEach(() => {
  useAnalysisStore.getState().reset()
})

// ---------------------------------------------------------------------------
// Initial state
// ---------------------------------------------------------------------------

describe('analysisStore — initial state', () => {
  it('starts at step 0 with empty data', () => {
    const state = useAnalysisStore.getState()
    expect(state.currentStep).toBe(0)
    expect(state.stepStatus).toEqual({})
    expect(state.fieldMapping).toBeNull()
    expect(state.jobFamilies).toEqual([])
    expect(state.levelFramework).toBeNull()
    expect(state.diagnosisResults).toBeNull()
    expect(state.actionPlan).toBeNull()
  })
})

// ---------------------------------------------------------------------------
// Step progression
// ---------------------------------------------------------------------------

describe('analysisStore — step management', () => {
  it('sets current step', () => {
    useAnalysisStore.getState().setCurrentStep(3)
    expect(useAnalysisStore.getState().currentStep).toBe(3)
  })

  it('sets step status', () => {
    useAnalysisStore.getState().setStepStatus(1, 'confirmed')
    useAnalysisStore.getState().setStepStatus(2, 'in_progress')

    const { stepStatus } = useAnalysisStore.getState()
    expect(stepStatus[1]).toBe('confirmed')
    expect(stepStatus[2]).toBe('in_progress')
  })

  it('overwrites step status', () => {
    useAnalysisStore.getState().setStepStatus(1, 'pending')
    useAnalysisStore.getState().setStepStatus(1, 'confirmed')
    expect(useAnalysisStore.getState().stepStatus[1]).toBe('confirmed')
  })
})

// ---------------------------------------------------------------------------
// Step data setters
// ---------------------------------------------------------------------------

describe('analysisStore — data setters', () => {
  it('sets field mapping', () => {
    const mapping: FieldMapping = {
      salaryFields: ['基本工资', '绩效工资'],
      idField: '工号',
      nameField: '姓名',
      departmentField: '部门',
      positionField: '岗位',
      hireDateField: '入职日期',
      totalRows: 500,
      validRows: 480,
      excludedRows: 20,
      excludeReasons: [{ reason: '缺少工号', count: 20 }],
      salaryStructure: [
        { pattern: '基本+绩效', ratio: '70:30', coverage: 85, count: 408 },
      ],
    }
    useAnalysisStore.getState().setFieldMapping(mapping)
    expect(useAnalysisStore.getState().fieldMapping).toEqual(mapping)
  })

  it('sets job families', () => {
    const families: JobFamily[] = [
      {
        id: 'T',
        code: 'TECH',
        name: '技术族',
        positions: [
          { originalTitle: '前端工程师', standardTitle: '软件工程师', jobFamilyId: 'T', count: 30 },
        ],
        count: 100,
      },
      {
        id: 'S',
        code: 'SALES',
        name: '销售族',
        positions: [
          { originalTitle: '销售经理', standardTitle: '销售经理', jobFamilyId: 'S', count: 20 },
        ],
        count: 50,
      },
    ]
    useAnalysisStore.getState().setJobFamilies(families)
    expect(useAnalysisStore.getState().jobFamilies).toHaveLength(2)
    expect(useAnalysisStore.getState().jobFamilies[0].name).toBe('技术族')
  })

  it('sets level framework', () => {
    const framework: LevelFramework = {
      sequences: [
        {
          code: 'P',
          name: '专业序列',
          levels: [
            { code: 'P1', name: '初级', count: 50 },
            { code: 'P2', name: '中级', count: 30 },
          ],
        },
        {
          code: 'M',
          name: '管理序列',
          levels: [
            { code: 'M1', name: '主管', count: 20 },
            { code: 'M2', name: '经理', count: 10 },
          ],
        },
      ],
      totalLevels: 4,
      assignmentComplete: true,
    }
    useAnalysisStore.getState().setLevelFramework(framework)
    expect(useAnalysisStore.getState().levelFramework).toEqual(framework)
  })

  it('sets diagnosis results', () => {
    const results: DiagnosisResults = {
      giniCoefficient: 0.32,
      rSquared: 0.78,
      complianceRate: 0.85,
      inversionRate: 0.03,
      anomalies: [
        { priority: 'high', title: '薪酬倒挂', description: '3人薪酬低于下级', count: 3 },
      ],
      rootCauses: [
        { count: 5, label: '历史遗留', detail: '入职时薪酬未校准', action: '逐步调整' },
      ],
    }
    useAnalysisStore.getState().setDiagnosisResults(results)
    expect(useAnalysisStore.getState().diagnosisResults).toEqual(results)
  })

  it('sets action plan', () => {
    const plan: ActionPlan = {
      scenarios: [
        {
          id: 's1',
          name: '保守方案',
          tag: '低成本',
          tagVariant: 'green',
          affectedCount: 10,
          annualCost: 50000,
        },
      ],
      execSummary: {
        highRiskCount: 3,
        highRiskDetail: '3人薪酬严重低于市场',
        recommendedInvestment: 150000,
        recommendedCoverage: 0.92,
        complianceBefore: 0.85,
        complianceAfter: 0.95,
      },
    }
    useAnalysisStore.getState().setActionPlan(plan)
    expect(useAnalysisStore.getState().actionPlan).toEqual(plan)
  })
})

// ---------------------------------------------------------------------------
// Reset
// ---------------------------------------------------------------------------

describe('analysisStore — reset', () => {
  it('resets all state to initial values', () => {
    const family: JobFamily = {
      id: 'T',
      code: 'TECH',
      name: 'Tech',
      positions: [],
      count: 1,
    }

    // Set some data first
    useAnalysisStore.getState().setCurrentStep(4)
    useAnalysisStore.getState().setStepStatus(1, 'confirmed')
    useAnalysisStore.getState().setJobFamilies([family])

    // Reset
    useAnalysisStore.getState().reset()

    const state = useAnalysisStore.getState()
    expect(state.currentStep).toBe(0)
    expect(state.stepStatus).toEqual({})
    expect(state.jobFamilies).toEqual([])
  })
})

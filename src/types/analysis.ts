/**
 * Analysis flow types.
 * Based on tech-architecture.md §3.2
 */

export type StepStatus = 'pending' | 'in_progress' | 'confirmed'

export interface AnalysisState {
  currentStep: number // 0-5, 0 = not started
  stepStatus: Record<number, StepStatus>
}

// --- Step 1: Field Mapping ---

export interface FieldMapping {
  salaryFields: string[]
  idField: string
  nameField: string
  departmentField: string
  positionField: string
  hireDateField: string
  totalRows: number
  validRows: number
  excludedRows: number
  excludeReasons: ExcludeReason[]
  salaryStructure: SalaryStructurePattern[]
}

export interface ExcludeReason {
  reason: string
  count: number
}

export interface SalaryStructurePattern {
  pattern: string
  ratio: string
  coverage: number
  count: number
}

// --- Step 2: Job Family ---

export interface JobFamily {
  id: string
  code: string
  name: string
  positions: PositionMapping[]
  count: number
}

export interface PositionMapping {
  originalTitle: string
  standardTitle: string
  jobFamilyId: string
  count: number
}

// --- Step 3: Level Framework ---

export interface LevelFramework {
  sequences: LevelSequence[]
  totalLevels: number
  assignmentComplete: boolean
}

export interface LevelSequence {
  code: string
  name: string
  levels: LevelDefinition[]
}

export interface LevelDefinition {
  code: string
  name: string
  minYears?: number
  maxYears?: number
  count: number
}

// --- Step 4: Diagnosis ---

export interface DiagnosisResults {
  giniCoefficient: number
  rSquared: number
  complianceRate: number
  inversionRate: number
  anomalies: AnomalyCategory[]
  rootCauses: RootCause[]
}

export interface AnomalyCategory {
  priority: 'high' | 'medium' | 'low'
  title: string
  description: string
  count: number
}

export interface RootCause {
  count: number
  label: string
  detail: string
  action: string
}

// --- Step 5: Action Plan ---

export interface ActionPlan {
  scenarios: AdjustmentScenario[]
  selectedScenarioId?: string
  execSummary: ExecSummaryData
}

export interface AdjustmentScenario {
  id: string
  name: string
  tag: string
  tagVariant: 'red' | 'accent' | 'green'
  affectedCount: number
  annualCost: number
}

export interface ExecSummaryData {
  highRiskCount: number
  highRiskDetail: string
  recommendedInvestment: number
  recommendedCoverage: number
  complianceBefore: number
  complianceAfter: number
}

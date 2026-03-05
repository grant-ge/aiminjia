/**
 * Analysis store — tracks the 5-step analysis flow.
 * Based on tech-architecture.md §3.2
 */
import { create } from 'zustand'
import type { StepStatus } from '@/types/analysis'
import type {
  FieldMapping,
  JobFamily,
  LevelFramework,
  DiagnosisResults,
  ActionPlan,
} from '@/types/analysis'

interface AnalysisState {
  currentStep: number // 0-5, 0 = not started
  stepStatus: Record<number, StepStatus>

  // Step data
  fieldMapping: FieldMapping | null
  jobFamilies: JobFamily[]
  levelFramework: LevelFramework | null
  diagnosisResults: DiagnosisResults | null
  actionPlan: ActionPlan | null

  // Actions
  setCurrentStep: (step: number) => void
  setStepStatus: (step: number, status: StepStatus) => void
  setFieldMapping: (data: FieldMapping) => void
  setJobFamilies: (data: JobFamily[]) => void
  setLevelFramework: (data: LevelFramework) => void
  setDiagnosisResults: (data: DiagnosisResults) => void
  setActionPlan: (data: ActionPlan) => void
  reset: () => void
}

const initialState = {
  currentStep: 0,
  stepStatus: {} as Record<number, StepStatus>,
  fieldMapping: null,
  jobFamilies: [],
  levelFramework: null,
  diagnosisResults: null,
  actionPlan: null,
}

export const useAnalysisStore = create<AnalysisState>((set) => ({
  ...initialState,

  setCurrentStep: (currentStep) => set({ currentStep }),

  setStepStatus: (step, status) =>
    set((state) => ({
      stepStatus: { ...state.stepStatus, [step]: status },
    })),

  setFieldMapping: (fieldMapping) => set({ fieldMapping }),

  setJobFamilies: (jobFamilies) => set({ jobFamilies }),

  setLevelFramework: (levelFramework) => set({ levelFramework }),

  setDiagnosisResults: (diagnosisResults) => set({ diagnosisResults }),

  setActionPlan: (actionPlan) => set({ actionPlan }),

  reset: () => set(initialState),
}))

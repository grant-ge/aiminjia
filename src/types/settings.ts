/**
 * Settings types.
 * Based on tech-architecture.md §3.2
 */

import type { AppLanguage } from '@/i18n'

export type LlmProvider = 'deepseek-v3' | 'qwen-plus' | 'volcano' | 'openai' | 'claude' | 'custom'
export type DataMaskingLevel = 'strict' | 'standard' | 'relaxed'

export interface Settings {
  primaryModel: LlmProvider
  primaryApiKey: string
  autoModelRouting: boolean
  workspacePath: string
  analysisThreshold: number
  dataMaskingLevel: DataMaskingLevel
  autoCleanupEnabled: boolean
  tempFileRetentionDays: number
  keepOldVersions: number
  tavilyApiKey: string
  bochaApiKey: string
  customModelEndpoint: string
  customModelName: string
  cloudModel: string
  cloudModelType: string
  personaOnboardingDone?: boolean
  appLanguage?: AppLanguage
}

export const DEFAULT_SETTINGS: Settings = {
  primaryModel: 'deepseek-v3',
  primaryApiKey: '',
  autoModelRouting: true,
  workspacePath: '',
  analysisThreshold: 1.65,
  dataMaskingLevel: 'strict',
  autoCleanupEnabled: true,
  tempFileRetentionDays: 7,
  keepOldVersions: 1,
  tavilyApiKey: '',
  bochaApiKey: '',
  customModelEndpoint: '',
  customModelName: '',
  cloudModel: '',
  cloudModelType: '',
  personaOnboardingDone: false,
  appLanguage: 'zh-CN',
}

export const LLM_PROVIDER_LABELS: Record<LlmProvider, string> = {
  'deepseek-v3': 'DeepSeek',
  'qwen-plus': 'Qwen Plus',
  'volcano': 'Volcano Engine',
  'openai': 'GPT-4o',
  'claude': 'Claude',
  'custom': 'Custom Model',
}

export const PROVIDER_CAPABILITIES: Record<LlmProvider, { modelsDesc: string; hasReasoning: boolean }> = {
  'deepseek-v3': { modelsDesc: 'Default: deepseek-chat | Reasoning: deepseek-reasoner', hasReasoning: true },
  'qwen-plus': { modelsDesc: 'Default: qwen-plus', hasReasoning: false },
  'openai': { modelsDesc: 'Default: GPT-4o', hasReasoning: false },
  'claude': { modelsDesc: 'Default: Claude Sonnet', hasReasoning: false },
  'volcano': { modelsDesc: 'Default: ByteDance LLM', hasReasoning: false },
  'custom': { modelsDesc: 'Custom OpenAI-compatible model', hasReasoning: false },
}

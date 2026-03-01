/**
 * Settings types.
 * Based on tech-architecture.md §3.2
 */

export type LlmProvider = 'deepseek-v3' | 'qwen-plus' | 'volcano' | 'openai' | 'claude'
export type DataMaskingLevel = 'strict' | 'standard' | 'relaxed'

export interface Settings {
  // Model config
  primaryModel: LlmProvider
  primaryApiKey: string
  autoModelRouting: boolean

  // Workspace
  workspacePath: string

  // Analysis parameters
  analysisThreshold: number // default 1.65
  dataMaskingLevel: DataMaskingLevel

  // File management
  autoCleanupEnabled: boolean
  tempFileRetentionDays: number // default 7
  keepOldVersions: number // default 1
  tavilyApiKey: string
}

export const DEFAULT_SETTINGS: Settings = {
  primaryModel: 'deepseek-v3',
  primaryApiKey: '',
  autoModelRouting: true,
  workspacePath: '',  // resolved at runtime by backend
  analysisThreshold: 1.65,
  dataMaskingLevel: 'strict',
  autoCleanupEnabled: true,
  tempFileRetentionDays: 7,
  keepOldVersions: 1,
  tavilyApiKey: '',
}

export const LLM_PROVIDER_LABELS: Record<LlmProvider, string> = {
  'deepseek-v3': 'DeepSeek',
  'qwen-plus': '通义千问',
  'volcano': '火山引擎',
  'openai': 'GPT-4o',
  'claude': 'Claude',
}

/** Provider model capabilities — mirrors router::get_provider_capabilities in Rust */
export const PROVIDER_CAPABILITIES: Record<LlmProvider, { modelsDesc: string; hasReasoning: boolean }> = {
  'deepseek-v3': { modelsDesc: '默认: deepseek-chat | 推理: deepseek-reasoner', hasReasoning: true },
  'qwen-plus': { modelsDesc: '默认: qwen-plus', hasReasoning: false },
  'openai': { modelsDesc: '默认: GPT-4o', hasReasoning: false },
  'claude': { modelsDesc: '默认: Claude Sonnet', hasReasoning: false },
  'volcano': { modelsDesc: '默认: 字节跳动大模型', hasReasoning: false },
}

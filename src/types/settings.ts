/**
 * Settings types.
 * Based on tech-architecture.md §3.2
 */

import type { AppLanguage } from "@/i18n";

export type LlmProvider =
  | "deepseek-v3"
  | "qwen-plus"
  | "volcano"
  | "openai"
  | "custom";
export type FontScale = "small" | "medium" | "large";
export type ChatWidthMode = "centered" | "full";
export type DefaultPermissionMode = "default" | "fullAccess";
export type CloudGatewayMode = "legacy" | "v2";
export type AppLogLevel = "error" | "warn" | "info" | "debug";
export type ProfileAvatarMode = "initial" | "emoji" | "image";

export interface Settings {
  primaryModel: LlmProvider;
  primaryApiKey: string;
  autoModelRouting: boolean;
  workspacePath: string;
  analysisThreshold: number;
  autoCleanupEnabled: boolean;
  tempFileRetentionDays: number;
  keepOldVersions: number;
  customModelEndpoint: string;
  customModelName: string;
  cloudModel: string;
  cloudModelType: string;
  cloudGatewayMode: CloudGatewayMode;
  personaOnboardingDone?: boolean;
  appLanguage?: AppLanguage;
  fontScale?: FontScale;
  chatWidthMode?: ChatWidthMode;
  profileAvatarMode?: ProfileAvatarMode;
  profileAvatarEmoji?: string;
  profileAvatarImagePath?: string;
  defaultPermissionMode?: DefaultPermissionMode;
  accentColor?: string;
  uiHomeSelectedWorkspace?: string;
  uiHomeRecentWorkspaces?: string;
  uiSidebarCollapsedProjects?: string;
  uiSidebarConversationStatuses?: string;
  contextWindow?: number | null;
}

export const DEFAULT_SETTINGS: Settings = {
  primaryModel: "deepseek-v3",
  primaryApiKey: "",
  autoModelRouting: true,
  workspacePath: "",
  analysisThreshold: 1.65,
  autoCleanupEnabled: true,
  tempFileRetentionDays: 7,
  keepOldVersions: 1,
  customModelEndpoint: "",
  customModelName: "",
  cloudModel: "",
  cloudModelType: "",
  cloudGatewayMode: "v2",
  personaOnboardingDone: false,
  appLanguage: "zh-CN",
  fontScale: "medium",
  chatWidthMode: "full",
  profileAvatarMode: "initial",
  profileAvatarEmoji: "",
  profileAvatarImagePath: "",
  defaultPermissionMode: "default",
  accentColor: "",
  uiHomeSelectedWorkspace: "",
  uiHomeRecentWorkspaces: "",
  uiSidebarCollapsedProjects: "",
  uiSidebarConversationStatuses: "",
};

export const LLM_PROVIDER_LABELS: Record<LlmProvider, string> = {
  "deepseek-v3": "DeepSeek",
  "qwen-plus": "Qwen Plus",
  volcano: "Volcano Engine",
  openai: "GPT-4o",
  custom: "Custom Model",
};

export const PROVIDER_CAPABILITIES: Record<
  LlmProvider,
  { modelsDesc: string; hasReasoning: boolean }
> = {
  "deepseek-v3": {
    modelsDesc: "Default: deepseek-chat | Reasoning: deepseek-reasoner",
    hasReasoning: true,
  },
  "qwen-plus": { modelsDesc: "Default: qwen-plus", hasReasoning: false },
  openai: { modelsDesc: "Default: GPT-4o", hasReasoning: false },
  volcano: { modelsDesc: "Default: ByteDance LLM", hasReasoning: false },
  custom: { modelsDesc: "Custom OpenAI-compatible model", hasReasoning: false },
};

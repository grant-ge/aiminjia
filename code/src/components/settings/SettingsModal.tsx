/**
 * SettingsModal — tabbed settings: model configuration (per-provider) + general settings.
 */
import { useEffect, useState } from 'react'
import { Modal } from '@/components/common/Modal'
import { Button } from '@/components/common/Button'
import { useSettingsStore } from '@/stores/settingsStore'
import { useNotificationStore } from '@/stores/notificationStore'
import {
  getSettings,
  updateSettings,
  validateApiKey,
  selectWorkspace,
  getAllProviderKeys,
  updateAllProviderKeys,
  getConfiguredProviders,
  switchProvider,
  openLogsDirectory,
  openWorkspaceDirectory,
} from '@/lib/tauri'
import type { LlmProvider } from '@/types/settings'
import { PROVIDER_CAPABILITIES, LLM_PROVIDER_LABELS } from '@/types/settings'

interface SettingsModalProps {
  open: boolean
  onClose: () => void
}

type MainTab = 'models' | 'search' | 'general'

const PROVIDER_LIST: { value: LlmProvider; label: string }[] = [
  { value: 'deepseek-v3', label: 'DeepSeek' },
  { value: 'qwen-plus', label: '通义千问' },
  { value: 'volcano', label: '火山引擎' },
  { value: 'openai', label: 'OpenAI' },
  { value: 'claude', label: 'Claude' },
  { value: 'custom', label: '自定义模型' },
]

const API_KEY_PLACEHOLDERS: Record<LlmProvider, string> = {
  'deepseek-v3': 'sk-...',
  'qwen-plus': 'sk-...',
  'volcano': 'API Key...',
  'openai': 'sk-...',
  'claude': 'sk-ant-...',
  'custom': 'API Key（可选，本地模型留空）',
}

export function SettingsModal({ open, onClose }: SettingsModalProps) {
  const settings = useSettingsStore()
  const notifications = useNotificationStore()

  const [mainTab, setMainTab] = useState<MainTab>('models')
  const [activeProvider, setActiveProvider] = useState<LlmProvider>('deepseek-v3')
  const [saving, setSaving] = useState(false)

  // Per-provider key cache: provider → plaintext key
  const [keyCache, setKeyCache] = useState<Partial<Record<LlmProvider, string>>>({})
  // Per-provider validation state
  const [validating, setValidating] = useState(false)
  const [keyValid, setKeyValid] = useState<Record<string, boolean | null>>({})
  // Show/hide toggles
  const [showApiKey, setShowApiKey] = useState(false)
  const [showTavilyKey, setShowTavilyKey] = useState(false)
  const [showBochaKey, setShowBochaKey] = useState(false)

  // Load settings + all provider keys when modal opens
  useEffect(() => {
    if (!open) return
    setShowApiKey(false)
    setShowTavilyKey(false)
    setKeyValid({})
    ;(async () => {
      try {
        const [saved, allKeys] = await Promise.all([getSettings(), getAllProviderKeys()])
        settings.setSettings(saved)
        setActiveProvider(saved.primaryModel)

        // Build key cache from all provider keys
        const cache: Partial<Record<LlmProvider, string>> = {}
        for (const [provider, key] of Object.entries(allKeys)) {
          cache[provider as LlmProvider] = key
        }
        // Ensure current provider key is in cache (migration compat)
        if (saved.primaryApiKey && !cache[saved.primaryModel]) {
          cache[saved.primaryModel] = saved.primaryApiKey
        }
        setKeyCache(cache)
      } catch (err) {
        console.error('Failed to load settings:', err)
      }
    })()
  }, [open])

  const handleSave = async () => {
    setSaving(true)
    try {
      // Build the final key cache with the current provider's key
      const finalKeyCache = { ...keyCache }

      // Batch-save all provider keys
      const keysToSave: Record<string, string> = {}
      for (const p of PROVIDER_LIST) {
        const key = finalKeyCache[p.value] ?? ''
        keysToSave[p.value] = key
      }
      await updateAllProviderKeys(keysToSave)

      // Save general settings — use the active provider's key as primaryApiKey
      const currentProviderKey = finalKeyCache[settings.primaryModel] ?? ''
      await updateSettings({
        primaryModel: settings.primaryModel,
        primaryApiKey: currentProviderKey,
        autoModelRouting: settings.autoModelRouting,
        workspacePath: settings.workspacePath,
        analysisThreshold: settings.analysisThreshold,
        dataMaskingLevel: settings.dataMaskingLevel,
        autoCleanupEnabled: settings.autoCleanupEnabled,
        tempFileRetentionDays: settings.tempFileRetentionDays,
        keepOldVersions: settings.keepOldVersions,
        tavilyApiKey: settings.tavilyApiKey,
        bochaApiKey: settings.bochaApiKey,
        customModelEndpoint: settings.customModelEndpoint,
        customModelName: settings.customModelName,
      })

      // Refresh configured providers list
      const providers = await getConfiguredProviders()
      useSettingsStore.getState().setConfiguredProviders(providers as LlmProvider[])

      onClose()
    } catch (err) {
      console.error('Failed to save settings:', err)
      notifications.push({
        level: 'error',
        title: '保存失败',
        message: err instanceof Error ? err.message : '保存设置时发生未知错误',
        actions: [],
        dismissible: true,
        autoHide: 6,
        context: 'toast',
      })
    } finally {
      setSaving(false)
    }
  }

  const handleSetAsPrimary = async (provider: LlmProvider) => {
    // Update local state
    settings.setPrimaryModel(provider)
    const cachedKey = keyCache[provider] ?? ''
    settings.setPrimaryApiKey(cachedKey)

    // Persist immediately — save the key first, then switch provider
    try {
      const keysToSave: Record<string, string> = {}
      keysToSave[provider] = cachedKey
      await updateAllProviderKeys(keysToSave)
      await switchProvider(provider)

      // For custom provider, also persist endpoint/model settings
      if (provider === 'custom') {
        await updateSettings({
          primaryModel: provider,
          primaryApiKey: cachedKey,
          autoModelRouting: settings.autoModelRouting,
          workspacePath: settings.workspacePath,
          analysisThreshold: settings.analysisThreshold,
          dataMaskingLevel: settings.dataMaskingLevel,
          autoCleanupEnabled: settings.autoCleanupEnabled,
          tempFileRetentionDays: settings.tempFileRetentionDays,
          keepOldVersions: settings.keepOldVersions,
          tavilyApiKey: settings.tavilyApiKey,
          bochaApiKey: settings.bochaApiKey,
          customModelEndpoint: settings.customModelEndpoint,
          customModelName: settings.customModelName,
        })
      }

      notifications.push({
        level: 'success',
        title: '已切换默认模型',
        message: `已设为 ${LLM_PROVIDER_LABELS[provider] ?? provider}`,
        actions: [],
        dismissible: true,
        autoHide: 3,
        context: 'toast',
      })
    } catch (err) {
      console.error('Failed to switch provider:', err)
      notifications.push({
        level: 'error',
        title: '切换失败',
        message: err instanceof Error ? err.message : '切换默认模型时发生错误',
        actions: [],
        dismissible: true,
        autoHide: 6,
        context: 'toast',
      })
    }
  }

  const currentKeyForProvider = keyCache[activeProvider] ?? ''
  const providerCaps = PROVIDER_CAPABILITIES[activeProvider]

  const footer = (
    <>
      <Button variant="secondary" onClick={onClose}>
        取消
      </Button>
      <Button variant="primary" onClick={handleSave} disabled={saving}>
        {saving ? '保存中...' : '保存设置'}
      </Button>
    </>
  )

  return (
    <Modal open={open} onClose={onClose} title="设置" footer={footer}>
      {/* Main Tab Bar */}
      <div
        className="mb-4 flex items-center gap-1 border-b pb-3"
        style={{ borderColor: 'var(--color-border)' }}
      >
        <TabButton
          active={mainTab === 'models'}
          onClick={() => setMainTab('models')}
        >
          模型配置
        </TabButton>
        <TabButton
          active={mainTab === 'search'}
          onClick={() => setMainTab('search')}
        >
          搜索配置
        </TabButton>
        <TabButton
          active={mainTab === 'general'}
          onClick={() => setMainTab('general')}
        >
          通用设置
        </TabButton>
      </div>

      {/* Tab Content */}
      {mainTab === 'models' && (
        <div>
          {/* Provider Sub-tabs */}
          <div className="mb-4 flex flex-wrap items-center gap-1">
            {PROVIDER_LIST.map((p) => (
              <SubTabButton
                key={p.value}
                active={activeProvider === p.value}
                onClick={() => {
                  setActiveProvider(p.value)
                  setShowApiKey(false)
                  setKeyValid((prev) => ({ ...prev, [p.value]: null }))
                }}
              >
                {p.label}
              </SubTabButton>
            ))}
          </div>

          {/* Active badge */}
          {activeProvider === settings.primaryModel && (
            <div
              className="mb-3 flex items-center gap-1.5 rounded-md px-2.5 py-1.5 text-xs"
              style={{
                background: 'var(--color-primary-subtle)',
                color: 'var(--color-primary)',
              }}
            >
              <span
                className="h-1.5 w-1.5 rounded-full"
                style={{ background: 'var(--color-semantic-green)' }}
              />
              当前默认模型
            </div>
          )}

          {/* API Key Input */}
          <FormGroup
            label="API Key"
            desc={`请输入 ${LLM_PROVIDER_LABELS[activeProvider]} 的 API Key`}
          >
            <div className="relative">
              <input
                type={showApiKey ? 'text' : 'password'}
                className="h-9 w-full rounded-md border px-3 py-2 pr-16 text-base outline-none"
                style={{
                  background: 'var(--color-bg-main)',
                  borderColor: 'var(--color-border)',
                  color: 'var(--color-text-primary)',
                }}
                placeholder={API_KEY_PLACEHOLDERS[activeProvider] ?? 'sk-...'}
                value={currentKeyForProvider}
                onChange={(e) => {
                  setKeyCache((prev) => ({ ...prev, [activeProvider]: e.target.value }))
                  setKeyValid((prev) => ({ ...prev, [activeProvider]: null }))
                }}
              />
              <button
                type="button"
                className="absolute right-2 top-1/2 -translate-y-1/2 rounded px-2 py-0.5 text-xs"
                style={{ color: 'var(--color-text-muted)' }}
                onClick={() => setShowApiKey(!showApiKey)}
              >
                {showApiKey ? '隐藏' : '显示'}
              </button>
            </div>
          </FormGroup>

          {/* Validate + Set as Primary */}
          <div className="-mt-3 mb-3 flex items-center gap-2">
            <Button
              variant="secondary"
              onClick={async () => {
                setValidating(true)
                setKeyValid((prev) => ({ ...prev, [activeProvider]: null }))
                try {
                  const valid = await validateApiKey(activeProvider, currentKeyForProvider)
                  setKeyValid((prev) => ({ ...prev, [activeProvider]: valid }))
                } catch {
                  setKeyValid((prev) => ({ ...prev, [activeProvider]: false }))
                }
                setValidating(false)
              }}
              disabled={(activeProvider !== 'custom' && !currentKeyForProvider) || validating}
            >
              {validating ? '验证中...' : activeProvider === 'custom' ? '测试连接' : '验证 Key'}
            </Button>

            {activeProvider !== settings.primaryModel && (
              <Button
                variant="secondary"
                onClick={() => handleSetAsPrimary(activeProvider)}
              >
                设为默认模型
              </Button>
            )}

            {keyValid[activeProvider] === true && (
              <span className="text-sm" style={{ color: 'var(--color-semantic-green)' }}>
                Key 有效
              </span>
            )}
            {keyValid[activeProvider] === false && (
              <span className="text-sm" style={{ color: 'var(--color-semantic-red)' }}>
                Key 无效或验证失败
              </span>
            )}
          </div>

          {/* Custom model config — only for 'custom' provider */}
          {activeProvider === 'custom' && (
            <>
              <FormGroup
                label="API Endpoint"
                desc="OpenAI 兼容的 API 地址（必填，填到 /v1 即可，系统自动拼接 /chat/completions）"
              >
                <FormInput
                  value={settings.customModelEndpoint}
                  placeholder="http://localhost:11434/v1"
                  onChange={(v) => settings.setCustomModelEndpoint(v)}
                />
              </FormGroup>
              <FormGroup
                label="模型名称"
                desc="要使用的模型 ID（必填）"
              >
                <FormInput
                  value={settings.customModelName}
                  placeholder="qwen2.5:7b"
                  onChange={(v) => settings.setCustomModelName(v)}
                />
              </FormGroup>

              {/* Common service examples */}
              <div
                className="rounded-md border px-3 py-2.5 text-xs"
                style={{
                  background: 'var(--color-bg-main)',
                  borderColor: 'var(--color-border)',
                  color: 'var(--color-text-muted)',
                }}
              >
                <div
                  className="mb-2 text-xs font-semibold"
                  style={{ color: 'var(--color-text-secondary)' }}
                >
                  常见服务配置示例
                </div>
                <div className="flex flex-col gap-1.5">
                  <div>
                    <span style={{ color: 'var(--color-text-primary)' }}>Ollama（本地）</span>
                    ：Endpoint <code style={{ color: 'var(--color-semantic-blue)' }}>http://localhost:11434/v1</code>
                    ，模型如 <code>qwen2.5:7b</code>
                    ，Key 留空
                  </div>
                  <div>
                    <span style={{ color: 'var(--color-text-primary)' }}>LM Studio（本地）</span>
                    ：Endpoint <code style={{ color: 'var(--color-semantic-blue)' }}>http://localhost:1234/v1</code>
                    ，模型如 <code>llama3</code>
                    ，Key 留空
                  </div>
                  <div>
                    <span style={{ color: 'var(--color-text-primary)' }}>OpenRouter</span>
                    ：Endpoint <code style={{ color: 'var(--color-semantic-blue)' }}>https://openrouter.ai/api/v1</code>
                    ，模型如 <code>google/gemini-pro</code>
                  </div>
                  <div>
                    <span style={{ color: 'var(--color-text-primary)' }}>硅基流动 SiliconFlow</span>
                    ：Endpoint <code style={{ color: 'var(--color-semantic-blue)' }}>https://api.siliconflow.cn/v1</code>
                    ，模型如 <code>deepseek-ai/DeepSeek-V3</code>
                  </div>
                  <div>
                    <span style={{ color: 'var(--color-text-primary)' }}>其他 OpenAI 兼容服务</span>
                    ：填入服务提供的 API 地址（到 /v1），填入对应的模型名和 Key
                  </div>
                </div>
              </div>
            </>
          )}

          {/* Model info */}
          {providerCaps && (
            <div
              className="rounded-md px-3 py-2 text-xs"
              style={{
                background: 'var(--color-bg-main)',
                color: 'var(--color-text-muted)',
              }}
            >
              <div>可用模型：{providerCaps.modelsDesc}</div>
              {providerCaps.hasReasoning && (
                <div className="mt-1">支持推理模型自动路由</div>
              )}
            </div>
          )}
        </div>
      )}

      {mainTab === 'search' && (
        <div>
          {/* Tavily Search API Key */}
          <FormGroup label="Tavily Search API Key" desc="用于联网搜索（可选，备用）">
            <div className="relative">
              <input
                type={showTavilyKey ? 'text' : 'password'}
                className="h-9 w-full rounded-md border px-3 py-2 pr-16 text-base outline-none"
                style={{
                  background: 'var(--color-bg-main)',
                  borderColor: 'var(--color-border)',
                  color: 'var(--color-text-primary)',
                }}
                placeholder="tvly-..."
                value={settings.tavilyApiKey}
                onChange={(e) => settings.setTavilyApiKey(e.target.value)}
              />
              <button
                type="button"
                className="absolute right-2 top-1/2 -translate-y-1/2 rounded px-2 py-0.5 text-xs"
                style={{ color: 'var(--color-text-muted)' }}
                onClick={() => setShowTavilyKey(!showTavilyKey)}
              >
                {showTavilyKey ? '隐藏' : '显示'}
              </button>
            </div>
          </FormGroup>

          {/* Bocha Search API Key */}
          <FormGroup label="博查搜索 API Key" desc={<>用于联网搜索市场薪酬数据（推荐）。<a href="https://open.bochaai.com" target="_blank" rel="noopener noreferrer" style={{ color: 'var(--color-primary)' }}>获取 Key →</a></>}>
            <div className="relative">
              <input
                type={showBochaKey ? 'text' : 'password'}
                className="h-9 w-full rounded-md border px-3 py-2 pr-16 text-base outline-none"
                style={{
                  background: 'var(--color-bg-main)',
                  borderColor: 'var(--color-border)',
                  color: 'var(--color-text-primary)',
                }}
                placeholder="sk-..."
                value={settings.bochaApiKey}
                onChange={(e) => settings.setBochaApiKey(e.target.value)}
              />
              <button
                type="button"
                className="absolute right-2 top-1/2 -translate-y-1/2 rounded px-2 py-0.5 text-xs"
                style={{ color: 'var(--color-text-muted)' }}
                onClick={() => setShowBochaKey(!showBochaKey)}
              >
                {showBochaKey ? '隐藏' : '显示'}
              </button>
            </div>
          </FormGroup>
        </div>
      )}

      {mainTab === 'general' && (
        <div>
          {/* Workspace */}
          <FormGroup label="工作目录" desc="Agent 会在此目录下存放分析文件、报告和临时文件">
            <div className="flex items-center gap-2">
              <FormInput
                value={settings.workspacePath}
                placeholder="/Users/hr/AI小家工作区"
                onChange={(v) => settings.setWorkspacePath(v)}
              />
              <Button
                variant="secondary"
                className="shrink-0"
                onClick={async () => {
                  try {
                    const { open } = await import('@tauri-apps/plugin-dialog')
                    const selected = await open({ directory: true, multiple: false })
                    if (selected && typeof selected === 'string') {
                      settings.setWorkspacePath(selected)
                      await selectWorkspace(selected)
                    }
                  } catch (err) {
                    console.error('Failed to select workspace directory:', err)
                  }
                }}
              >
                选择目录
              </Button>
              <Button
                variant="secondary"
                className="shrink-0"
                onClick={async () => {
                  try {
                    await openWorkspaceDirectory()
                  } catch (err) {
                    console.error('Failed to open workspace directory:', err)
                  }
                }}
                disabled={!settings.workspacePath}
              >
                打开目录
              </Button>
            </div>
          </FormGroup>

          {/* System Info */}
          <div
            className="mt-6 border-t pt-4"
            style={{ borderColor: 'var(--color-border)' }}
          >
            <div
              className="mb-3 text-sm font-semibold"
              style={{ color: 'var(--color-text-secondary)' }}
            >
              系统信息
            </div>
            <div className="flex flex-col gap-2">
              <div className="flex items-center gap-3">
                <span className="text-sm" style={{ color: 'var(--color-text-muted)' }}>运行日志：</span>
                <Button
                  variant="secondary"
                  onClick={async () => {
                    try {
                      await openLogsDirectory()
                    } catch (err) {
                      console.error('Failed to open logs directory:', err)
                    }
                  }}
                >
                  打开日志目录
                </Button>
              </div>
            </div>
          </div>
        </div>
      )}

    </Modal>
  )
}

// --- Tab buttons ---

function TabButton({
  active,
  onClick,
  children,
}: {
  active: boolean
  onClick: () => void
  children: React.ReactNode
}) {
  return (
    <button
      className="cursor-pointer rounded-xl border-none px-3 py-1.5 text-sm font-medium transition-colors duration-150"
      style={{
        background: active ? 'var(--color-primary-subtle)' : 'transparent',
        color: active ? 'var(--color-primary)' : 'var(--color-text-muted)',
      }}
      onClick={onClick}
    >
      {children}
    </button>
  )
}

function SubTabButton({
  active,
  onClick,
  children,
}: {
  active: boolean
  onClick: () => void
  children: React.ReactNode
}) {
  return (
    <button
      className="cursor-pointer rounded-lg border-none px-2.5 py-1 text-xs font-medium transition-colors duration-150"
      style={{
        background: active ? 'var(--color-primary-subtle)' : 'transparent',
        color: active ? 'var(--color-primary)' : 'var(--color-text-muted)',
      }}
      onClick={onClick}
    >
      {children}
    </button>
  )
}

// --- Internal form primitives ---

function FormGroup({
  label,
  desc,
  className,
  children,
}: {
  label: string
  desc?: React.ReactNode
  className?: string
  children: React.ReactNode
}) {
  return (
    <div className={`mb-4.5 ${className ?? ''}`}>
      <label
        className="mb-1.5 block text-sm font-semibold"
        style={{ color: 'var(--color-text-secondary)' }}
      >
        {label}
      </label>
      {desc && (
        <div
          className="mb-2 text-xs"
          style={{ color: 'var(--color-text-muted)' }}
        >
          {desc}
        </div>
      )}
      {children}
    </div>
  )
}

function FormInput({
  value,
  placeholder,
  type = 'text',
  onChange,
}: {
  value: string
  placeholder?: string
  type?: string
  onChange: (v: string) => void
}) {
  return (
    <input
      type={type}
      className="h-9 w-full rounded-md border px-3 py-2 text-base outline-none"
      style={{
        background: 'var(--color-bg-main)',
        borderColor: 'var(--color-border)',
        color: 'var(--color-text-primary)',
      }}
      placeholder={placeholder}
      value={value}
      onChange={(e) => onChange(e.target.value)}
    />
  )
}

/**
 * SettingsModal — tabbed settings: model configuration (per-provider) + general settings.
 */
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Modal } from '@/components/common/Modal'
import { Button } from '@/components/common/Button'
import { useSettingsStore } from '@/stores/settingsStore'
import { useNotificationStore } from '@/stores/notificationStore'
import {
  getSettings,
  updateSettings,
  validateApiKey,
  selectWorkspace,
  pickLocalDirectory,
  getAllProviderKeys,
  updateAllProviderKeys,
  getConfiguredProviders,
  switchProvider,
  openLogsDirectory,
  openWorkspaceDirectory,
  exportMetrics,
  clearMetrics,
  getMetricsInfo,
} from '@/lib/tauri'
import type { LlmProvider } from '@/types/settings'
import { PROVIDER_CAPABILITIES } from '@/types/settings'
import { useAuthStore } from '@/stores/authStore'
import { LoginSection } from '@/components/settings/LoginSection'
import { PersonaTab } from '@/components/settings/PersonaTab'
import { SkillsTab } from '@/components/settings/SkillsTab'
import { WorkspaceAuthPanel } from '@/components/settings/WorkspaceAuthPanel'
import { useChatStore } from '@/stores/chatStore'
import type { AppLanguage } from '@/i18n'

interface SettingsModalProps {
  open: boolean
  onClose: () => void
}

type MainTab = 'account' | 'models' | 'search' | 'general' | 'persona' | 'skills'

const PROVIDER_LIST: { value: LlmProvider; labelKey: string }[] = [
  { value: 'deepseek-v3', labelKey: 'providers.deepseek-v3' },
  { value: 'qwen-plus', labelKey: 'providers.qwen-plus' },
  { value: 'volcano', labelKey: 'providers.volcano' },
  { value: 'openai', labelKey: 'providers.openai' },
  { value: 'claude', labelKey: 'providers.claude' },
  { value: 'custom', labelKey: 'providers.custom' },
]

const API_KEY_PLACEHOLDERS: Record<LlmProvider, string> = {
  'deepseek-v3': 'sk-...',
  'qwen-plus': 'sk-...',
  'volcano': 'API Key...',
  'openai': 'sk-...',
  'claude': 'sk-ant-...',
  'custom': '', // will use t() at render time
}

export function SettingsModal({ open, onClose }: SettingsModalProps) {
  const settings = useSettingsStore()
  const notifications = useNotificationStore()
  const { t } = useTranslation()
  const activeConversationId = useChatStore((s) => s.activeConversationId)

  const [mainTab, setMainTab] = useState<MainTab>('account')
  const [activeProvider, setActiveProvider] = useState<LlmProvider>('deepseek-v3')
  const [saving, setSaving] = useState(false)
  const isLoggedIn = useAuthStore((s) => s.isLoggedIn)

  // Per-provider key cache: provider → plaintext key
  const [keyCache, setKeyCache] = useState<Partial<Record<LlmProvider, string>>>({})
  // Per-provider validation state
  const [validating, setValidating] = useState(false)
  const [keyValid, setKeyValid] = useState<Record<string, boolean | null>>({})
  // Show/hide toggles
  const [showApiKey, setShowApiKey] = useState(false)
  const [showTavilyKey, setShowTavilyKey] = useState(false)
  const [showBochaKey, setShowBochaKey] = useState(false)

  // Metrics info state
  const [metricsCount, setMetricsCount] = useState(0)
  const [metricsBytes, setMetricsBytes] = useState(0)
  const [metricsLoading, setMetricsLoading] = useState(false)

  // Load settings + all provider keys when modal opens
  useEffect(() => {
    if (!open) return
    setShowApiKey(false)
    setShowTavilyKey(false)
    setKeyValid({})
    // Load metrics info
    getMetricsInfo()
      .then(({ entryCount, totalBytes }) => {
        setMetricsCount(entryCount)
        setMetricsBytes(totalBytes)
      })
      .catch(() => {
        setMetricsCount(0)
        setMetricsBytes(0)
      })
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
        useCloud: settings.useCloud,
        cloudModel: settings.cloudModel,
        cloudModelType: settings.cloudModelType,
        appLanguage: settings.appLanguage,
      })
      const providers = await getConfiguredProviders()
      useSettingsStore.getState().setConfiguredProviders(providers as LlmProvider[])

      onClose()
    } catch (err) {
      console.error('Failed to save settings:', err)
      notifications.push({
        level: 'error',
        title: t('settings.saveFailed'),
        message: err instanceof Error ? err.message : t('settings.saveFailedDesc'),
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
          useCloud: settings.useCloud,
          cloudModel: settings.cloudModel,
          cloudModelType: settings.cloudModelType,
        })
      }

      notifications.push({
        level: 'success',
        title: t('settings.switchedModel'),
        message: `${t('providers.' + provider)}`,
        actions: [],
        dismissible: true,
        autoHide: 3,
        context: 'toast',
      })
    } catch (err) {
      console.error('Failed to switch provider:', err)
      notifications.push({
        level: 'error',
        title: t('settings.switchFailed'),
        message: err instanceof Error ? err.message : t('settings.switchFailedDesc'),
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
        {t('common.cancel')}
      </Button>
      <Button variant="primary" onClick={handleSave} disabled={saving}>
        {saving ? t('settings.saving') : t('settings.saveSettings')}
      </Button>
    </>
  )

  return (
    <Modal open={open} onClose={onClose} title={t('settings.title')} footer={footer}>
      {/* Main Tab Bar */}
      <div
        className="mb-4 flex items-center gap-1 border-b pb-3"
        style={{ borderColor: 'var(--color-border)' }}
      >
        <TabButton
          active={mainTab === 'account'}
          onClick={() => setMainTab('account')}
        >
          {t('settings.tabs.account')}
        </TabButton>
        {!isLoggedIn && (
          <TabButton
            active={mainTab === 'models'}
            onClick={() => setMainTab('models')}
          >
            {t('settings.tabs.models')}
          </TabButton>
        )}
        {!isLoggedIn && (
          <TabButton
            active={mainTab === 'search'}
            onClick={() => setMainTab('search')}
          >
            {t('settings.tabs.search')}
          </TabButton>
        )}
        <TabButton
          active={mainTab === 'general'}
          onClick={() => setMainTab('general')}
        >
          {t('settings.tabs.general')}
        </TabButton>
        <TabButton
          active={mainTab === 'persona'}
          onClick={() => setMainTab('persona')}
        >
          {t('settings.tabs.persona')}
        </TabButton>
        <TabButton
          active={mainTab === 'skills'}
          onClick={() => setMainTab('skills')}
        >
          {t('settings.tabs.skills')}
        </TabButton>
      </div>

      {/* Tab Content */}
      {mainTab === 'account' && (
        <LoginSection onLoginSuccess={() => setMainTab('account')} />
      )}

      {mainTab === 'models' && !isLoggedIn && (
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
                {t(p.labelKey)}
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
              {t('settings.currentDefault')}
            </div>
          )}

          {/* API Key Input */}
          <FormGroup
            label="API Key"
            desc={t('settings.enterApiKey', { provider: t('providers.' + activeProvider) })}
          >
            <div className="relative">
              <input
                type={showApiKey ? 'text' : 'password'}
                className="h-9 w-full rounded-md border px-3 pr-16 text-sm outline-none"
                style={{
                  background: 'var(--color-bg-main)',
                  borderColor: 'var(--color-border)',
                  color: 'var(--color-text-primary)',
                }}
                placeholder={activeProvider === 'custom' ? t('settings.apiKeyOptional') : (API_KEY_PLACEHOLDERS[activeProvider] ?? 'sk-...')}
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
                {showApiKey ? t('common.hide') : t('common.show')}
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
              {validating ? t('settings.validating') : activeProvider === 'custom' ? t('settings.testConnection') : t('settings.validateKey')}
            </Button>

            {activeProvider !== settings.primaryModel && (
              <Button
                variant="secondary"
                onClick={() => handleSetAsPrimary(activeProvider)}
              >
                {t('settings.setAsDefault')}
              </Button>
            )}

            {keyValid[activeProvider] === true && (
              <span className="text-sm" style={{ color: 'var(--color-semantic-green)' }}>
                {t('settings.keyValid')}
              </span>
            )}
            {keyValid[activeProvider] === false && (
              <span className="text-sm" style={{ color: 'var(--color-semantic-red)' }}>
                {t('settings.keyInvalid')}
              </span>
            )}
          </div>

          {/* Custom model config — only for 'custom' provider */}
          {activeProvider === 'custom' && (
            <>
              <FormGroup
                label={t('settings.customModel.endpointLabel')}
                desc={t('settings.customModel.endpointDesc')}
              >
                <FormInput
                  value={settings.customModelEndpoint}
                  placeholder="http://localhost:11434/v1"
                  onChange={(v) => settings.setCustomModelEndpoint(v)}
                />
              </FormGroup>
              <FormGroup
                label={t('settings.customModel.modelNameLabel')}
                desc={t('settings.customModel.modelNameDesc')}
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
                  {t('settings.customModel.examplesTitle')}
                </div>
                <div className="flex flex-col gap-1.5">
                  <div>
                    <span style={{ color: 'var(--color-text-primary)' }}>{t('settings.customModel.ollamaLocal')}</span>
                    ：{t('settings.customModel.endpoint')} <code style={{ color: 'var(--color-semantic-blue)' }}>http://localhost:11434/v1</code>
                    ，{t('settings.customModel.modelLike')} <code>qwen2.5:7b</code>
                    ，{t('settings.customModel.keyEmpty')}
                  </div>
                  <div>
                    <span style={{ color: 'var(--color-text-primary)' }}>{t('settings.customModel.lmStudioLocal')}</span>
                    ：{t('settings.customModel.endpoint')} <code style={{ color: 'var(--color-semantic-blue)' }}>http://localhost:1234/v1</code>
                    ，{t('settings.customModel.modelLike')} <code>llama3</code>
                    ，{t('settings.customModel.keyEmpty')}
                  </div>
                  <div>
                    <span style={{ color: 'var(--color-text-primary)' }}>{t('settings.customModel.openRouter')}</span>
                    ：{t('settings.customModel.endpoint')} <code style={{ color: 'var(--color-semantic-blue)' }}>https://openrouter.ai/api/v1</code>
                    ，{t('settings.customModel.modelLike')} <code>google/gemini-pro</code>
                  </div>
                  <div>
                    <span style={{ color: 'var(--color-text-primary)' }}>{t('settings.customModel.siliconFlow')}</span>
                    ：{t('settings.customModel.endpoint')} <code style={{ color: 'var(--color-semantic-blue)' }}>https://api.siliconflow.cn/v1</code>
                    ，{t('settings.customModel.modelLike')} <code>deepseek-ai/DeepSeek-V3</code>
                  </div>
                  <div>
                    <span style={{ color: 'var(--color-text-primary)' }}>{t('settings.customModel.otherCompatible')}</span>
                    ：{t('settings.customModel.otherDesc')}
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
              <div>{t('settings.availableModels')}：{t('providers.capabilities.' + activeProvider)}</div>
              {providerCaps.hasReasoning && (
                <div className="mt-1">{t('settings.reasoningRouting')}</div>
              )}
            </div>
          )}
        </div>
      )}

      {mainTab === 'search' && !isLoggedIn && (
        <div>
          {/* Tavily Search API Key */}
          <FormGroup label={t('settings.search.tavilyLabel')} desc={t('settings.search.tavilyDesc')}>
            <div className="relative">
              <input
                type={showTavilyKey ? 'text' : 'password'}
                className="h-9 w-full rounded-md border px-3 pr-16 text-sm outline-none"
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
                {showTavilyKey ? t('common.hide') : t('common.show')}
              </button>
            </div>
          </FormGroup>

          {/* Bocha Search API Key */}
          <FormGroup label={t('settings.search.bochaLabel')} desc={<>{t('settings.search.bochaDesc')}。<a href="https://open.bochaai.com" target="_blank" rel="noopener noreferrer" style={{ color: 'var(--color-primary)' }}>{t('settings.search.bochaGetKey')}</a></>}>
            <div className="relative">
              <input
                type={showBochaKey ? 'text' : 'password'}
                className="h-9 w-full rounded-md border px-3 pr-16 text-sm outline-none"
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
                {showBochaKey ? t('common.hide') : t('common.show')}
              </button>
            </div>
          </FormGroup>
        </div>
      )}

      {mainTab === 'general' && (
        <div>
          {/* Language toggle */}
          <FormGroup label={t('settings.general.language')} desc={t('settings.general.languageDesc')}>
            <div
              className="inline-flex rounded-md border"
              style={{ borderColor: 'var(--color-border)' }}
            >
              {([['zh-CN', '中文'], ['en-US', 'English']] as const).map(([lang, label], idx) => (
                <button
                  key={lang}
                  className={`px-4 py-1.5 text-sm font-medium transition-colors ${idx === 0 ? 'rounded-l-md' : 'rounded-r-md'}`}
                  style={{
                    background: (settings.appLanguage || 'zh-CN') === lang ? 'var(--color-primary-subtle)' : 'transparent',
                    color: (settings.appLanguage || 'zh-CN') === lang ? 'var(--color-primary)' : 'var(--color-text-muted)',
                    border: 'none',
                    borderLeft: idx > 0 ? '1px solid var(--color-border)' : 'none',
                    cursor: 'pointer',
                  }}
                  onClick={() => settings.setAppLanguage(lang as AppLanguage)}
                >
                  {label}
                </button>
              ))}
            </div>
          </FormGroup>

          {/* Workspace */}
          <FormGroup label={t('settings.general.workspace')} desc={t('settings.general.workspaceDesc')}>
            <div className="flex items-center gap-2">
              <FormInput
                value={settings.workspacePath}
                placeholder="/Users/hr/workspace"
                onChange={(v) => settings.setWorkspacePath(v)}
              />
              <Button
                variant="secondary"
                className="shrink-0"
                onClick={async () => {
                  try {
                    const selected = await pickLocalDirectory({
                      defaultPath: settings.workspacePath || undefined,
                      title: t('settings.general.workspacePickerTitle'),
                    })
                    if (selected) {
                      settings.setWorkspacePath(selected)
                      await selectWorkspace(selected)
                    }
                  } catch (err) {
                    console.error('Failed to select workspace directory:', err)
                  }
                }}
              >
                {t('settings.general.selectDir')}
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
                {t('settings.general.openDir')}
              </Button>
            </div>
            <p
              className="mt-2 text-xs"
              style={{ color: 'var(--color-text-muted)' }}
            >
              {t('settings.general.workspaceHint')}
            </p>
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
              {t('settings.general.systemInfo')}
            </div>
            <div className="flex flex-col gap-2">
              <div className="flex items-center gap-3">
                <span className="text-sm" style={{ color: 'var(--color-text-muted)' }}>{t('settings.general.logs')}</span>
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
                  {t('settings.general.openLogs')}
                </Button>
              </div>
              <div className="flex items-center gap-3">
                <span className="text-sm" style={{ color: 'var(--color-text-muted)' }}>{t('settings.general.metrics')}</span>
                <span className="text-sm" style={{ color: 'var(--color-text-secondary)' }}>
                  {t('settings.general.metricsCount', {
                    count: metricsCount,
                    size: metricsBytes < 1024 ? `${metricsBytes} B` : `${(metricsBytes / 1024).toFixed(0)} KB`,
                  })}
                </span>
                <Button
                  variant="secondary"
                  disabled={metricsCount === 0 || metricsLoading}
                  onClick={async () => {
                    try {
                      setMetricsLoading(true)
                      const { save } = await import('@tauri-apps/plugin-dialog')
                      const now = new Date()
                      const dateStr = `${now.getFullYear()}${String(now.getMonth() + 1).padStart(2, '0')}${String(now.getDate()).padStart(2, '0')}`
                      const dest = await save({
                        defaultPath: `aijia-metrics-${dateStr}.json`,
                        filters: [{ name: 'JSON', extensions: ['json'] }],
                      })
                      if (dest) {
                        const result = await exportMetrics(dest)
                        notifications.push({ level: 'success', title: t('settings.general.exportSuccess'), message: t('settings.general.exportedCount', { count: result.entryCount }), actions: [], dismissible: true, autoHide: 3, context: 'toast' })
                      }
                    } catch (err) {
                      console.error('Failed to export metrics:', err)
                      notifications.push({ level: 'error', title: t('settings.general.exportFailed'), message: String(err), actions: [], dismissible: true, autoHide: 5, context: 'toast' })
                    } finally {
                      setMetricsLoading(false)
                    }
                  }}
                >
                  {t('common.export')}
                </Button>
                <Button
                  variant="secondary"
                  disabled={metricsCount === 0 || metricsLoading}
                  onClick={async () => {
                    try {
                      setMetricsLoading(true)
                      await clearMetrics()
                      setMetricsCount(0)
                      setMetricsBytes(0)
                      notifications.push({ level: 'success', title: t('settings.general.cleanupDone'), message: t('settings.general.metricsCleared'), actions: [], dismissible: true, autoHide: 3, context: 'toast' })
                    } catch (err) {
                      console.error('Failed to clear metrics:', err)
                      notifications.push({ level: 'error', title: t('settings.general.cleanupFailed'), message: String(err), actions: [], dismissible: true, autoHide: 5, context: 'toast' })
                    } finally {
                      setMetricsLoading(false)
                    }
                  }}
                >
                  {t('settings.general.cleanup')}
                </Button>
              </div>
            </div>
          </div>

          {/* Authorized local workspace */}
          <div
            className="mt-6 border-t pt-4"
            style={{ borderColor: 'var(--color-border)' }}
          >
            {activeConversationId ? (
              <WorkspaceAuthPanel sessionId={activeConversationId} />
            ) : (
              <div className="space-y-2">
                <div
                  className="text-sm font-medium"
                  style={{ color: 'var(--color-text-secondary)' }}
                >
                  本地工作目录
                </div>
                <p
                  className="text-xs"
                  style={{ color: 'var(--color-text-muted)' }}
                >
                  先创建或选中一个会话，再为该会话授权本地目录。授权会绑定到当前会话，随后 AI 才会在聊天中暴露本地目录读取能力。
                </p>
              </div>
            )}
          </div>
        </div>
      )}


      {mainTab === 'persona' && (
        <PersonaTab />
      )}

      {mainTab === 'skills' && (
        <SkillsTab />
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
      className="cursor-pointer rounded-md border-none px-3 py-1.5 text-sm font-medium transition-colors duration-150"
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
      className="cursor-pointer rounded-md border-none px-2.5 py-1 text-xs font-medium transition-colors duration-150"
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
      className="h-9 w-full rounded-md border px-3 text-sm outline-none"
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

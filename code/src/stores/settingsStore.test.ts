import { describe, it, expect, beforeEach } from 'vitest'
import { useSettingsStore } from './settingsStore'
import { DEFAULT_SETTINGS } from '@/types/settings'

// Reset store between tests
beforeEach(() => {
  useSettingsStore.setState({ ...DEFAULT_SETTINGS, isLoaded: false })
})

// ---------------------------------------------------------------------------
// Default values
// ---------------------------------------------------------------------------

describe('settingsStore — defaults', () => {
  it('starts with default settings', () => {
    const state = useSettingsStore.getState()
    expect(state.primaryModel).toBe('deepseek-v3')
    expect(state.primaryApiKey).toBe('')
    expect(state.autoModelRouting).toBe(true)
    expect(state.analysisThreshold).toBe(1.65)
    expect(state.dataMaskingLevel).toBe('strict')
    expect(state.autoCleanupEnabled).toBe(true)
    expect(state.tempFileRetentionDays).toBe(7)
    expect(state.keepOldVersions).toBe(1)
    expect(state.isLoaded).toBe(false)
  })
})

// ---------------------------------------------------------------------------
// Individual setters
// ---------------------------------------------------------------------------

describe('settingsStore — setters', () => {
  it('sets primary model', () => {
    useSettingsStore.getState().setPrimaryModel('openai')
    expect(useSettingsStore.getState().primaryModel).toBe('openai')
  })

  it('sets primary API key', () => {
    useSettingsStore.getState().setPrimaryApiKey('sk-test-key')
    expect(useSettingsStore.getState().primaryApiKey).toBe('sk-test-key')
  })

  it('sets workspace path', () => {
    useSettingsStore.getState().setWorkspacePath('/home/user/workspace')
    expect(useSettingsStore.getState().workspacePath).toBe('/home/user/workspace')
  })

  it('sets auto model routing', () => {
    useSettingsStore.getState().setAutoModelRouting(false)
    expect(useSettingsStore.getState().autoModelRouting).toBe(false)
  })

  it('sets Tavily API key', () => {
    useSettingsStore.getState().setTavilyApiKey('tvly-xxx')
    expect(useSettingsStore.getState().tavilyApiKey).toBe('tvly-xxx')
  })

  it('marks as loaded', () => {
    useSettingsStore.getState().markLoaded()
    expect(useSettingsStore.getState().isLoaded).toBe(true)
  })
})

// ---------------------------------------------------------------------------
// Bulk update
// ---------------------------------------------------------------------------

describe('settingsStore — setSettings (bulk)', () => {
  it('updates multiple settings at once', () => {
    useSettingsStore.getState().setSettings({
      primaryModel: 'claude',
      autoModelRouting: false,
    })

    const state = useSettingsStore.getState()
    expect(state.primaryModel).toBe('claude')
    expect(state.autoModelRouting).toBe(false)
    // Other settings remain at defaults
    expect(state.autoCleanupEnabled).toBe(true)
  })
})

// ---------------------------------------------------------------------------
// Setter independence
// ---------------------------------------------------------------------------

describe('settingsStore — setter independence', () => {
  it('changing one setting does not affect others', () => {
    useSettingsStore.getState().setPrimaryApiKey('key123')
    useSettingsStore.getState().setAutoModelRouting(false)

    // Both changes should persist
    expect(useSettingsStore.getState().primaryApiKey).toBe('key123')
    expect(useSettingsStore.getState().autoModelRouting).toBe(false)
    // Unrelated settings untouched
    expect(useSettingsStore.getState().primaryModel).toBe('deepseek-v3')
  })
})

import { describe, it, expect, beforeEach, afterEach } from 'vitest'
import { useSettingsStore } from './settingsStore'
import i18n from '@/i18n'
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
    expect(state.dataMaskingLevel).toBe('relaxed')
    expect(state.autoCleanupEnabled).toBe(true)
    expect(state.tempFileRetentionDays).toBe(7)
    expect(state.keepOldVersions).toBe(1)
    expect(state.cloudModel).toBe('')
    expect(state.cloudModelType).toBe('')
    expect(state.fontScale).toBe('medium')
    expect(state.chatWidthMode).toBe('full')
    expect(state.uiSidebarCollapsedProjects).toBe('')
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

  it('sets font scale and applies the root font size immediately', () => {
    useSettingsStore.getState().setFontScale('large')
    expect(useSettingsStore.getState().fontScale).toBe('large')
    expect(document.documentElement.style.fontSize).toBe('17.2307692308px')

    useSettingsStore.getState().setFontScale('small')
    expect(document.documentElement.style.fontSize).toBe('14.7692307692px')
  })

  it('sets chat width mode', () => {
    useSettingsStore.getState().setChatWidthMode('full')
    expect(useSettingsStore.getState().chatWidthMode).toBe('full')

    useSettingsStore.getState().setChatWidthMode('centered')
    expect(useSettingsStore.getState().chatWidthMode).toBe('centered')
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
      chatWidthMode: 'full',
    })

    const state = useSettingsStore.getState()
    expect(state.primaryModel).toBe('claude')
    expect(state.autoModelRouting).toBe(false)
    expect(state.chatWidthMode).toBe('full')
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

// ---------------------------------------------------------------------------
// Language: device choice (login page / settings) wins over backend on load
// ---------------------------------------------------------------------------

describe('settingsStore — appLanguage consistency', () => {
  afterEach(() => {
    void i18n.changeLanguage('zh-CN')
  })

  it('setAppLanguage updates both i18n and the store', () => {
    useSettingsStore.getState().setAppLanguage('en-US')
    expect(i18n.language).toBe('en-US')
    expect(useSettingsStore.getState().appLanguage).toBe('en-US')
  })

  it('keeps the live UI language when backend settings load with a different appLanguage', () => {
    // Simulate the user picking English on the login screen…
    useSettingsStore.getState().setAppLanguage('en-US')

    // …then backend settings (saved on another session) arriving post sign-in.
    useSettingsStore.getState().setSettings({ primaryModel: 'claude', appLanguage: 'zh-CN' })

    const state = useSettingsStore.getState()
    // Device choice wins: the on-screen language and the store agree.
    expect(i18n.language).toBe('en-US')
    expect(state.appLanguage).toBe('en-US')
    // Unrelated settings still apply.
    expect(state.primaryModel).toBe('claude')
  })

  it('leaves appLanguage untouched on a partial update that omits it', () => {
    useSettingsStore.getState().setAppLanguage('en-US')
    useSettingsStore.getState().setSettings({ primaryModel: 'openai' })
    expect(useSettingsStore.getState().appLanguage).toBe('en-US')
  })
})

describe('settingsStore — setDataMaskingLevel', () => {
  it('updates dataMaskingLevel in store', () => {
    useSettingsStore.getState().setDataMaskingLevel('strict')
    expect(useSettingsStore.getState().dataMaskingLevel).toBe('strict')
    useSettingsStore.getState().setDataMaskingLevel('relaxed')
    expect(useSettingsStore.getState().dataMaskingLevel).toBe('relaxed')
  })
})

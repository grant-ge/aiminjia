import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Sidebar } from '@/components/layout/Sidebar'
import { TopBar } from '@/components/layout/TopBar'
import { TitleBar } from '@/components/layout/TitleBar'
import { ChatArea } from '@/components/layout/ChatArea'
import { InputBar } from '@/components/layout/InputBar'
import { SettingsModal } from '@/components/settings/SettingsModal'
import { ToastContainer } from '@/components/common/ToastContainer'
import { PersonaSelector } from '@/components/onboarding/PersonaSelector'
import { BrowserPanel } from '@/components/browser/BrowserPanel'
import { useStreaming } from '@/hooks/useStreaming'
import { useUpdater } from '@/hooks/useUpdater'
import { useChat } from '@/hooks/useChat'
import { onConversationTitleUpdated, onAuthExpired, onBrowserNavigating, onBrowserPageReady, onBrowserClosed, getCloudAuth, getCloudModels, getSettings, updateSettings, getPluginInfo } from '@/lib/tauri'
import { useChatStore } from '@/stores/chatStore'
import { useAuthStore } from '@/stores/authStore'
import { usePluginStore } from '@/stores/pluginStore'
import { usePersonaStore } from '@/stores/personaStore'
import { useSettingsStore } from '@/stores/settingsStore'
import { useNotificationStore } from '@/stores/notificationStore'
import { useBrowserStore } from '@/stores/browserStore'
import { useBrandingStore } from '@/stores/brandingStore'

function App() {
  useStreaming()
  useUpdater()
  const { t, i18n } = useTranslation()

  const { loadConversations } = useChat()

  const [showPersonaSelector, setShowPersonaSelector] = useState(false)

  // Check persona onboarding status
  useEffect(() => {
    getSettings()
      .then((saved) => {
        if (!saved.personaOnboardingDone) {
          setShowPersonaSelector(true)
        }
        // Sync persisted language preference to i18next
        if (saved.appLanguage && saved.appLanguage !== i18n.language) {
          i18n.changeLanguage(saved.appLanguage)
        }
      })
      .catch((err) => console.error('Failed to check onboarding:', err))
  }, [])

  const handlePersonaOnboardingComplete = async () => {
    try {
      const saved = await getSettings()
      await updateSettings({ ...saved, personaOnboardingDone: true })
      setShowPersonaSelector(false)
    } catch (err) {
      console.error('Failed to complete onboarding:', err)
    }
  }

  
  useEffect(() => {
    loadConversations()
  }, [loadConversations])

  // Load plugin info (tools + skills) on startup
  useEffect(() => {
    getPluginInfo()
      .then(({ tools, skills }) => {
        usePluginStore.getState().setAll(tools, skills)
      })
      .catch((err) => console.error('Failed to load plugin info:', err))
  }, [])

  // Load active persona on startup
  useEffect(() => {
    usePersonaStore.getState().reload()
      .catch((err) => console.error('Failed to load persona:', err))
  }, [])

  // Restore cloud auth state + branding on startup (single authoritative source)
  useEffect(() => {
    getCloudAuth()
      .then(async (info) => {
        if (info.loggedIn) {
          useAuthStore.getState().setAuth(info)
          // Apply tenant branding (product name, logo, colors)
          useBrandingStore.getState().applyBranding(info.tenant ?? null)
          // Fetch cloud models (get_auth_info returns empty models)
          try {
            const models = await getCloudModels()
            useAuthStore.getState().setCloudModels(models)
            // Restore selectedCloudModel and useCloud from persisted settings
            const saved = await getSettings()
            useSettingsStore.getState().setSettings({ useCloud: saved.useCloud ?? false })
            if (saved.cloudModel && models.find((m) => m.id === saved.cloudModel)) {
              useAuthStore.getState().setSelectedCloudModel(saved.cloudModel)
            } else if (models.length > 0) {
              useAuthStore.getState().setSelectedCloudModel(models[0].id)
            }
          } catch (err) {
            console.error('Failed to fetch cloud models on restore:', err)
          }
        } else {
          // Not logged in — ensure useCloud is false
          const saved = await getSettings()
          if (saved.useCloud) {
            await updateSettings({ ...saved, useCloud: false }).catch(() => {})
          }
          useSettingsStore.getState().setSettings({ useCloud: false })
        }
      })
      .catch((err) => console.error('Failed to restore cloud auth:', err))
  }, [])

  // Listen for auth:expired events from backend
  useEffect(() => {
    const unlisten = onAuthExpired(({ message }) => {
      console.warn('[auth:expired]', message)
      useAuthStore.getState().clearAuth()
      useBrandingStore.getState().reset()
      // Keep useCloud unchanged — user must explicitly switch
      useNotificationStore.getState().push({
        level: 'warning',
        title: t('auth.expired'),
        message: t('auth.expiredDesc'),
        actions: [],
        dismissible: true,
        autoHide: 8,
        context: 'toast',
      })
    })
    return () => {
      unlisten.then((fn) => fn())
    }
  }, [])

  // Listen for conversation title updates from backend
  useEffect(() => {
    const unlisten = onConversationTitleUpdated(({ conversationId, title }) => {
      const store = useChatStore.getState()
      store.setConversations(
        store.conversations.map((c) =>
          c.id === conversationId ? { ...c, title } : c,
        ),
      )
    })
    return () => {
      unlisten.then((fn) => fn())
    }
  }, [])

  // Listen for browser events from backend (WebView state sync)
  useEffect(() => {
    const unlistenNavigating = onBrowserNavigating(({ appId, url }) => {
      useBrowserStore.getState().setNavigating(appId ?? 0, url)
    })
    const unlistenReady = onBrowserPageReady(({ appId, url, title }) => {
      useBrowserStore.getState().setPageReady(appId ?? 0, url, title)
    })
    const unlistenClosed = onBrowserClosed(({ appId }) => {
      useBrowserStore.getState().setClosed(appId ?? 0)
    })
    return () => {
      unlistenNavigating.then((fn) => fn())
      unlistenReady.then((fn) => fn())
      unlistenClosed.then((fn) => fn())
    }
  }, [])

  const [settingsOpen, setSettingsOpen] = useState(false)

  return (
    <div className="flex h-screen w-full flex-col">
      <TitleBar />
      <div className="flex flex-1 overflow-hidden">
        <Sidebar onOpenSettings={() => setSettingsOpen(true)} />
        <main className="flex flex-1 flex-col overflow-hidden">
          <TopBar />
          <div className="relative flex flex-1 overflow-hidden">
            <div className="flex flex-1 flex-col overflow-hidden">
              <ChatArea />
              <InputBar />
            </div>
            <BrowserPanel />
          </div>
        </main>
      </div>
      <SettingsModal open={settingsOpen} onClose={() => setSettingsOpen(false)} />
      <ToastContainer />
    
      {showPersonaSelector && (
        <PersonaSelector onComplete={handlePersonaOnboardingComplete} />
      )}

    </div>
  )
}

export default App

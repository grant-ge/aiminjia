import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import '@/i18n'
import '@/styles/globals.css'
import App from './App'
import { useChatStore } from '@/stores/chatStore'
import { useSessionStore } from '@/stores/sessionStore'

if (import.meta.env.DEV) {
  ;(window as unknown as { __aijia?: unknown }).__aijia = {
    chatStore: useChatStore,
    sessionStore: useSessionStore,
  }
}

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <App />
  </StrictMode>,
)

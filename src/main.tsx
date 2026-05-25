// Must be first: installs Array.prototype.findLast etc. for Big Sur Safari 14.
import '@/legacy-polyfills'
import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import '@/i18n'
import '@/styles/globals.css'
import App from './App'
import { useAuthStore } from '@/stores/authStore'
import { useChatStore } from '@/stores/chatStore'
import { useSessionStore } from '@/stores/sessionStore'
import { useUiStore } from '@/stores/uiStore'

if (import.meta.env.DEV || import.meta.env.VITE_E2E_ENABLED === 'true') {
  ;(window as unknown as { __aijia?: unknown }).__aijia = {
    chatStore: useChatStore,
    sessionStore: useSessionStore,
    authStore: useAuthStore,
    uiStore: useUiStore,
    // E2E one-shot mock for `pickAttachments()` — CLI pushes a string[] of
    // absolute paths here; the next call to `pickAttachments()` in
    // `useChatAttachments.ts` shifts it instead of opening the OS dialog.
    // Real path is taken downstream (makePendingAttachment / token insert).
    _pickAttachmentsMockQueue: [] as string[][],
    // E2E one-shot mock for `pickLocalDirectory()` — CLI pushes single
    // absolute folder paths (one per pick); next call in `src/lib/tauri.ts`
    // shifts one and skips the OS folder dialog. Downstream
    // `authorizeLocalDirectory` IPC and home composer state updates still run.
    _pickDirectoryMockQueue: [] as string[],
    // E2E one-shot mock for skill-center's `openDialog()` calls in
    // `handleImportDirectory` / `handleImportArchive`. CLI pushes a single
    // absolute path (folder for directory variant, .zip file for archive
    // variant). The next matching dropdown click in SkillCenterPage shifts
    // one and skips the OS dialog. Real `runInstall(picked)` →
    // `installCustomSkill` → backend `install_custom_skill` chain runs as
    // normal. The variant selection (which item the user clicks) determines
    // which handler consumes the queue—queue itself is variant-agnostic.
    _pickSkillImportMockQueue: [] as string[],
  }
}

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <App />
  </StrictMode>,
)

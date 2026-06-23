// Global Tauri IPC stub for the jsdom test environment.
//
// Components / stores that call `@tauri-apps/api` (invoke, event `listen`) at
// mount would otherwise throw `Cannot read properties of undefined (reading
// 'invoke' / 'transformCallback')` because Tauri only injects
// `window.__TAURI_INTERNALS__` inside the real webview.
//
// - `invoke` rejects: every typed wrapper in `@/lib/tauri` and every store that
//   fires IPC at mount already wraps the call in try/catch and logs a "failed"
//   warning, so a rejection reproduces production's "backend unavailable" path
//   instead of a hard TypeError that aborts the whole render.
// - `transformCallback` returns a numeric id so `event.listen()` can register a
//   no-op subscription (its follow-up `invoke('plugin:event|listen')` rejects
//   and is caught by `useTauriEvent`).
//
// Per-file `vi.mock('@/lib/tauri', ...)` still takes precedence for tests that
// assert on specific IPC calls — this stub only catches the un-mocked paths.

const internals = {
  metadata: {
    currentWindow: {
      label: 'main',
    },
  },
  invoke: (cmd: string) => {
    if (cmd === 'plugin:event|listen') return Promise.resolve(Math.floor(Math.random() * 1e9))
    if (cmd === 'plugin:event|unlisten') return Promise.resolve(undefined)
    if (cmd === 'plugin:window|is_fullscreen') return Promise.resolve(false)
    return Promise.reject(new Error('[test] Tauri IPC not available'))
  },
  transformCallback: (callback?: unknown) => {
    void callback
    return Math.floor(Math.random() * 1e9)
  },
  unregisterCallback: () => undefined,
}

;(globalThis as Record<string, unknown>).__TAURI_INTERNALS__ = internals
;(globalThis as Record<string, unknown>).__TAURI_EVENT_PLUGIN_INTERNALS__ = {
  unregisterListener: () => undefined,
}
if (typeof window !== 'undefined') {
  ;(window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = internals
  ;(window as unknown as Record<string, unknown>).__TAURI_EVENT_PLUGIN_INTERNALS__ = {
    unregisterListener: () => undefined,
  }
}

/// <reference types="vitest/config" />
import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import tailwindcss from '@tailwindcss/vite'
import path from 'path'

// https://vite.dev/config/
export default defineConfig({
  plugins: [
    tailwindcss(),
    react(),
  ],

  // Vitest configuration
  test: {
    globals: true,
    environment: 'jsdom',
    include: ['src/**/*.test.{ts,tsx}'],
    setupFiles: ['./src/test/setup-tauri.ts', './src/test/setup-i18n.ts', './src/test/setup-tiptap-jsdom.ts'],
  },

  // Prevent vite from obscuring Rust errors
  clearScreen: false,

  // Tauri expects a fixed port; fail if not available
  server: {
    host: '0.0.0.0',
    port: 5173,
    strictPort: true,
    watch: {
      // Tell Vite to ignore watching `src-tauri`
      ignored: ['**/src-tauri/**'],
    },
  },

  // Path aliases for cleaner imports
  resolve: {
    alias: {
      '@': path.resolve(__dirname, './src'),
    },
  },

  // Build target floor for the Tauri webview (system WebKit on macOS / WebView2
  // on Windows). macOS Monterey 12.x ships Safari 15.x; Vite 7's default target
  // (baseline-widely-available ≈ Safari 16) emits syntax that throws at parse on
  // those systems → blank white screen. Pin a conservative floor.
  // NOTE: `target` only down-levels *JS syntax* — it does NOT transpile regex
  // (e.g. lookbehind, unsupported < Safari 16.4) nor polyfill runtime APIs.
  // Keep risky deps version-locked (see `pnpm.overrides` in package.json).
  build: {
    target: process.env.TAURI_ENV_PLATFORM === 'windows' ? 'chrome105' : 'safari13',
  },

  // Env prefix for Tauri
  envPrefix: ['VITE_', 'TAURI_'],
})

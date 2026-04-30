import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'

import { describe, expect, it } from 'vitest'

import { useChatStore } from '@/stores/chatStore'

import { TAURI_EVENTS } from './tauri'

describe('review_permission_ask: architecture constraints', () => {
  it('review_permission_ask_event_constant_value is permission:ask', () => {
    expect(TAURI_EVENTS.PERMISSION_ASK).toBe('permission:ask')
  })

  it('review_permission_ask_pending_asks_is_map: store uses Map for O(1) lookup', () => {
    const store = useChatStore.getState()
    expect(store.pendingAsks).toBeInstanceOf(Map)
  })

  it('review_permission_ask_dialog_has_no_direct_invoke: dialog stays callback-driven', () => {
    const source = readFileSync(
      resolve(process.cwd(), 'src/components/common/PermissionAskDialog.tsx'),
      'utf8',
    )

    expect(source).not.toContain('@tauri-apps/api/core')
    expect(source).not.toMatch(/\binvoke\s*\(/)
    expect(source).toContain('onAllow')
    expect(source).toContain('onDeny')
    expect(source).toContain('onCancel')
  })

  it('review_permission_ask_dialog_exposes_remember_controls', () => {
    const source = readFileSync(
      resolve(process.cwd(), 'src/components/common/PermissionAskDialog.tsx'),
      'utf8',
    )

    expect(source).toContain('rememberOptions')
    expect(source).toContain('defaultDestination')
    expect(source).toContain('记住到工作区')
    expect(source).toContain('记住到用户级')
  })
})

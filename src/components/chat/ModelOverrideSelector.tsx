import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'

import {
  getConversationModelOverride,
  setConversationModelOverride,
} from '@/lib/tauri'
import type { LlmProvider } from '@/types/settings'

const MODEL_OPTIONS: LlmProvider[] = [
  'deepseek-v3',
  'qwen-plus',
  'volcano',
  'openai',
  'claude',
  'custom',
]

interface ModelOverrideSelectorProps {
  conversationId: string
}

export function ModelOverrideSelector({
  conversationId,
}: ModelOverrideSelectorProps) {
  const { t } = useTranslation()
  const [value, setValue] = useState<string>('')

  useEffect(() => {
    let active = true
    getConversationModelOverride(conversationId)
      .then((model) => {
        if (active) {
          setValue(model ?? '')
        }
      })
      .catch((err) => {
        console.error('[ModelOverrideSelector] failed to load override', err)
      })
    return () => {
      active = false
    }
  }, [conversationId])

  const handleChange = async (nextValue: string) => {
    setValue(nextValue)
    await setConversationModelOverride(conversationId, nextValue || null)
  }

  return (
    <label className="flex items-center gap-2 text-sm" style={{ color: 'var(--color-text-muted)' }}>
      <span>Model</span>
      <select
        className="rounded-md border px-2 py-1 text-sm"
        style={{
          background: 'var(--color-bg-card)',
          borderColor: 'var(--color-border)',
          color: 'var(--color-text-primary)',
        }}
        value={value}
        onChange={(event) => {
          void handleChange(event.target.value)
        }}
      >
        <option value="">{t('sidebar.useGlobalModel', { defaultValue: '使用全局设置' })}</option>
        {MODEL_OPTIONS.map((model) => (
          <option key={model} value={model}>
            {model}
          </option>
        ))}
      </select>
    </label>
  )
}

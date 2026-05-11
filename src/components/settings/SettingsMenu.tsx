/**
 * @designSource design.pen#YboA7/Z9asD/r95Aa
 * @sizing 220 width, bg secondary, top-left radius 18; row r-10 padding [10,12]
 */
import type { SettingsModalKey } from '@/stores/uiStore'
import { cn } from '@/lib/utils'

export interface SettingsMenuItem {
  key: SettingsModalKey
  label: string
  disabled?: boolean
}

// eslint-disable-next-line react-refresh/only-export-components
export const SETTINGS_MENU_ITEMS: SettingsMenuItem[] = [
  { key: 'account', label: '通用' },
  { key: 'usage', label: '用量', disabled: true },
  { key: 'permissions', label: '系统权限', disabled: true },
  { key: 'mcp', label: 'MCP 服务', disabled: true },
  { key: 'sso', label: 'SSO 集成', disabled: true },
  { key: 'shortcuts', label: '快捷键', disabled: true },
  { key: 'archived', label: '归档记录' },
  { key: 'about', label: '关于 AI 小家' },
]

interface SettingsMenuProps {
  activeKey: SettingsModalKey
  onSelect: (key: SettingsModalKey) => void
}

export function SettingsMenu({ activeKey, onSelect }: SettingsMenuProps) {
  return (
    <aside className="flex min-h-0 flex-col rounded-l-xl bg-secondary px-4 py-6">
      <div className="mb-2 shrink-0 text-lg font-bold text-foreground">设置</div>
      <div className="flex min-h-0 flex-1 flex-col gap-1 overflow-y-auto pr-1">
        {SETTINGS_MENU_ITEMS.filter((it) => !it.disabled).map((it) => {
          const active = it.key === activeKey
          return (
            <button
              key={it.key}
              type="button"
              aria-label={it.label}
              onClick={() => {
                onSelect(it.key)
              }}
              className={cn(
                'flex items-center rounded-md px-3 py-2.5 text-left text-sm',
                active
                  ? 'bg-card font-semibold text-foreground'
                  : 'font-medium text-muted-foreground transition-colors hover:bg-card/60',
              )}
            >
              <span className="min-w-0 flex-1 truncate">{it.label}</span>
            </button>
          )
        })}
      </div>
    </aside>
  )
}

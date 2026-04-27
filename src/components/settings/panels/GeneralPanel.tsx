import { useBrandingStore } from '@/stores/brandingStore'
import { useSettingsStore } from '@/stores/settingsStore'
import type { AppLanguage } from '@/i18n'
import { Button } from '@/components/ui/button'

const ACCENT_PRESETS = [
  '#DBAA22',
  '#4f46e5',
  '#0ea5e9',
  '#10b981',
  '#f43f5e',
  '#8b5cf6',
  '#f97316',
]

interface GeneralPanelProps {
  user: { name: string; tenantName: string; avatarUrl: string }
  onLogout: () => void
}

export function GeneralPanel({ user, onLogout }: GeneralPanelProps) {
  const accentColor = useBrandingStore((s) => s.accentColor)
  const applyBranding = useBrandingStore((s) => s.applyBranding)
  const appLanguage = useSettingsStore((s) => s.appLanguage)
  const setAppLanguage = useSettingsStore((s) => s.setAppLanguage)

  return (
    <div className="flex flex-col gap-6">
      {/* 用户信息卡 */}
      <div className="flex items-center gap-3.5 rounded-[14px] bg-secondary p-[18px]">
        <div className="h-12 w-12 shrink-0 overflow-hidden rounded-full bg-primary">
          {user.avatarUrl ? (
            <img src={user.avatarUrl} alt="" className="h-full w-full object-cover" />
          ) : (
            <span className="flex h-full w-full items-center justify-center text-lg font-semibold text-primary-foreground">
              {user.name.charAt(0).toUpperCase()}
            </span>
          )}
        </div>
        <div className="flex min-w-0 flex-1 flex-col gap-1">
          <div className="text-sm font-bold text-foreground">{user.name}</div>
          <div className="truncate text-[13px] text-muted-foreground">{user.tenantName}</div>
        </div>
        <Button variant="outline" onClick={onLogout}>
          退出登录
        </Button>
      </div>

      {/* 通用分组 */}
      <div className="flex flex-col gap-2">
        <div className="text-sm font-semibold text-foreground">通用</div>
        <div className="divide-y divide-border rounded-[14px] border border-border bg-card">
          {/* 语言 */}
          <div className="flex items-center justify-between px-4 py-3.5">
            <div className="flex flex-col gap-0.5">
              <span className="text-sm font-medium text-foreground">语言</span>
              <span className="text-xs text-muted-foreground">选择应用界面显示的语言</span>
            </div>
            <select
              aria-label="语言"
              value={appLanguage ?? 'zh-CN'}
              onChange={(e) => setAppLanguage(e.target.value as AppLanguage)}
              className="rounded-md border border-border bg-background px-3 py-1.5 text-sm text-foreground outline-none focus:ring-2 focus:ring-ring"
            >
              <option value="zh-CN">跟随系统（简体中文）</option>
              <option value="en-US">English</option>
            </select>
          </div>

          {/* 开机自启动（禁用） */}
          <div className="flex items-center justify-between px-4 py-3.5 opacity-50">
            <div className="flex flex-col gap-0.5">
              <div className="flex items-center gap-2">
                <span className="text-sm font-medium text-foreground">开机自启动</span>
                <span className="rounded bg-muted px-1.5 py-0.5 text-[10px] text-muted-foreground">即将支持</span>
              </div>
              <span className="text-xs text-muted-foreground">系统启动时自动运行</span>
            </div>
            <button
              role="switch"
              aria-checked={false}
              aria-label="开机自启动"
              disabled
              className="relative h-6 w-10 cursor-not-allowed rounded-full bg-muted"
            >
              <span className="absolute left-1 top-1 h-4 w-4 rounded-full bg-muted-foreground/40 shadow" />
            </button>
          </div>

          {/* 阻止休眠（禁用） */}
          <div className="flex items-center justify-between px-4 py-3.5 opacity-50">
            <div className="flex flex-col gap-0.5">
              <div className="flex items-center gap-2">
                <span className="text-sm font-medium text-foreground">任务运行时阻止自动休眠</span>
                <span className="rounded bg-muted px-1.5 py-0.5 text-[10px] text-muted-foreground">即将支持</span>
              </div>
              <span className="text-xs text-muted-foreground">任务处理期间阻止电脑因空闲自动进入休眠</span>
            </div>
            <button
              role="switch"
              aria-checked={false}
              aria-label="任务运行时阻止自动休眠"
              disabled
              className="relative h-6 w-10 cursor-not-allowed rounded-full bg-muted"
            >
              <span className="absolute left-1 top-1 h-4 w-4 rounded-full bg-muted-foreground/40 shadow" />
            </button>
          </div>
        </div>
      </div>

      {/* 外观分组 */}
      <div className="flex flex-col gap-2">
        <div className="text-sm font-semibold text-foreground">外观</div>
        <div className="rounded-[14px] border border-border bg-card">
          <div className="flex items-center justify-between px-4 py-3.5">
            <div className="flex flex-col gap-0.5">
              <span className="text-sm font-medium text-foreground">强调色</span>
              <span className="text-xs text-muted-foreground">选择界面的主题强调色</span>
            </div>
            <div className="flex items-center gap-2" role="radiogroup" aria-label="强调色">
              {ACCENT_PRESETS.map((color) => (
                <button
                  key={color}
                  role="radio"
                  aria-checked={accentColor === color}
                  aria-label={color}
                  onClick={() => applyBranding({ accentColor: color })}
                  className="h-6 w-6 rounded-full transition-transform hover:scale-110 focus:outline-none focus-visible:ring-2 focus-visible:ring-ring"
                  style={{
                    background: color,
                    outline: accentColor === color ? '2px solid currentColor' : 'none',
                    outlineOffset: '2px',
                  }}
                />
              ))}
            </div>
          </div>
        </div>
      </div>
    </div>
  )
}

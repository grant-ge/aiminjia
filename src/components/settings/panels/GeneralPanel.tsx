import { Select } from '@/components/common/Select'
import { Switch } from '@/components/common/Switch'
import { Button } from '@/components/ui/button'
import { getSettings, updateSettings } from '@/lib/tauri'
import { useBrandingStore } from '@/stores/brandingStore'
import { useSettingsStore } from '@/stores/settingsStore'
import type { FontScale } from '@/types/settings'

const ACCENT_PRESETS = [
  '#DBAA22',
  '#4f46e5',
  '#0ea5e9',
  '#10b981',
  '#f43f5e',
  '#8b5cf6',
  '#f97316',
]

const LANGUAGE_OPTIONS = [
  { value: 'zh-CN', label: '跟随系统（简体中文）' },
  { value: 'en-US', label: 'English' },
]

const FONT_SCALE_OPTIONS: Array<{ value: FontScale; label: string; description: string }> = [
  { value: 'small', label: '小', description: '14px' },
  { value: 'medium', label: '中', description: '16px' },
  { value: 'large', label: '大', description: '18px' },
]

interface GeneralPanelProps {
  user: { name: string; tenantName: string; avatarUrl: string }
  onLogout: () => void
}

function ComingSoonBadge() {
  return <span className="rounded bg-muted px-1.5 py-0.5 text-[0.625rem] text-muted-foreground">即将支持</span>
}

export function GeneralPanel({ user, onLogout }: GeneralPanelProps) {
  const accentColor = useBrandingStore((s) => s.accentColor)
  const applyBranding = useBrandingStore((s) => s.applyBranding)
  const appLanguage = useSettingsStore((s) => s.appLanguage)
  const fontScale = useSettingsStore((s) => s.fontScale ?? 'medium')
  const setFontScale = useSettingsStore((s) => s.setFontScale)

  const persistToBackend = async (patch: { fontScale?: FontScale; accentColor?: string }) => {
    try {
      const current = await getSettings()
      await updateSettings({ ...current, ...patch })
    } catch (err) {
      console.error('Failed to persist appearance settings:', err)
    }
  }

  const handleFontScaleChange = (value: FontScale) => {
    setFontScale(value)
    void persistToBackend({ fontScale: value })
  }

  const handleAccentChange = (color: string) => {
    applyBranding({ accentColor: color })
    void persistToBackend({ accentColor: color })
  }

  return (
    <div className="flex flex-col gap-5 text-foreground">
      <section className="flex items-center justify-between gap-8">
        <div className="flex min-w-0 items-center gap-4">
          <div className="h-12 w-12 shrink-0 overflow-hidden rounded-[14px] bg-primary">
            {user.avatarUrl ? (
              <img src={user.avatarUrl} alt="" className="h-full w-full object-cover" />
            ) : (
              <span className="flex h-full w-full items-center justify-center text-2xl font-semibold text-white">
                {(user.name.charAt(0) || '?').toUpperCase()}
              </span>
            )}
          </div>
          <div className="flex min-w-0 flex-col gap-2">
            <div className="text-base font-bold leading-none text-foreground">{user.name}</div>
            <div className="truncate text-sm leading-none text-muted-foreground">{user.tenantName}</div>
          </div>
        </div>
        <Button variant="outline" className="h-10 rounded-[12px] px-5 text-sm font-semibold" onClick={onLogout}>
          退出登录
        </Button>
      </section>

      <div className="h-px bg-border mb-2" />

      <section className="flex flex-col gap-4">
        <div className="text-xl font-bold tracking-tight text-foreground">通用</div>

        <div className="flex items-center justify-between gap-8 opacity-60">
          <div className="flex min-w-0 flex-col gap-1">
            <div className="flex items-center gap-2">
              <span className="text-base font-semibold text-foreground">语言</span>
              <ComingSoonBadge />
            </div>
            <div className="text-sm text-muted-foreground">选择应用界面显示的语言</div>
          </div>
          <Select aria-label="语言" value={appLanguage ?? 'zh-CN'} options={LANGUAGE_OPTIONS} disabled />
        </div>

        <div className="flex items-center justify-between gap-8 opacity-60">
          <div className="flex min-w-0 flex-col gap-1">
            <div className="flex items-center gap-2">
              <span className="text-sm font-semibold text-foreground">开机自启动</span>
              <ComingSoonBadge />
            </div>
            <div className="text-sm text-muted-foreground">系统启动时自动运行</div>
          </div>
          <Switch aria-label="开机自启动" checked={false} disabled />
        </div>

        <div className="flex items-center justify-between gap-8 opacity-60">
          <div className="flex min-w-0 flex-col gap-1">
            <div className="flex items-center gap-2">
              <span className="text-sm font-semibold text-foreground">任务运行时阻止自动休眠</span>
              <ComingSoonBadge />
            </div>
            <div className="text-sm text-muted-foreground">任务处理期间阻止电脑因空闲自动进入休眠</div>
          </div>
          <Switch aria-label="任务运行时阻止自动休眠" checked={false} disabled />
        </div>
      </section>

      <div className="h-px bg-border mb-2" />

      <section className="flex flex-col gap-4 pb-2">
        <div className="text-xl font-bold tracking-tight text-foreground">外观</div>

        <div className="flex items-center justify-between gap-8">
          <div className="flex min-w-0 flex-col gap-1">
            <div className="text-base font-semibold text-foreground">字体大小</div>
            <div className="text-sm text-muted-foreground">调整界面文字和间距的整体缩放</div>
          </div>
          <div className="inline-flex rounded-[12px] bg-muted p-1" role="radiogroup" aria-label="字体大小">
            {FONT_SCALE_OPTIONS.map((option) => {
              const selected = fontScale === option.value
              return (
                <button
                  key={option.value}
                  type="button"
                  role="radio"
                  aria-checked={selected}
                  aria-label={option.label}
                  title={option.description}
                  onClick={() => handleFontScaleChange(option.value)}
                  className={
                    selected
                      ? 'rounded-[10px] bg-card px-3 py-1.5 text-sm font-semibold text-foreground shadow-sm'
                      : 'rounded-[10px] px-3 py-1.5 text-sm font-medium text-muted-foreground transition-colors hover:text-foreground'
                  }
                >
                  {option.label}
                </button>
              )
            })}
          </div>
        </div>

        <div className="flex items-center justify-between gap-8">
          <div className="flex min-w-0 flex-col gap-1">
            <div className="text-base font-semibold text-foreground">强调色</div>
            <div className="text-sm text-muted-foreground">选择界面的主题强调色</div>
          </div>
          <div className="flex items-center gap-2" role="radiogroup" aria-label="强调色">
            {ACCENT_PRESETS.map((color) => (
              <button
                key={color}
                role="radio"
                aria-checked={accentColor === color}
                aria-label={color}
                onClick={() => handleAccentChange(color)}
                className="h-6 w-6 rounded-full transition-transform hover:scale-110 focus:outline-none focus-visible:ring-2 focus-visible:ring-ring"
                style={{
                  background: color,
                  outline: accentColor === color ? '2px solid #cbcbcb' : 'none',
                  outlineOffset: '2px',
                }}
              />
            ))}
          </div>
        </div>
      </section>
    </div>
  )
}

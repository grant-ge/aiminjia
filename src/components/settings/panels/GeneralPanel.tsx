import { Button } from '@/components/ui/button'
import { getSettings, updateSettings } from '@/lib/tauri'
import { useSettingsStore } from '@/stores/settingsStore'
import type { FontScale } from '@/types/settings'

const FONT_SCALE_OPTIONS: Array<{ value: FontScale; label: string; description: string }> = [
  { value: 'small', label: '小', description: '14px' },
  { value: 'medium', label: '中', description: '16px' },
  { value: 'large', label: '大', description: '18px' },
]

interface GeneralPanelProps {
  user: { name: string; tenantName: string; avatarUrl: string }
  onLogout: () => void
}

export function GeneralPanel({ user, onLogout }: GeneralPanelProps) {
  const fontScale = useSettingsStore((s) => s.fontScale ?? 'medium')
  const setFontScale = useSettingsStore((s) => s.setFontScale)

  const persistToBackend = async (patch: { fontScale?: FontScale }) => {
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

  return (
    <div className="flex flex-col gap-5 text-foreground">
      <section className="flex items-center justify-between gap-8">
        <div className="flex min-w-0 items-center gap-4">
          <div className="h-12 w-12 shrink-0 overflow-hidden rounded-[14px] bg-primary">
            {user.avatarUrl ? (
              <img src={user.avatarUrl} alt="" className="h-full w-full object-cover" />
            ) : (
              <span className="flex h-full w-full items-center justify-center text-2xl font-semibold text-primary-foreground">
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
      </section>
    </div>
  )
}

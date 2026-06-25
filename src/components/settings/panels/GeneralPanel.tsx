import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { convertFileSrc } from '@tauri-apps/api/core'
import { getSettings, saveProfileAvatarImage, updateSettings } from '@/lib/tauri'
import { useBrandingStore } from '@/stores/brandingStore'
import { useSettingsStore } from '@/stores/settingsStore'
import type { ChatWidthMode, FontScale, ProfileAvatarMode } from '@/types/settings'
import type { AppLanguage } from '@/i18n'
import { Button } from '@/components/ui/button'

interface GeneralPanelProps {
  user: { name: string; tenantName: string; avatarUrl: string }
  onLogout: () => void
}

const DEFAULT_PROFILE_EMOJI = '🐱'

const PROFILE_EMOJIS = [
  '🐱', '🐶', '🦊', '🐻', '🐼', '🐨', '🦁', '🐸', '🐵', '🦉',
  '🦋', '🐙', '⭐', '🌙', '☀️', '🌈', '⚡', '🎯', '🚀', '💡',
  '🎨', '🎮', '📚', '☕', '🐧', '🐢', '🦆', '🐝', '🦄', '🦒',
  '🍄', '🌻', '🪴', '✈️', '🎹', '🎸', '🎬', '📷', '🎤', '🥁',
  '🧩', '🚲', '🍕', '🍜', '🧁', '🍦', '⚽', '🏀', '🎾', '🏓',
  '🧸', '🎁', '👾', '🤖', '🏔️', '🌋', '🎪', '🔭', '💎', '🧠',
]

function normalizeProfileAvatarMode(value: unknown): ProfileAvatarMode {
  return value === 'emoji' || value === 'image' ? value : 'initial'
}

export function GeneralPanel({ user, onLogout }: GeneralPanelProps) {
  const { t, i18n } = useTranslation()
  const [isAvatarUploading, setIsAvatarUploading] = useState(false)
  const [avatarUploadError, setAvatarUploadError] = useState<string | null>(null)
  const [avatarImageLoadFailed, setAvatarImageLoadFailed] = useState(false)
  const fontScale = useSettingsStore((s) => s.fontScale ?? 'medium')
  const setFontScale = useSettingsStore((s) => s.setFontScale)
  const chatWidthMode = useSettingsStore((s) => s.chatWidthMode ?? 'full')
  const setChatWidthMode = useSettingsStore((s) => s.setChatWidthMode)
  const profileAvatarMode = useSettingsStore((s) => normalizeProfileAvatarMode(s.profileAvatarMode))
  const profileAvatarEmoji = useSettingsStore((s) => s.profileAvatarEmoji || DEFAULT_PROFILE_EMOJI)
  const profileAvatarImagePath = useSettingsStore((s) => s.profileAvatarImagePath ?? '')
  const setProfileAvatar = useSettingsStore((s) => s.setProfileAvatar)
  const productName = useBrandingStore((s) => s.productName)
  const appLanguage: AppLanguage = i18n.language === 'en-US' ? 'en-US' : 'zh-CN'
  const setAppLanguage = useSettingsStore((s) => s.setAppLanguage)
  const accountSubtitle = productName.trim() || user.tenantName
  const initialAvatarText = (user.name.charAt(0) || '?').toUpperCase()
  const imageAvatarSrc =
    profileAvatarMode === 'image' && profileAvatarImagePath.trim() && !avatarImageLoadFailed
      ? convertFileSrc(profileAvatarImagePath.trim())
      : ''

  useEffect(() => {
    setAvatarImageLoadFailed(false)
  }, [profileAvatarMode, profileAvatarImagePath])

  const persistToBackend = async (patch: {
    fontScale?: FontScale
    appLanguage?: AppLanguage
    chatWidthMode?: ChatWidthMode
    profileAvatarMode?: ProfileAvatarMode
    profileAvatarEmoji?: string
    profileAvatarImagePath?: string
  }) => {
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

  const handleLanguageChange = (value: AppLanguage) => {
    setAppLanguage(value)
    void persistToBackend({ appLanguage: value })
  }

  const handleChatWidthModeChange = (value: ChatWidthMode) => {
    setChatWidthMode(value)
    void persistToBackend({ chatWidthMode: value })
  }

  const handleInitialAvatar = () => {
    setProfileAvatar({ mode: 'initial' })
    void persistToBackend({ profileAvatarMode: 'initial' })
  }

  const handleEmojiAvatarMode = () => {
    const emoji = profileAvatarEmoji || DEFAULT_PROFILE_EMOJI
    setAvatarUploadError(null)
    setProfileAvatar({ mode: 'emoji', emoji })
    void persistToBackend({ profileAvatarMode: 'emoji', profileAvatarEmoji: emoji })
  }

  const handleEmojiAvatar = (emoji: string) => {
    setAvatarUploadError(null)
    setProfileAvatar({ mode: 'emoji', emoji })
    void persistToBackend({ profileAvatarMode: 'emoji', profileAvatarEmoji: emoji })
  }

  const handleImageAvatarMode = () => {
    setAvatarUploadError(null)
    setProfileAvatar({ mode: 'image' })
    void persistToBackend({ profileAvatarMode: 'image' })
  }

  const handleImageAvatar = async () => {
    setAvatarUploadError(null)
    setIsAvatarUploading(true)
    try {
      const { open } = await import('@tauri-apps/plugin-dialog')
      const selected = await open({
        multiple: false,
        directory: false,
        filters: [
          {
            name: t('settings.general.avatarImageFiles'),
            extensions: ['png', 'jpg', 'jpeg', 'webp', 'gif', 'bmp'],
          },
        ],
      })
      const filePath = Array.isArray(selected) ? selected[0] : selected
      if (!filePath) return

      const copiedPath = await saveProfileAvatarImage(filePath)
      setAvatarImageLoadFailed(false)
      setProfileAvatar({ mode: 'image', imagePath: copiedPath })
      void persistToBackend({
        profileAvatarMode: 'image',
        profileAvatarImagePath: copiedPath,
      })
    } catch (err) {
      console.error('Failed to update profile avatar image:', err)
      setAvatarUploadError(err instanceof Error ? err.message : String(err))
    } finally {
      setIsAvatarUploading(false)
    }
  }

  const FONT_SCALE_OPTIONS: Array<{ value: FontScale; description: string; labelKey: string }> = [
    { value: 'small', description: '12px', labelKey: 'settings.general.fontSmall' },
    { value: 'medium', description: '13px', labelKey: 'settings.general.fontMedium' },
    { value: 'large', description: '14px', labelKey: 'settings.general.fontLarge' },
  ]

  const LANGUAGE_OPTIONS: Array<{ value: AppLanguage; label: string }> = [
    { value: 'zh-CN', label: t('settings.general.languageZh') },
    { value: 'en-US', label: t('settings.general.languageEn') },
  ]

  const CHAT_WIDTH_OPTIONS: Array<{ value: ChatWidthMode; labelKey: string }> = [
    { value: 'centered', labelKey: 'settings.general.chatWidthCentered' },
    { value: 'full', labelKey: 'settings.general.chatWidthFull' },
  ]
  const previewAvatarBackground = imageAvatarSrc ? 'transparent' : 'rgba(var(--primary-rgb), 0.12)'

  return (
    <div className="flex flex-col gap-5 text-foreground">
      <section className="flex items-center justify-between gap-8">
        <div className="flex min-w-0 items-center gap-4">
          <div
            data-testid="settings-profile-avatar-preview"
            style={{ background: previewAvatarBackground }}
            className="flex h-12 w-12 shrink-0 items-center justify-center overflow-hidden rounded-md text-primary"
          >
            {imageAvatarSrc ? (
              <img
                src={imageAvatarSrc}
                alt={t('settings.general.currentAvatar')}
                className="h-full w-full object-cover"
                onError={() => setAvatarImageLoadFailed(true)}
              />
            ) : profileAvatarMode === 'emoji' ? (
              <span className="text-2xl leading-none">{profileAvatarEmoji}</span>
            ) : (
              <span className="text-2xl font-semibold leading-none">{initialAvatarText}</span>
            )}
          </div>
          <div className="flex min-w-0 flex-col gap-2">
            <div className="text-base font-bold leading-none text-foreground">{user.name}</div>
            <div className="truncate text-sm leading-none text-muted-foreground">{accountSubtitle}</div>
          </div>
        </div>
        <Button variant="outline" data-aijia-logout-button onClick={onLogout}>
          {t('settings.general.logout')}
        </Button>
      </section>

      <div className="h-px bg-border mb-2" />

      <section className="flex flex-col gap-4 pb-2">
        <div className="text-xl font-bold text-foreground">{t('settings.general.profile')}</div>

        <div className="flex flex-col gap-3">
          <div className="text-base font-semibold text-foreground">{t('settings.general.avatarIcon')}</div>
          <div
            className="flex flex-col gap-3"
            role="radiogroup"
            aria-label={t('settings.general.avatarIcon')}
          >
            <Button unstyled
              type="button"
              role="radio"
              aria-checked={profileAvatarMode === 'initial'}
              aria-label={t('settings.general.avatarInitial')}
              onClick={() => {
                setAvatarUploadError(null)
                handleInitialAvatar()
              }}
              className="flex items-center gap-3 rounded-md px-1 py-1 text-left text-foreground transition-colors hover:bg-muted/70"
            >
              <span className={profileAvatarMode === 'initial' ? 'h-4 w-4 rounded-full border-4 border-primary' : 'h-4 w-4 rounded-full border border-border bg-background'} />
              <span className="text-base font-medium">{t('settings.general.avatarInitial')}</span>
            </Button>

            <Button unstyled
              type="button"
              role="radio"
              aria-checked={profileAvatarMode === 'emoji'}
              aria-label="Emoji"
              onClick={handleEmojiAvatarMode}
              className="flex items-center gap-3 rounded-md px-1 py-1 text-left text-foreground transition-colors hover:bg-muted/70"
            >
              <span className={profileAvatarMode === 'emoji' ? 'h-4 w-4 rounded-full border-4 border-primary' : 'h-4 w-4 rounded-full border border-border bg-background'} />
              <span className="text-base font-medium">Emoji</span>
            </Button>

            {profileAvatarMode === 'emoji' ? (
              <div className="grid grid-cols-[repeat(auto-fill,minmax(36px,1fr))] gap-2 pl-7">
                {PROFILE_EMOJIS.map((emoji) => {
                  const selected = profileAvatarEmoji === emoji
                  return (
                    <Button unstyled
                      key={emoji}
                      type="button"
                      aria-label={`${t('settings.general.chooseAvatar')} ${emoji}`}
                      onClick={() => handleEmojiAvatar(emoji)}
                      className={
                        selected
                          ? 'flex h-9 w-9 items-center justify-center rounded-md bg-primary/10 text-xl ring-1 ring-primary'
                          : 'flex h-9 w-9 items-center justify-center rounded-md text-xl transition-colors hover:bg-muted'
                      }
                    >
                      {emoji}
                    </Button>
                  )
                })}
              </div>
            ) : null}

            <Button unstyled
              type="button"
              role="radio"
              aria-checked={profileAvatarMode === 'image'}
              aria-label={t('settings.general.avatarUpload')}
              onClick={handleImageAvatarMode}
              className="flex items-center gap-3 rounded-md px-1 py-1 text-left text-foreground transition-colors hover:bg-muted/70"
            >
              <span className={profileAvatarMode === 'image' ? 'h-4 w-4 rounded-full border-4 border-primary' : 'h-4 w-4 rounded-full border border-border bg-background'} />
              <span className="text-base font-medium">{t('settings.general.avatarUpload')}</span>
            </Button>
            {profileAvatarMode === 'image' ? (
              <div className="flex flex-col items-start gap-2 pl-7">
                <Button
                  type="button"
                  variant="outline"
                  onClick={() => void handleImageAvatar()}
                  disabled={isAvatarUploading}
                >
                  {isAvatarUploading ? t('settings.general.avatarUploading') : t('settings.general.avatarChooseImage')}
                </Button>
                {avatarUploadError ? (
                  <div className="text-sm text-destructive">{avatarUploadError}</div>
                ) : null}
              </div>
            ) : null}
          </div>
        </div>
      </section>

      <div className="h-px bg-border mb-2" />

      <section className="flex flex-col gap-4 pb-2">
        <div className="text-xl font-bold text-foreground">{t('settings.general.appearance')}</div>

        <div className="flex items-center justify-between gap-8">
          <div className="flex min-w-0 flex-col gap-1">
            <div className="text-base font-semibold text-foreground">{t('settings.general.fontSize')}</div>
            <div className="text-sm text-muted-foreground">{t('settings.general.fontSizeDesc')}</div>
          </div>
          <div
            className="inline-flex rounded-md bg-muted p-1"
            role="radiogroup"
            aria-label={t('settings.general.fontSize')}
          >
            {FONT_SCALE_OPTIONS.map((option) => {
              const selected = fontScale === option.value
              const label = t(option.labelKey)
              return (
                <Button unstyled
                  key={option.value}
                  type="button"
                  role="radio"
                  aria-checked={selected}
                  aria-label={label}
                  title={option.description}
                  onClick={() => handleFontScaleChange(option.value)}
                  className={
                    selected
                      ? 'rounded-md bg-card px-3 py-1.5 text-sm font-semibold text-foreground shadow-sm'
                      : 'rounded-md px-3 py-1.5 text-sm font-medium text-muted-foreground transition-colors hover:text-foreground'
                  }
                >
                  {label}
                </Button>
              )
            })}
          </div>
        </div>

        <div className="flex items-center justify-between gap-8">
          <div className="flex min-w-0 flex-col gap-1">
            <div className="text-base font-semibold text-foreground">{t('settings.general.chatWidth')}</div>
            <div className="text-sm text-muted-foreground">{t('settings.general.chatWidthDesc')}</div>
          </div>
          <div
            className="inline-flex rounded-md bg-muted p-1"
            role="radiogroup"
            aria-label={t('settings.general.chatWidth')}
          >
            {CHAT_WIDTH_OPTIONS.map((option) => {
              const selected = chatWidthMode === option.value
              const label = t(option.labelKey)
              return (
                <Button unstyled
                  key={option.value}
                  type="button"
                  role="radio"
                  aria-checked={selected}
                  aria-label={label}
                  onClick={() => handleChatWidthModeChange(option.value)}
                  className={
                    selected
                      ? 'rounded-md bg-card px-3 py-1.5 text-sm font-semibold text-foreground shadow-sm'
                      : 'rounded-md px-3 py-1.5 text-sm font-medium text-muted-foreground transition-colors hover:text-foreground'
                  }
                >
                  {label}
                </Button>
              )
            })}
          </div>
        </div>

        <div className="flex items-center justify-between gap-8">
          <div className="flex min-w-0 flex-col gap-1">
            <div className="text-base font-semibold text-foreground">
              {t('settings.general.language')}
            </div>
            <div className="text-sm text-muted-foreground">
              {t('settings.general.languageDesc')}
            </div>
          </div>
          <div
            className="inline-flex rounded-md bg-muted p-1"
            role="radiogroup"
            aria-label={t('settings.general.language')}
            data-testid="settings-language-switch"
          >
            {LANGUAGE_OPTIONS.map((option) => {
              const selected = appLanguage === option.value
              return (
                <Button unstyled
                  key={option.value}
                  type="button"
                  role="radio"
                  aria-checked={selected}
                  aria-label={option.label}
                  onClick={() => handleLanguageChange(option.value)}
                  className={
                    selected
                      ? 'rounded-md bg-card px-3 py-1.5 text-sm font-semibold text-foreground shadow-sm'
                      : 'rounded-md px-3 py-1.5 text-sm font-medium text-muted-foreground transition-colors hover:text-foreground'
                  }
                >
                  {option.label}
                </Button>
              )
            })}
          </div>
        </div>
      </section>
    </div>
  )
}

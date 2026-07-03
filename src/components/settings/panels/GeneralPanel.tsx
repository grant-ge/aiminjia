import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { convertFileSrc } from '@tauri-apps/api/core'
import { LogOut } from 'lucide-react'
import { getSettings, saveProfileAvatarImage, setImChannelKeepAwake, updateSettings } from '@/lib/tauri'
import { useBrandingStore } from '@/stores/brandingStore'
import { useSettingsStore } from '@/stores/settingsStore'
import type { ChatWidthMode, FontScale, ProfileAvatarMode } from '@/types/settings'
import type { AppLanguage } from '@/i18n'
import { Button } from '@/components/ui/button'
import { SegmentedControl } from '@/components/common/SegmentedControl'

interface GeneralPanelProps {
  user: { name: string; accountName?: string; tenantName: string; avatarUrl: string }
  onLogout: () => void
  section?: 'profile' | 'system' | 'all'
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

function takeQueuedAvatarImagePath(): string | null {
  if (!(import.meta.env.DEV || import.meta.env.VITE_E2E_ENABLED === 'true')) return null
  return (
    window as unknown as {
      __aijia?: { _pickAvatarImageMockQueue?: string[] }
    }
  ).__aijia?._pickAvatarImageMockQueue?.shift() ?? null
}

function normalizeProfileAvatarMode(value: unknown): ProfileAvatarMode {
  return value === 'emoji' || value === 'image' ? value : 'initial'
}

export function GeneralPanel({ user, onLogout, section = 'all' }: GeneralPanelProps) {
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
  const imChannelKeepAwakeEnabled = useSettingsStore((s) => Boolean(s.imChannelKeepAwakeEnabled))
  const setImChannelKeepAwakeEnabled = useSettingsStore((s) => s.setImChannelKeepAwakeEnabled)
  const productName = useBrandingStore((s) => s.productName)
  const appLanguage: AppLanguage = i18n.language === 'en-US' ? 'en-US' : 'zh-CN'
  const setAppLanguage = useSettingsStore((s) => s.setAppLanguage)
  const accountSubtitle = productName.trim() || user.tenantName
  const accountName = user.accountName?.trim() || user.name
  const organizationName = user.tenantName.trim() || accountSubtitle
  const keepAwakeProductName = productName.trim() || t('sidebar.account.productFallback')
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
    imChannelKeepAwakeEnabled?: boolean
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

  const handleImChannelKeepAwakeChange = (value: 'off' | 'on') => {
    const enabled = value === 'on'
    setImChannelKeepAwakeEnabled(enabled)
    void setImChannelKeepAwake(enabled).catch((err) => {
      console.error('Failed to apply IM channel keep-awake setting:', err)
    })
    void persistToBackend({ imChannelKeepAwakeEnabled: enabled })
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
      const selected = takeQueuedAvatarImagePath() ?? await (async () => {
        const { open } = await import('@tauri-apps/plugin-dialog')
        return open({
          multiple: false,
          directory: false,
          filters: [
            {
              name: t('settings.general.avatarImageFiles'),
              extensions: ['png', 'jpg', 'jpeg', 'webp', 'gif', 'bmp'],
            },
          ],
        })
      })()
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
  const IM_KEEP_AWAKE_OPTIONS: Array<{ value: 'off' | 'on'; labelKey: string }> = [
    { value: 'off', labelKey: 'settings.general.switchOff' },
    { value: 'on', labelKey: 'settings.general.switchOn' },
  ]
  const previewAvatarBackground = imageAvatarSrc ? 'transparent' : 'rgba(var(--primary-rgb), 0.12)'
  const showProfile = section !== 'system'
  const showSystem = section !== 'profile'

  return (
    <div className="flex flex-col gap-5 text-foreground">
      {showProfile ? (
        <>
          <section className="flex flex-col gap-2 pb-1">
            <div className="text-xl font-bold text-foreground">{t('settings.tabs.general')}</div>
            <div className="max-w-2xl text-sm leading-6 text-muted-foreground">
              {t('settings.profile.description')}
            </div>
          </section>

          <section className="rounded-md border border-border bg-card">
            <div className="flex items-center gap-4 border-b border-border px-4 py-4">
              <div
                data-aijia-profile-avatar-preview
                data-testid="settings-profile-avatar-preview"
                style={{ background: previewAvatarBackground }}
                className="flex h-14 w-14 shrink-0 items-center justify-center overflow-hidden rounded-full border border-border text-primary"
              >
                {imageAvatarSrc ? (
                  <img
                    data-aijia-profile-avatar-image
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
              <div className="flex min-w-0 flex-col gap-1.5">
                <div className="truncate text-base font-bold leading-none text-foreground">{user.name}</div>
                <div className="truncate text-sm leading-none text-muted-foreground">{accountSubtitle}</div>
              </div>
            </div>

            <div className="flex flex-col gap-4 px-4 py-4">
              <div className="flex flex-col gap-1">
                <div className="text-base font-semibold text-foreground">{t('settings.profile.avatarTitle')}</div>
                <div className="text-sm leading-5 text-muted-foreground">{t('settings.profile.avatarDesc')}</div>
              </div>
              <div
                className="flex flex-col gap-2"
                role="radiogroup"
                aria-label={t('settings.general.avatarIcon')}
              >
                <Button unstyled
                  type="button"
                  role="radio"
                  data-aijia-profile-avatar-action="select-initial"
                  aria-checked={profileAvatarMode === 'initial'}
                  aria-label={t('settings.general.avatarInitial')}
                  onClick={() => {
                    setAvatarUploadError(null)
                    handleInitialAvatar()
                  }}
                  className="flex items-center gap-3 rounded px-1 py-0.5 text-left text-foreground transition-colors hover:bg-[rgba(var(--muted-rgb),0.70)]"
                >
                  <span className={profileAvatarMode === 'initial' ? 'h-4 w-4 rounded-full border-4 border-primary' : 'h-4 w-4 rounded-full border border-border bg-background'} />
                  <span className="text-base font-medium">{t('settings.general.avatarInitial')}</span>
                </Button>

                <Button unstyled
                  type="button"
                  role="radio"
                  data-aijia-profile-avatar-action="select-emoji"
                  aria-checked={profileAvatarMode === 'emoji'}
                  aria-label="Emoji"
                  onClick={handleEmojiAvatarMode}
                  className="flex items-center gap-3 rounded px-1 py-0.5 text-left text-foreground transition-colors hover:bg-[rgba(var(--muted-rgb),0.70)]"
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
                              ? 'flex h-9 w-9 items-center justify-center rounded-md bg-[rgba(var(--primary-rgb),0.10)] text-xl ring-1 ring-primary'
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
                  data-aijia-profile-avatar-action="select-image"
                  aria-checked={profileAvatarMode === 'image'}
                  aria-label={t('settings.general.avatarUpload')}
                  onClick={handleImageAvatarMode}
                  className="flex items-center gap-3 rounded px-1 py-0.5 text-left text-foreground transition-colors hover:bg-[rgba(var(--muted-rgb),0.70)]"
                >
                  <span className={profileAvatarMode === 'image' ? 'h-4 w-4 rounded-full border-4 border-primary' : 'h-4 w-4 rounded-full border border-border bg-background'} />
                  <span className="text-base font-medium">{t('settings.general.avatarUpload')}</span>
                </Button>
                {profileAvatarMode === 'image' ? (
                  <div className="flex flex-col items-start gap-1.5 pl-7">
                    <Button
                      type="button"
                      variant="outline"
                      data-aijia-profile-avatar-action="choose-image"
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

          <section className="rounded-md border border-border bg-card">
            <div className="divide-y divide-border">
              <div className="flex items-center justify-between gap-8 px-4 py-4">
                <div className="flex min-w-0 flex-col gap-1">
                  <div className="text-base font-semibold text-foreground">{t('settings.profile.accountTitle')}</div>
                  <div className="text-sm text-muted-foreground">{t('settings.profile.accountDesc')}</div>
                </div>
                <div className="max-w-[280px] truncate text-right text-sm font-semibold text-foreground">
                  {accountName}
                </div>
              </div>

              <div className="flex items-center justify-between gap-8 px-4 py-4">
                <div className="flex min-w-0 flex-col gap-1">
                  <div className="text-base font-semibold text-foreground">
                    {t('settings.profile.organizationTitle')}
                  </div>
                  <div className="text-sm text-muted-foreground">
                    {t('settings.profile.organizationDesc')}
                  </div>
                </div>
                <div className="max-w-[280px] truncate text-right text-sm font-semibold text-foreground">
                  {organizationName}
                </div>
              </div>

              <div className="flex items-center justify-between gap-8 px-4 py-4">
                <div className="flex min-w-0 flex-col gap-1">
                  <div className="text-base font-semibold text-foreground">{t('settings.general.logout')}</div>
                  <div className="text-sm text-muted-foreground">{t('settings.profile.logoutDesc')}</div>
                </div>
                <Button
                  variant="destructive"
                  icon={<LogOut />}
                  data-aijia-logout-button
                  onClick={onLogout}
                >
                  {t('settings.general.logout')}
                </Button>
              </div>
            </div>
          </section>
        </>
      ) : null}

      {showProfile && showSystem ? <div className="h-px bg-border mb-2" /> : null}

      {showSystem ? (
        <section className="flex flex-col gap-5 pb-2">
          <div className="flex flex-col gap-2">
            <div className="text-xl font-bold text-foreground">{t('settings.tabs.system')}</div>
            <div className="max-w-2xl text-sm leading-6 text-muted-foreground">
              {t('settings.system.description')}
            </div>
          </div>

          <section className="rounded-md border border-border bg-card">
            <div className="border-b border-border bg-[rgba(var(--muted-rgb),0.25)] px-4 py-3">
              <div className="flex gap-3">
                <span className="mt-1 h-8 w-1 shrink-0 rounded-full bg-[rgba(var(--primary-rgb),0.70)]" aria-hidden="true" />
                <div className="min-w-0">
                  <h3 className="text-sm font-bold leading-5 text-foreground">
                    {t('settings.system.interfaceTitle')}
                  </h3>
                  <div className="mt-0.5 text-sm leading-5 text-muted-foreground">
                    {t('settings.system.interfaceDesc')}
                  </div>
                </div>
              </div>
            </div>

            <div className="divide-y divide-border">
              <div className="flex items-center justify-between gap-8 px-4 py-4">
                <div className="flex min-w-0 flex-col gap-1">
                  <div className="text-base font-semibold text-foreground">{t('settings.general.fontSize')}</div>
                  <div className="text-sm text-muted-foreground">{t('settings.general.fontSizeDesc')}</div>
                </div>
                <SegmentedControl<FontScale>
                  ariaLabel={t('settings.general.fontSize')}
                  value={fontScale}
                  onValueChange={handleFontScaleChange}
                  options={FONT_SCALE_OPTIONS.map((option) => ({
                    value: option.value,
                    label: t(option.labelKey),
                    title: option.description,
                  }))}
                />
              </div>

              <div className="flex items-center justify-between gap-8 px-4 py-4">
                <div className="flex min-w-0 flex-col gap-1">
                  <div className="text-base font-semibold text-foreground">{t('settings.general.chatWidth')}</div>
                  <div className="text-sm text-muted-foreground">{t('settings.general.chatWidthDesc')}</div>
                </div>
                <SegmentedControl<ChatWidthMode>
                  ariaLabel={t('settings.general.chatWidth')}
                  value={chatWidthMode}
                  onValueChange={handleChatWidthModeChange}
                  options={CHAT_WIDTH_OPTIONS.map((option) => ({
                    value: option.value,
                    label: t(option.labelKey),
                  }))}
                />
              </div>

              <div className="flex items-center justify-between gap-8 px-4 py-4">
                <div className="flex min-w-0 flex-col gap-1">
                  <div className="text-base font-semibold text-foreground">
                    {t('settings.general.language')}
                  </div>
                  <div className="text-sm text-muted-foreground">
                    {t('settings.general.languageDesc')}
                  </div>
                </div>
                <SegmentedControl<AppLanguage>
                  ariaLabel={t('settings.general.language')}
                  value={appLanguage}
                  onValueChange={handleLanguageChange}
                  testId="settings-language-switch"
                  options={LANGUAGE_OPTIONS}
                />
              </div>
            </div>
          </section>

          <section className="rounded-md border border-border bg-card">
            <div className="border-b border-border bg-[rgba(var(--muted-rgb),0.25)] px-4 py-3">
              <div className="flex gap-3">
                <span className="mt-1 h-8 w-1 shrink-0 rounded-full bg-[rgba(var(--primary-rgb),0.70)]" aria-hidden="true" />
                <div className="min-w-0">
                  <h3 className="text-sm font-bold leading-5 text-foreground">
                    {t('settings.system.runtimeTitle')}
                  </h3>
                  <div className="mt-0.5 text-sm leading-5 text-muted-foreground">
                    {t('settings.system.runtimeDesc')}
                  </div>
                </div>
              </div>
            </div>

            <div className="px-4 py-4">
              <div className="flex items-center justify-between gap-8">
                <div className="flex min-w-0 flex-col gap-1">
                  <div className="text-base font-semibold text-foreground">{t('settings.general.imChannelKeepAwake')}</div>
                  <div className="text-sm text-muted-foreground">
                    {t('settings.general.imChannelKeepAwakeDesc', { productName: keepAwakeProductName })}
                  </div>
                </div>
                <SegmentedControl<'off' | 'on'>
                  ariaLabel={t('settings.general.imChannelKeepAwake')}
                  value={imChannelKeepAwakeEnabled ? 'on' : 'off'}
                  onValueChange={handleImChannelKeepAwakeChange}
                  options={IM_KEEP_AWAKE_OPTIONS.map((option) => ({
                    value: option.value,
                    label: t(option.labelKey),
                  }))}
                />
              </div>
            </div>
          </section>
        </section>
      ) : null}
    </div>
  )
}

import { useCallback, useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Button } from '@/components/common/Button'
import { listMarketplaceSkills, installMarketplaceSkill } from '@/lib/tauri'
import type { MarketplaceSkillItem } from '@/lib/tauri'
import { useNotificationStore } from '@/stores/notificationStore'

const CATEGORIES = [
  { key: '', label: 'allCategories' },
  { key: 'hr', label: 'HR' },
  { key: 'finance', label: 'Finance' },
  { key: 'legal', label: 'Legal' },
  { key: 'sales', label: 'Sales' },
  { key: 'ops', label: 'Ops' },
  { key: 'general', label: 'General' },
] as const

const CATEGORY_LABELS: Record<string, Record<string, string>> = {
  'zh-CN': { '': '全部', hr: 'HR', finance: '财务', legal: '法务', sales: '销售', ops: '运营', general: '通用' },
  'en-US': { '': 'All', hr: 'HR', finance: 'Finance', legal: 'Legal', sales: 'Sales', ops: 'Ops', general: 'General' },
}

const PAGE_SIZE = 20

export function SkillMarketplace() {
  const { t, i18n } = useTranslation()
  const pushNotification = useNotificationStore((s) => s.push)

  const [items, setItems] = useState<MarketplaceSkillItem[]>([])
  const [total, setTotal] = useState(0)
  const [page, setPage] = useState(1)
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)
  const [category, setCategory] = useState('')
  const [search, setSearch] = useState('')
  const [installingId, setInstallingId] = useState<number | null>(null)

  const searchTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null)

  const loadSkills = useCallback(async (p: number, cat: string, q: string) => {
    setLoading(true)
    setError(null)
    try {
      const resp = await listMarketplaceSkills(p, PAGE_SIZE, cat || undefined, q || undefined)
      setItems(resp.items)
      setTotal(resp.total)
      setPage(resp.page)
    } catch (e) {
      setError(String(e))
      setItems([])
    }
    setLoading(false)
  }, [])

  useEffect(() => {
    loadSkills(1, category, search)
  }, [category, loadSkills])

  const handleSearchChange = (value: string) => {
    setSearch(value)
    if (searchTimerRef.current) clearTimeout(searchTimerRef.current)
    searchTimerRef.current = setTimeout(() => {
      loadSkills(1, category, value)
    }, 400)
  }

  const handleInstall = async (item: MarketplaceSkillItem) => {
    setInstallingId(item.id)
    try {
      await installMarketplaceSkill(item.id, item.pluginId)
      pushNotification({
        level: 'success',
        title: t('settings.skills.installSuccess', { name: item.name }),
        message: '',
        actions: [],
        dismissible: true,
        autoHide: 5,
        context: 'toast',
      })
    } catch (e) {
      pushNotification({
        level: 'error',
        title: t('settings.skills.installFailed'),
        message: String(e),
        actions: [],
        dismissible: true,
        autoHide: 5,
        context: 'toast',
      })
    }
    setInstallingId(null)
  }

  const totalPages = Math.ceil(total / PAGE_SIZE)
  const lang = i18n.language
  const catLabels = CATEGORY_LABELS[lang] || CATEGORY_LABELS['en-US']

  return (
    <div>
      {/* Description */}
      <p className="mb-3 text-sm" style={{ color: 'var(--color-text-secondary)' }}>
        {t('settings.skills.marketplaceDesc')}
      </p>

      {/* Category tabs + search */}
      <div className="mb-4 flex flex-wrap items-center gap-2">
        {CATEGORIES.map((cat) => (
          <button
            key={cat.key}
            onClick={() => setCategory(cat.key)}
            className="rounded-md px-3 py-1 text-xs font-medium transition-colors cursor-pointer"
            style={{
              background: category === cat.key ? 'var(--color-primary)' : 'var(--color-bg-card)',
              color: category === cat.key ? 'var(--color-text-on-primary)' : 'var(--color-text-secondary)',
              border: `1px solid ${category === cat.key ? 'var(--color-primary)' : 'var(--color-border)'}`,
            }}
          >
            {catLabels[cat.key] || cat.label}
          </button>
        ))}
        <div className="ml-auto">
          <input
            type="text"
            value={search}
            onChange={(e) => handleSearchChange(e.target.value)}
            placeholder={t('settings.skills.searchPlaceholder')}
            className="h-7 rounded-md border px-2 text-xs"
            style={{
              borderColor: 'var(--color-border)',
              background: 'var(--color-bg-main)',
              color: 'var(--color-text-primary)',
              width: '180px',
              outline: 'none',
            }}
          />
        </div>
      </div>

      {/* Content */}
      {loading ? (
        <p style={{ color: 'var(--color-text-secondary)' }}>{t('common.loading')}</p>
      ) : error ? (
        <div
          className="rounded-lg border p-6 text-center"
          style={{ borderColor: 'var(--color-border)', color: 'var(--color-text-secondary)' }}
        >
          <p>{t('settings.skills.loadFailed')}</p>
          <Button variant="secondary" size="sm" className="mt-2" onClick={() => loadSkills(page, category, search)}>
            {t('settings.skills.retry')}
          </Button>
        </div>
      ) : items.length === 0 ? (
        <div
          className="rounded-lg border p-6 text-center"
          style={{ borderColor: 'var(--color-border)', color: 'var(--color-text-secondary)' }}
        >
          <p>{t('settings.skills.noResults')}</p>
        </div>
      ) : (
        <>
          {/* Grid */}
          <div className="grid gap-3" style={{ gridTemplateColumns: 'repeat(auto-fill, minmax(260px, 1fr))' }}>
            {items.map((item) => (
              <div
                key={item.id}
                className="rounded-lg border p-4 flex flex-col gap-2"
                style={{
                  borderColor: 'var(--color-border)',
                  background: 'var(--color-bg-card)',
                }}
              >
                {/* Header: icon + name + featured badge */}
                <div className="flex items-center gap-2">
                  <span style={{ fontSize: '1.3rem' }}>{item.icon || '🔧'}</span>
                  <span
                    className="font-medium truncate flex-1 text-sm"
                    style={{ color: 'var(--color-text-primary)' }}
                  >
                    {item.name}
                  </span>
                  {item.featured && (
                    <span
                      className="rounded-full px-1.5 py-0.5 text-xs font-medium"
                      style={{ background: 'var(--color-semantic-yellow-bg, rgba(232,200,106,0.15))', color: 'var(--color-semantic-yellow, #E8C86A)' }}
                    >
                      {t('settings.skills.featured')}
                    </span>
                  )}
                </div>

                {/* Description */}
                <p
                  className="text-xs line-clamp-2"
                  style={{ color: 'var(--color-text-secondary)', minHeight: '2.4em' }}
                >
                  {item.description}
                </p>

                {/* Meta: author + version */}
                <div className="text-xs" style={{ color: 'var(--color-text-muted)' }}>
                  {t('settings.skills.byAuthor', { name: item.tenantName })} &middot; v{item.version}
                </div>

                {/* Footer: downloads + install button */}
                <div className="flex items-center justify-between mt-auto pt-1">
                  <span className="text-xs" style={{ color: 'var(--color-text-muted)' }}>
                    {t('settings.skills.downloads', { count: item.downloads })}
                  </span>
                  <Button
                    variant="primary"
                    size="sm"
                    disabled={installingId === item.id}
                    onClick={() => handleInstall(item)}
                  >
                    {installingId === item.id
                      ? t('settings.skills.installing')
                      : t('settings.skills.install')}
                  </Button>
                </div>
              </div>
            ))}
          </div>

          {/* Pagination */}
          {totalPages > 1 && (
            <div className="mt-4 flex items-center justify-center gap-2">
              <Button
                variant="secondary"
                size="sm"
                disabled={page <= 1}
                onClick={() => loadSkills(page - 1, category, search)}
              >
                &larr;
              </Button>
              <span className="text-xs" style={{ color: 'var(--color-text-secondary)' }}>
                {page} / {totalPages}
              </span>
              <Button
                variant="secondary"
                size="sm"
                disabled={page >= totalPages}
                onClick={() => loadSkills(page + 1, category, search)}
              >
                &rarr;
              </Button>
            </div>
          )}
        </>
      )}
    </div>
  )
}

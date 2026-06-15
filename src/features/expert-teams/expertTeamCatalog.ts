import {
  expertTeamTemplateCatalog,
  expertTeamTemplateRefresh,
  workplaceDirectoryCatalog,
  type ExpertTeamTemplateAvatar,
  type ExpertTeamTemplateExpert,
  type ExpertTeamTemplatePromptText,
  type ExpertTeamTemplateSnapshot,
  type WorkplaceDirectoryCategory,
  type WorkplaceDirectoryItem,
} from '@/lib/tauri'
import type { ExpertPersona, ExpertTeam, FacilitationStyle } from './teams'

export interface ExpertTeamCategory {
  categoryId: string
  name: string
  description?: string
  icon?: string
  color?: string
  sortOrder: number
}

export interface ExpertTeamCatalogResult {
  teams: ExpertTeam[]
  categories: ExpertTeamCategory[]
  error: unknown | null
}

export interface ExpertTeamCatalogOptions {
  forceRefresh?: boolean
}

export interface ExpertTeamGroup {
  key: string
  category: ExpertTeamCategory | null
  teams: ExpertTeam[]
}

function versionKey(id: string, version: string): string {
  return `${id}@@${version}`
}

function normalizeLocale(language?: string): 'zh-CN' | 'en-US' {
  return language?.toLowerCase().startsWith('en') ? 'en-US' : 'zh-CN'
}

function defaultComposerPlaceholder(language?: string): string {
  return language?.toLowerCase().startsWith('en')
    ? 'Share a topic for the team to discuss...'
    : '抛出你的议题，专家团会一起讨论...'
}

function iconEmoji(icon?: string): string {
  if (!icon) return '👥'
  return Array.from(icon).length <= 2 ? icon : '👥'
}

function avatarText(avatar: ExpertTeamTemplateAvatar | undefined, fallback: string): string {
  if (typeof avatar === 'string' && avatar.trim()) return avatar.trim()
  return Array.from(fallback.trim())[0] ?? '专'
}

function normalizeFacilitationStyle(style: string | undefined, expertCount: number): FacilitationStyle {
  if (style === 'rounds' || style === 'debate' || style === 'open') return style
  return expertCount > 0 ? 'rounds' : 'open'
}

function stripTerminalPunctuation(text: string): string {
  return text.trim().replace(/[。.!！?？；;，,、\s]+$/g, '')
}

function compactPersonaForDescription(persona: string, language?: string): string {
  const text = stripTerminalPunctuation(persona)
  if (language?.toLowerCase().startsWith('en')) {
    return text
      .replace(/^(Focuses on|Uses|Connects|Represents|Reviews|Prepares|Balances|Adds)\s+/i, '')
      .replace(/^(Focuses|Uses|Connects|Represents|Reviews|Prepares|Balances|Adds)\s+/i, '')
      .replace(/^(on|with)\s+/i, '')
  }
  return text
    .replace(/^(关注|熟悉|善用|擅长|用|看|从|连接|组织|准备|论证|扮演|事后点评)/, '')
    .replace(/^于/, '')
}

function joinList(items: string[], language?: string): string {
  if (items.length === 0) return ''
  if (!language?.toLowerCase().startsWith('en')) return items.join('、')
  if (items.length === 1) return items[0]
  if (items.length === 2) return `${items[0]} and ${items[1]}`
  return `${items.slice(0, -1).join(', ')}, and ${items[items.length - 1]}`
}

function buildTeamDescription(
  base: string,
  experts: ExpertPersona[],
  examples: string[],
  language?: string,
): string {
  const normalizedBase = stripTerminalPunctuation(base)
  const primaryExample = examples.find((example) => example.trim().length > 0)?.trim()
  const en = language?.toLowerCase().startsWith('en')

  if (experts.length === 0) {
    if (en) {
      const exampleText = primaryExample ? `, often for topics like ${stripTerminalPunctuation(primaryExample)}` : ''
      return `${normalizedBase || 'Open-ended discussion'}; the host selects suitable expert roles for each topic${exampleText}.`
    }
    const exampleText = primaryExample ? `，常用于「${stripTerminalPunctuation(primaryExample)}」这类开放问题` : ''
    return `${normalizedBase || '开放议题讨论'}；主持人会按议题动态召集合适专家${exampleText}。`
  }

  const names = experts.slice(0, 3).map((expert) => expert.name.trim()).filter(Boolean)
  const rolesSuffix = experts.length > 3 ? (en ? ' and others' : '等角色') : (en ? '' : '等角色')
  const perspectives = experts
    .slice(0, 3)
    .map((expert) => compactPersonaForDescription(expert.persona, language))
    .filter(Boolean)
  const exampleText = primaryExample
    ? (en
        ? `, often used for ${stripTerminalPunctuation(primaryExample)}`
        : `，常用于「${stripTerminalPunctuation(primaryExample)}」`)
    : ''

  if (en) {
    const nameText = joinList(names, language)
    const action = experts.length === 1 ? 'reviews' : 'review'
    const perspectiveText = perspectives.length > 0
      ? ` across ${joinList(perspectives, language)}`
      : ''
    return `${normalizedBase || 'Multi-expert review'}; ${nameText}${rolesSuffix} ${action} the issue${perspectiveText}${exampleText}.`
  }

  const nameText = joinList(names, language)
  const perspectiveText = perspectives.length > 0
    ? `从${joinList(perspectives, language)}等视角共同判断`
    : '共同判断'
  return `${normalizedBase || '多专家协同评审'}；由${nameText}${rolesSuffix}${perspectiveText}${exampleText}。`
}

function toCatalogCategory(category: WorkplaceDirectoryCategory): ExpertTeamCategory {
  return {
    categoryId: category.categoryId,
    name: category.display.name || category.categoryId,
    description: category.display.description || category.display.tagline,
    icon: category.icon,
    color: category.color,
    sortOrder: category.sortOrder,
  }
}

function selectLocaleText<T>(value: Record<string, T> | null | undefined, language?: string): T | undefined {
  const locale = normalizeLocale(language)
  return value?.[locale] ?? value?.['zh-CN'] ?? value?.['en-US']
}

function expertDisplay(
  expert: ExpertTeamTemplateExpert,
  language?: string,
): { name?: string; persona?: string } {
  const localized = selectLocaleText(expert.displayI18n, language)
  return {
    name: localized?.name || expert.name || expert.title || expert.stableName || expert.agentName,
    persona: localized?.persona || expert.persona || expert.title || expert.name || expert.stableName || expert.agentName,
  }
}

function snapshotExpertsToRoster(snapshot: ExpertTeamTemplateSnapshot, language?: string): ExpertPersona[] {
  return (snapshot.experts ?? [])
    .map((expert, index) => {
      const display = expertDisplay(expert, language)
      const name = display.name?.trim() || `Expert ${index + 1}`
      const persona = display.persona?.trim() || name
      return {
        name,
        avatarName: expert.avatarName || expert.name || name,
        agentName: expert.agentName || expert.stableName,
        avatar: expert.avatar ?? null,
        avatarText: expert.avatarText?.trim() || avatarText(expert.avatar, name),
        persona,
        emoji: expert.emoji || (typeof expert.avatar === 'string' ? iconEmoji(expert.avatar) : '👤'),
      }
    })
    .filter((expert) => expert.name.trim().length > 0)
}

function snapshotDirectorPrompt(
  snapshot: ExpertTeamTemplateSnapshot,
  language?: string,
): ExpertTeamTemplatePromptText | undefined {
  return selectLocaleText(snapshot.directorPromptI18n, language)
}

function snapshotByVersion(snapshots: ExpertTeamTemplateSnapshot[]) {
  return new Map(snapshots.map((snapshot) => [versionKey(snapshot.teamId, snapshot.version), snapshot]))
}

function sortDirectoryItems(
  items: WorkplaceDirectoryItem[],
  categories: ExpertTeamCategory[],
): WorkplaceDirectoryItem[] {
  const categorySort = new Map(categories.map((category) => [category.categoryId, category.sortOrder]))
  return [...items].sort((a, b) => {
    const categoryDelta =
      (categorySort.get(a.workplaceCategoryId ?? '') ?? Number.MAX_SAFE_INTEGER) -
      (categorySort.get(b.workplaceCategoryId ?? '') ?? Number.MAX_SAFE_INTEGER)
    if (categoryDelta !== 0) return categoryDelta
    const itemDelta = (a.sortOrder ?? Number.MAX_SAFE_INTEGER) - (b.sortOrder ?? Number.MAX_SAFE_INTEGER)
    if (itemDelta !== 0) return itemDelta
    return a.resourceId.localeCompare(b.resourceId)
  })
}

function directoryItemToTeam(
  item: WorkplaceDirectoryItem,
  category: ExpertTeamCategory | null,
  snapshot: ExpertTeamTemplateSnapshot | undefined,
  language?: string,
): ExpertTeam | null {
  if (item.resourceType !== 'expert_team_template' || !snapshot) return null
  const remoteDisplay = selectLocaleText(snapshot.displayI18n, language)
  const itemDisplay = item.display
  const experts = snapshotExpertsToRoster(snapshot, language)
  const examples = (
    remoteDisplay?.examples?.filter(Boolean) ??
    itemDisplay.examples?.filter(Boolean) ??
    []
  )
  return {
    id: item.resourceId,
    name: remoteDisplay?.name || itemDisplay.name || item.resourceId,
    emoji: iconEmoji(item.icon),
    tagline: remoteDisplay?.tagline || itemDisplay.tagline || remoteDisplay?.description || itemDisplay.description || '',
    description: buildTeamDescription(
      remoteDisplay?.description || itemDisplay.description || remoteDisplay?.tagline || itemDisplay.tagline || '',
      experts,
      examples,
      language,
    ),
    experts,
    examples,
    composerPlaceholder: remoteDisplay?.composerPlaceholder || defaultComposerPlaceholder(language),
    facilitationStyle: normalizeFacilitationStyle(snapshot.facilitationStyle, experts.length),
    directorPromptTemplate: snapshotDirectorPrompt(snapshot, language)?.template || null,
    workplaceCategoryId: item.workplaceCategoryId ?? null,
    workplaceCategoryName: category?.name ?? null,
    workplaceCategoryDescription: category?.description ?? null,
    workplaceCategoryIcon: category?.icon ?? null,
    workplaceCategoryColor: category?.color ?? null,
    workplaceCategorySortOrder: category?.sortOrder ?? null,
    sortOrder: item.sortOrder ?? null,
  }
}

export async function loadExpertTeamCatalog(
  language?: string,
  options: ExpertTeamCatalogOptions = {},
): Promise<ExpertTeamCatalogResult> {
  const directory = await workplaceDirectoryCatalog(language, { forceRefresh: options.forceRefresh })
  const teamItems = directory.items.filter((item) => item.resourceType === 'expert_team_template')
  if (teamItems.length === 0) return { teams: [], categories: [], error: null }

  const categories = directory.categories.map(toCatalogCategory)
  const categoryById = new Map(categories.map((category) => [category.categoryId, category]))
  let snapshots = await expertTeamTemplateCatalog()
  let snapshotsByKey = snapshotByVersion(snapshots)
  const hasMissingSnapshot = teamItems.some((item) => !snapshotsByKey.has(versionKey(item.resourceId, item.version)))
  if (hasMissingSnapshot) {
    await expertTeamTemplateRefresh().catch((e) => {
      console.warn('[expertTeamCatalog] expert_team_template_refresh after directory sync failed:', e)
      return 0
    })
    snapshots = await expertTeamTemplateCatalog()
    snapshotsByKey = snapshotByVersion(snapshots)
  }

  const teams = sortDirectoryItems(teamItems, categories)
    .map((item) => directoryItemToTeam(
      item,
      item.workplaceCategoryId ? categoryById.get(item.workplaceCategoryId) ?? null : null,
      snapshotsByKey.get(versionKey(item.resourceId, item.version)),
      language,
    ))
    .filter((team): team is ExpertTeam => !!team)

  return {
    teams,
    categories,
    error: teams.length < teamItems.length
      ? new Error('workplace directory returned expert teams but some snapshots are not cached')
      : null,
  }
}

export function groupExpertTeams(teams: ExpertTeam[], categories: ExpertTeamCategory[]): ExpertTeamGroup[] {
  const categoryById = new Map(categories.map((category) => [category.categoryId, category]))
  const groups = new Map<string, ExpertTeamGroup>()
  for (const team of teams) {
    const key = team.workplaceCategoryId || '__default__'
    let group = groups.get(key)
    if (!group) {
      group = {
        key,
        category: team.workplaceCategoryId
          ? categoryById.get(team.workplaceCategoryId) ?? {
              categoryId: team.workplaceCategoryId,
              name: team.workplaceCategoryName || team.workplaceCategoryId,
              sortOrder: Number.MAX_SAFE_INTEGER,
            }
          : null,
        teams: [],
      }
      groups.set(key, group)
    }
    group.teams.push(team)
  }
  return Array.from(groups.values()).sort((a, b) => {
    const sortDelta =
      (a.category?.sortOrder ?? Number.MAX_SAFE_INTEGER) -
      (b.category?.sortOrder ?? Number.MAX_SAFE_INTEGER)
    if (sortDelta !== 0) return sortDelta
    return (a.category?.name ?? '').localeCompare(b.category?.name ?? '')
  })
}

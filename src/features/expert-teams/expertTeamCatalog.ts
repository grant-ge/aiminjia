import {
  expertTeamTemplateCatalog,
  expertTeamTemplateRefresh,
  workplaceDirectoryCatalog,
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

function normalizeFacilitationStyle(style: string | undefined, expertCount: number): FacilitationStyle {
  if (style === 'rounds' || style === 'debate' || style === 'open') return style
  return expertCount > 0 ? 'rounds' : 'open'
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
        persona,
        emoji: expert.emoji || iconEmoji(expert.avatar),
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

export async function loadExpertTeamCatalog(language?: string): Promise<ExpertTeamCatalogResult> {
  const directory = await workplaceDirectoryCatalog(language)
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

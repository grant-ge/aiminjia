import {
  employeeTemplateCatalog,
  employeeTemplateRefresh,
  workplaceDirectoryCatalog,
  type EmployeeTemplateSnapshot,
  type WorkplaceDirectoryCategory,
  type WorkplaceDirectoryItem,
} from '@/lib/tauri'
import { snapshotToTemplate, type EmployeeTemplate } from './templates'

export interface EmployeeCatalogCategory {
  categoryId: string
  name: string
  description?: string
  icon?: string
  color?: string
  sortOrder: number
}

export interface EmployeeCatalogGroup {
  key: string
  category: EmployeeCatalogCategory | null
  templates: EmployeeTemplate[]
}

export interface EmployeeTemplateCatalogResult {
  catalog: EmployeeTemplate[]
  categories: EmployeeCatalogCategory[]
  error: unknown | null
}

function templateVersionKey(templateId: string, version: string): string {
  return `${templateId}@@${version}`
}

function toCatalogCategory(category: WorkplaceDirectoryCategory): EmployeeCatalogCategory {
  return {
    categoryId: category.categoryId,
    name: category.display.name || category.categoryId,
    description: category.display.description || category.display.tagline,
    icon: category.icon,
    color: category.color,
    sortOrder: category.sortOrder,
  }
}

function categoryNameById(items: WorkplaceDirectoryItem[], categories: WorkplaceDirectoryCategory[]) {
  const usedCategoryIds = new Set(
    items
      .map((item) => item.workplaceCategoryId)
      .filter((id): id is string => !!id),
  )
  return new Map(
    categories
      .filter((category) => usedCategoryIds.has(category.categoryId))
      .map((category) => [category.categoryId, category.display.name || category.categoryId]),
  )
}

function sortDirectoryItems(
  items: WorkplaceDirectoryItem[],
  categories: EmployeeCatalogCategory[],
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

async function loadDirectoryEmployeeTemplates(language?: string): Promise<{
  catalog: EmployeeTemplate[]
  categories: EmployeeCatalogCategory[]
}> {
  const directory = await workplaceDirectoryCatalog(language)
  const employeeItems = directory.items.filter((item) => item.resourceType === 'employee_template')
  if (employeeItems.length === 0) return { catalog: [], categories: [] }

  const categories = directory.categories.map(toCatalogCategory)
  const categoryNames = categoryNameById(employeeItems, directory.categories)
  const snapshots: EmployeeTemplateSnapshot[] = await employeeTemplateCatalog()
  const snapshotsByVersion = new Map(
    snapshots.map((snap) => [templateVersionKey(snap.templateId, snap.version), snap]),
  )
  const templates: EmployeeTemplate[] = []
  for (const item of sortDirectoryItems(employeeItems, categories)) {
    const snapshot = snapshotsByVersion.get(templateVersionKey(item.resourceId, item.version))
    if (!snapshot) continue
    templates.push(snapshotToTemplate(
      snapshot,
      language,
      item,
      item.workplaceCategoryId ? categoryNames.get(item.workplaceCategoryId) : null,
    ))
  }
  return { catalog: templates, categories }
}

async function loadCachedEmployeeTemplates(language?: string): Promise<EmployeeTemplate[]> {
  await employeeTemplateRefresh().catch((e) => {
    console.warn('[employeeCatalog] employee_template_refresh failed:', e)
    return 0
  })
  const snapshots: EmployeeTemplateSnapshot[] = await employeeTemplateCatalog()
  return snapshots.map((snap) => snapshotToTemplate(snap, language))
}

export async function loadEmployeeTemplateCatalog(language?: string): Promise<EmployeeTemplateCatalogResult> {
  let directoryError: unknown = null
  try {
    const directoryCatalog = await loadDirectoryEmployeeTemplates(language)
    if (directoryCatalog.catalog.length > 0) {
      return { ...directoryCatalog, error: null }
    }
  } catch (e) {
    directoryError = e
    console.warn('[employeeCatalog] workplace_directory_catalog failed:', e)
  }

  const cachedCatalog = await loadCachedEmployeeTemplates(language)
  return {
    catalog: cachedCatalog,
    categories: [],
    error: cachedCatalog.length === 0 ? directoryError : null,
  }
}

export function groupEmployeeCatalog(
  catalog: EmployeeTemplate[],
  categories: EmployeeCatalogCategory[],
): EmployeeCatalogGroup[] {
  const categoryById = new Map(categories.map((category) => [category.categoryId, category]))
  const groups = new Map<string, EmployeeCatalogGroup>()
  for (const template of catalog) {
    const key = template.workplaceCategoryId || '__uncategorized__'
    let group = groups.get(key)
    if (!group) {
      group = {
        key,
        category: template.workplaceCategoryId
          ? categoryById.get(template.workplaceCategoryId) ?? {
              categoryId: template.workplaceCategoryId,
              name: template.workplaceCategoryName || template.workplaceCategoryId,
              sortOrder: Number.MAX_SAFE_INTEGER,
            }
          : null,
        templates: [],
      }
      groups.set(key, group)
    }
    group.templates.push(template)
  }
  return Array.from(groups.values()).sort((a, b) => {
    const sortDelta =
      (a.category?.sortOrder ?? Number.MAX_SAFE_INTEGER) -
      (b.category?.sortOrder ?? Number.MAX_SAFE_INTEGER)
    if (sortDelta !== 0) return sortDelta
    return (a.category?.name ?? '').localeCompare(b.category?.name ?? '')
  })
}

export function requiredSkillNames(template: EmployeeTemplate): string[] {
  return (template.requiredSkills ?? [])
    .map((skill) => skill.display.name.trim())
    .filter(Boolean)
}

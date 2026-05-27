import i18n from '@/i18n'
import type { SkillInfo } from '@/lib/tauri'

function isEnglish(language = i18n.language): boolean {
  return language.toLowerCase().startsWith('en')
}

function translatedBuiltinName(skill: SkillInfo): string {
  const key = `skills.${skill.id}`
  const translated = i18n.t(key, { defaultValue: '' })
  return translated && translated !== key ? translated : ''
}

export function localizeSkill(skill: SkillInfo, language = i18n.language) {
  if (!isEnglish(language)) {
    return {
      name: skill.displayName || skill.id,
      description: skill.shortDescription || skill.description,
    }
  }

  return {
    name: skill.displayNameEn || translatedBuiltinName(skill) || skill.displayName || skill.id,
    description: skill.shortDescriptionEn || skill.shortDescription || skill.description,
  }
}

export function localizedSkillName(skill: SkillInfo | null | undefined, fallbackId: string, language = i18n.language): string {
  return skill ? localizeSkill(skill, language).name : fallbackId
}

export function localizedSkillDescription(skill: SkillInfo | null | undefined, fallbackId: string, language = i18n.language): string {
  return skill ? (localizeSkill(skill, language).description || fallbackId) : fallbackId
}

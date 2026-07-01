import { ask } from '@tauri-apps/plugin-dialog'
import { DEFAULTS, useBrandingStore } from '@/stores/brandingStore'
import { SkillAlreadyExistsError } from '@/stores/skillStore'

/**
 * Wraps a skill upload action with overwrite confirmation. Calls `upload(false)`
 * first; on `SkillAlreadyExistsError`, asks the user; if confirmed, retries with
 * `upload(true)`. Cancellation is silent (no error thrown).
 *
 * Returns 'installed' on success, 'cancelled' if user declined overwrite.
 * Other errors propagate.
 */
export async function uploadWithOverwriteConfirm(
  upload: (force: boolean) => Promise<void>,
): Promise<'installed' | 'cancelled'> {
  try {
    await upload(false)
    return 'installed'
  } catch (err) {
    if (err instanceof SkillAlreadyExistsError) {
      const productName = useBrandingStore.getState().productName.trim() || DEFAULTS.productName
      const confirmed = await ask(
        `技能 "${err.skillId}" 已存在，是否覆盖？`,
        { title: productName, kind: 'warning' },
      )
      if (!confirmed) return 'cancelled'
      await upload(true)
      return 'installed'
    }
    throw err
  }
}

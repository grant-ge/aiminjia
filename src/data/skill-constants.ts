/** Skill id of the bundled skill-smith template that drives the create-skill flow. */
export const SKILL_SMITH_ID = 'skill-smith'

/** Slash command pre-filled into the chat composer when user clicks "+ 创建技能". */
export const CREATE_SKILL_COMMAND = '/create-skill '

/**
 * Magic prefix returned by the `install_custom_skill` Tauri command when the
 * target skill id already exists in the user dir. Frontend code parses the
 * `<id>` suffix and prompts the user to overwrite.
 */
export const ALREADY_EXISTS_PREFIX = 'ALREADY_EXISTS:'

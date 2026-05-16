// Vitest setup — initialize i18n so components that use useTranslation() work
// in tests without needing explicit mocking. Uses zh-CN as fallback language
// (same as production default) so existing tests that assert Chinese text pass.
import '../i18n'

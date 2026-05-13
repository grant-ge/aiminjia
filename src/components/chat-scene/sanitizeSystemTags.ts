/**
 * Sanitize LLM assistant text for the group-chat UI.
 *
 * The Lead occasionally echoes system-injected XML wrappers
 * (`<task-notification>`, `<peer-messages>`, legacy `<tool_response>`) back
 * into its own assistant text — because it sees those tags in upstream
 * `user` messages and mistakes them for "format I should follow."
 *
 * We never store sanitized data: this is a render-time-only transform so
 * the original transcript stays intact for debugging / export.  Removes the
 * full tag including inner payload (which is always an internal JSON dump
 * with no business meaning in the group chat).
 *
 * Why not a remark/rehype plugin: react-markdown already runs with
 * `skipHtml`, so raw HTML/XML is rendered as text anyway.  A simple
 * regex-strip on the input string is the smallest reversible fix.
 */
const SYSTEM_TAGS = [
  'task-notification',
  'peer-messages',
  'tool_response',
  'tool-response',
] as const

export function stripSystemXmlTags(text: string): string {
  if (!text) return text
  let out = text
  for (const tag of SYSTEM_TAGS) {
    // Greedy match for paired tags; also handles self-closing variants.
    // `[\s\S]` is the standard cross-line `.` substitute.
    const pairRe = new RegExp(`<${tag}\\b[\\s\\S]*?</${tag}>`, 'gi')
    out = out.replace(pairRe, '')
    // Defensive: drop a lone unclosed open-tag through end of string.
    const openRe = new RegExp(`<${tag}\\b[\\s\\S]*$`, 'gi')
    out = out.replace(openRe, '')
  }
  return out
}

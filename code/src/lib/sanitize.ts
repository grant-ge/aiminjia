/**
 * Content sanitization utilities for LLM output.
 *
 * Strips hallucinated XML function-call blocks that some models emit
 * in their text stream, preventing raw XML from appearing in the UI.
 */

const XML_TAG_PAIRS: ReadonlyArray<readonly [RegExp, RegExp]> = [
  [/<function_calls>/, /<\/function_calls>/],
  [/<invoke[\s>]/, /<\/invoke>/],
  [/<tool_call>/, /<\/tool_call>/],
  [/<tool_calls>/, /<\/tool_calls>/],
] as const

export function stripHallucinatedXml(text: string): string {
  let result = text
  for (const [open, close] of XML_TAG_PAIRS) {
    while (true) {
      const openMatch = result.match(open)
      if (!openMatch || openMatch.index === undefined) break
      const start = openMatch.index
      const afterOpen = result.slice(start)
      const closeMatch = afterOpen.match(close)
      if (closeMatch && closeMatch.index !== undefined) {
        result =
          result.slice(0, start) +
          result.slice(start + closeMatch.index + closeMatch[0].length)
      } else {
        // Unclosed tag: truncate to tag start (common in streaming)
        result = result.slice(0, start)
        break
      }
    }
  }
  return result.trim()
}

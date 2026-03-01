/**
 * Lightweight markdown-to-HTML renderer for LLM output.
 *
 * Supports: headings, tables, bold, italic, inline code, code blocks,
 * ordered/unordered lists, blockquotes, horizontal rules.
 *
 * Security: all text content is HTML-escaped before insertion.
 */

/** Escape HTML entities to prevent XSS from LLM-generated content. */
function esc(s: string): string {
  return s
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
    .replace(/'/g, '&#39;')
}

/** Apply inline formatting: bold, italic, inline code, strikethrough. */
function inlineFmt(text: string): string {
  let s = esc(text)
  // Restore <br> tags that the LLM uses for line breaks within table cells.
  // esc() converts <br> to &lt;br&gt; — restore them as actual <br/>.
  s = s.replace(/&lt;br\s*\/?&gt;/gi, '<br/>')
  // Inline code (must come first to avoid bold/italic inside code)
  s = s.replace(
    /`([^`]+)`/g,
    '<code style="background:var(--color-bg-base);padding:1px 5px;border-radius:3px;font-family:var(--font-mono);font-size:0.82em;color:var(--color-text-primary)">$1</code>',
  )
  // Bold + italic
  s = s.replace(
    /\*\*\*(.+?)\*\*\*/g,
    '<strong style="color:var(--color-text-primary);font-weight:600"><em>$1</em></strong>',
  )
  // Bold
  s = s.replace(
    /\*\*(.+?)\*\*/g,
    '<strong style="color:var(--color-text-primary);font-weight:600">$1</strong>',
  )
  // Italic
  s = s.replace(/\*(.+?)\*/g, '<em>$1</em>')
  // Strikethrough
  s = s.replace(/~~(.+?)~~/g, '<del>$1</del>')
  return s
}

/** Check if a line is a table separator row (e.g. |---|---|). */
function isTableSeparator(line: string): boolean {
  return /^\|[\s:]*-+[\s:]*(\|[\s:]*-+[\s:]*)*\|?\s*$/.test(line.trim())
}

/** Parse a table row into cells. */
function parseTableRow(line: string): string[] {
  const trimmed = line.trim()
  // Remove leading/trailing pipes
  const inner = trimmed.startsWith('|') ? trimmed.slice(1) : trimmed
  const final = inner.endsWith('|') ? inner.slice(0, -1) : inner
  return final.split('|').map((cell) => cell.trim())
}

/** Parse alignment from separator row. */
function parseAlignments(line: string): string[] {
  return parseTableRow(line).map((cell) => {
    const t = cell.trim()
    if (t.startsWith(':') && t.endsWith(':')) return 'center'
    if (t.endsWith(':')) return 'right'
    return 'left'
  })
}

/** Render a markdown table block into HTML. */
function renderTable(lines: string[]): string {
  if (lines.length < 2) return lines.map((l) => `<p>${inlineFmt(l)}</p>`).join('')

  const headerCells = parseTableRow(lines[0])
  const alignments = isTableSeparator(lines[1]) ? parseAlignments(lines[1]) : []
  const bodyStart = isTableSeparator(lines[1]) ? 2 : 1

  const alignStyle = (i: number) => {
    const a = alignments[i]
    if (!a || a === 'left') return ''
    return ` style="text-align:${a}"`
  }

  let html =
    '<div style="overflow-x:auto;margin:12px 0"><table style="width:100%;border-collapse:collapse;font-size:0.85rem;line-height:1.5">'

  // Header
  html += '<thead><tr>'
  for (let i = 0; i < headerCells.length; i++) {
    html += `<th${alignStyle(i)} style="padding:8px 12px;border-bottom:2px solid var(--color-border);text-align:${alignments[i] || 'left'};font-weight:600;color:var(--color-text-primary);background:var(--color-bg-base);white-space:nowrap">${inlineFmt(headerCells[i])}</th>`
  }
  html += '</tr></thead>'

  // Body
  html += '<tbody>'
  for (let r = bodyStart; r < lines.length; r++) {
    const cells = parseTableRow(lines[r])
    const isEven = (r - bodyStart) % 2 === 0
    const rowBg = isEven ? '' : ' background:var(--color-bg-base)'
    html += '<tr>'
    for (let i = 0; i < Math.max(cells.length, headerCells.length); i++) {
      const cell = cells[i] || ''
      html += `<td${alignStyle(i)} style="padding:7px 12px;border-bottom:1px solid var(--color-border-subtle);color:var(--color-text-secondary);${rowBg}">${inlineFmt(cell)}</td>`
    }
    html += '</tr>'
  }
  html += '</tbody></table></div>'

  return html
}

/** Render a fenced code block. */
function renderCodeBlock(lines: string[], lang: string): string {
  const code = lines.map(esc).join('\n')
  const rawCode = lines.join('\n')
  // Base64-encode the raw code for copy-to-clipboard via event delegation.
  // btoa only handles Latin-1, so we encode UTF-8 via TextEncoder first.
  const encoded = typeof btoa !== 'undefined'
    ? btoa(Array.from(new TextEncoder().encode(rawCode), (b) => String.fromCharCode(b)).join(''))
    : ''
  const copyBtn = encoded
    ? `<button data-copy-code="${encoded}" style="cursor:pointer;border:none;background:none;font-size:0.7rem;color:var(--color-text-muted);font-family:var(--font-mono);padding:2px 6px;border-radius:3px;transition:color 0.2s">复制</button>`
    : ''
  return `<div style="margin:12px 0;border-radius:8px;overflow:hidden;border:1px solid var(--color-border-subtle)"><div style="display:flex;align-items:center;justify-content:space-between;padding:6px 12px;background:var(--color-bg-base);font-size:0.75rem;color:var(--color-text-muted);font-family:var(--font-mono)"><span>${esc(lang || 'code')}</span>${copyBtn}</div><pre style="margin:0;padding:12px 14px;overflow-x:auto;background:var(--color-bg-elevated);font-size:0.82rem;line-height:1.55;font-family:var(--font-mono);color:var(--color-text-primary)"><code>${code}</code></pre></div>`
}

/**
 * Convert a markdown string to HTML.
 *
 * This is a lightweight, purpose-built renderer — not a full CommonMark
 * implementation. It handles the subset of markdown that LLMs typically
 * produce in data analysis output.
 */
export function markdownToHtml(md: string): string {
  const lines = md.split('\n')
  const output: string[] = []
  let i = 0

  while (i < lines.length) {
    const line = lines[i]
    const trimmed = line.trim()

    // --- Empty line → spacing ---
    if (trimmed === '') {
      i++
      continue
    }

    // --- Fenced code block ---
    if (trimmed.startsWith('```')) {
      const lang = trimmed.slice(3).trim()
      const codeLines: string[] = []
      i++
      while (i < lines.length && !lines[i].trim().startsWith('```')) {
        codeLines.push(lines[i])
        i++
      }
      i++ // skip closing ```
      output.push(renderCodeBlock(codeLines, lang))
      continue
    }

    // --- Headings ---
    const headingMatch = trimmed.match(/^(#{1,4})\s+(.+)/)
    if (headingMatch) {
      const level = headingMatch[1].length
      const text = headingMatch[2]
      const styles: Record<number, string> = {
        1: 'font-size:1.15em;font-weight:700;margin:20px 0 10px;color:var(--color-text-primary);border-bottom:1px solid var(--color-border-subtle);padding-bottom:6px',
        2: 'font-size:1.05em;font-weight:600;margin:16px 0 8px;color:var(--color-text-primary)',
        3: 'font-size:0.95em;font-weight:600;margin:12px 0 6px;color:var(--color-text-primary)',
        4: 'font-size:0.9em;font-weight:600;margin:10px 0 4px;color:var(--color-text-secondary)',
      }
      output.push(
        `<h${level} style="${styles[level] || styles[4]}">${inlineFmt(text)}</h${level}>`,
      )
      i++
      continue
    }

    // --- Horizontal rule ---
    if (/^[-*_]{3,}\s*$/.test(trimmed)) {
      output.push(
        '<hr style="border:none;border-top:1px solid var(--color-border-subtle);margin:16px 0"/>',
      )
      i++
      continue
    }

    // --- Table block ---
    if (trimmed.includes('|') && trimmed.startsWith('|')) {
      const tableLines: string[] = []
      while (i < lines.length && lines[i].trim().startsWith('|')) {
        tableLines.push(lines[i])
        i++
      }
      output.push(renderTable(tableLines))
      continue
    }

    // --- Blockquote ---
    if (trimmed.startsWith('>')) {
      const quoteLines: string[] = []
      while (i < lines.length && lines[i].trim().startsWith('>')) {
        quoteLines.push(lines[i].trim().replace(/^>\s?/, ''))
        i++
      }
      output.push(
        `<blockquote style="margin:10px 0;padding:8px 14px;border-left:3px solid var(--color-accent);background:var(--color-accent-subtle);border-radius:0 6px 6px 0;color:var(--color-text-secondary);font-size:0.88rem">${quoteLines.map(inlineFmt).join('<br/>')}</blockquote>`,
      )
      continue
    }

    // --- Unordered list ---
    if (/^[-*+]\s/.test(trimmed)) {
      const items: string[] = []
      while (i < lines.length && /^[-*+]\s/.test(lines[i].trim())) {
        items.push(lines[i].trim().replace(/^[-*+]\s/, ''))
        i++
      }
      output.push(
        `<ul style="margin:8px 0;padding-left:20px;list-style:disc">${items.map((item) => `<li style="margin:3px 0;color:var(--color-text-secondary);font-size:0.88rem;line-height:1.65">${inlineFmt(item)}</li>`).join('')}</ul>`,
      )
      continue
    }

    // --- Ordered list ---
    if (/^\d+[.)]\s/.test(trimmed)) {
      const items: string[] = []
      while (i < lines.length && /^\d+[.)]\s/.test(lines[i].trim())) {
        items.push(lines[i].trim().replace(/^\d+[.)]\s/, ''))
        i++
      }
      output.push(
        `<ol style="margin:8px 0;padding-left:20px;list-style:decimal">${items.map((item) => `<li style="margin:3px 0;color:var(--color-text-secondary);font-size:0.88rem;line-height:1.65">${inlineFmt(item)}</li>`).join('')}</ol>`,
      )
      continue
    }

    // --- Regular paragraph ---
    // Collect consecutive non-special lines as a paragraph
    const paraLines: string[] = []
    while (
      i < lines.length &&
      lines[i].trim() !== '' &&
      !lines[i].trim().startsWith('#') &&
      !lines[i].trim().startsWith('```') &&
      !lines[i].trim().startsWith('|') &&
      !lines[i].trim().startsWith('>') &&
      !/^[-*+]\s/.test(lines[i].trim()) &&
      !/^\d+[.)]\s/.test(lines[i].trim()) &&
      !/^[-*_]{3,}\s*$/.test(lines[i].trim())
    ) {
      paraLines.push(lines[i])
      i++
    }
    if (paraLines.length > 0) {
      output.push(
        `<p style="margin:6px 0;color:var(--color-text-secondary);font-size:0.88rem;line-height:1.7">${paraLines.map(inlineFmt).join('<br/>')}</p>`,
      )
    } else {
      // Safety: if a line was not collected by any handler (e.g. "#tag" without
      // space doesn't match heading regex, but paragraph excludes "#" prefix),
      // render it as a standalone paragraph and advance to prevent infinite loop.
      output.push(
        `<p style="margin:6px 0;color:var(--color-text-secondary);font-size:0.88rem;line-height:1.7">${inlineFmt(lines[i])}</p>`,
      )
      i++
    }
  }

  return output.join('')
}

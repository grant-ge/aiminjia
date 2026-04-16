import { describe, it, expect } from 'vitest'
import { markdownToHtml } from './markdown'

describe('markdownToHtml', () => {
  // ─── Regression: nested-list HTML must NOT be escaped ─────────────────────
  //
  // Bug: 2026-04-15. The unordered/ordered list renderer used to concat the
  // bullet text and pre-rendered nested HTML into one string, then pipe it
  // through inlineFmt() which calls esc() and HTML-escaped the already
  // emitted <span>/<strong> tags. Users saw raw markup like
  //   <span style="padding-left:16px;...">·  激光下料工...</span>
  // displayed as visible text in the chat. Fix: keep raw text and rendered
  // HTML in separate fields, only inlineFmt the raw text on emit.
  describe('nested list double-escape regression', () => {
    it('unordered list with indented continuation renders <span>, not &lt;span&gt;', () => {
      // Note: this renderer treats "  - nested" after trim() as another
      // top-level bullet (regex matches before indented-continuation branch).
      // So the actual nested-continuation case is plain indented text:
      const md = `- top item
  more **detail** appended`
      const html = markdownToHtml(md)
      expect(html).toContain('<span style="padding-left:16px')
      expect(html).toContain('<strong')
      expect(html).not.toContain('&lt;span')
      expect(html).not.toContain('&lt;strong')
    })

    it('ordered list with nested bullet renders <span>, not &lt;span&gt;', () => {
      const md = `2. **可疑归类**是否需要调整？比如：
   - 激光下料工等是否应归入**生产制造**？
   - 招聘专员是否应归入**职能支持**？`
      const html = markdownToHtml(md)
      expect(html).toContain('<span style="padding-left:16px')
      expect(html).toContain('<strong')
      expect(html).toContain('生产制造')
      // The smoking gun: must not appear as escaped text
      expect(html).not.toContain('&lt;span')
      expect(html).not.toContain('&lt;strong')
    })

    it('top-level bold inside ordered list still bolds', () => {
      const md = `1. **bold text** here`
      const html = markdownToHtml(md)
      expect(html).toContain('<strong')
      expect(html).toContain('bold text')
      expect(html).not.toContain('**bold text**')
    })

    it('nested bullet preserves bold inside it', () => {
      const md = `1. parent
   - child has **bold** text`
      const html = markdownToHtml(md)
      // Both the indent <span> and inner <strong> must survive
      expect(html).toContain('<span style="padding-left:16px')
      expect(html).toContain('<strong')
      expect(html).toContain('child has')
    })
  })

  // ─── Sanity: stray HTML in input is stripped (per pre-processing) ───────
  // markdownToHtml has a pre-processing step that strips HTML tags some
  // models spontaneously emit (deepseek/qwen). This is intentional —
  // see the comment at the top of markdown.ts. So `<script>` ends up
  // removed entirely, not escaped.
  it('strips stray HTML tags from input rather than escaping them', () => {
    const md = '正常文本含 <script>alert(1)</script> 标签'
    const html = markdownToHtml(md)
    expect(html).not.toContain('<script>')
    expect(html).not.toContain('&lt;script&gt;') // not escaped either, just gone
    expect(html).toContain('正常文本含')
    expect(html).toContain('标签')
  })
})

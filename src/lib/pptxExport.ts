/**
 * PPT export — converts conversation messages into a PPTX file.
 * Uses pptxgenjs to generate slides from structured message content.
 */
import type PptxGenJS from 'pptxgenjs'
import type {
  MessageContent,
  DataTable,
  ExecSummary,
  MetricCard,
  InsightBlock,
  AnomalyItem,
} from '@/types/message'

export interface BrandConfig {
  productName?: string
  accentColor?: string
  logoUrl?: string
}

type ColorTheme = 'professional' | 'clean' | 'vibrant'

interface ThemeColors {
  primary: string
  secondary: string
  bg: string
  text: string
  textLight: string
  accent: string
}

const THEMES: Record<ColorTheme, ThemeColors> = {
  professional: {
    primary: '1B3A5C',
    secondary: '2C5F8A',
    bg: 'FFFFFF',
    text: '1A1A2E',
    textLight: '6B7280',
    accent: '3B82F6',
  },
  clean: {
    primary: '4B5563',
    secondary: '6B7280',
    bg: 'F9FAFB',
    text: '111827',
    textLight: '9CA3AF',
    accent: '6366F1',
  },
  vibrant: {
    primary: '7C3AED',
    secondary: '8B5CF6',
    bg: 'FFFFFF',
    text: '1F2937',
    textLight: '6B7280',
    accent: 'EC4899',
  },
}

function resolveTheme(accentColor?: string): ThemeColors {
  if (accentColor) {
    const hex = accentColor.replace('#', '')
    return { ...THEMES.vibrant, primary: hex, secondary: hex, accent: hex }
  }
  return THEMES.professional
}

function addCoverSlide(pptx: PptxGenJS, title: string, theme: ThemeColors, brand?: BrandConfig) {
  const slide = pptx.addSlide()
  slide.background = { color: theme.primary }

  // Title
  slide.addText(title, {
    x: 0.8,
    y: 1.8,
    w: 8.4,
    h: 1.2,
    fontSize: 32,
    fontFace: 'Arial',
    color: 'FFFFFF',
    bold: true,
  })

  // Date
  const dateStr = new Date().toLocaleDateString('en-US', {
    year: 'numeric',
    month: 'long',
    day: 'numeric',
  })
  slide.addText(dateStr, {
    x: 0.8,
    y: 3.2,
    w: 8.4,
    h: 0.5,
    fontSize: 14,
    fontFace: 'Arial',
    color: 'D1D5DB',
  })

  // Brand name
  const brandName = brand?.productName ?? 'AIjia'
  slide.addText(brandName, {
    x: 0.8,
    y: 4.2,
    w: 8.4,
    h: 0.4,
    fontSize: 12,
    fontFace: 'Arial',
    color: 'A0AEC0',
  })
}

function addSummarySlide(pptx: PptxGenJS, summary: ExecSummary, theme: ThemeColors) {
  const slide = pptx.addSlide()
  slide.background = { color: theme.bg }

  slide.addText(summary.title || 'Executive Summary', {
    x: 0.8,
    y: 0.4,
    w: 8.4,
    h: 0.6,
    fontSize: 24,
    fontFace: 'Arial',
    color: theme.primary,
    bold: true,
  })

  // Summary boxes as a simple grid
  const boxesPerRow = Math.min(summary.boxes.length, 4)
  const boxWidth = 8.4 / boxesPerRow
  summary.boxes.forEach((box, i) => {
    const col = i % boxesPerRow
    const row = Math.floor(i / boxesPerRow)
    slide.addText(
      [
        { text: box.value + '\n', options: { fontSize: 28, bold: true, color: theme.primary } },
        { text: box.label, options: { fontSize: 12, color: theme.textLight } },
        ...(box.subtitle
          ? [{ text: '\n' + box.subtitle, options: { fontSize: 10, color: theme.textLight } }]
          : []),
      ],
      {
        x: 0.8 + col * boxWidth,
        y: 1.4 + row * 1.6,
        w: boxWidth - 0.2,
        h: 1.4,
        align: 'center',
        valign: 'middle',
        fill: { color: 'F3F4F6' },
        rectRadius: 0.1,
      },
    )
  })
}

function addTableSlide(pptx: PptxGenJS, table: DataTable, theme: ThemeColors) {
  const slide = pptx.addSlide()
  slide.background = { color: theme.bg }

  // Title
  slide.addText(table.title ?? 'Data', {
    x: 0.8,
    y: 0.3,
    w: 8.4,
    h: 0.5,
    fontSize: 20,
    fontFace: 'Arial',
    color: theme.primary,
    bold: true,
  })

  // Build table rows
  const headerRow: PptxGenJS.TableCell[] = table.columns.map((col) => ({
    text: col.label,
    options: {
      bold: true,
      fontSize: 10,
      fontFace: 'Arial',
      color: 'FFFFFF',
      fill: { color: theme.primary },
      align: (col.align ?? 'left') as PptxGenJS.HAlign,
      border: { type: 'solid' as const, pt: 0.5, color: 'D1D5DB' },
    },
  }))

  const dataRows: PptxGenJS.TableCell[][] = table.rows.map((row) =>
    table.columns.map((col) => ({
      text: row[col.key]?.text ?? '',
      options: {
        fontSize: 9,
        fontFace: 'Arial',
        color: theme.text,
        align: (col.align ?? 'left') as PptxGenJS.HAlign,
        bold: row[col.key]?.bold ?? false,
        border: { type: 'solid' as const, pt: 0.5, color: 'E5E7EB' },
      },
    })),
  )

  const colW = table.columns.map(() => 8.4 / table.columns.length)

  slide.addTable([headerRow, ...dataRows], {
    x: 0.8,
    y: 1.0,
    w: 8.4,
    colW,
    fontSize: 9,
    autoPage: true,
    autoPageRepeatHeader: true,
  })
}

function addMetricsSlide(pptx: PptxGenJS, metrics: MetricCard[], theme: ThemeColors) {
  const slide = pptx.addSlide()
  slide.background = { color: theme.bg }

  slide.addText('Key Metrics', {
    x: 0.8,
    y: 0.3,
    w: 8.4,
    h: 0.5,
    fontSize: 20,
    fontFace: 'Arial',
    color: theme.primary,
    bold: true,
  })

  const perRow = Math.min(metrics.length, 4)
  const boxW = 8.4 / perRow
  metrics.forEach((m, i) => {
    const col = i % perRow
    const row = Math.floor(i / perRow)
    slide.addText(
      [
        { text: m.value + '\n', options: { fontSize: 24, bold: true, color: theme.primary } },
        { text: m.label, options: { fontSize: 11, color: theme.textLight } },
        ...(m.subtitle
          ? [{ text: '\n' + m.subtitle, options: { fontSize: 9, color: theme.textLight } }]
          : []),
      ],
      {
        x: 0.8 + col * boxW,
        y: 1.2 + row * 1.6,
        w: boxW - 0.2,
        h: 1.4,
        align: 'center',
        valign: 'middle',
        fill: { color: 'F3F4F6' },
        rectRadius: 0.1,
      },
    )
  })
}

function addInsightsSlide(pptx: PptxGenJS, insights: InsightBlock[], theme: ThemeColors) {
  const slide = pptx.addSlide()
  slide.background = { color: theme.bg }

  slide.addText('Key Insights', {
    x: 0.8,
    y: 0.3,
    w: 8.4,
    h: 0.5,
    fontSize: 20,
    fontFace: 'Arial',
    color: theme.primary,
    bold: true,
  })

  insights.forEach((insight, i) => {
    const y = 1.0 + i * 1.0
    if (y > 6.5) return // avoid overflow
    slide.addText(
      [
        { text: insight.title + '\n', options: { fontSize: 14, bold: true, color: theme.text } },
        { text: insight.content, options: { fontSize: 11, color: theme.textLight } },
      ],
      {
        x: 0.8,
        y,
        w: 8.4,
        h: 0.8,
      },
    )
  })
}

function addAnomaliesSlide(pptx: PptxGenJS, anomalies: AnomalyItem[], theme: ThemeColors) {
  const slide = pptx.addSlide()
  slide.background = { color: theme.bg }

  slide.addText('Anomalies & Findings', {
    x: 0.8,
    y: 0.3,
    w: 8.4,
    h: 0.5,
    fontSize: 20,
    fontFace: 'Arial',
    color: theme.primary,
    bold: true,
  })

  const priorityColor: Record<string, string> = {
    high: 'EF4444',
    medium: 'F59E0B',
    low: '10B981',
  }

  anomalies.forEach((a, i) => {
    const y = 1.0 + i * 0.9
    if (y > 6.5) return
    slide.addText(
      [
        {
          text: `[${a.priority.toUpperCase()}] `,
          options: { fontSize: 11, bold: true, color: priorityColor[a.priority] ?? theme.text },
        },
        { text: a.title + '\n', options: { fontSize: 12, bold: true, color: theme.text } },
        { text: a.description, options: { fontSize: 10, color: theme.textLight } },
      ],
      {
        x: 0.8,
        y,
        w: 8.4,
        h: 0.7,
      },
    )
  })
}

function addTextSlide(pptx: PptxGenJS, title: string, text: string, theme: ThemeColors) {
  const slide = pptx.addSlide()
  slide.background = { color: theme.bg }

  slide.addText(title, {
    x: 0.8,
    y: 0.3,
    w: 8.4,
    h: 0.5,
    fontSize: 20,
    fontFace: 'Arial',
    color: theme.primary,
    bold: true,
  })

  slide.addText(text, {
    x: 0.8,
    y: 1.0,
    w: 8.4,
    h: 5.5,
    fontSize: 12,
    fontFace: 'Arial',
    color: theme.text,
    valign: 'top',
    autoFit: true,
  })
}

/**
 * Export conversation messages as a PPTX presentation.
 *
 * @param title - Presentation title (conversation title)
 * @param messages - Array of parsed message contents
 * @param brandConfig - Optional branding configuration
 * @returns The generated Blob
 */
export async function exportAsPptx(
  title: string,
  messages: MessageContent[],
  brandConfig?: BrandConfig,
): Promise<ArrayBuffer> {
  const PptxGenJS = (await import('pptxgenjs')).default
  const theme = resolveTheme(brandConfig?.accentColor)

  const pptx = new PptxGenJS()
  pptx.title = title
  pptx.subject = 'Analysis Report'
  pptx.company = brandConfig?.productName ?? 'AIjia'
  pptx.layout = 'LAYOUT_WIDE' // 13.33 x 7.5

  // 1. Cover slide
  addCoverSlide(pptx, title, theme, brandConfig)

  // 2. Collect content from all messages
  const allTables: DataTable[] = []
  const allMetrics: MetricCard[] = []
  const allInsights: InsightBlock[] = []
  const allAnomalies: AnomalyItem[] = []
  let execSummary: ExecSummary | undefined
  let lastText = ''

  for (const msg of messages) {
    if (msg.execSummary) execSummary = msg.execSummary
    if (msg.tables) allTables.push(...msg.tables)
    if (msg.metrics) allMetrics.push(...msg.metrics)
    if (msg.insights) allInsights.push(...msg.insights)
    if (msg.anomalies) allAnomalies.push(...msg.anomalies)
    if (msg.text) lastText = msg.text
  }

  // 3. Summary slide
  if (execSummary) {
    addSummarySlide(pptx, execSummary, theme)
  }

  // 4. Metrics slide
  if (allMetrics.length > 0) {
    addMetricsSlide(pptx, allMetrics, theme)
  }

  // 5. Data slides — one per table
  for (const table of allTables) {
    addTableSlide(pptx, table, theme)
  }

  // 6. Insights slide
  if (allInsights.length > 0) {
    addInsightsSlide(pptx, allInsights, theme)
  }

  // 7. Anomalies slide
  if (allAnomalies.length > 0) {
    addAnomaliesSlide(pptx, allAnomalies, theme)
  }

  // 8. Conclusions / last text from assistant
  if (lastText) {
    addTextSlide(pptx, 'Summary & Recommendations', lastText, theme)
  }

  // Generate arraybuffer for Tauri fs write
  const output = await pptx.write({ outputType: 'arraybuffer' })
  return output as ArrayBuffer
}

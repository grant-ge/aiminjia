/**
 * Playwright screenshot script for plan-E visual verification.
 * Usage: pnpm capture:ui
 *
 * NOTE: The app must be running (pnpm tauri:dev or pnpm dev) before running this script.
 * Captured PNGs land in tmp/ui-capture/ (gitignored).
 * Compare with baseline in docs/superpowers/specs/assets/design-pen-exports/
 *
 * If the app doesn't support ?ui= route params yet, navigate to each page
 * manually in the dev window and run: node scripts/capture-single.mjs <name> <url>
 */
import { mkdir } from 'node:fs/promises'
import path from 'node:path'
import { chromium } from 'playwright'

const OUT_DIR = path.resolve(process.cwd(), 'tmp/ui-capture')
const BASE_URL = process.env.UI_CAPTURE_BASE || 'http://localhost:1420'
const VIEWPORT = { width: 1280, height: 900 }

const PAGES = [
  { name: 'home', path: '/' },
  { name: 'skill-center', path: '/?route=skill-center' },
  { name: 'schedules', path: '/?route=schedules' },
  { name: 'login', path: '/?route=login' },
]

async function main() {
  await mkdir(OUT_DIR, { recursive: true })
  const browser = await chromium.launch()
  const ctx = await browser.newContext({ viewport: VIEWPORT })
  const page = await ctx.newPage()
  for (const p of PAGES) {
    const url = BASE_URL + p.path
    console.log(`[capture] ${p.name} → ${url}`)
    try {
      await page.goto(url, { waitUntil: 'networkidle', timeout: 15_000 })
    } catch (err) {
      console.warn(`[capture] issue for ${p.name}: ${err.message}`)
    }
    await page.waitForTimeout(600)
    const file = path.join(OUT_DIR, `${p.name}.png`)
    await page.screenshot({ path: file, fullPage: false })
    console.log(`[capture] saved ${file}`)
  }
  await browser.close()
}

main().catch((err) => {
  console.error(err)
  process.exit(1)
})

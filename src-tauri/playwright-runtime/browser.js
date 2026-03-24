/**
 * AI小家 Playwright Browser Sidecar
 *
 * Long-running Node.js process that controls Chromium via Playwright.
 * Communicates with Rust backend via stdin/stdout JSON line protocol.
 *
 * Protocol: one JSON object per line.
 *   Request:  {"id":1, "method":"navigate", "params":{"url":"..."}}
 *   Response: {"id":1, "result":{...}} or {"id":1, "error":"..."}
 */

const { chromium } = require('playwright');
const readline = require('readline');
const path = require('path');
const fs = require('fs');

let browser = null;
let context = null;
let page = null;

// ── Helpers ─────────────────────────────────────────────────────

function log(msg) {
  process.stderr.write(`[playwright] ${msg}\n`);
}

/** Extract tables from a single frame (same logic, runs in browser context). */
async function extractFromFrame(frame) {
  try {
    return await frame.evaluate(() => {
      const tables = [];
      document.querySelectorAll('table').forEach((t, i) => {
        if (i >= 10) return;
        const headers = [];
        // Only use <th> elements for headers (not <td> which could be data)
        const thEls = t.querySelectorAll('thead th, tr:first-child th');
        thEls.forEach(h => headers.push(h.innerText.trim()));
        const rows = [];
        const trEls = t.querySelectorAll('tbody tr, tr');
        // Skip first row only if it contained <th> elements we used as headers
        const startIdx = (thEls.length > 0 && !t.querySelector('thead')) ? 1 : 0;
        for (let r = startIdx; r < trEls.length && rows.length < 200; r++) {
          const cells = trEls[r].querySelectorAll('td');
          if (cells.length === 0) continue;
          const row = {};
          for (let c = 0; c < cells.length; c++) {
            const key = (c < headers.length) ? headers[c] : ('col_' + c);
            row[key] = cells[c].innerText.trim();
          }
          rows.push(row);
        }
        if (headers.length > 0 || rows.length > 0) {
          tables.push({ headers, rows });
        }
      });
      return tables;
    });
  } catch (e) {
    return []; // cross-origin or detached frame
  }
}

async function extractLinksFromFrame(frame) {
  try {
    return await frame.evaluate(() => {
      const links = [];
      const seen = {};

      // Menu items
      const menuSels = 'nav a, [role="menu"] a, .sidebar a, .ant-menu a, .el-menu a, ' +
        '.layui-nav a, .layui-side a, .left-nav a, #menu a, .menu a, ' +
        '.ant-menu-item, .el-menu-item, .el-sub-menu__title';
      document.querySelectorAll(menuSels).forEach(el => {
        if (links.length >= 80) return;
        const label = (el.innerText || el.title || '').trim().substring(0, 80).replace(/\n/g, ' ');
        if (!label) return;
        const href = el.href || el.getAttribute('data-href') || el.getAttribute('data-url') || '';
        const key = 'menu|' + label;
        if (seen[key]) return;
        seen[key] = true;
        links.push({ label, href, type: 'menu', selector: '' });
      });

      // Regular links
      document.querySelectorAll('a[href]').forEach(a => {
        if (links.length >= 120) return;
        const label = (a.innerText || a.title || '').trim().substring(0, 80).replace(/\n/g, ' ');
        const href = a.href || '';
        if (!label || !href || href === '#' || href.startsWith('javascript:')) return;
        const key = label + '|' + href;
        if (seen[key]) return;
        seen[key] = true;
        links.push({ label, href, type: 'link', selector: '' });
      });

      // Buttons
      document.querySelectorAll('button, [role="button"], input[type="submit"]').forEach(btn => {
        if (links.length >= 150) return;
        const label = (btn.innerText || btn.value || btn.title || '').trim().substring(0, 80).replace(/\n/g, ' ');
        if (!label) return;
        const key = 'btn|' + label;
        if (seen[key]) return;
        seen[key] = true;
        links.push({ label, href: '', type: 'button', selector: '' });
      });

      return links;
    });
  } catch (e) {
    return [];
  }
}

async function extractFormsFromFrame(frame) {
  try {
    return await frame.evaluate(() => {
      const forms = [];
      document.querySelectorAll('form').forEach((f, i) => {
        if (i >= 10) return;
        const fields = [];
        f.querySelectorAll('input, select, textarea').forEach(el => {
          const name = el.name || el.id || '';
          if (!name) return;
          let ftype = el.type || el.tagName.toLowerCase();
          let value = el.value || '';
          if (el.tagName === 'SELECT') {
            const opts = Array.from(el.options).map(o => o.value);
            value = opts.join(',');
            ftype = 'select(' + opts.length + ')';
          }
          fields.push({ name, fieldType: ftype, value: value.substring(0, 100) });
        });
        forms.push({
          id: f.id || ('form_' + i),
          action: f.action || '',
          method: (f.method || 'GET').toUpperCase(),
          fields,
        });
      });
      return forms;
    });
  } catch (e) {
    return [];
  }
}

async function extractTextFromFrame(frame) {
  try {
    return await frame.evaluate(() => {
      const el = document.querySelector('main') || document.querySelector('[role="main"]')
        || document.querySelector('.main-content') || document.querySelector('#app') || document.body;
      return el ? el.innerText.substring(0, 4000) : '';
    });
  } catch (e) {
    return '';
  }
}

// ── Command Handlers ────────────────────────────────────────────

async function handleLaunch(params) {
  if (browser) {
    log('Browser already running, reusing');
    return { ok: true };
  }

  const launchOpts = {
    headless: false,
    args: [
      '--disable-blink-features=AutomationControlled',
      '--disable-infobars',
      '--no-first-run',
      '--no-default-browser-check',
      '--disable-extensions',
      '--disable-session-crashed-bubble',
      '--hide-crash-restore-bubble',
      '--noerrdialogs',
      '--disable-features=ProfilePicker,ChromeWhatsNewUI,TranslateUI',
    ],
  };

  // Use user data dir for session persistence (cookies, login state)
  const userDataDir = params.userDataDir || null;
  if (userDataDir) {
    // Clean up crash markers to prevent "profile error" dialogs
    const crashFiles = ['Default/Preferences'];
    for (const f of crashFiles) {
      const fp = path.join(userDataDir, f);
      try {
        if (fs.existsSync(fp)) {
          const content = fs.readFileSync(fp, 'utf-8');
          // Remove exit_type: Crashed marker
          const fixed = content.replace(/"exit_type"\s*:\s*"Crashed"/g, '"exit_type":"Normal"');
          if (fixed !== content) {
            fs.writeFileSync(fp, fixed);
            log('Fixed crash marker in ' + f);
          }
        }
      } catch (e) { /* ignore */ }
    }
    // launchPersistentContext keeps cookies between sessions
    context = await chromium.launchPersistentContext(userDataDir, launchOpts);
    browser = context; // PersistentContext acts as both browser and context
    page = context.pages()[0] || await context.newPage();
  } else {
    browser = await chromium.launch(launchOpts);
    context = await browser.newContext();
    page = await context.newPage();
  }

  log('Browser launched');
  return { ok: true };
}

async function handleNavigate(params) {
  if (!page) return { error: 'Browser not launched' };

  const url = params.url;
  log(`navigate: ${url}`);

  try {
    await page.goto(url, {
      waitUntil: 'domcontentloaded',
      timeout: 30000,
    });
  } catch (e) {
    log(`goto error (may be redirect): ${e.message}`);
  }

  await page.waitForLoadState('networkidle', { timeout: 10000 }).catch(() => {});

  // Iframe detection: if data tables are in a child frame, navigate directly to iframe URL
  let iframeUrl = null;
  const frames = page.frames();
  if (frames.length > 1) {
    let mainFrameRows = 0;
    let bestIframeUrl = null;
    let bestIframeRows = 0;

    for (const frame of frames) {
      const frameTables = await extractFromFrame(frame);
      const rowCount = frameTables.reduce((sum, t) => sum + t.rows.length, 0);

      if (frame === page.mainFrame()) {
        mainFrameRows = rowCount;
      } else {
        const frameUrl = frame.url();
        if (rowCount > bestIframeRows && frameUrl && frameUrl !== 'about:blank') {
          bestIframeRows = rowCount;
          bestIframeUrl = frameUrl;
        }
      }
    }

    // If iframe has more data than main frame, navigate to iframe URL
    if (bestIframeRows > mainFrameRows && bestIframeUrl) {
      log(`navigate: detected iframe site — data in ${bestIframeUrl} (${bestIframeRows} rows vs main ${mainFrameRows})`);
      iframeUrl = bestIframeUrl;

      try {
        await page.goto(bestIframeUrl, {
          waitUntil: 'domcontentloaded',
          timeout: 30000,
        });
        await page.waitForLoadState('networkidle', { timeout: 10000 }).catch(() => {});
        log(`navigate: redirected to iframe URL`);
      } catch (e) {
        log(`navigate: iframe redirect failed: ${e.message}`);
      }
    }
  }

  const title = await page.title();
  const finalUrl = page.url();

  return { url: finalUrl, title, iframeUrl };
}

async function handleExtract(params) {
  if (!page) return { error: 'Browser not launched' };

  log('extract: scanning all frames');

  // Playwright's key advantage: page.frames() returns ALL frames including nested iframes
  const allFrames = page.frames();
  log(`extract: found ${allFrames.length} frames`);

  let tables = [];
  let links = [];
  let forms = [];
  let text = '';

  for (const frame of allFrames) {
    // Tables from every frame
    const frameTables = await extractFromFrame(frame);
    tables.push(...frameTables);

    // Links from every frame
    const frameLinks = await extractLinksFromFrame(frame);
    links.push(...frameLinks);

    // Forms from every frame
    const frameForms = await extractFormsFromFrame(frame);
    forms.push(...frameForms);

    // Text: pick the longest
    const frameText = await extractTextFromFrame(frame);
    if (frameText.length > text.length) {
      text = frameText;
    }
  }

  // Deduplicate links
  const seen = {};
  links = links.filter(l => {
    const key = l.type + '|' + l.label + '|' + l.href;
    if (seen[key]) return false;
    seen[key] = true;
    return true;
  });

  log(`extract complete: tables=${tables.length}, links=${links.length}, forms=${forms.length}, text_len=${text.length}`);

  return {
    url: page.url(),
    title: await page.title(),
    tables,
    links,
    forms,
    text,
  };
}

async function handleExecuteJs(params) {
  if (!page) return { error: 'Browser not launched' };

  const script = params.script;
  log(`execute_js: script_len=${script.length}`);

  const urlBefore = page.url();

  try {
    const result = await page.evaluate(async (code) => {
      try {
        const fn = new Function('return (async () => {' + code + '})()');
        const value = await fn();
        return {
          type: 'result',
          value: value === undefined ? null : value,
          url: window.location.href,
          title: document.title,
        };
      } catch (e) {
        return {
          type: 'error',
          error: e.message,
          url: window.location.href,
          title: document.title,
        };
      }
    }, script);

    // Check if navigation happened
    const urlAfter = page.url();
    if (urlAfter !== urlBefore) {
      await page.waitForLoadState('networkidle', { timeout: 5000 }).catch(() => {});
      result.url = page.url();
      result.title = await page.title();
    }

    return result;
  } catch (e) {
    return { type: 'error', error: e.message, url: page.url(), title: await page.title() };
  }
}

async function handleFetch(params) {
  if (!page) return { error: 'Browser not launched' };

  log(`fetch: ${params.method || 'GET'} ${params.url}`);

  try {
    const result = await page.evaluate(async (p) => {
      const opts = {
        method: p.method || 'GET',
        headers: Object.assign(
          { 'Accept': 'application/json', 'Content-Type': 'application/json' },
          p.headers || {}
        ),
      };
      if (p.body) opts.body = typeof p.body === 'string' ? p.body : JSON.stringify(p.body);

      const resp = await fetch(p.url, opts);
      const ct = resp.headers.get('content-type') || '';
      const text = await resp.text();

      let data = null;
      let totalRows = null;

      if (ct.includes('json')) {
        try {
          data = JSON.parse(text);
          if (Array.isArray(data)) {
            totalRows = data.length;
          } else if (data && typeof data === 'object') {
            const arr = data.list || data.rows || data.data || data.items || data.records || data.content;
            if (Array.isArray(arr)) {
              totalRows = data.total || data.totalCount || data.count || arr.length;
            }
          }
        } catch (e) {
          data = text;
        }
      } else {
        data = text.substring(0, 10000);
      }

      return { status: resp.status, contentType: ct, data, totalRows };
    }, params);

    return result;
  } catch (e) {
    return { status: 0, contentType: '', data: e.message, totalRows: null };
  }
}

async function handleScreenshot(params) {
  if (!page) return { error: 'Browser not launched' };

  const screenshotDir = params.dir || '/tmp';
  const filename = `page_${Date.now()}.png`;
  const filePath = path.join(screenshotDir, filename);

  await page.screenshot({ path: filePath, type: 'png' });
  const stats = fs.statSync(filePath);

  log(`screenshot: ${filePath} (${stats.size} bytes)`);
  return { path: filePath, size: stats.size };
}

async function handleShowPage() {
  if (!page) return { error: 'Browser not launched' };
  await page.bringToFront();
  return { ok: true };
}

async function handleShutdown() {
  log('shutting down');
  if (browser) {
    await browser.close().catch(() => {});
    browser = null;
    context = null;
    page = null;
  }
  return { ok: true };
}

/**
 * Extract table data from the current page (all frames).
 * Does NOT auto-paginate — just reads what's visible now.
 * Returns the largest table's data + pagination info for the LLM to decide next steps.
 *
 * params.savePath — if set, append rows to this JSON file (for incremental collection)
 */
async function handleExtractTableData(params) {
  if (!page) return { error: 'Browser not launched' };

  const savePath = params.savePath || null;

  log('extract_table_data: extracting current page');

  // Extract from all frames
  const allFrames = page.frames();
  let allTables = [];
  for (const frame of allFrames) {
    const frameTables = await extractFromFrame(frame);
    allTables.push(...frameTables);
  }

  // Pick the table with most rows (data table)
  let bestTable = { headers: [], rows: [] };
  let bestScore = -1;
  for (const t of allTables) {
    const score = t.rows.length * 100 + t.headers.length;
    if (score > bestScore) {
      bestScore = score;
      bestTable = t;
    }
  }

  // If selected table has rows but no headers, borrow from header-only table
  let headers = bestTable.headers;
  let rows = bestTable.rows;
  if (headers.length === 0 && rows.length > 0) {
    const colCount = Object.keys(rows[0]).length;
    for (const t of allTables) {
      if (t.headers.length >= colCount && t.rows.length === 0) {
        headers = t.headers;
        log(`extract_table_data: borrowed ${headers.length} headers from header-only table`);
        break;
      }
    }
  }

  // Detect pagination info from all frames
  let paginationInfo = { total: 0, currentPage: 0, totalPages: 0, hasNext: false, hasPager: false };
  for (const frame of allFrames) {
    try {
      const info = await frame.evaluate(() => {
        const text = document.body.innerText;
        const patterns = [/共\s*(\d+)\s*条/, /共有数据[：:]\s*(\d+)\s*条/, /total[:\s]*(\d+)/i];
        let total = 0;
        for (const p of patterns) { const m = text.match(p); if (m) { total = parseInt(m[1]); break; } }
        const activePage = document.querySelector('.layui-laypage-curr em, .ant-pagination-item-active a, .el-pager .active, .pagination .active');
        let currentPage = activePage ? (parseInt(activePage.textContent) || 0) : 0;
        const totalPagesMatch = text.match(/共\s*(\d+)\s*页/);
        let totalPages = totalPagesMatch ? parseInt(totalPagesMatch[1]) : 0;
        const nextBtn = document.querySelector('.layui-laypage-next:not(.layui-disabled), .ant-pagination-next:not(.ant-pagination-disabled), .el-pagination .btn-next:not(.disabled), a.next:not(.disabled)');
        const hasPager = !!document.querySelector('.layui-laypage, .ant-pagination, .el-pagination, .pagination');
        return { total, currentPage, totalPages, hasNext: !!nextBtn, hasPager };
      });
      if (info.total > paginationInfo.total) paginationInfo.total = info.total;
      if (info.currentPage > paginationInfo.currentPage) paginationInfo.currentPage = info.currentPage;
      if (info.totalPages > paginationInfo.totalPages) paginationInfo.totalPages = info.totalPages;
      if (info.hasNext) paginationInfo.hasNext = true;
      if (info.hasPager) paginationInfo.hasPager = true;
    } catch (e) {}
  }

  log(`extract_table_data: ${rows.length} rows, ${headers.length} headers, pagination: total=${paginationInfo.total}, page=${paginationInfo.currentPage}, hasNext=${paginationInfo.hasNext}`);

  // If savePath specified, append rows to file
  if (savePath && rows.length > 0) {
    let existing = { headers: [], rows: [], totalRows: 0 };
    try {
      if (fs.existsSync(savePath)) {
        const raw = JSON.parse(fs.readFileSync(savePath, 'utf-8'));
        existing.headers = raw.headers || [];
        existing.rows = Array.isArray(raw.rows) ? raw.rows : [];
        existing.totalRows = existing.rows.length;
      }
    } catch (e) { /* start fresh */ }
    if (headers.length > 0) existing.headers = headers;
    existing.rows.push(...rows);
    existing.totalRows = existing.rows.length;
    fs.writeFileSync(savePath, JSON.stringify(existing, null, 2));
    log(`extract_table_data: appended ${rows.length} rows to ${savePath} (total: ${existing.totalRows})`);
  }

  return {
    rows: rows.length,
    headers,
    sampleRows: rows.slice(0, 3),
    pagination: paginationInfo,
    url: page.url(),
    savedTo: savePath || null,
    totalSaved: savePath ? (() => { try { return JSON.parse(fs.readFileSync(savePath, 'utf-8')).totalRows; } catch(e) { return 0; } })() : 0,
  };
}

/**
 * Extract ALL table data by looping: extract current page → run pagination JS → repeat.
 * The LLM provides the pagination JS code (e.g. click next button).
 * This function handles the deterministic loop — no LLM involvement per page.
 *
 * params.savePath — where to save the JSON file (required)
 * params.paginationJs — JS code to execute to flip to the next page (required)
 * params.maxPages — max pages to extract (default 100)
 * params.waitMs — ms to wait after pagination JS before extracting (default 1000)
 */

/**
 * Extract ALL paginated data using HTTP replay — no DOM manipulation needed.
 *
 * Strategy:
 * 1. Use current page URL (after iframe redirect) as the data URL
 * 2. Try URL pagination via page.request.get() (fast HTTP, no page render)
 * 3. Parse tables from HTML response using DOMParser (in browser context)
 * 4. If URL pagination fails, fall back to click-based pagination
 *
 * params.savePath — where to save JSON file (required)
 * params.paginationJs — JS to click next button (fallback only)
 * params.maxPages — max pages (default 100)
 * params.pageSize — rows per page (default 100)
 */
async function handleExtractWithPagination(params) {
  if (!page) return { error: 'Browser not launched' };

  const savePath = params.savePath;
  const paginationJs = params.paginationJs || '';
  const maxPages = params.maxPages || 100;
  const pageSize = params.pageSize || 100;

  if (!savePath) return { error: 'savePath is required' };

  const dataUrl = page.url();
  log(`extract_with_pagination: dataUrl=${dataUrl}, maxPages=${maxPages}, pageSize=${pageSize}`);

  // Helper: parse tables from raw HTML string using browser's DOMParser
  async function parseTablesFromHTML(html) {
    return await page.evaluate((htmlStr) => {
      const doc = new DOMParser().parseFromString(htmlStr, 'text/html');
      const tables = [];
      doc.querySelectorAll('table').forEach(t => {
        const headers = [];
        t.querySelectorAll('thead th, tr:first-child th').forEach(h => headers.push(h.textContent.trim()));
        const rows = [];
        const trs = t.querySelectorAll('tbody tr, tr');
        const startIdx = (headers.length > 0 && !t.querySelector('thead')) ? 1 : 0;
        for (let r = startIdx; r < trs.length; r++) {
          const cells = trs[r].querySelectorAll('td');
          if (cells.length < 3) continue; // Skip tiny rows
          const row = {};
          for (let c = 0; c < cells.length; c++) {
            const key = (c < headers.length) ? headers[c] : ('col_' + c);
            row[key] = cells[c].textContent.trim();
          }
          rows.push(row);
        }
        if (rows.length > 0) tables.push({ headers, rows });
      });
      return tables;
    }, html);
  }

  // Step 1: Get headers from current page (live DOM, most reliable)
  let headers = [];
  const allFrames = page.frames();
  for (const frame of allFrames) {
    const frameTables = await extractFromFrame(frame);
    for (const t of frameTables) {
      if (t.headers.length > headers.length) headers = t.headers;
    }
  }
  log(`extract_with_pagination: got ${headers.length} headers from live DOM`);

  // Step 1.5: Determine the actual data URL
  // If current page has iframes with data, use the iframe URL instead
  let dataUrlToUse = dataUrl;
  const frames = page.frames();
  if (frames.length > 1) {
    for (const frame of frames) {
      if (frame === page.mainFrame()) continue;
      const frameUrl = frame.url();
      if (frameUrl && frameUrl !== 'about:blank') {
        const frameTables = await extractFromFrame(frame);
        const frameRows = frameTables.reduce((sum, t) => sum + t.rows.length, 0);
        if (frameRows > 0) {
          dataUrlToUse = frameUrl;
          log(`extract_with_pagination: using iframe URL for HTTP pagination: ${frameUrl} (${frameRows} rows)`);
          break;
        }
      }
    }
  }

  // Step 2: Try URL-based pagination via HTTP requests
  let allRows = [];
  let totalPages = 0;
  let urlPaginationWorks = false;
  let lastPageHash = null;

  const urlObj = new URL(dataUrlToUse);

  // Detect existing page param names
  let pageParam = null;
  let pageSizeParam = null;
  for (const p of ['page', 'pageNum', 'pageNo', 'p', 'currentPage']) {
    if (urlObj.searchParams.has(p)) { pageParam = p; break; }
  }
  for (const p of ['pageSize', 'size', 'limit', 'rows', 'per_page']) {
    if (urlObj.searchParams.has(p)) { pageSizeParam = p; break; }
  }
  if (!pageParam) pageParam = 'page';
  if (!pageSizeParam) pageSizeParam = 'pageSize';

  // Test page 1 via HTTP
  urlObj.searchParams.set(pageParam, '1');
  urlObj.searchParams.set(pageSizeParam, String(pageSize));
  const testUrl = urlObj.toString();

  log(`extract_with_pagination: testing HTTP pagination — ${testUrl}`);

  try {
    const resp = await page.request.get(testUrl);
    if (resp.ok()) {
      const html = await resp.text();
      const tables = await parseTablesFromHTML(html);

      // Pick best table (most rows)
      let bestRows = [];
      for (const t of tables) {
        if (t.rows.length > bestRows.length) {
          bestRows = t.rows;
          if (t.headers.length > 0 && headers.length === 0) headers = t.headers;
        }
      }

      if (bestRows.length > 0) {
        urlPaginationWorks = true;
        allRows = bestRows;
        totalPages = 1;
        lastPageHash = JSON.stringify(bestRows.slice(0, 3).map(r => Object.values(r).join('|')));
        log(`extract_with_pagination: HTTP pagination works! page 1 → ${bestRows.length} rows`);
      } else {
        log(`extract_with_pagination: HTTP response OK but no table rows found (${tables.length} tables, html ${html.length} chars)`);
      }
    }
  } catch (e) {
    log(`extract_with_pagination: HTTP test failed: ${e.message}`);
  }

  if (urlPaginationWorks) {
    // Loop through remaining pages via HTTP (fast, no DOM)
    for (let p = 2; p <= maxPages; p++) {
      urlObj.searchParams.set(pageParam, String(p));
      const pageUrl = urlObj.toString();

      try {
        const resp = await page.request.get(pageUrl);
        if (!resp.ok()) {
          log(`extract_with_pagination: page ${p} HTTP ${resp.status()}, stopping`);
          break;
        }
        const html = await resp.text();
        const tables = await parseTablesFromHTML(html);

        let pageRows = [];
        for (const t of tables) {
          if (t.rows.length > pageRows.length) pageRows = t.rows;
        }

        if (pageRows.length === 0) {
          log(`extract_with_pagination: page ${p} has 0 rows, stopping`);
          break;
        }

        // Dedup check
        const pageHash = JSON.stringify(pageRows.slice(0, 3).map(r => Object.values(r).join('|')));
        if (pageHash === lastPageHash) {
          log(`extract_with_pagination: page ${p} is duplicate, stopping`);
          break;
        }
        lastPageHash = pageHash;

        allRows.push(...pageRows);
        totalPages = p;
        log(`extract_with_pagination: page ${p} → ${pageRows.length} rows (total: ${allRows.length})`);
      } catch (e) {
        log(`extract_with_pagination: page ${p} error: ${e.message}, stopping`);
        break;
      }
    }
  } else if (paginationJs) {
    // Fallback: click-based pagination from live DOM
    log('extract_with_pagination: HTTP pagination failed, falling back to click-based');

    // Extract first page from live DOM
    for (const frame of page.frames()) {
      const frameTables = await extractFromFrame(frame);
      for (const t of frameTables) {
        if (t.rows.length > allRows.length) allRows = t.rows;
      }
    }
    totalPages = 1;
    lastPageHash = allRows.length > 0 ? JSON.stringify(allRows.slice(0, 3).map(r => Object.values(r).join('|'))) : null;

    for (let p = 2; p <= maxPages; p++) {
      // Click next page
      let clicked = false;
      const selectorMatch = paginationJs.match(/querySelector\(['"]([^'"]+)['"]\)/);
      for (const frame of page.frames()) {
        try {
          if (selectorMatch) {
            const el = await frame.$(selectorMatch[1]);
            if (el) { await el.click(); clicked = true; break; }
          }
          await frame.evaluate((code) => new Function(code)(), paginationJs);
          clicked = true;
          break;
        } catch (e) {}
      }
      if (!clicked) {
        log(`extract_with_pagination: click pagination failed on page ${p - 1}, stopping`);
        break;
      }

      await page.waitForLoadState('networkidle', { timeout: 5000 }).catch(() => {});
      await new Promise(r => setTimeout(r, 800));

      let pageRows = [];
      for (const frame of page.frames()) {
        const frameTables = await extractFromFrame(frame);
        for (const t of frameTables) {
          if (t.rows.length > pageRows.length) pageRows = t.rows;
        }
      }
      if (pageRows.length === 0) break;
      const pageHash = JSON.stringify(pageRows.slice(0, 3).map(r => Object.values(r).join('|')));
      if (pageHash === lastPageHash) break;
      lastPageHash = pageHash;
      allRows.push(...pageRows);
      totalPages = p;
      log(`extract_with_pagination: click page ${p} → ${pageRows.length} rows (total: ${allRows.length})`);
    }
  } else {
    return { error: 'No pagination method available. Provide paginationJs or ensure URL supports page param.' };
  }

  log(`extract_with_pagination: complete — ${allRows.length} total rows, ${totalPages} pages`);

  // Save to file
  const output = { headers, rows: allRows, totalRows: allRows.length, totalPages };
  fs.writeFileSync(savePath, JSON.stringify(output, null, 2));
  const stats = fs.statSync(savePath);
  log(`extract_with_pagination: saved to ${savePath} (${stats.size} bytes)`);

  return {
    totalRows: allRows.length,
    totalPages,
    headers,
    savedTo: savePath,
    fileSize: stats.size,
    sampleRows: allRows.slice(0, 3),
  };
}
// ── Main Loop ───────────────────────────────────────────────────

const HANDLERS = {
  launch: handleLaunch,
  navigate: handleNavigate,
  extract: handleExtract,
  extract_table_data: handleExtractTableData,
  extract_with_pagination: handleExtractWithPagination,
  execute_js: handleExecuteJs,
  fetch: handleFetch,
  screenshot: handleScreenshot,
  show_page: handleShowPage,
  shutdown: handleShutdown,
};

const rl = readline.createInterface({ input: process.stdin, terminal: false });

rl.on('line', async (line) => {
  let req;
  try {
    req = JSON.parse(line.trim());
  } catch (e) {
    process.stdout.write(JSON.stringify({ id: null, error: 'Invalid JSON' }) + '\n');
    return;
  }

  const { id, method, params } = req;
  const handler = HANDLERS[method];

  if (!handler) {
    process.stdout.write(JSON.stringify({ id, error: `Unknown method: ${method}` }) + '\n');
    return;
  }

  try {
    const result = await handler(params || {});
    process.stdout.write(JSON.stringify({ id, result }) + '\n');
  } catch (e) {
    log(`handler error: ${method}: ${e.message}`);
    process.stdout.write(JSON.stringify({ id, error: e.message }) + '\n');
  }

  // After shutdown, exit process
  if (method === 'shutdown') {
    process.exit(0);
  }
});

// Handle process termination
process.on('SIGTERM', async () => {
  await handleShutdown();
  process.exit(0);
});

process.on('SIGINT', async () => {
  await handleShutdown();
  process.exit(0);
});

log('Playwright sidecar ready, waiting for commands on stdin');

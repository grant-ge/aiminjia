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
        const thEls = t.querySelectorAll('thead th, thead td, tr:first-child th');
        if (thEls.length === 0) {
          const firstRow = t.querySelector('tr');
          if (firstRow) firstRow.querySelectorAll('td, th').forEach(h => headers.push(h.innerText.trim()));
        } else {
          thEls.forEach(h => headers.push(h.innerText.trim()));
        }
        const rows = [];
        const trEls = t.querySelectorAll('tbody tr, tr');
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
    ],
  };

  // Use user data dir for session persistence (cookies, login state)
  const userDataDir = params.userDataDir || null;
  if (userDataDir) {
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
    // Page may have loaded despite error (e.g., redirect)
  }

  // Wait for network to settle (best-effort, don't block forever)
  await page.waitForLoadState('networkidle', { timeout: 10000 }).catch(() => {});

  const title = await page.title();
  const finalUrl = page.url();

  return { url: finalUrl, title };
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
 * Auto-paginate and extract ALL table data from the current page.
 * Detects pagination pattern (URL param or next button), iterates through
 * all pages, merges all table rows. Saves result to JSON file.
 *
 * params.savePath — where to save the JSON file
 * params.maxPages — max pages to fetch (default 50)
 * params.pageSize — override pageSize param (default: detected from URL or 100)
 */
async function handleExtractAllPages(params) {
  if (!page) return { error: 'Browser not launched' };

  const savePath = params.savePath;
  const maxPages = params.maxPages || 50;
  const pageSizeOverride = params.pageSize || null;

  log(`extract_all_pages: savePath=${savePath}, maxPages=${maxPages}`);

  // Step 1: Extract current page tables to determine headers
  const allFrames = page.frames();
  let headers = [];
  let currentRows = [];

  for (const frame of allFrames) {
    const frameTables = await extractFromFrame(frame);
    for (const t of frameTables) {
      if (t.headers.length > headers.length) {
        headers = t.headers;
      }
      currentRows.push(...t.rows);
    }
  }

  if (headers.length === 0 && currentRows.length === 0) {
    return { error: 'No tables found on current page', totalRows: 0 };
  }

  log(`extract_all_pages: first page has ${currentRows.length} rows, ${headers.length} headers`);

  // Step 2: Detect pagination from URL
  const currentUrl = page.url();
  const urlObj = new URL(currentUrl);
  let pageParam = null;
  let pageSizeParam = null;

  // Common pagination param names
  for (const p of ['page', 'pageNum', 'pageNo', 'p', 'currentPage']) {
    if (urlObj.searchParams.has(p)) {
      pageParam = p;
      break;
    }
  }
  for (const p of ['pageSize', 'size', 'limit', 'rows', 'per_page']) {
    if (urlObj.searchParams.has(p)) {
      pageSizeParam = p;
      break;
    }
  }

  // If no page param in URL, try to detect from page content
  if (!pageParam) {
    // Check if there's pagination info in the page (e.g. "共 X 条" or page controls)
    const paginationInfo = await page.evaluate(() => {
      const text = document.body.innerText;
      const totalMatch = text.match(/共\s*(\d+)\s*条/);
      return { total: totalMatch ? parseInt(totalMatch[1]) : 0 };
    }).catch(() => ({ total: 0 }));

    if (paginationInfo.total > currentRows.length) {
      // There's more data but no page param — try adding page param
      pageParam = 'page';
      urlObj.searchParams.set('page', '1');
    } else {
      // All data is on one page
      log(`extract_all_pages: all data on one page (${currentRows.length} rows)`);
      if (savePath) {
        fs.writeFileSync(savePath, JSON.stringify(currentRows, null, 2));
        log(`extract_all_pages: saved to ${savePath}`);
      }
      return { totalRows: currentRows.length, totalPages: 1, headers, savedTo: savePath || null };
    }
  }

  // Step 3: Set page size larger if possible
  const effectivePageSize = pageSizeOverride ||
    (pageSizeParam ? parseInt(urlObj.searchParams.get(pageSizeParam)) : null) || 100;

  if (pageSizeParam) {
    urlObj.searchParams.set(pageSizeParam, String(effectivePageSize));
  } else {
    urlObj.searchParams.set('pageSize', String(effectivePageSize));
  }

  // Step 4: Paginate
  let allRows = [];
  let totalPages = 0;

  for (let p = 1; p <= maxPages; p++) {
    urlObj.searchParams.set(pageParam, String(p));
    const pageUrl = urlObj.toString();

    log(`extract_all_pages: fetching page ${p} — ${pageUrl}`);

    await page.goto(pageUrl, { waitUntil: 'domcontentloaded', timeout: 30000 }).catch(() => {});
    await page.waitForLoadState('networkidle', { timeout: 8000 }).catch(() => {});

    // Extract tables from all frames
    const pageFrames = page.frames();
    let pageRows = [];
    for (const frame of pageFrames) {
      const frameTables = await extractFromFrame(frame);
      for (const t of frameTables) {
        pageRows.push(...t.rows);
      }
    }

    if (pageRows.length === 0) {
      log(`extract_all_pages: page ${p} has 0 rows, stopping`);
      break;
    }

    allRows.push(...pageRows);
    totalPages = p;
    log(`extract_all_pages: page ${p} → ${pageRows.length} rows (total: ${allRows.length})`);

    // If we got fewer rows than page size, we're on the last page
    if (pageRows.length < effectivePageSize * 0.8) {
      log(`extract_all_pages: last page detected (${pageRows.length} < ${effectivePageSize})`);
      break;
    }
  }

  log(`extract_all_pages: complete — ${allRows.length} total rows, ${totalPages} pages`);

  // Step 5: Save to file
  if (savePath) {
    const output = { headers, rows: allRows, totalRows: allRows.length, totalPages };
    fs.writeFileSync(savePath, JSON.stringify(output, null, 2));
    const stats = fs.statSync(savePath);
    log(`extract_all_pages: saved to ${savePath} (${stats.size} bytes)`);
  }

  return {
    totalRows: allRows.length,
    totalPages,
    headers,
    savedTo: savePath || null,
    sampleRows: allRows.slice(0, 3),
  };
}

// ── Main Loop ───────────────────────────────────────────────────

const HANDLERS = {
  launch: handleLaunch,
  navigate: handleNavigate,
  extract: handleExtract,
  extract_all_pages: handleExtractAllPages,
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

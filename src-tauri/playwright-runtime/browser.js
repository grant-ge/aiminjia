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

// ── Main Loop ───────────────────────────────────────────────────

const HANDLERS = {
  launch: handleLaunch,
  navigate: handleNavigate,
  extract: handleExtract,
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

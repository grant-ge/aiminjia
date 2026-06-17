#!/usr/bin/env python3
"""Generate downloads.html listing all available builds and upload to OSS.

Scans the OSS bucket for dev/, beta/, and release builds, then generates
an HTML page with download links.

Env vars required:
  OSS_ACCESS_KEY_ID
  OSS_ACCESS_KEY_SECRET
"""

import json
import os
import sys
from datetime import datetime, timezone

import oss2

BUCKET_NAME = "lotus-releases"
ENDPOINT = "https://oss-cn-beijing.aliyuncs.com"
CDN_BASE = "https://lotus.renlijia.com"
OSS_PREFIX = "aijia"


def list_files(bucket, prefix):
    """List all files under a prefix, return list of (key, size, last_modified)."""
    files = []
    for obj in oss2.ObjectIteratorV2(bucket, prefix=prefix):
        if obj.key.endswith("/") or obj.key.endswith(".html"):
            continue
        files.append({
            "key": obj.key,
            "name": obj.key.split("/")[-1],
            "size": obj.size,
            "modified": obj.last_modified,
            "url": f"{CDN_BASE}/{obj.key}",
        })
    return files


def format_size(size):
    if size > 1024 * 1024:
        return f"{size / 1024 / 1024:.1f} MB"
    return f"{size / 1024:.0f} KB"


def format_time(ts):
    if isinstance(ts, (int, float)):
        dt = datetime.fromtimestamp(ts, tz=timezone.utc)
    else:
        dt = ts
    return dt.strftime("%Y-%m-%d %H:%M UTC")


def detect_platform(name):
    """Return a human-readable platform label for a release artifact filename."""
    n = name.lower()
    if n.endswith(".msi.sig"):
        return "Windows updater sig"
    if n.endswith(".msi"):
        return "Windows x64"
    if n.endswith(".exe.sig"):
        return "Windows updater sig"
    if n.endswith(".exe"):
        return "Windows x64"
    if n.endswith(".dmg"):
        if "aarch64" in n or "arm64" in n:
            return "macOS Apple Silicon"
        if "x64" in n or "x86_64" in n:
            return "macOS Intel"
        return "macOS"
    if n.endswith(".tar.gz.sig"):
        return "macOS Intel updater sig" if "x64" in n or "x86_64" in n else "macOS Apple Silicon updater sig"
    if n.endswith(".tar.gz"):
        return "macOS Intel updater" if "x64" in n or "x86_64" in n else "macOS Apple Silicon updater"
    if n.endswith(".sig"):
        return "signature"
    return "—"


def _platform_sort_key(name):
    # Sort: macOS arm64 dmg → macOS x64 dmg → Windows installer → updaters/sigs
    order = [
        (lambda n: n.endswith(".dmg") and ("aarch64" in n or "arm64" in n), 0),
        (lambda n: n.endswith(".dmg") and ("x64" in n or "x86_64" in n), 1),
        (lambda n: n.endswith(".dmg"), 2),
        (lambda n: n.endswith(".msi") or n.endswith(".exe"), 3),
        (lambda n: n.endswith(".tar.gz"), 4),
        (lambda n: True, 5),
    ]
    nl = name.lower()
    for pred, rank in order:
        if pred(nl):
            return (rank, nl)
    return (99, nl)


def _semver_key(v):
    # "0.5.24-beta.3" -> (0, 5, 24, 0, beta, 3) ; release outranks any -tag.
    base = v.lstrip("v")
    if "-" in base:
        core, tag = base.split("-", 1)
    else:
        core, tag = base, ""
    parts = [int(x) if x.isdigit() else 0 for x in core.split(".")]
    while len(parts) < 3: parts.append(0)
    # release (no tag) should sort after any pre-release of same core,
    # so put empty tag at the END alphabetically by using a sentinel.
    tag_key = (1, "") if tag == "" else (0, tag)
    return tuple(parts[:3]) + tag_key


def _pick_installers(files):
    """From a version's files, pick the user-facing installers per platform.

    Returns (windows_installer, mac_arm_dmg, mac_intel_dmg) — each a file dict or None.
    Updater tar.gz / .sig are skipped (those are for the auto-updater, not humans).
    """
    win = mac_arm = mac_intel = None
    for f in files:
        n = f["name"].lower()
        if n.endswith(".msi"):
            win = f
        elif n.endswith(".exe") and win is None:
            # Historical releases used NSIS .exe. Keep them visible, but prefer
            # the MSI when both are present for a version.
            win = f
        elif n.endswith(".dmg"):
            if "aarch64" in n or "arm64" in n:
                mac_arm = f
            elif "x64" in n or "x86_64" in n:
                mac_intel = f
    return win, mac_arm, mac_intel


def _hero_data(release_versions):
    """Build the 'recommended download' payload from the latest release."""
    if not release_versions:
        return None
    ver = sorted(release_versions.keys(), key=_semver_key, reverse=True)[0]
    win, mac_arm, mac_intel = _pick_installers(release_versions[ver])
    return {
        "version": ver.lstrip("v"),
        "win": win["url"] if win else "",
        "macArm": mac_arm["url"] if mac_arm else "",
        "macIntel": mac_intel["url"] if mac_intel else "",
    }


HERO_SCRIPT = """<script>
(function () {
  var REL = __REL_JSON__;
  var hero = document.getElementById('hero');
  if (!hero || !REL) return;
  var titleEl = document.getElementById('heroTitle');
  var btn = document.getElementById('heroBtn');

  function show(label, url) {
    if (!url) { hero.style.display = 'none'; return; }
    titleEl.textContent = '为你的系统推荐 · ' + label + ' v' + REL.version;
    btn.textContent = '下载 ' + label;
    btn.href = url;
    hero.style.display = '';
  }
  function pickMac(arch) {
    if (arch === 'intel' && REL.macIntel) show('macOS (Intel)', REL.macIntel);
    else if (REL.macArm) show('macOS (Apple Silicon)', REL.macArm);
    else if (REL.macIntel) show('macOS (Intel)', REL.macIntel);
    else show('macOS', '');
  }
  // Browsers report navigator.platform as "MacIntel" even on Apple Silicon,
  // so distinguish via the WebGL renderer string (or UA-CH architecture).
  function macArchFromWebGL() {
    try {
      var c = document.createElement('canvas');
      var gl = c.getContext('webgl') || c.getContext('experimental-webgl');
      if (gl) {
        var ext = gl.getExtension('WEBGL_debug_renderer_info');
        if (ext) {
          var r = (gl.getParameter(ext.UNMASKED_RENDERER_WEBGL) || '') + '';
          if (/intel|amd|radeon/i.test(r)) return 'intel';
          if (/apple/i.test(r)) return 'arm';
        }
      }
    } catch (e) {}
    return 'unknown';
  }

  var ua = navigator.userAgent || '';
  var plat = (navigator.platform || '') + ' ' + ua;
  var isWin = /Win/i.test(plat);
  var isMac = /Mac/i.test(plat) && !/iPhone|iPad|iPod/i.test(ua);

  if (isWin) { show('Windows', REL.win); return; }
  if (isMac) {
    if (navigator.userAgentData && navigator.userAgentData.getHighEntropyValues) {
      navigator.userAgentData.getHighEntropyValues(['architecture']).then(function (v) {
        var a = v && v.architecture;
        pickMac(a === 'arm' ? 'arm' : (a ? 'intel' : macArchFromWebGL()));
      }).catch(function () { pickMac(macArchFromWebGL()); });
    } else {
      pickMac(macArchFromWebGL());
    }
    return;
  }
  show('', ''); // unknown OS (e.g. Linux) — fall back to the full list below
})();
</script>"""


def generate_html(dev_files, beta_versions, release_versions):
    sections = []

    # Dev section
    if dev_files:
        rows = ""
        for f in sorted(dev_files, key=lambda x: _platform_sort_key(x["name"])):
            rows += f'<tr><td>{detect_platform(f["name"])}</td><td><a href="{f["url"]}">{f["name"]}</a></td><td>{format_size(f["size"])}</td><td>{format_time(f["modified"])}</td></tr>\n'
        sections.append(f"""
        <div class="section">
            <h2>Dev Builds <span class="badge dev">unsigned</span></h2>
            <p>Latest CI builds. Not signed — macOS users need <code>xattr -cr</code>, Windows may show SmartScreen warning.</p>
            <table><thead><tr><th>Platform</th><th>File</th><th>Size</th><th>Built</th></tr></thead><tbody>{rows}</tbody></table>
        </div>""")

    # Beta section — only the single newest beta (testers want the latest;
    # older betas just clutter the page). Sort by semver, not lexicographic.
    for ver, files in sorted(beta_versions.items(), key=lambda kv: _semver_key(kv[0]), reverse=True)[:1]:
        rows = ""
        for f in sorted(files, key=lambda x: _platform_sort_key(x["name"])):
            rows += f'<tr><td>{detect_platform(f["name"])}</td><td><a href="{f["url"]}">{f["name"]}</a></td><td>{format_size(f["size"])}</td></tr>\n'
        released = max((f["modified"] for f in files), default=None)
        released_html = f' <span class="released">released {format_time(released)}</span>' if released else ""
        sections.append(f"""
        <div class="section">
            <h2>Beta {ver} <span class="badge beta">beta</span>{released_html}</h2>
            <table><thead><tr><th>Platform</th><th>File</th><th>Size</th></tr></thead><tbody>{rows}</tbody></table>
        </div>""")

    # Release section — sort by semver, not lexicographic.
    for ver, files in sorted(release_versions.items(), key=lambda kv: _semver_key(kv[0]), reverse=True)[:5]:
        rows = ""
        for f in sorted(files, key=lambda x: _platform_sort_key(x["name"])):
            rows += f'<tr><td>{detect_platform(f["name"])}</td><td><a href="{f["url"]}">{f["name"]}</a></td><td>{format_size(f["size"])}</td></tr>\n'
        released = max((f["modified"] for f in files), default=None)
        released_html = f' <span class="released">released {format_time(released)}</span>' if released else ""
        sections.append(f"""
        <div class="section">
            <h2>Release {ver} <span class="badge release">release</span>{released_html}</h2>
            <table><thead><tr><th>Platform</th><th>File</th><th>Size</th></tr></thead><tbody>{rows}</tbody></table>
        </div>""")

    hero = _hero_data(release_versions)
    rel_json = json.dumps(hero, ensure_ascii=False) if hero else "null"
    hero_html = """
<div class="hero" id="hero" style="display:none">
  <div class="hero-title" id="heroTitle">推荐下载</div>
  <a class="hero-btn" id="heroBtn" href="#all">下载</a>
  <div class="hero-sub"><a href="#all">其他平台与历史版本 ↓</a></div>
</div>""" if hero else ""

    page = f"""<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>AIjia Downloads</title>
<style>
  * {{ margin: 0; padding: 0; box-sizing: border-box; }}
  body {{ font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif;
         max-width: 800px; margin: 40px auto; padding: 0 20px; color: #333; background: #f8f9fa; }}
  h1 {{ margin-bottom: 8px; }}
  .subtitle {{ color: #666; margin-bottom: 32px; }}
  .hero {{ background: linear-gradient(135deg, #0066cc, #004a99); color: #fff; border-radius: 12px;
           padding: 28px 24px; margin-bottom: 24px; text-align: center;
           box-shadow: 0 4px 16px rgba(0,80,180,0.25); }}
  .hero-title {{ font-size: 15px; opacity: 0.92; margin-bottom: 14px; }}
  .hero-btn {{ display: inline-block; background: #fff; color: #0066cc; font-weight: 600;
               font-size: 16px; padding: 12px 28px; border-radius: 8px; text-decoration: none; }}
  .hero-btn:hover {{ background: #f0f6ff; text-decoration: none; }}
  .hero-sub {{ margin-top: 14px; font-size: 13px; }}
  .hero-sub a {{ color: #cfe4ff; }}
  .hero-sub a:hover {{ color: #fff; }}
  .section {{ background: #fff; border-radius: 8px; padding: 20px; margin-bottom: 16px;
              box-shadow: 0 1px 3px rgba(0,0,0,0.1); }}
  h2 {{ font-size: 18px; margin-bottom: 12px; }}
  .badge {{ font-size: 12px; padding: 2px 8px; border-radius: 4px; font-weight: normal; }}
  .badge.dev {{ background: #fff3cd; color: #856404; }}
  .badge.beta {{ background: #cce5ff; color: #004085; }}
  .badge.release {{ background: #d4edda; color: #155724; }}
  table {{ width: 100%; border-collapse: collapse; }}
  th {{ text-align: left; padding: 6px 8px; font-size: 12px; color: #666; font-weight: 600; border-bottom: 1px solid #ddd; }}
  th:last-child, td:last-child {{ text-align: right; white-space: nowrap; }}
  td {{ padding: 6px 8px; border-bottom: 1px solid #eee; }}
  td:first-child {{ color: #555; font-size: 13px; white-space: nowrap; }}
  .released {{ font-size: 12px; color: #888; font-weight: normal; margin-left: 6px; }}
  a {{ color: #0066cc; text-decoration: none; }}
  a:hover {{ text-decoration: underline; }}
  p {{ color: #666; font-size: 14px; margin-bottom: 12px; }}
  .footer {{ text-align: center; color: #999; font-size: 13px; margin-top: 32px; }}
</style>
</head>
<body>
<h1>AIjia Downloads</h1>
<p class="subtitle">AI小家 Desktop App</p>
{hero_html}
<div id="all">
{''.join(sections) if sections else '<p>No builds available yet.</p>'}
</div>
<p class="footer">Generated {datetime.now(timezone.utc).strftime("%Y-%m-%d %H:%M UTC")}</p>
__HERO_SCRIPT__
</body>
</html>"""
    script = HERO_SCRIPT.replace("__REL_JSON__", rel_json) if hero else ""
    return page.replace("__HERO_SCRIPT__", script)


def main():
    key_id = os.environ.get("OSS_ACCESS_KEY_ID", "")
    key_secret = os.environ.get("OSS_ACCESS_KEY_SECRET", "")
    if not key_id or not key_secret:
        print("[warn] OSS credentials not set, skipping download page generation")
        sys.exit(0)

    auth = oss2.Auth(key_id, key_secret)
    bucket = oss2.Bucket(auth, ENDPOINT, BUCKET_NAME)

    # Scan OSS
    print("Scanning OSS for builds...")
    dev_files = [f for f in list_files(bucket, f"{OSS_PREFIX}/dev/")
                 if f["name"].startswith("AIjia_latest")]

    beta_versions = {}
    for f in list_files(bucket, f"{OSS_PREFIX}/beta/"):
        parts = f["key"].split("/")
        if len(parts) >= 4:
            ver = parts[2]  # e.g., "v0.5.22"
            # Only include downloadable files
            if f["name"].endswith((".dmg", ".msi", ".exe", ".tar.gz")):
                beta_versions.setdefault(ver, []).append(f)

    release_versions = {}
    for f in list_files(bucket, f"{OSS_PREFIX}/v"):
        parts = f["key"].split("/")
        if len(parts) >= 3 and parts[1].startswith("v"):
            ver = parts[1]
            if f["name"].endswith((".dmg", ".msi", ".exe", ".tar.gz")):
                release_versions.setdefault(ver, []).append(f)

    print(f"  dev: {len(dev_files)} files")
    print(f"  beta: {len(beta_versions)} versions")
    print(f"  release: {len(release_versions)} versions")

    html = generate_html(dev_files, beta_versions, release_versions)

    # Upload
    key = f"{OSS_PREFIX}/downloads.html"
    bucket.put_object(key, html.encode("utf-8"), headers={
        "Content-Type": "text/html; charset=utf-8",
        "Cache-Control": "no-cache",
    })
    print(f"\n[ok] Download page uploaded: {CDN_BASE}/{key}")


if __name__ == "__main__":
    main()

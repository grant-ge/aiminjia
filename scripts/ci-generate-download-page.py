#!/usr/bin/env python3
"""Generate downloads.html listing all available builds and upload to OSS.

Scans the OSS bucket for dev/, beta/, and release builds, then generates
an HTML page with download links.

Env vars required:
  OSS_ACCESS_KEY_ID
  OSS_ACCESS_KEY_SECRET
"""

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



def generate_html(dev_files, beta_versions, release_versions):
    sections = []

    # Dev section
    if dev_files:
        rows = ""
        for f in sorted(dev_files, key=lambda x: x["name"]):
            rows += f'<tr><td><a href="{f["url"]}">{f["name"]}</a></td><td>{format_size(f["size"])}</td><td>{format_time(f["modified"])}</td></tr>\n'
        sections.append(f"""
        <div class="section">
            <h2>Dev Builds <span class="badge dev">unsigned</span></h2>
            <p>Latest CI builds. Not signed — macOS users need <code>xattr -cr</code>, Windows may show SmartScreen warning.</p>
            <table>{rows}</table>
        </div>""")

    # Beta section — sort by semver, not lexicographic ("0.5.9" > "0.5.10" lex).
    for ver, files in sorted(beta_versions.items(), key=lambda kv: _semver_key(kv[0]), reverse=True)[:5]:
        rows = ""
        for f in sorted(files, key=lambda x: x["name"]):
            rows += f'<tr><td><a href="{f["url"]}">{f["name"]}</a></td><td>{format_size(f["size"])}</td></tr>\n'
        sections.append(f"""
        <div class="section">
            <h2>Beta {ver} <span class="badge beta">beta</span></h2>
            <table>{rows}</table>
        </div>""")

    # Release section — sort by semver, not lexicographic.
    for ver, files in sorted(release_versions.items(), key=lambda kv: _semver_key(kv[0]), reverse=True)[:5]:
        rows = ""
        for f in sorted(files, key=lambda x: x["name"]):
            rows += f'<tr><td><a href="{f["url"]}">{f["name"]}</a></td><td>{format_size(f["size"])}</td></tr>\n'
        sections.append(f"""
        <div class="section">
            <h2>Release {ver} <span class="badge release">release</span></h2>
            <table>{rows}</table>
        </div>""")

    return f"""<!DOCTYPE html>
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
  .section {{ background: #fff; border-radius: 8px; padding: 20px; margin-bottom: 16px;
              box-shadow: 0 1px 3px rgba(0,0,0,0.1); }}
  h2 {{ font-size: 18px; margin-bottom: 12px; }}
  .badge {{ font-size: 12px; padding: 2px 8px; border-radius: 4px; font-weight: normal; }}
  .badge.dev {{ background: #fff3cd; color: #856404; }}
  .badge.beta {{ background: #cce5ff; color: #004085; }}
  .badge.release {{ background: #d4edda; color: #155724; }}
  table {{ width: 100%; border-collapse: collapse; }}
  td {{ padding: 6px 0; border-bottom: 1px solid #eee; }}
  td:last-child {{ text-align: right; white-space: nowrap; }}
  a {{ color: #0066cc; text-decoration: none; }}
  a:hover {{ text-decoration: underline; }}
  p {{ color: #666; font-size: 14px; margin-bottom: 12px; }}
  .footer {{ text-align: center; color: #999; font-size: 13px; margin-top: 32px; }}
</style>
</head>
<body>
<h1>AIjia Downloads</h1>
<p class="subtitle">AI小家 Desktop App</p>
{''.join(sections) if sections else '<p>No builds available yet.</p>'}
<p class="footer">Generated {datetime.now(timezone.utc).strftime("%Y-%m-%d %H:%M UTC")}</p>
</body>
</html>"""


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
            if f["name"].endswith((".dmg", ".exe", ".tar.gz")):
                beta_versions.setdefault(ver, []).append(f)

    release_versions = {}
    for f in list_files(bucket, f"{OSS_PREFIX}/v"):
        parts = f["key"].split("/")
        if len(parts) >= 3 and parts[1].startswith("v"):
            ver = parts[1]
            if f["name"].endswith((".dmg", ".exe", ".tar.gz")):
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

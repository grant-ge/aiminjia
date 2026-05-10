#!/usr/bin/env python3
"""Pack all in-tree built-in skills (under ../docs/skill-bundles/) into a
single distributable artifact + a sync script.

Used at build time to ship the "managed global skills" that every AIjia
install gets out-of-the-box (currently: 小程 / meeting-notes-polish /
wechat-article-ideation / weekly-priority-planner).

Outputs (under code/dist-skills/):
- <skill-id>-v<version>.aijia-skill   per-skill zip packs (same format
  as user export, importable by anyone)
- builtin-skills-<date>.tar.gz        one tarball with all SKILL dirs,
  for managed runtime to extract into ~/.renlijia/skills/ on first start
- INDEX.json                          { skills: [{id, version, ...}] }
- README.md                           how to consume

Validation: each SKILL.md must parse (frontmatter + body), name must be
kebab-case, description non-empty, scripts/references referenced in body
must exist.

Usage:
  python3 scripts/skills/build-bundle.py
  python3 scripts/skills/build-bundle.py --dry-run    # validate only, no artifacts
"""

from __future__ import annotations

import argparse
import datetime as _dt
import hashlib
import json
import re
import shutil
import sys
import tarfile
import zipfile
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent.parent
BUNDLES_DIR = REPO_ROOT.parent / "docs" / "skill-bundles"
DIST_DIR = REPO_ROOT / "dist-skills"

NAME_RE = re.compile(r"^[a-z0-9]+(-[a-z0-9]+)*$")
VERSION_RE = re.compile(r"^\d+\.\d+(\.\d+)?(-[a-zA-Z0-9.]+)?$")
ALLOWED_SUBDIRS = {"scripts", "references"}


def parse_frontmatter(text: str) -> dict:
    if not text.startswith("---"):
        raise ValueError("missing leading '---'")
    rest = text[3:].lstrip("\r\n")
    end = rest.find("\n---")
    if end < 0:
        raise ValueError("missing closing '---'")
    yaml_text = rest[:end]
    # Minimal YAML parser sufficient for our subset (key: value + lists)
    out: dict = {}
    cur_list: list | None = None
    cur_key: str | None = None
    for line in yaml_text.splitlines():
        if not line.strip():
            continue
        if line.startswith("  - ") or line.startswith("- "):
            if cur_list is None:
                raise ValueError(f"unexpected list item: {line!r}")
            cur_list.append(line.split("- ", 1)[1].strip())
            continue
        if ":" not in line:
            continue
        key, _, val = line.partition(":")
        key = key.strip()
        val = val.strip()
        if not val:
            cur_list = []
            cur_key = key
            out[key] = cur_list
        else:
            cur_list = None
            cur_key = None
            # Strip wrapping quotes if any
            if (val.startswith('"') and val.endswith('"')) or (
                val.startswith("'") and val.endswith("'")
            ):
                val = val[1:-1]
            out[key] = val
    return out


def find_referenced_files(body: str, subdir: str) -> list[str]:
    """Find `subdir/<file>` references in body text. Single-level only.

    Mirrors the Rust validator: rejects `...` placeholder, paths with `/`,
    and trailing punctuation. A token must look like a real filename
    (`<word>.<ext>` with at least one dot, or a bare word with no dots).
    """
    # Match the chars that would make up a single filename component
    pattern = re.compile(rf"\b{re.escape(subdir)}/([\w][\w.\-]*)")
    out: list[str] = []
    for cap in pattern.findall(body):
        # strip trailing punctuation we don't want as part of a filename
        cap = cap.rstrip(".,;:")
        if not cap:
            continue
        if cap == ".." or cap.startswith("."):
            continue
        # Skip the placeholder pattern `references/...` — that's docs not a real ref
        if "..." in cap:
            continue
        out.append(cap)
    return sorted(set(out))


def validate_skill(skill_dir: Path) -> dict:
    """Validate one skill bundle, return its manifest dict."""
    skill_md = skill_dir / "SKILL.md"
    if not skill_md.is_file():
        raise SystemExit(f"[FAIL] {skill_dir.name}: SKILL.md missing")

    text = skill_md.read_text(encoding="utf-8")
    fm = parse_frontmatter(text)
    name = fm.get("name", "")
    description = fm.get("description", "")
    version = str(fm.get("version", "0.1.0"))

    if name != skill_dir.name:
        raise SystemExit(
            f"[FAIL] {skill_dir.name}: frontmatter.name='{name}' != dir name"
        )
    if not NAME_RE.match(name):
        raise SystemExit(
            f"[FAIL] {skill_dir.name}: name '{name}' is not kebab-case"
        )
    if not description.strip():
        raise SystemExit(f"[FAIL] {skill_dir.name}: description empty")
    if not VERSION_RE.match(version):
        raise SystemExit(
            f"[FAIL] {skill_dir.name}: version '{version}' invalid (use x.y.z)"
        )

    # Body after frontmatter
    body = text[text.find("---", 3) + 3 :]

    # Validate referenced files exist
    for sub in ALLOWED_SUBDIRS:
        for fname in find_referenced_files(body, sub):
            f = skill_dir / sub / fname
            if not f.is_file():
                raise SystemExit(
                    f"[FAIL] {skill_dir.name}: body references {sub}/{fname} but file missing"
                )

    # Disallow extra top-level files / unsupported subdirs
    for entry in skill_dir.iterdir():
        if entry.name in ("SKILL.md", *ALLOWED_SUBDIRS):
            continue
        if entry.name.startswith("."):
            continue
        raise SystemExit(
            f"[FAIL] {skill_dir.name}: unexpected entry '{entry.name}' (only SKILL.md / scripts/ / references/ allowed)"
        )

    return {
        "id": name,
        "name": name,
        "description": description,
        "version": version,
        "label": _extract_label(fm),
    }


def _extract_label(fm: dict) -> str:
    md = fm.get("metadata") or {}
    if isinstance(md, dict):
        return str(md.get("label", fm.get("name", "")))
    return fm.get("name", "")


def collect_entries(skill_dir: Path) -> list[tuple[str, Path]]:
    """Sorted list of (rel_path, abs_path) for files we ship."""
    out: list[tuple[str, Path]] = []
    out.append(("SKILL.md", skill_dir / "SKILL.md"))
    for sub in sorted(ALLOWED_SUBDIRS):
        d = skill_dir / sub
        if not d.is_dir():
            continue
        for f in sorted(d.iterdir()):
            if f.is_file():
                out.append((f"{sub}/{f.name}", f))
    return out


def compute_checksum(entries: list[tuple[str, Path]]) -> str:
    """Same algorithm as Rust pack_skill_dir: sha256 of sorted (rel\\0content)."""
    h = hashlib.sha256()
    for rel, abs_path in entries:
        h.update(rel.encode("utf-8"))
        h.update(b"\0")
        h.update(abs_path.read_bytes())
    return h.hexdigest()


def build_aijia_skill(skill_dir: Path, manifest: dict, out_path: Path) -> None:
    """Write a .aijia-skill zip identical in shape to the Rust packer."""
    entries = collect_entries(skill_dir)
    full_manifest = {
        "format_version": 1,
        "id": manifest["id"],
        "name": manifest["name"],
        "version": manifest["version"],
        "author": "AIjia builtin",
        "created_at": _dt.datetime.now(tz=_dt.timezone.utc)
        .isoformat(timespec="seconds")
        .replace("+00:00", "Z"),
        "exported_from": "build-bundle.py",
        "checksum_sha256": compute_checksum(entries),
    }
    out_path.parent.mkdir(parents=True, exist_ok=True)
    with zipfile.ZipFile(out_path, "w", zipfile.ZIP_DEFLATED) as z:
        z.writestr("manifest.json", json.dumps(full_manifest, indent=2))
        for rel, abs_path in entries:
            z.write(abs_path, f"skill/{rel}")


def build_managed_tarball(bundles: list[Path], out_path: Path) -> None:
    """Tarball that managed runtime extracts into ~/.renlijia/skills/."""
    out_path.parent.mkdir(parents=True, exist_ok=True)
    with tarfile.open(out_path, "w:gz") as tar:
        for bundle in bundles:
            tar.add(bundle, arcname=bundle.name)


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--dry-run", action="store_true", help="validate only")
    ap.add_argument(
        "--out",
        type=Path,
        default=DIST_DIR,
        help=f"output dir (default {DIST_DIR})",
    )
    args = ap.parse_args()

    if not BUNDLES_DIR.is_dir():
        print(f"❌ skill-bundles dir not found: {BUNDLES_DIR}", file=sys.stderr)
        return 2

    bundles = sorted(p for p in BUNDLES_DIR.iterdir() if p.is_dir())
    if not bundles:
        print(f"⚠️  no skill bundles under {BUNDLES_DIR}", file=sys.stderr)
        return 1

    print(f"🔍 validating {len(bundles)} skill bundles in {BUNDLES_DIR}")
    manifests: list[dict] = []
    for b in bundles:
        m = validate_skill(b)
        manifests.append(m)
        print(f"  ✅ {m['id']}@{m['version']}  ({m['label']})")

    if args.dry_run:
        print("✅ dry-run: validation passed")
        return 0

    out = args.out
    if out.exists():
        shutil.rmtree(out)
    out.mkdir(parents=True)

    print(f"📦 packing into {out}")
    for b, m in zip(bundles, manifests):
        archive = out / f"{m['id']}-v{m['version']}.aijia-skill"
        build_aijia_skill(b, m, archive)
        print(f"  · {archive.name}  ({archive.stat().st_size} B)")

    today = _dt.date.today().isoformat()
    tarball = out / f"builtin-skills-{today}.tar.gz"
    build_managed_tarball(bundles, tarball)
    print(f"  · {tarball.name}  ({tarball.stat().st_size} B)")

    index = {
        "generated_at": _dt.datetime.now(tz=_dt.timezone.utc)
        .isoformat(timespec="seconds")
        .replace("+00:00", "Z"),
        "skills": manifests,
    }
    (out / "INDEX.json").write_text(json.dumps(index, indent=2, ensure_ascii=False))

    (out / "README.md").write_text(
        f"""# AIjia Builtin Skills Bundle

Generated by `scripts/skills/build-bundle.py` from `docs/skill-bundles/`.

## Contents

- **`*.aijia-skill`** — per-skill zip packs. Identical format to user-exported
  packs. Drop into the app to install one at a time.
- **`builtin-skills-YYYY-MM-DD.tar.gz`** — single tarball of all SKILL
  directories. Managed runtime extracts this into `~/.renlijia/skills/` on
  first start so every install gets the latest builtin skills.
- **`INDEX.json`** — manifest of what's in this bundle (id / version / label).

Total skills: {len(manifests)}.

## Skills

| id | version | label |
|---|---|---|
""" + "\n".join(f"| `{m['id']}` | {m['version']} | {m['label']} |" for m in manifests) + "\n"
    )

    print(f"\n✅ bundle ready: {out}")
    return 0


if __name__ == "__main__":
    sys.exit(main())

#!/usr/bin/env python3
"""
AIjia Desktop Release Manager — Interactive release flow enforcer.

Cross-platform (macOS + Windows). Guides the developer through the correct
release sequence, blocks skipping steps, and provides clear next-action prompts.

Usage:
    python scripts/release.py              # Interactive menu
    python scripts/release.py beta         # Jump to beta flow
    python scripts/release.py release      # Jump to release flow
    python scripts/release.py finalize     # Jump to finalize
    python scripts/release.py status       # Show current release state

State is tracked in .release-state.json (gitignored) to prevent step-skipping.
"""

import json
import os
import platform
import re
import subprocess
import sys
from datetime import datetime
from pathlib import Path

# --- Constants ---
PROJECT_ROOT = Path(__file__).resolve().parent.parent
STATE_FILE = PROJECT_ROOT / ".release-state.json"
TAURI_CONF = PROJECT_ROOT / "src-tauri" / "tauri.conf.json"
PACKAGE_JSON = PROJECT_ROOT / "package.json"
CARGO_TOML = PROJECT_ROOT / "src-tauri" / "Cargo.toml"
GH_REPO = "grant-ge/aiminjia"
IS_WINDOWS = platform.system() == "Windows"

# Release stages (must complete in order)
# Windows builds run on GitHub-hosted CI then sign locally.
# macOS arm64 + x86_64 are built locally on the maintainer's Mac via
# scripts/build-and-sign-macos.sh (build + sign + notarize + upload in one).
STAGES = [
    "version_bumped",      # Step 0: version synced and committed
    "beta_tagged",         # Step 1: beta tag pushed, CI builds + signs both platforms
    "beta_tested",         # Step 2: tester confirms beta is good
    "release_tagged",      # Step 3: release tag pushed, CI builds + signs both platforms
    "finalized",           # Step 4: update.json generated
]


# --- Utilities ---
def color(text, code):
    if IS_WINDOWS or not sys.stdout.isatty():
        return text
    return f"\033[{code}m{text}\033[0m"


def green(t): return color(t, "32")
def yellow(t): return color(t, "33")
def red(t): return color(t, "31")
def cyan(t): return color(t, "36")
def bold(t): return color(t, "1")


def ask(prompt, default=None):
    suffix = f" [{default}]" if default else ""
    result = input(f"{prompt}{suffix}: ").strip()
    return result or default


def confirm(prompt):
    result = input(f"{prompt} [y/N]: ").strip().lower()
    return result in ("y", "yes")


def run(cmd, check=True, capture=False):
    """Run a shell command."""
    print(f"  $ {cmd}")
    if capture:
        r = subprocess.run(cmd, shell=True, capture_output=True, text=True)
        if check and r.returncode != 0:
            print(red(f"  FAILED: {r.stderr.strip()}"))
            sys.exit(1)
        return r
    else:
        r = subprocess.run(cmd, shell=True)
        if check and r.returncode != 0:
            print(red(f"  Command failed with exit code {r.returncode}"))
            sys.exit(1)
        return r


# --- State Management ---
def load_state():
    if STATE_FILE.exists():
        return json.loads(STATE_FILE.read_text())
    return {"version": None, "stages_completed": [], "started_at": None}


def save_state(state):
    STATE_FILE.write_text(json.dumps(state, indent=2, ensure_ascii=False))


def reset_state():
    if STATE_FILE.exists():
        STATE_FILE.unlink()


def complete_stage(state, stage):
    if stage not in state["stages_completed"]:
        state["stages_completed"].append(stage)
    save_state(state)


def is_stage_done(state, stage):
    return stage in state["stages_completed"]


def check_prereq(state, required_stage, action_name):
    """Block if a prerequisite stage is not complete."""
    if not is_stage_done(state, required_stage):
        stage_names = {
            "version_bumped": "Version bump (Step 0)",
            "beta_tagged": "Beta build (Step 1)",
            "beta_tested": "Beta testing (Step 2)",
            "release_tagged": "Release build (Step 3)",
        }
        print(red(f"\n  BLOCKED: Cannot {action_name}."))
        print(red(f"  Required: {stage_names.get(required_stage, required_stage)} must complete first."))
        print(f"\n  Run: python scripts/release.py status")
        sys.exit(1)


# --- Version Helpers ---
def get_current_version():
    """Read version from tauri.conf.json."""
    conf = json.loads(TAURI_CONF.read_text())
    return conf.get("version", "unknown")


def check_versions_synced():
    """Verify all 3 config files have the same version."""
    v1 = get_current_version()

    pkg = json.loads(PACKAGE_JSON.read_text())
    v2 = pkg.get("version", "")

    cargo = CARGO_TOML.read_text()
    m = re.search(r'^\s*version\s*=\s*"([^"]+)"', cargo, re.MULTILINE)
    v3 = m.group(1) if m else ""

    if v1 == v2 == v3:
        return v1
    print(red(f"  Version mismatch!"))
    print(f"    tauri.conf.json: {v1}")
    print(f"    package.json:    {v2}")
    print(f"    Cargo.toml:      {v3}")
    print(f"\n  Run: python scripts/bump-version.py {v1}")
    return None


def check_git_clean():
    r = run("git status --porcelain", capture=True, check=False)
    if r.stdout.strip():
        print(yellow("  Warning: working tree has uncommitted changes:"))
        print(f"  {r.stdout.strip()[:200]}")
        return False
    return True


# --- Flow Steps ---
def step_0_bump(state):
    """Step 0: Set base version (X.Y.Z, no pre-release suffix).

    Only writes files for plain X.Y.Z. The actual beta builds in Step 1
    will mutate version into X.Y.Z-beta.N each round.
    """
    print(bold("\n═══ Step 0: Version Bump ═══"))

    current = get_current_version()
    # If config has a pre-release suffix (mid-beta-cycle), strip it for the prompt
    base_default = re.match(r'^(\d+\.\d+\.\d+)', current).group(1) if current else "0.0.0"
    print(f"  Current version: {current}")

    version = ask("  New base version (X.Y.Z, no -beta suffix)", base_default)
    if not re.match(r'^\d+\.\d+\.\d+$', version):
        print(red("  Invalid version format. Use X.Y.Z (Step 1 will append -beta.N)"))
        return

    if version != current:
        # Run bump script
        if IS_WINDOWS:
            run(f"python scripts/bump-version.py {version}")
        else:
            run(f"bash scripts/bump-version.sh {version}")

        # Verify
        synced = check_versions_synced()
        if not synced:
            return

        # Commit
        if confirm("  Commit version bump?"):
            run(f'git add package.json src-tauri/tauri.conf.json src-tauri/Cargo.toml')
            run(f'git commit -m "chore: bump to {version}"')
            print(green("  Committed."))
    else:
        synced = check_versions_synced()
        if not synced:
            return

    # Push
    if confirm("  Push to remotes (codeup + origin)?"):
        run("git push codeup main", check=False)
        run("git push origin main")

    state["base_version"] = version
    state["version"] = version  # mirror for legacy step lookups; mutates in step_1
    state["beta_seq"] = 0
    state["started_at"] = datetime.now().isoformat()
    state["stages_completed"] = []
    complete_stage(state, "version_bumped")
    print(green(f"\n  ✓ Base version {version} ready. Next: Step 1 (Beta build)"))


def step_1_beta(state):
    """Step 1: Bump beta seq → rewrite version → commit → tag → push.

    Each invocation produces a fresh tag like beta-v0.5.22-beta.2, with
    config files mirroring the full pre-release string so the binary's
    About dialog and Tauri-bundled artifact filenames show the same
    beta.N suffix as the OSS path and Homebrew cask URL.
    """
    check_prereq(state, "version_bumped", "create beta tag")
    base = state.get("base_version") or state.get("version")
    if not base or not re.match(r'^\d+\.\d+\.\d+$', base):
        print(red(f"  Missing base_version in state (got: {base!r}). Run Step 0 first."))
        return

    seq = state.get("beta_seq", 0) + 1
    beta_version = f"{base}-beta.{seq}"

    print(bold(f"\n═══ Step 1: Beta Build (v{beta_version}) ═══"))
    print(f"  base={base}  beta_seq={seq}  →  {beta_version}")

    if not check_git_clean():
        if not confirm("  Continue anyway?"):
            return

    # Bump version files to X.Y.Z-beta.N
    if IS_WINDOWS:
        run(f"python scripts/bump-version.py {beta_version}")
    else:
        run(f"bash scripts/bump-version.sh {beta_version}")
    # Refresh Cargo.lock so its [package] name = "aijia" version matches
    run("cargo update -p aijia --manifest-path src-tauri/Cargo.toml", check=False)

    if not check_versions_synced():
        return

    if confirm(f"  Commit version bump → {beta_version}?"):
        run('git add package.json src-tauri/tauri.conf.json src-tauri/Cargo.toml src-tauri/Cargo.lock')
        run(f'git commit -m "chore: bump to {beta_version}"', check=False)
        run("git push codeup main", check=False)
        run("git push origin main")

    tag = f"beta-v{beta_version}"
    print(f"  Creating tag: {tag}")
    if confirm(f"  Create and push tag {tag}?"):
        run(f"git tag {tag}", check=False)
        run(f"git push origin {tag}")
        run(f"git push codeup {tag}", check=False)

        state["version"] = beta_version  # active version
        state["beta_seq"] = seq
        # Reset downstream stages — every new beta round restarts testing
        for s in ("beta_tested",):
            if s in state.get("stages_completed", []):
                state["stages_completed"].remove(s)
        save_state(state)
        complete_stage(state, "beta_tagged")
        print(green(f"\n  ✓ Beta tag pushed. CI is building Windows (unsigned artifact)."))
        print(f"\n  Monitor: https://github.com/{GH_REPO}/actions")
        print(f"\n  {yellow('Next actions (do BOTH):')}")
        print(f"  1. macOS — build + sign + upload locally (serial arm64 → x86_64):")
        print(f"       bash scripts/build-and-sign-macos.sh {beta_version} beta")
        print(f"  2. Windows — after CI finishes, download + sign + upload:")
        print(f"       gh run download <run-id> -n windows-unsigned -D build/")
        print(f"       # sign with SimpleSign, then:")
        print(f"       .\\scripts\\sign-and-upload-windows.ps1 -Version {beta_version} -Type beta")
        print(f"  3. Then test:")
        print(f"       python scripts/release.py test-passed")



def step_2_test(state):
    """Step 2: Mark beta as tested."""
    check_prereq(state, "beta_tagged", "mark beta as tested")
    version = state["version"]

    print(bold(f"\n═══ Step 2: Beta Test Verification (v{version}) ═══"))
    print(f"\n  Beta download links:")
    print(f"    macOS:   https://lotus.renlijia.com/aijia/beta/v{version}/AIjia_{version}_aarch64.dmg")
    print(f"    Windows: https://lotus.renlijia.com/aijia/beta/v{version}/AIjia_{version}_x64-setup.exe")
    print(f"\n  Test checklist:")
    print(f"    [ ] Windows install - no security warning")
    print(f"    [ ] macOS install - no security warning")
    print(f"    [ ] Core functionality smoke test")
    print(f"    [ ] Version number displays correctly")
    print(f"    [ ] New features / fixes verified")

    if confirm("\n  All tests PASSED?"):
        complete_stage(state, "beta_tested")
        print(green("\n  ✓ Beta testing passed. Next: Step 3 (Release build)"))
        print(f"    python scripts/release.py release")
    else:
        print(yellow("\n  Beta testing not passed. Fix issues and rebuild beta:"))
        print(f"    python scripts/release.py beta")


def step_3_release(state):
    """Step 3: Bump version to plain X.Y.Z (drop -beta.N) → tag → CI builds."""
    check_prereq(state, "beta_tested", "create release tag")
    base = state.get("base_version") or re.match(r'^(\d+\.\d+\.\d+)', state["version"]).group(1)

    print(bold(f"\n═══ Step 3: Release Build (v{base}) ═══"))
    print(f"  Beta cycle ended at: {state.get('version')}")
    print(f"  Releasing as:         {base}  (pre-release suffix dropped)")

    if get_current_version() != base:
        print(f"  Bumping config files back to plain {base}")
        if IS_WINDOWS:
            run(f"python scripts/bump-version.py {base}")
        else:
            run(f"bash scripts/bump-version.sh {base}")
        run("cargo update -p aijia --manifest-path src-tauri/Cargo.toml", check=False)
        if not check_versions_synced():
            return
        if confirm(f"  Commit version bump → {base}?"):
            run('git add package.json src-tauri/tauri.conf.json src-tauri/Cargo.toml src-tauri/Cargo.lock')
            run(f'git commit -m "chore: bump to {base}"', check=False)
            run("git push codeup main", check=False)
            run("git push origin main")

    tag = f"v{base}"
    print(f"\n  {red('WARNING: This will create a PRODUCTION release tag!')}")
    print(f"  Tag: {tag}")
    print(f"  Users will receive this update after finalize.")

    if not confirm(f"\n  Create and push release tag {tag}?"):
        print("  Aborted.")
        return

    run(f"git tag {tag}", check=False)
    run(f"git push origin {tag}")
    run(f"git push codeup {tag}", check=False)
    state["version"] = base
    save_state(state)
    complete_stage(state, "release_tagged")
    print(green(f"\n  ✓ Release tag pushed. CI is building Windows (unsigned artifact)."))
    print(f"\n  Monitor: https://github.com/{GH_REPO}/actions")
    print(f"\n  {yellow('Next actions (do BOTH):')}")
    print(f"  1. macOS — build + sign + upload locally (serial arm64 → x86_64):")
    print(f"       bash scripts/build-and-sign-macos.sh {base} release")
    print(f"  2. Windows — after CI finishes, download + sign + upload:")
    print(f"       gh run download <run-id> -n windows-unsigned -D build/")
    print(f"       # sign with SimpleSign, then:")
    print(f"       .\\scripts\\sign-and-upload-windows.ps1 -Version {base} -Type release")
    print(f"  3. Then finalize:")
    print(f"       python scripts/release.py finalize")



def step_4_finalize(state):
    """Step 4: Generate update.json → users get auto-update."""
    check_prereq(state, "release_tagged", "finalize release")
    version = state["version"]

    print(bold(f"\n═══ Step 4: Finalize Release (v{version}) ═══"))
    print(f"  This will generate update.json and push auto-updates to all users.")

    if not confirm("  Proceed?"):
        return

    # Try gh CLI first
    r = run("gh --version", capture=True, check=False)
    if r.returncode == 0:
        run(f'gh workflow run "Finalize Release" --repo {GH_REPO} -f version={version}')
        print(green(f"\n  ✓ Finalize workflow triggered."))
    else:
        print(yellow("  gh CLI not available. Trigger manually:"))
        print(f"  GitHub → Actions → 'Finalize Release' → Run workflow → version: {version}")
        if not confirm("  Done?"):
            return

    complete_stage(state, "finalized")
    print(green(f"\n  ✓ Release v{version} finalized! Auto-updates are live."))
    print(f"\n  Remaining tasks:")
    print(f"    1. Homebrew: python3 scripts/bump-homebrew.py {version}")
    print(f"    2. Changelog: cd ../lotus && ./scripts/update-changelog.sh desktop {version}")
    print(f"\n  Push codeup:")
    print(f"    git push codeup main && git push codeup v{version}")


def show_status(state):
    """Show current release state."""
    print(bold("\n═══ Release Status ═══"))

    if not state.get("version"):
        print("  No release in progress.")
        print(f"  Start with: python scripts/release.py")
        return

    version = state["version"]
    print(f"  Version: {bold(version)}")
    print(f"  Started: {state.get('started_at', 'unknown')}")
    print(f"\n  Progress:")

    stage_labels = {
        "version_bumped": "Version bumped & pushed",
        "beta_tagged": "Beta tag pushed (Windows CI building; macOS built locally)",
        "beta_tested": "Beta testing passed",
        "release_tagged": "Release tag pushed (Windows CI building; macOS built locally)",
        "finalized": "Finalized (auto-update live)",
    }

    completed = state.get("stages_completed", [])
    next_found = False
    for stage in STAGES:
        done = stage in completed
        label = stage_labels[stage]
        if done:
            print(f"    {green('✓')} {label}")
        elif not next_found:
            print(f"    {yellow('→')} {label}  {yellow('← NEXT')}")
            next_found = True
        else:
            print(f"      {label}")

    if all(s in completed for s in STAGES):
        print(green(f"\n  Release v{version} complete!"))


def check_environment():
    """Check if required tools are available, guide first-time setup."""
    issues = []
    info = []

    # Git
    r = subprocess.run("git --version", shell=True, capture_output=True)
    if r.returncode != 0:
        issues.append("git not found")
    else:
        info.append(f"git: {r.stdout.decode().strip()}")

    # gh CLI (optional but recommended)
    r = subprocess.run("gh --version", shell=True, capture_output=True)
    if r.returncode != 0:
        issues.append("gh CLI not found (needed for finalize step)")
    else:
        info.append(f"gh: {r.stdout.decode().strip().split(chr(10))[0]}")

    # Python oss2
    r = subprocess.run(
        [sys.executable, "-c", "import oss2; print(oss2.__version__)"],
        capture_output=True, text=True
    )
    if r.returncode != 0:
        issues.append("Python oss2 not installed (pip install oss2)")
    else:
        info.append(f"oss2: {r.stdout.strip()}")

    # OSS credentials
    if not os.environ.get("OSS_ACCESS_KEY_ID"):
        issues.append("OSS_ACCESS_KEY_ID not set")
    if not os.environ.get("OSS_ACCESS_KEY_SECRET"):
        issues.append("OSS_ACCESS_KEY_SECRET not set")

    return issues, info


def show_setup_guide():
    """Show first-time setup instructions."""
    print(bold("\n═══ First-Time Setup Guide ═══"))
    print()
    print("  This guide helps you set up your machine for AIjia desktop releases.")
    print()

    issues, info = check_environment()

    if info:
        print(green("  Available:"))
        for i in info:
            print(f"    ✓ {i}")
        print()

    if issues:
        print(yellow("  Missing / needs setup:"))
        for i in issues:
            print(f"    ✗ {i}")
        print()

    print(bold("  === All Machines (macOS / Windows) ==="))
    print("""
    1. Install Python 3.8+ and oss2:
       pip install oss2

    2. Install gh CLI:
       macOS:   brew install gh
       Windows: winget install GitHub.cli

    3. Set environment variables:
       OSS_ACCESS_KEY_ID=<from team admin>
       OSS_ACCESS_KEY_SECRET=<from team admin>
""")

    print(bold("  === macOS signing machine (this Mac) ==="))
    print("""
    macOS arm64 + x86_64 are built locally — no self-hosted runner.

    4. Apple Developer ID Application certificate in login keychain
       Verify: security find-identity -v -p codesigning

    5. Environment variables for notarization + Tauri updater:
       APPLE_ID=<apple id email>
       APPLE_PASSWORD=<app-specific password>
       APPLE_TEAM_ID=<10-char team id>
       TAURI_SIGNING_PRIVATE_KEY=<contents of ~/.tauri/aijia.key>
       TAURI_SIGNING_PRIVATE_KEY_PASSWORD=<key password>

    6. Run:
       bash scripts/build-and-sign-macos.sh <version> <beta|release>
""")

    print(bold("  === Windows signing machine ==="))
    print("""
    Windows builds run on GitHub-hosted runner, signing is local.

    7. SimpleSign tool + EV hardware token for Authenticode signing
    8. Run:
       gh run download <run-id> -n windows-unsigned -D build/
       # sign .exe with SimpleSign
       .\\scripts\\sign-and-upload-windows.ps1 -Version <ver> -Type <beta|release>
""")

    print(bold("  === Workflow Overview ==="))
    print("""
    python scripts/release.py start              # bump version
    python scripts/release.py beta               # push beta tag (Windows CI; mac local)
    bash scripts/build-and-sign-macos.sh X.Y.Z-beta.N beta    # mac arm64 + x86_64
    # download windows-unsigned artifact, sign, upload via sign-and-upload-windows.ps1
    python scripts/release.py test-passed        # confirm beta works
    python scripts/release.py release            # push release tag
    bash scripts/build-and-sign-macos.sh X.Y.Z release
    # sign windows release
    python scripts/release.py finalize           # generate update.json → auto-update live
""")


def show_menu(state):
    """Show interactive menu."""
    version = get_current_version()
    print(bold(f"\n═══ AIjia Release Manager (current: v{version}) ═══"))
    print()

    if state.get("version") and state.get("stages_completed"):
        show_status(state)
        print()

    print("  Commands:")
    print(f"    {cyan('start')}          Start new release (bump version)")
    print(f"    {cyan('beta')}           Push beta tag (Windows CI; build mac locally via build-and-sign-macos.sh)")
    print(f"    {cyan('test-passed')}    Confirm beta testing passed")
    print(f"    {cyan('release')}        Push release tag (Windows CI; build mac locally via build-and-sign-macos.sh)")
    print(f"    {cyan('finalize')}       Generate update.json (go live)")
    print(f"    {cyan('status')}         Show current state")
    print(f"    {cyan('setup')}          First-time setup guide + environment check")
    print(f"    {cyan('reset')}          Reset state (start over)")
    print(f"    {cyan('quit')}           Exit")
    print()

    cmd = ask("  Select command").strip().lower()
    return cmd


# --- Main ---
def handle_command(cmd, state):
    if cmd in ("start", "bump", "0"):
        step_0_bump(state)
    elif cmd in ("beta", "1"):
        step_1_beta(state)
    elif cmd in ("test-passed", "tested", "2"):
        step_2_test(state)
    elif cmd in ("release", "3"):
        step_3_release(state)
    elif cmd in ("finalize", "4"):
        step_4_finalize(state)
    elif cmd == "status":
        show_status(state)
    elif cmd in ("setup", "init", "check"):
        show_setup_guide()
    elif cmd == "reset":
        if confirm("  Reset release state? (current progress will be lost)"):
            reset_state()
            print(green("  ✓ State reset."))
            return load_state()
    elif cmd in ("quit", "q", "exit"):
        sys.exit(0)
    else:
        print(red(f"  Unknown command: {cmd}"))
    return state


def main():
    os.chdir(PROJECT_ROOT)
    state = load_state()

    # First-run detection: if no state and no args, show a welcome
    if not state.get("version") and len(sys.argv) <= 1:
        print(bold("\n  Welcome to AIjia Release Manager!"))
        print(f"  Current version: {get_current_version()}")
        print()
        print(f"  First time? Run {cyan('setup')} to check your environment.")
        print(f"  Ready to release? Run {cyan('start')} to begin.")

    if len(sys.argv) > 1:
        cmd = sys.argv[1].lower()
        handle_command(cmd, state)
    else:
        # Interactive loop
        while True:
            try:
                cmd = show_menu(state)
                if cmd:
                    state = handle_command(cmd, state) or state
            except KeyboardInterrupt:
                print("\n")
                sys.exit(0)
            except EOFError:
                sys.exit(0)


if __name__ == "__main__":
    main()

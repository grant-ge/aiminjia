#!/usr/bin/env bash
# ensure-e2e-prereq.sh — Auto-bootstrap local environment for `pnpm dev:with-pilot`.
#
# What this script does (idempotent):
#   1. Verify ~/.cargo/config.toml has `[net] git-fetch-with-cli = true` and
#      add it if missing. Required for cargo to fetch the codeup-hosted
#      tauri-plugin-pilot dep via ssh — libgit2 can't handle ed25519 / macOS
#      keychain-encrypted ssh keys.
#   2. Verify `jq` is on PATH (used by prepare-bundled-runtime.sh). Try
#      `brew install` on macOS or `apt-get install` on Debian/Ubuntu. If
#      neither is available or auto-install fails, prints how to fix.
#   3. Verify ssh-agent has at least one identity loaded. If not, print
#      `ssh-add` instructions but don't block — the user may rely on
#      keychain integration instead.
#
# Skip everything: `SKIP_E2E_PREREQ=1 pnpm dev:with-pilot`
# Triggered automatically via the `predev:with-pilot` pnpm lifecycle hook.

set -euo pipefail

if [[ "${SKIP_E2E_PREREQ:-}" == "1" ]]; then
  echo "[e2e-prereq] SKIP_E2E_PREREQ=1 — bypassing prerequisite checks"
  exit 0
fi

YELLOW='\033[33m'
GREEN='\033[32m'
RED='\033[31m'
RESET='\033[0m'
say()  { printf "[e2e-prereq] %s\n" "$*"; }
ok()   { printf "${GREEN}[e2e-prereq] ✓ %s${RESET}\n" "$*"; }
warn() { printf "${YELLOW}[e2e-prereq] ⚠ %s${RESET}\n" "$*"; }
fail() { printf "${RED}[e2e-prereq] ✗ %s${RESET}\n" "$*"; }

# ─── 1. cargo git-fetch-with-cli ─────────────────────────────────────────────

CARGO_CONFIG="${CARGO_HOME:-$HOME/.cargo}/config.toml"

ensure_cargo_git_cli() {
  mkdir -p "$(dirname "$CARGO_CONFIG")"

  if [[ -f "$CARGO_CONFIG" ]] && grep -qE '^\s*git-fetch-with-cli\s*=\s*true' "$CARGO_CONFIG"; then
    ok "cargo: git-fetch-with-cli already enabled in $CARGO_CONFIG"
    return 0
  fi

  if [[ -f "$CARGO_CONFIG" ]] && grep -qE '^\s*\[net\]' "$CARGO_CONFIG"; then
    # [net] section exists but the key isn't set — append the key under that section.
    # Use awk to insert right after the [net] header.
    local tmp
    tmp=$(mktemp)
    awk '
      /^\[net\]/ { print; print "git-fetch-with-cli = true  # auto-added by ensure-e2e-prereq.sh"; inserted=1; next }
      { print }
      END { if (!inserted) exit 1 }
    ' "$CARGO_CONFIG" > "$tmp"
    mv "$tmp" "$CARGO_CONFIG"
    ok "cargo: added git-fetch-with-cli under existing [net] section"
    return 0
  fi

  cat >> "$CARGO_CONFIG" << 'EOF'

# Auto-added by lotus-app scripts/ensure-e2e-prereq.sh.
# Required for `pnpm dev:with-pilot` — cargo needs the system git CLI to
# authenticate the codeup-hosted tauri-plugin-pilot dep over ssh.
[net]
git-fetch-with-cli = true
EOF
  ok "cargo: wrote [net] git-fetch-with-cli = true to $CARGO_CONFIG"
}

# ─── 2. jq (needed by prepare-bundled-runtime.sh) ────────────────────────────

ensure_jq() {
  if command -v jq >/dev/null 2>&1; then
    ok "jq: $(jq --version) already installed"
    return 0
  fi

  warn "jq missing — required by scripts/prepare-bundled-runtime.sh"

  local os
  os="$(uname -s)"
  if [[ "$os" == "Darwin" ]]; then
    if command -v brew >/dev/null 2>&1; then
      say "jq: running 'brew install jq' (may take ~30s)"
      if brew install jq; then
        ok "jq: installed via brew"
        return 0
      fi
      fail "jq: 'brew install jq' failed — install manually or set SKIP_BUNDLED_RUNTIME=1"
      return 1
    fi
    fail "jq: Homebrew not found. Install Homebrew (https://brew.sh) then re-run, or set SKIP_BUNDLED_RUNTIME=1 to skip"
    return 1
  elif [[ "$os" == "Linux" ]]; then
    if command -v apt-get >/dev/null 2>&1; then
      say "jq: running 'sudo apt-get install -y jq'"
      if sudo apt-get install -y jq; then
        ok "jq: installed via apt-get"
        return 0
      fi
      fail "jq: apt-get install failed — install manually or set SKIP_BUNDLED_RUNTIME=1"
      return 1
    fi
    fail "jq: apt-get not found (non-Debian Linux?). Install jq manually, or set SKIP_BUNDLED_RUNTIME=1"
    return 1
  fi

  fail "jq: unknown OS $os — install jq manually or set SKIP_BUNDLED_RUNTIME=1"
  return 1
}

# ─── 3. ssh-agent has at least one identity ──────────────────────────────────

check_ssh_keys() {
  # macOS keychain integration loads keys lazily, so `ssh-add -l` may return
  # "no identities" even when keys work. We treat this as a soft warning.
  if ssh-add -l >/dev/null 2>&1; then
    ok "ssh-agent: identities loaded"
    return 0
  fi

  if [[ "$(uname -s)" == "Darwin" ]] && [[ -f ~/.ssh/config ]] && grep -qE 'UseKeychain\s+yes' ~/.ssh/config; then
    ok "ssh: macOS keychain integration enabled — keys load lazily, no action needed"
    return 0
  fi

  warn "ssh-agent: no identities loaded"
  warn "  If your default key is at ~/.ssh/id_ed25519 (or id_rsa), run:"
  warn "    ssh-add ~/.ssh/id_ed25519"
  warn "  Then retry. (Soft warning — won't block; codeup auth will fail at fetch time if keys aren't reachable.)"
}

# ─── orchestrate ─────────────────────────────────────────────────────────────

main() {
  say "checking prerequisites for 'pnpm dev:with-pilot'..."
  local exit_code=0
  ensure_cargo_git_cli || exit_code=$?
  ensure_jq            || exit_code=$?
  check_ssh_keys                       # soft, never affects exit code
  if [[ "$exit_code" -ne 0 ]]; then
    fail "one or more prerequisites failed — fix above issues and retry"
    exit "$exit_code"
  fi
  ok "all prerequisites satisfied — starting tauri dev"
}

main

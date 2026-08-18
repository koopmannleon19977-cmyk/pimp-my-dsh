#!/usr/bin/env bash
#
# release-setup.sh — walks a human through the manual steps of the desktop
# release pipeline: updater-key custody, GitHub secrets, signing provider,
# optional Authenticode PFX, and the first release.
#
# Everything above the "STAGES" marker is the wizard library: do not hand-edit
# it. Author the per-step stages below the marker.

set -euo pipefail

# ──────────────────────────────────────────────────────────────────────────
# Wizard library — delightful, consistent UX. Identical across every wizard.
# ──────────────────────────────────────────────────────────────────────────

if [[ -t 1 ]] && command -v tput >/dev/null 2>&1 && [[ "$(tput colors 2>/dev/null || echo 0)" -ge 8 ]]; then
  BOLD=$(tput bold); DIM=$(tput dim); RESET=$(tput sgr0)
  BLUE=$(tput setaf 4); GREEN=$(tput setaf 2); YELLOW=$(tput setaf 3); RED=$(tput setaf 1)
else
  BOLD=""; DIM=""; RESET=""; BLUE=""; GREEN=""; YELLOW=""; RED=""
fi

# Author sets this at the top of the stages section.
TOTAL_STAGES=0

_STAGE_INDEX=0
ENV_FILE="${ENV_FILE:-.env}"
WRITTEN_ENV=()    # KEYs written to ENV_FILE this run
WRITTEN_SECRET=() # secret NAMEs set this run
SKIPPED=()        # things we couldn't do (e.g. gh missing)

# _clear — wipe the terminal so only the current step is on screen. No-op when
# output isn't a terminal, so piped logs stay readable.
_clear() {
  [[ -t 1 ]] || return 0
  if command -v tput >/dev/null 2>&1; then tput clear; else printf '\033[2J\033[3J\033[H'; fi
}

# banner "Title" — opening frame: what this wizard does.
banner() {
  _clear
  printf '\n%s%s  %s%s\n' "$BOLD" "$BLUE" "$1" "$RESET"
  printf '%s  %s stages%s\n\n' "$DIM" "$TOTAL_STAGES" "$RESET"
  printf '%s  You drive the browser; this wizard tells you exactly what to do and\n' "$DIM"
  printf '  captures the values you copy back. Stop any time with Ctrl-C and re-run\n'
  printf '  later — it remembers values already saved.%s\n' "$RESET"
  pause "Ready to start?"
}

# stage "Name" — clear the screen, then announce a stage and show progress.
# Clearing keeps only the current step on screen.
stage() {
  _clear
  _STAGE_INDEX=$((_STAGE_INDEX + 1))
  printf '\n%s%s▸ Stage %s/%s · %s%s\n' \
    "$BOLD" "$BLUE" "$_STAGE_INDEX" "$TOTAL_STAGES" "$1" "$RESET"
}

# say "..." — a plain instruction line.
say()  { printf '  %s\n' "$1"; }
# step "..." — a numbered-feeling action the human takes in the browser.
step() { printf '  %s•%s %s\n' "$BLUE" "$RESET" "$1"; }
note() { printf '  %s%s%s\n' "$DIM" "$1" "$RESET"; }
warn() { printf '  %s⚠ %s%s\n' "$YELLOW" "$1" "$RESET"; }

# open_url URL — open in the human's browser, cross-platform incl. WSL.
open_url() {
  local url="$1"
  printf '  %s↗ opening%s %s\n' "$GREEN" "$RESET" "$url"
  { if   command -v wslview     >/dev/null 2>&1; then wslview "$url"
    elif command -v explorer.exe >/dev/null 2>&1; then explorer.exe "$url"
    elif command -v xdg-open    >/dev/null 2>&1; then xdg-open "$url"
    elif command -v open        >/dev/null 2>&1; then open "$url"
    else warn "couldn't open a browser — visit it manually: $url"; fi
  } >/dev/null 2>&1 || warn "couldn't open a browser — visit it manually: $url"
}

# pause "msg" — wait for the human to confirm they've done the manual part.
pause() {
  printf '  %s%s%s ' "$DIM" "${1:-Press Enter to continue}" "$RESET"
  read -r _ || true
}

# confirm "question" — y/N gate; returns success on yes.
confirm() {
  local reply=""
  printf '  %s? %s [y/N] ' "$YELLOW" "$1"
  read -r reply || true
  [[ "$reply" =~ ^[Yy] ]]
}

# _existing KEY — current value of KEY in ENV_FILE, if any.
_existing() {
  [[ -f "$ENV_FILE" ]] || return 1
  local line; line=$(grep -E "^${1}=" "$ENV_FILE" | tail -n1) || return 1
  printf '%s' "${line#*=}"
}

# ask KEY "Prompt" — read a value into $KEY. Offers the existing .env value as
# a default on re-runs (Enter keeps it). Visible input (non-secret).
ask() {
  local key="$1" prompt="$2" current input
  current=$(_existing "$key" || true)
  if [[ -n "$current" ]]; then
    printf '  %s%s%s %s[Enter keeps current]%s ' "$BOLD" "$prompt" "$RESET" "$DIM" "$RESET"
  else
    printf '  %s%s%s ' "$BOLD" "$prompt" "$RESET"
  fi
  read -r input || true
  [[ -z "$input" && -n "$current" ]] && input="$current"
  printf -v "$key" '%s' "$input"
}

# ask_secret KEY "Prompt" — like ask, but input is hidden.
ask_secret() {
  local key="$1" prompt="$2" current input
  current=$(_existing "$key" || true)
  if [[ -n "$current" ]]; then
    printf '  %s%s%s %s[Enter keeps current]%s ' "$BOLD" "$prompt" "$RESET" "$DIM" "$RESET"
  else
    printf '  %s%s%s ' "$BOLD" "$prompt" "$RESET"
  fi
  read -rs input || true
  printf '\n'
  [[ -z "$input" && -n "$current" ]] && input="$current"
  printf -v "$key" '%s' "$input"
}

# write_env KEY VALUE — upsert KEY=VALUE into ENV_FILE (creates it; replaces
# any existing line). Idempotent.
write_env() {
  local key="$1" value="$2" tmp
  touch "$ENV_FILE"
  tmp=$(mktemp)
  grep -vE "^${key}=" "$ENV_FILE" > "$tmp" || true
  printf '%s=%s\n' "$key" "$value" >> "$tmp"
  mv "$tmp" "$ENV_FILE"
  WRITTEN_ENV+=("$key")
  printf '  %s✓ wrote%s %s → %s\n' "$GREEN" "$RESET" "$key" "$ENV_FILE"
}

# set_secret NAME VALUE — set a GitHub Actions repo secret via gh. Falls back
# to a warning (and records it) if gh is unavailable or unauthenticated.
set_secret() {
  local name="$1" value="$2"
  if command -v gh >/dev/null 2>&1 && gh auth status >/dev/null 2>&1; then
    if printf '%s' "$value" | gh secret set "$name" >/dev/null 2>&1; then
      WRITTEN_SECRET+=("$name")
      printf '  %s✓ set%s GitHub secret %s\n' "$GREEN" "$RESET" "$name"
      return
    fi
  fi
  SKIPPED+=("GitHub secret $name (set it manually: gh secret set $name)")
  warn "skipped GitHub secret $name — gh not ready; set it later"
}

# set_var NAME VALUE — set a GitHub Actions repo variable (non-secret).
set_var() {
  local name="$1" value="$2"
  if command -v gh >/dev/null 2>&1 && gh auth status >/dev/null 2>&1; then
    if gh variable set "$name" --body "$value" >/dev/null 2>&1; then
      printf '  %s✓ set%s GitHub variable %s\n' "$GREEN" "$RESET" "$name"
      return
    fi
  fi
  SKIPPED+=("GitHub variable $name")
  warn "skipped GitHub variable $name — gh not ready; set it later"
}

# finish — clear, then a closing summary of everything configured.
finish() {
  _clear
  printf '\n%s%s  ✓ Setup complete%s\n' "$BOLD" "$GREEN" "$RESET"
  (( ${#WRITTEN_ENV[@]} ))    && note "wrote ${#WRITTEN_ENV[@]} value(s) to $ENV_FILE: ${WRITTEN_ENV[*]}"
  (( ${#WRITTEN_SECRET[@]} )) && note "set ${#WRITTEN_SECRET[@]} GitHub secret(s): ${WRITTEN_SECRET[*]}"
  if (( ${#SKIPPED[@]} )); then
    printf '\n'; warn "still to do by hand:"
    for s in "${SKIPPED[@]}"; do note "  - $s"; done
  fi
  printf '\n'
}

# ──────────────────────────────────────────────────────────────────────────
# STAGES — release pipeline setup for pimp-my-dsh.
# ──────────────────────────────────────────────────────────────────────────

TOTAL_STAGES=5
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
KEY_FILE="$REPO_ROOT/keys/tauri-updater.key"
PW_FILE="$REPO_ROOT/keys/tauri-updater.password.txt"

banner "Desktop release setup (signing + updater)"

# ── Stage 1 — updater key custody ─────────────────────────────────────────
stage "Updater key — move it offline"
say "The update-signing keypair was generated:"
note "  public key  → committed (tauri.conf.json plugins.updater.pubkey)"
note "  private key → $KEY_FILE (gitignored)"
note "  password    → $PW_FILE (gitignored)"
warn "Losing the private key OR password bricks updates for every installed user. No recovery."
step "Copy BOTH files into a password manager (e.g. Bitwarden secure note)."
if confirm "Key and password are stored offline + backed up?"; then
  note "Custody confirmed — the offline backup is the source of truth."
else
  warn "Do this before the first release — updates are un-signable without it."
fi
pause "Press Enter to continue."

# ── Stage 2 — GitHub secrets for updater signing ──────────────────────────
stage "GitHub secrets — updater key"
say "CI signs updater artifacts with these two secrets."
if [[ -f "$KEY_FILE" && -f "$PW_FILE" ]]; then
  UPDATER_KEY="$(cat "$KEY_FILE")"
  UPDATER_PW="$(tr -d '\r\n' < "$PW_FILE")"
  note "  reading key + password from the local keys/ files"
else
  warn "keys/ files not found (moved already?) — paste the values instead."
  ask_secret UPDATER_KEY "Paste the updater private key (file contents):"
  ask_secret UPDATER_PW "Paste the updater key password:"
fi
set_secret TAURI_SIGNING_PRIVATE_KEY "$UPDATER_KEY"
set_secret TAURI_SIGNING_PRIVATE_KEY_PASSWORD "$UPDATER_PW"
note "Local 'pnpm desktop:bundle' builds read keys/ automatically via scripts/build-desktop.ps1 — keep the files here while you build locally."
note "These are a working copy — the offline backup from Stage 1 is the source of truth."

# ── Stage 3 — free signing provider (no purchase) ─────────────────────────
stage "Authenticode — choose a free signing path"
say "Paid OV/EV certificates are off the table. Two free options for open-source projects:"
say "  1. SignPath Foundation — free code signing for OSS projects (recommended starting point)."
say "  2. Microsoft Azure Trusted Signing — free tier, signs via their service (no PFX download)."
open_url "https://about.signpath.io/product/open-source"
open_url "https://learn.microsoft.com/en-us/azure/trusted-signing/overview"
step "Compare both; pick the one whose account/identity verification you can complete."
note "Both integrate with CI, but differently from the PFX flow:"
note "  - SignPath: their GitHub Action submits artifacts to the SignPath service."
note "  - Trusted Signing: signtool with the Azure CodeSigning dlib + metadata file."
note "The release workflow's PFX step will be swapped for the chosen provider's integration when you pick one."
ask SIGNING_PROVIDER "Which provider do you plan to use (signpath / trusted-signing / later)?"
note "Noted: $SIGNING_PROVIDER — update .github/workflows/release.yml accordingly before the first release."
pause "Press Enter when you have an account (or have decided to defer)."

# ── Stage 4 — Authenticode PFX (only if you already have a cert) ──────────
stage "Authenticode PFX — optional, only with an existing certificate"
if confirm "Do you have an Authenticode PFX file right now?"; then
  ask PFX_PATH "Path to the .pfx file (e.g. C:\\certs\\mycert.pfx):"
  if [[ -f "$PFX_PATH" ]]; then
    PFX_B64="$(base64 -w0 "$PFX_PATH" 2>/dev/null || base64 "$PFX_PATH" | tr -d '\n')"
    ask_secret PFX_PW "PFX password:"
    set_secret CERT_PFX_BASE64 "$PFX_B64"
    set_secret CERT_PFX_PASSWORD "$PFX_PW"
    note "Local signing: import the PFX into your cert store and build with"
    note "  tauri build --config '{\"bundle\":{\"windows\":{\"certificateThumbprint\":\"<thumb>\",\"timestampUrl\":\"http://timestamp.digicert.com\",\"digestAlgorithm\":\"sha256\"}}}'"
  else
    warn "File not found at $PFX_PATH — re-run this wizard once the PFX exists."
  fi
else
  note "No PFX — fine. The release workflow's release blocker stays active:"
  note "  no signing secrets → no release build. Only dev artifacts until then."
fi

# ── Stage 5 — first release ───────────────────────────────────────────────
stage "First release"
say "The release workflow triggers on tags matching v*."
if confirm "Do you want to tag and push a release now?"; then
  ask TAG "Tag name (e.g. v0.1.0):"
  step "From the repo root:  git tag $TAG && git push origin $TAG"
  if command -v gh >/dev/null 2>&1 && gh auth status >/dev/null 2>&1; then
    open_url "https://github.com/koopmannleon19977-cmyk/pimp-my-dsh/actions"
    pause "Watch the Release workflow finish, then press Enter."
    note "Verify the release assets: latest.json + *.sig + the signed *-setup.exe."
    note "SmartScreen note: a brand-new signing identity has no reputation yet;"
    note "  first downloads may still warn until the cert builds trust."
  else
    warn "gh not authenticated — push the tag manually, then watch the Actions tab."
  fi
else
  note "Skipping. When ready: git tag v0.1.0 && git push origin v0.1.0"
fi

finish

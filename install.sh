#!/usr/bin/env bash
set -euo pipefail

REPO="oscarmuya/aw-watcher-docker"
BIN_NAME="aw-watcher-docker"
MODULE_ARGS="--poll-time 10 --no-collect-stats"

say() {
  printf '%s\n' "$*"
}

die() {
  printf 'error: %s\n' "$*" >&2
  exit 1
}

need_cmd() {
  command -v "$1" >/dev/null 2>&1 || die "missing required command: $1"
}

detect_platform() {
  os="$(uname -s)"
  arch="$(uname -m)"

  case "$os" in
    Linux*) platform="linux" ;;
    Darwin*) platform="macos" ;;
    MINGW*|MSYS*|CYGWIN*) platform="windows" ;;
    *) die "unsupported OS: $os" ;;
  esac

  case "$arch" in
    x86_64|amd64) cpu="x86_64" ;;
    arm64|aarch64) cpu="aarch64" ;;
    *) die "unsupported architecture: $arch" ;;
  esac

  case "$platform-$cpu" in
    linux-x86_64)
      asset="aw-watcher-docker-linux-x86_64.tar.gz"
      archive_type="tar.gz"
      ;;
    linux-aarch64)
      asset="aw-watcher-docker-linux-aarch64.tar.gz"
      archive_type="tar.gz"
      ;;
    macos-x86_64)
      asset="aw-watcher-docker-macos-x86_64.tar.gz"
      archive_type="tar.gz"
      ;;
    macos-aarch64)
      asset="aw-watcher-docker-macos-aarch64.tar.gz"
      archive_type="tar.gz"
      ;;
    windows-x86_64)
      asset="aw-watcher-docker-windows-x86_64.zip"
      archive_type="zip"
      BIN_NAME="aw-watcher-docker.exe"
      ;;
    *)
      die "no release asset mapping for $platform-$cpu"
      ;;
  esac
}

config_path() {
  case "$platform" in
    linux)
      printf '%s/activitywatch/aw-tauri/config.toml' "${XDG_CONFIG_HOME:-$HOME/.config}"
      ;;
    macos)
      printf '%s/Library/Application Support/activitywatch/aw-tauri/config.toml' "$HOME"
      ;;
    windows)
      local appdata="${APPDATA:-}"
      if [ -z "$appdata" ]; then
        appdata="$HOME/AppData/Roaming"
      fi
      printf '%s/activitywatch/aw-tauri/config.toml' "$appdata"
      ;;
  esac
}

module_dir() {
  case "$platform" in
    linux|macos)
      printf '%s/aw-modules' "$HOME"
      ;;
    windows)
      local userprofile="${USERPROFILE:-$HOME}"
      printf '%s/aw-modules' "$userprofile"
      ;;
  esac
}

download() {
  local url="$1"
  local output="$2"

  if command -v curl >/dev/null 2>&1; then
    curl -fL "$url" -o "$output"
  elif command -v wget >/dev/null 2>&1; then
    wget -O "$output" "$url"
  else
    die "missing curl or wget"
  fi
}

extract_archive() {
  local archive="$1"
  local dest="$2"

  mkdir -p "$dest"

  case "$archive_type" in
    tar.gz)
      need_cmd tar
      tar -xzf "$archive" -C "$dest"
      ;;
    zip)
      if command -v unzip >/dev/null 2>&1; then
        unzip -o "$archive" -d "$dest" >/dev/null
      elif command -v powershell.exe >/dev/null 2>&1; then
        powershell.exe -NoProfile -Command "Expand-Archive -Force '$archive' '$dest'"
      else
        die "missing unzip or powershell.exe"
      fi
      ;;
  esac
}

ensure_config() {
  local cfg="$1"

  if [ ! -f "$cfg" ]; then
    say "aw-tauri config not found: $cfg"
    say "skipping config edit"
    return 0
  fi

  if grep -q 'aw-watcher-docker' "$cfg"; then
    say "aw-watcher-docker already registered in: $cfg"
    return 0
  fi

  command -v python3 >/dev/null 2>&1 || die "python3 is required to safely edit aw-tauri config"

  cp "$cfg" "$cfg.bak"

  python3 - "$cfg" <<'PY'
import re
import sys
from pathlib import Path

cfg_path = Path(sys.argv[1])
text = cfg_path.read_text()

module = '  { name = "aw-watcher-docker", args = "--poll-time 10 --no-collect-stats" },'

if "aw-watcher-docker" in text:
    sys.exit(0)

section_match = re.search(r'(?m)^\[autostart\]\s*$', text)
if not section_match:
    print("aw-tauri config has no [autostart] section; skipping config edit")
    sys.exit(0)

section_start = section_match.end()
next_section = re.search(r'(?m)^\[.*\]\s*$', text[section_start:])
section_end = section_start + next_section.start() if next_section else len(text)

before = text[:section_start]
section = text[section_start:section_end]
after = text[section_end:]

modules_match = re.search(r'(?ms)^modules\s*=\s*\[(.*?)^\]', section)

if not modules_match:
    print("[autostart] has no multiline modules array; skipping config edit")
    sys.exit(0)

insert_at = modules_match.end() - 1
section = section[:insert_at] + module + "\n" + section[insert_at:]

cfg_path.write_text(before + section + after)
PY

  say "registered aw-watcher-docker in: $cfg"
  say "backup saved at: $cfg.bak"
}

main() {
  detect_platform

  install_dir="$(module_dir)"
  cfg="$(config_path)"
  tmpdir="$(mktemp -d)"
  trap 'rm -rf "$tmpdir"' EXIT

  url="https://github.com/$REPO/releases/latest/download/$asset"
  archive="$tmpdir/$asset"
  extract_dir="$tmpdir/extract"

  say "downloading $asset"
  download "$url" "$archive"

  say "extracting"
  extract_archive "$archive" "$extract_dir"

  found_bin="$(find "$extract_dir" -type f -name "$BIN_NAME" | head -n 1)"
  [ -n "$found_bin" ] || die "binary not found in release archive: $BIN_NAME"

  mkdir -p "$install_dir"
  cp "$found_bin" "$install_dir/$BIN_NAME"

  if [ "$platform" != "windows" ]; then
    chmod +x "$install_dir/$BIN_NAME"
  fi

  ensure_config "$cfg"

  say "installed: $install_dir/$BIN_NAME"
  say "restart aw-tauri so it discovers and starts aw-watcher-docker"
}

main "$@"

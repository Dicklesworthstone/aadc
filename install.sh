#!/usr/bin/env bash
#
# aadc installer
#
# One-liner install (with cache buster):
#   curl -fsSL "https://raw.githubusercontent.com/Dicklesworthstone/aadc/main/install.sh?$(date +%s)" | bash
#
# Or without cache buster:
#   curl -fsSL https://raw.githubusercontent.com/Dicklesworthstone/aadc/main/install.sh | bash
#
# Options:
#   --version vX.Y.Z     Install specific version (default: latest)
#   --dest DIR           Install to DIR (default: ~/.local/bin)
#   --system             Install to /usr/local/bin (requires sudo)
#   --easy-mode          Auto-update PATH in shell rc files
#   --verify             Run self-test after install
#   --from-source        Build from source instead of downloading binary
#   --no-verify-checksum Skip SHA-256 verification of downloaded archives
#   --quiet              Suppress non-error output
#   --no-gum             Disable gum formatting even if available
#
set -euo pipefail
umask 022
shopt -s lastpipe 2>/dev/null || true

ORIG_PWD=$(pwd)
VERSION="${VERSION:-}"
OWNER="${OWNER:-Dicklesworthstone}"
REPO="${REPO:-aadc}"
REPO_URL="${REPO_URL:-https://github.com/${OWNER}/${REPO}.git}"
DEST_DEFAULT="$HOME/.local/bin"
DEST="${DEST:-$DEST_DEFAULT}"
EASY=0
QUIET=0
VERIFY=0
FROM_SOURCE=0
SKIP_CHECKSUM="${SKIP_CHECKSUM:-0}"
CHECKSUM="${CHECKSUM:-}"
CHECKSUM_URL="${CHECKSUM_URL:-}"
ARTIFACT_URL="${ARTIFACT_URL:-}"
LOCK_FILE="${LOCK_FILE:-/tmp/aadc-install.lock}"
SYSTEM=0
NO_GUM=0

# Detect gum for fancy output (https://github.com/charmbracelet/gum)
HAS_GUM=0
if command -v gum &> /dev/null && [ -t 1 ]; then
  HAS_GUM=1
fi

# Logging functions with optional gum formatting
log() { [ "$QUIET" -eq 1 ] && return 0; echo -e "$@"; }

info() {
  [ "$QUIET" -eq 1 ] && return 0
  if [ "$HAS_GUM" -eq 1 ] && [ "$NO_GUM" -eq 0 ]; then
    gum style --foreground 39 "→ $*"
  else
    echo -e "\033[0;34m→\033[0m $*"
  fi
}

ok() {
  [ "$QUIET" -eq 1 ] && return 0
  if [ "$HAS_GUM" -eq 1 ] && [ "$NO_GUM" -eq 0 ]; then
    gum style --foreground 42 "✓ $*"
  else
    echo -e "\033[0;32m✓\033[0m $*"
  fi
}

warn() {
  [ "$QUIET" -eq 1 ] && return 0
  if [ "$HAS_GUM" -eq 1 ] && [ "$NO_GUM" -eq 0 ]; then
    gum style --foreground 214 "⚠ $*"
  else
    echo -e "\033[1;33m⚠\033[0m $*"
  fi
}

err() {
  if [ "$HAS_GUM" -eq 1 ] && [ "$NO_GUM" -eq 0 ]; then
    gum style --foreground 196 "✗ $*" >&2
  else
    echo -e "\033[0;31m✗\033[0m $*" >&2
  fi
}

# Spinner wrapper for long operations
run_with_spinner() {
  local title="$1"
  shift
  if [ "$HAS_GUM" -eq 1 ] && [ "$NO_GUM" -eq 0 ] && [ "$QUIET" -eq 0 ]; then
    gum spin --spinner dot --title "$title" -- "$@"
  else
    info "$title"
    "$@"
  fi
}

resolve_version() {
  if [ -n "$VERSION" ]; then return 0; fi
  if [ "$FROM_SOURCE" -eq 0 ] && [ -n "$ARTIFACT_URL" ]; then return 0; fi

  info "Resolving latest version..."
  local latest_url="https://api.github.com/repos/${OWNER}/${REPO}/releases/latest"
  local tag=""
  if ! tag=$(curl -fsSL -H "Accept: application/vnd.github.v3+json" "$latest_url" 2>/dev/null | grep -o '"tag_name": *"[^"]*"' | head -n 1 | sed -E 's/.*"([^"]*)"$/\1/'); then
    tag=""
  fi
  case "$tag" in v[0-9]*|[0-9]*) ;; *) tag="" ;; esac

  if [ -n "$tag" ]; then
    VERSION="$tag"
    info "Resolved latest version: $VERSION"
    return 0
  fi

  # Try redirect-based resolution as fallback
  local redirect_url="https://github.com/${OWNER}/${REPO}/releases/latest"
  if tag=$(curl -fsSL -o /dev/null -w '%{url_effective}' "$redirect_url" 2>/dev/null | sed -E 's|.*/tag/||'); then
    # Validate: tag must be non-empty, start with 'v' + digit, and not contain URL chars
    if [ -n "$tag" ] && [[ "$tag" =~ ^v[0-9] ]] && [[ "$tag" != *"/"* ]]; then
      VERSION="$tag"
      info "Resolved latest version via redirect: $VERSION"
      return 0
    fi
  fi

  err "Could not resolve latest release version from GitHub. Specify --version vX.Y.Z or check network connectivity."
  exit 1
}

maybe_add_path() {
  case ":$PATH:" in
    *:"$DEST":*)
      return 0
      ;;
    *)
      if [ "$EASY" -eq 1 ]; then
        UPDATED=0
        for rc in "$HOME/.zshrc" "$HOME/.bashrc"; do
          if [ -e "$rc" ] && [ -w "$rc" ]; then
            if ! grep -F "$DEST" "$rc" >/dev/null 2>&1; then
              echo "export PATH=\"$DEST:\$PATH\"" >> "$rc"
            fi
            UPDATED=1
          fi
        done
        if [ "$UPDATED" -eq 1 ]; then
          warn "PATH updated in ~/.zshrc/.bashrc; restart shell to use aadc"
        else
          warn "Add $DEST to PATH to use aadc"
        fi
      else
        warn "Add $DEST to PATH to use aadc"
      fi
      ;;
  esac
}

ensure_rust() {
  if [ "${RUSTUP_INIT_SKIP:-0}" != "0" ]; then
    info "Skipping rustup install (RUSTUP_INIT_SKIP set)"
    return 0
  fi
  if command -v cargo >/dev/null 2>&1 && rustc --version 2>/dev/null | grep -q nightly; then return 0; fi
  if [ "$EASY" -ne 1 ]; then
    if [ -t 0 ]; then
      echo -n "Install Rust nightly via rustup? (y/N): "
      read -r ans
      case "$ans" in y|Y) :;; *) warn "Skipping rustup install"; return 0;; esac
    fi
  fi
  info "Installing rustup (nightly)"
  local rustup_init="${TMP:-/tmp}/rustup-init-$$.sh"
  curl -fsSL https://sh.rustup.rs -o "$rustup_init"
  sh "$rustup_init" -y --default-toolchain nightly --profile minimal
  rm -f "$rustup_init"
  export PATH="$HOME/.cargo/bin:$PATH"
  rustup component add rustfmt clippy || true
}

checkout_source_ref() {
  local src_dir="$1"
  local ref="$2"

  if ! command -v git >/dev/null 2>&1; then
    err "git is required to build from source"
    exit 1
  fi

  if [ -n "$ref" ]; then
    info "Fetching source at ${ref} from ${REPO_URL}"
    if git clone --depth 1 --branch "$ref" --single-branch "$REPO_URL" "$src_dir" 2>/dev/null; then
      return 0
    fi

    # Fallback for detached tags/commits that --branch cannot clone directly
    rm -rf "$src_dir"
    mkdir -p "$src_dir"
    if git -C "$src_dir" init -q \
      && git -C "$src_dir" remote add origin "$REPO_URL" \
      && { git -C "$src_dir" fetch --depth 1 origin "refs/tags/${ref}:refs/tags/${ref}" 2>/dev/null \
           || git -C "$src_dir" fetch --depth 1 origin "$ref" 2>/dev/null; } \
      && git -C "$src_dir" checkout -q --detach FETCH_HEAD; then
      return 0
    fi

    err "Requested version/ref '${ref}' could not be checked out from ${REPO_URL}"
    exit 1
  fi

  info "Cloning default branch from ${REPO_URL}"
  git clone --depth 1 "$REPO_URL" "$src_dir"
}

resolve_cargo_binary() {
  local src_dir="$1"
  local build_jsonl="$2"
  local candidate=""

  # 1. Prefer the exact executable reported by cargo build --message-format=json-render-diagnostics
  if [ -f "$build_jsonl" ]; then
    if command -v jq >/dev/null 2>&1; then
      candidate=$(jq -R -r 'fromjson? | select(.reason == "compiler-artifact" and .target.name == "aadc" and ((.target.kind // []) | index("bin")) and (.executable != null)) | .executable' "$build_jsonl" 2>/dev/null | tail -n 1 || true)
    elif command -v python3 >/dev/null 2>&1; then
      candidate=$(python3 - "$build_jsonl" <<'PY' 2>/dev/null || true
import json, sys
exe = ""
with open(sys.argv[1], "r", encoding="utf-8", errors="replace") as fh:
    for line in fh:
        line = line.strip()
        if not line.startswith("{"):
            continue
        try:
            obj = json.loads(line)
        except Exception:
            continue
        if obj.get("reason") != "compiler-artifact":
            continue
        target = obj.get("target") or {}
        if target.get("name") == "aadc" and "bin" in (target.get("kind") or []):
            if obj.get("executable"):
                exe = obj["executable"]
if exe:
    print(exe)
PY
)
    else
      candidate=$(grep '"reason":"compiler-artifact"' "$build_jsonl" 2>/dev/null \
        | grep '"name":"aadc"' \
        | sed -n 's/.*"executable":"\([^"]*\)".*/\1/p' \
        | tail -n 1 || true)
    fi
    if [ -n "$candidate" ] && [ -x "$candidate" ]; then
      printf '%s\n' "$candidate"
      return 0
    fi
  fi

  # 2. Fall back to Cargo's resolved target_directory (respects CARGO_TARGET_DIR and .cargo/config.toml)
  local target_dir=""
  local meta_json=""
  meta_json=$(cd "$src_dir" && cargo metadata --format-version 1 --no-deps 2>/dev/null || true)
  if [ -n "$meta_json" ]; then
    if command -v jq >/dev/null 2>&1; then
      target_dir=$(printf '%s' "$meta_json" | jq -r '.target_directory // empty' 2>/dev/null || true)
    elif command -v python3 >/dev/null 2>&1; then
      target_dir=$(printf '%s' "$meta_json" | python3 -c 'import json,sys; print(json.load(sys.stdin).get("target_directory", ""))' 2>/dev/null || true)
    else
      target_dir=$(printf '%s' "$meta_json" | sed -n 's/.*"target_directory":"\([^"]*\)".*/\1/p' | head -n 1 || true)
    fi
  fi

  if [ -n "$target_dir" ]; then
    if [ -n "${CARGO_BUILD_TARGET:-}" ] && [ -x "${target_dir}/${CARGO_BUILD_TARGET}/release/aadc" ]; then
      printf '%s\n' "${target_dir}/${CARGO_BUILD_TARGET}/release/aadc"
      return 0
    fi
    if [ -x "${target_dir}/release/aadc" ]; then
      printf '%s\n' "${target_dir}/release/aadc"
      return 0
    fi
  fi

  # 3. Final fallback to default workspace target directory
  if [ -x "${src_dir}/target/release/aadc" ]; then
    printf '%s\n' "${src_dir}/target/release/aadc"
    return 0
  fi

  return 1
}

run_self_test() {
  local bin="$1"
  local ver_out
  if ! ver_out=$("$bin" --version 2>&1); then
    err "Self-test failed: '$bin --version' exited non-zero"
    exit 1
  fi
  info "Installed binary reports: ${ver_out}"
  local sample_in sample_out
  sample_in=$(printf '+-----+\n| hi|\n+-----+\n')
  if ! sample_out=$(printf '%s\n' "$sample_in" | "$bin"); then
    err "Self-test failed: diagram correction exited non-zero"
    exit 1
  fi
  if ! printf '%s\n' "$sample_out" | grep -Fq '| hi  |'; then
    err "Self-test failed: unexpected correction output"
    exit 1
  fi
  ok "Self-test complete"
}

usage() {
  cat <<EOFU
Usage: install.sh [--version vX.Y.Z] [--dest DIR] [--system] [--easy-mode] [--verify] \\
                  [--from-source] [--artifact-url URL] [--checksum HEX] [--checksum-url URL] \\
                  [--no-verify-checksum] [--quiet] [--no-gum]

Options:
  --version vX.Y.Z     Install specific version (default: latest)
  --dest DIR           Install to DIR (default: ~/.local/bin)
  --system             Install to /usr/local/bin (requires sudo)
  --easy-mode          Auto-update PATH in shell rc files
  --verify             Run self-test after install
  --from-source        Build from source instead of downloading binary
  --artifact-url URL   Download prebuilt archive directly from URL
  --checksum HEX       Expected SHA-256 hex digest for prebuilt archive
  --checksum-url URL   URL to .sha256 file for prebuilt archive
  --no-verify-checksum Skip SHA-256 verification of downloaded archives
  --quiet, -q          Suppress non-error output
  --no-gum             Disable gum formatting even if available
  -h, --help           Show this help message
EOFU
}

require_arg() {
  if [ "$2" -lt 2 ]; then
    err "Missing required value for $1"
    usage >&2
    exit 2
  fi
}

while [ $# -gt 0 ]; do
  case "$1" in
    --version) require_arg "$1" "$#"; VERSION="$2"; shift 2;;
    --dest) require_arg "$1" "$#"; DEST="$2"; shift 2;;
    --system) SYSTEM=1; DEST="/usr/local/bin"; shift;;
    --easy-mode) EASY=1; shift;;
    --verify) VERIFY=1; shift;;
    --artifact-url) require_arg "$1" "$#"; ARTIFACT_URL="$2"; shift 2;;
    --checksum) require_arg "$1" "$#"; CHECKSUM="$2"; shift 2;;
    --checksum-url) require_arg "$1" "$#"; CHECKSUM_URL="$2"; shift 2;;
    --no-verify-checksum) SKIP_CHECKSUM=1; shift;;
    --from-source) FROM_SOURCE=1; shift;;
    --quiet|-q) QUIET=1; shift;;
    --no-gum) NO_GUM=1; shift;;
    -h|--help) usage; exit 0;;
    *) err "Unknown option: $1"; usage >&2; exit 2;;
  esac
done

# Show fancy header
if [ "$QUIET" -eq 0 ]; then
  if [ "$HAS_GUM" -eq 1 ] && [ "$NO_GUM" -eq 0 ]; then
    gum style \
      --border normal \
      --border-foreground 39 \
      --padding "0 1" \
      --margin "1 0" \
      "$(gum style --foreground 42 --bold 'aadc installer')" \
      "$(gum style --foreground 245 'ASCII Art Diagram Corrector')"
  else
    echo ""
    echo -e "\033[1;32maadc installer\033[0m"
    echo -e "\033[0;90mASCII Art Diagram Corrector\033[0m"
    echo ""
  fi
fi

resolve_version

if [ "$SYSTEM" -eq 1 ]; then
  [ -d "$DEST" ] || sudo mkdir -p "$DEST"
else
  mkdir -p "$DEST"
fi

OS=$(uname -s | tr '[:upper:]' '[:lower:]')
ARCH=$(uname -m)
case "$ARCH" in
  x86_64|amd64) ARCH="amd64" ;;
  arm64|aarch64) ARCH="arm64" ;;
  *) warn "Unknown arch $ARCH, using as-is" ;;
esac

# Map OS/ARCH to release asset name
ASSET=""
case "${OS}-${ARCH}" in
  linux-amd64) ASSET="aadc-linux-amd64" ;;
  linux-arm64) ASSET="aadc-linux-arm64" ;;
  darwin-amd64) ASSET="aadc-darwin-amd64" ;;
  darwin-arm64) ASSET="aadc-darwin-arm64" ;;
  *) :;;
esac

# Prefer prebuilt artifact when we know the target or the caller supplied a direct URL.
TAR=""
URL=""
if [ "$FROM_SOURCE" -eq 0 ]; then
  if [ -n "$ARTIFACT_URL" ]; then
    TAR=$(basename "${ARTIFACT_URL%%\?*}")
    URL="$ARTIFACT_URL"
  elif [ -n "$ASSET" ]; then
    TAR="${ASSET}.tar.xz"
    URL="https://github.com/${OWNER}/${REPO}/releases/download/${VERSION}/${TAR}"
  else
    warn "No prebuilt artifact for ${OS}/${ARCH}; falling back to build-from-source"
    FROM_SOURCE=1
  fi
fi

# Cross-platform locking using mkdir (atomic on all POSIX systems including macOS)
LOCK_DIR="${LOCK_FILE}.d"
LOCKED=0
if mkdir "$LOCK_DIR" 2>/dev/null; then
  LOCKED=1
  echo $$ > "$LOCK_DIR/pid"
else
  # Check if existing lock is stale (process no longer running)
  if [ -f "$LOCK_DIR/pid" ]; then
    OLD_PID=$(cat "$LOCK_DIR/pid" 2>/dev/null || echo "")
    if [ -n "$OLD_PID" ] && ! kill -0 "$OLD_PID" 2>/dev/null; then
      rm -rf "$LOCK_DIR"
      if mkdir "$LOCK_DIR" 2>/dev/null; then
        LOCKED=1
        echo $$ > "$LOCK_DIR/pid"
      fi
    fi
  fi
  if [ "$LOCKED" -eq 0 ]; then
    err "Another installer is running (lock $LOCK_DIR)"
    exit 1
  fi
fi

cleanup() {
  if [ -n "${TMP:-}" ]; then rm -rf "$TMP"; fi
  if [ "$LOCKED" -eq 1 ]; then rm -rf "$LOCK_DIR"; fi
}

TMP=$(mktemp -d)
trap cleanup EXIT

if [ "$FROM_SOURCE" -eq 0 ]; then
  info "Downloading $URL"
  if ! curl -fsSL "$URL" -o "$TMP/$TAR"; then
    if [ -n "$ARTIFACT_URL" ]; then
      err "Failed to download artifact from $URL"
      exit 1
    fi
    warn "Artifact download failed; falling back to build-from-source"
    FROM_SOURCE=1
  fi
fi

if [ "$FROM_SOURCE" -eq 1 ]; then
  info "Building from source (requires git, rust nightly)"
  ensure_rust
  if ! command -v cargo >/dev/null 2>&1; then
    err "cargo not found on PATH; install Rust or pass a prebuilt binary"
    exit 1
  fi

  # Preserve caller-relative CARGO_TARGET_DIR across cd into temp clone
  if [ -n "${CARGO_TARGET_DIR:-}" ]; then
    case "$CARGO_TARGET_DIR" in
      /*) ;;
      *) export CARGO_TARGET_DIR="${ORIG_PWD}/${CARGO_TARGET_DIR}" ;;
    esac
  fi

  checkout_source_ref "$TMP/src" "$VERSION"
  SRC_SHA=$(git -C "$TMP/src" rev-parse --short HEAD 2>/dev/null || echo "unknown")
  info "Building commit ${SRC_SHA} (${VERSION:-default})"

  BUILD_JSONL="$TMP/cargo-build.jsonl"
  (cd "$TMP/src" && cargo build --release --message-format=json-render-diagnostics >"$BUILD_JSONL")

  if ! BIN=$(resolve_cargo_binary "$TMP/src" "$BUILD_JSONL") || [ ! -x "$BIN" ]; then
    err "Build succeeded but compiled aadc binary could not be located"
    exit 1
  fi

  if [ "$SYSTEM" -eq 1 ]; then
    sudo install -m 0755 "$BIN" "$DEST/aadc"
  else
    install -m 0755 "$BIN" "$DEST/aadc"
  fi
  ok "Installed to $DEST/aadc (source build @ ${VERSION:-HEAD}, commit ${SRC_SHA})"
  maybe_add_path
  if [ "$VERIFY" -eq 1 ]; then
    run_self_test "$DEST/aadc"
  fi
  ok "Done. Binary at: $DEST/aadc"
  exit 0
fi

# Verify checksum
if [ "$SKIP_CHECKSUM" -eq 1 ]; then
  warn "Skipping SHA-256 verification (--no-verify-checksum)"
  CHECKSUM="SKIP"
elif [ -z "$CHECKSUM" ]; then
  if [ -z "$CHECKSUM_URL" ]; then
    if [ -n "$ARTIFACT_URL" ]; then
      CHECKSUM_URL="${ARTIFACT_URL}.sha256"
    else
      CHECKSUM_URL="https://github.com/${OWNER}/${REPO}/releases/download/${VERSION}/${TAR}.sha256"
    fi
  fi
  info "Fetching checksum from ${CHECKSUM_URL}"
  CHECKSUM_FILE="$TMP/checksum.sha256"
  if ! curl -fsSL "$CHECKSUM_URL" -o "$CHECKSUM_FILE"; then
    err "Failed to download checksum from ${CHECKSUM_URL} (use --no-verify-checksum to bypass)"
    exit 1
  fi
  CHECKSUM=$(awk '{print $1}' "$CHECKSUM_FILE" 2>/dev/null | head -n 1 || true)
  if ! [[ "$CHECKSUM" =~ ^[0-9a-fA-F]{64}$ ]]; then
    err "Invalid SHA-256 checksum format from ${CHECKSUM_URL}"
    exit 1
  fi
fi

if [ "$CHECKSUM" != "SKIP" ]; then
  # Cross-platform checksum verification (Linux uses sha256sum, macOS uses shasum)
  if command -v sha256sum >/dev/null 2>&1; then
    echo "$CHECKSUM  $TMP/$TAR" | sha256sum -c - || { err "Checksum mismatch"; exit 1; }
    ok "Checksum verified"
  elif command -v shasum >/dev/null 2>&1; then
    echo "$CHECKSUM  $TMP/$TAR" | shasum -a 256 -c - || { err "Checksum mismatch"; exit 1; }
    ok "Checksum verified"
  else
    err "Neither sha256sum nor shasum is available to verify checksum (use --no-verify-checksum to bypass)"
    exit 1
  fi
fi

info "Extracting"
tar -xJf "$TMP/$TAR" -C "$TMP"
BIN="$TMP/aadc"
if [ ! -x "$BIN" ]; then
  # Try finding it recursively
  BIN=$(find "$TMP" -maxdepth 3 -type f -name "aadc" -perm -111 2>/dev/null | head -n 1)
fi

[ -x "$BIN" ] || { err "Binary not found in archive"; exit 1; }

if [ "$SYSTEM" -eq 1 ]; then
  sudo install -m 0755 "$BIN" "$DEST/aadc"
else
  install -m 0755 "$BIN" "$DEST/aadc"
fi
ok "Installed to $DEST/aadc"
maybe_add_path

if [ "$VERIFY" -eq 1 ]; then
  run_self_test "$DEST/aadc"
fi

ok "Done. Binary at: $DEST/aadc"
echo ""
info "Quick start:"
echo "  # Fix a diagram file"
echo "  aadc diagram.txt"
echo ""
echo "  # Edit in place"
echo "  aadc -i diagram.txt"
echo ""
echo "  # Pipe from stdin"
echo "  cat diagram.txt | aadc"
echo ""

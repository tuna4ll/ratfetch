#!/bin/sh
# ratfetch installer.
#
#   curl -fsSL https://tuna4ll.github.io/ratfetch/install.sh | sh
#
# Downloads the prebuilt binary for this machine from the latest GitHub
# release, verifies its checksum, and drops it in a bin directory on PATH.
#
# Knobs (all optional):
#   RATFETCH_VERSION=v0.1.0   install a specific tag instead of the latest
#   RATFETCH_INSTALL_DIR=DIR  where the binary lands
#   RATFETCH_NO_MODIFY_PATH=1 never touch the shell profile

set -eu

REPO="tuna4ll/ratfetch"
VERSION="${RATFETCH_VERSION:-}"
INSTALL_DIR="${RATFETCH_INSTALL_DIR:-}"

# ANSI only when stderr is a terminal, so piped output stays clean.
if [ -t 2 ]; then
    bold=$(printf '\033[1m'); dim=$(printf '\033[2m')
    red=$(printf '\033[31m'); green=$(printf '\033[32m'); reset=$(printf '\033[0m')
else
    bold=''; dim=''; red=''; green=''; reset=''
fi

say() { printf '%s\n' "${dim}::${reset} $*" >&2; }
ok() { printf '%s\n' "${green}✓${reset} $*" >&2; }
die() { printf '%s\n' "${red}error:${reset} $*" >&2; exit 1; }

need() { command -v "$1" >/dev/null 2>&1 || die "\`$1\` is required but not installed."; }

# --- what are we running on -------------------------------------------------

[ "$(uname -s)" = "Linux" ] || die "ratfetch is Linux only — the collectors read /proc and /sys."

case "$(uname -m)" in
    x86_64 | amd64) arch=x86_64 ;;
    aarch64 | arm64) arch=aarch64 ;;
    *) die "unsupported architecture: $(uname -m). Build from source: cargo install --git https://github.com/$REPO" ;;
esac

# musl systems (Alpine, Void musl, …) cannot run the gnu build; glibc systems
# can run either, and the gnu build is the better citizen there.
libc=gnu
if ldd --version 2>&1 | grep -qi musl; then
    libc=musl
else
    for loader in /lib/ld-musl-*; do
        [ -e "$loader" ] && libc=musl
    done
fi

target="${arch}-unknown-linux-${libc}"

# --- how do we fetch --------------------------------------------------------

if command -v curl >/dev/null 2>&1; then
    fetch() { curl -fsSL "$1"; }
    fetch_to() { curl -fsSL -o "$2" "$1"; }
elif command -v wget >/dev/null 2>&1; then
    fetch() { wget -qO- "$1"; }
    fetch_to() { wget -qO "$2" "$1"; }
else
    die "neither curl nor wget found."
fi

need tar
need mktemp

# --- which version ----------------------------------------------------------

if [ -z "$VERSION" ]; then
    say "resolving the latest release"
    # One grep over the release JSON is enough; pulling in jq for a single
    # field would be a heavier dependency than the whole installer.
    # Buffered into a variable first: piping curl straight into grep -m1 kills
    # the transfer mid-write and prints a spurious "failure writing output".
    latest=$(fetch "https://api.github.com/repos/$REPO/releases/latest" || true)
    VERSION=$(printf '%s\n' "$latest" | grep '"tag_name"' | head -1 | cut -d'"' -f4)
    [ -n "$VERSION" ] || die "could not resolve the latest release. Set RATFETCH_VERSION=vX.Y.Z."
fi

name="ratfetch-${VERSION}-${target}"
base="https://github.com/$REPO/releases/download/$VERSION"

say "installing ${bold}ratfetch ${VERSION}${reset} for ${bold}${target}${reset}"

# --- download and verify ----------------------------------------------------

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT INT TERM

fetch_to "$base/${name}.tar.gz" "$tmp/${name}.tar.gz" \
    || die "no build published for ${target} in ${VERSION}."

if fetch_to "$base/SHA256SUMS" "$tmp/SHA256SUMS" 2>/dev/null; then
    expected=$(grep " ${name}.tar.gz\$" "$tmp/SHA256SUMS" | cut -d' ' -f1 || true)
    if [ -n "$expected" ] && command -v sha256sum >/dev/null 2>&1; then
        actual=$(sha256sum "$tmp/${name}.tar.gz" | cut -d' ' -f1)
        [ "$expected" = "$actual" ] || die "checksum mismatch for ${name}.tar.gz — refusing to install."
        ok "checksum verified"
    else
        say "skipping checksum verification (no entry or no sha256sum)"
    fi
fi

tar -xzf "$tmp/${name}.tar.gz" -C "$tmp"
[ -x "$tmp/$name/ratfetch" ] || die "the archive did not contain a ratfetch binary."

# --- pick a destination -----------------------------------------------------

if [ -z "$INSTALL_DIR" ]; then
    if [ "$(id -u)" -eq 0 ]; then
        INSTALL_DIR=/usr/local/bin
    else
        INSTALL_DIR="${XDG_BIN_HOME:-$HOME/.local/bin}"
    fi
fi

mkdir -p "$INSTALL_DIR" || die "cannot create $INSTALL_DIR."
install -m 755 "$tmp/$name/ratfetch" "$INSTALL_DIR/ratfetch" \
    || die "cannot write to $INSTALL_DIR. Set RATFETCH_INSTALL_DIR to somewhere writable."

ok "installed ${bold}$INSTALL_DIR/ratfetch${reset}"

# --- PATH -------------------------------------------------------------------

case ":$PATH:" in
    *":$INSTALL_DIR:"*) on_path=1 ;;
    *) on_path=0 ;;
esac

if [ "$on_path" -eq 0 ] && [ "${RATFETCH_NO_MODIFY_PATH:-0}" != "1" ]; then
    line="export PATH=\"$INSTALL_DIR:\$PATH\""
    added=''
    for profile in "$HOME/.bashrc" "$HOME/.zshrc"; do
        [ -f "$profile" ] || continue
        grep -qF "$line" "$profile" && continue
        printf '\n# added by the ratfetch installer\n%s\n' "$line" >> "$profile"
        added="$added $(basename "$profile")"
    done
    if [ -n "$added" ]; then
        ok "added $INSTALL_DIR to PATH in$added — open a new shell, or run: $line"
    else
        say "$INSTALL_DIR is not on your PATH. Add it with: $line"
    fi
elif [ "$on_path" -eq 0 ]; then
    say "$INSTALL_DIR is not on your PATH. Add it with: export PATH=\"$INSTALL_DIR:\$PATH\""
fi

printf '\n%s\n' "${green}ratfetch ${VERSION} is ready.${reset} Run ${bold}ratfetch${reset} to start, or ${bold}ratfetch --once${reset} for a single frame." >&2

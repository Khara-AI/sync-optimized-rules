#!/bin/sh
# Install or update to the latest sor release (Linux, macOS).
#   curl -fsSL https://github.com/Khara-AI/sync-optimized-rules/releases/latest/download/install.sh | sh
# Uninstall: same command followed by `-s -- --uninstall`.
# SOR_INSTALL_DIR overrides the target folder (default ~/.local/bin),
# SOR_VERSION pins a release (e.g. 0.1.0).
set -eu

repo="Khara-AI/sync-optimized-rules"
dest="${SOR_INSTALL_DIR:-$HOME/.local/bin}"

if [ "${1:-}" = "--uninstall" ]; then
  if [ -e "$dest/sor" ]; then
    rm "$dest/sor"
    echo "sor removed from $dest."
  else
    echo "sor is not installed in $dest."
  fi
  echo "Your configuration is left in ${XDG_CONFIG_HOME:-$HOME/.config}/sync-optimized-rules."
  exit 0
fi

case "$(uname -s)" in
  Linux) os="unknown-linux-musl" ;;
  Darwin) os="apple-darwin" ;;
  *) echo "sor: unsupported system $(uname -s), use install.ps1 on Windows" >&2; exit 1 ;;
esac
case "$(uname -m)" in
  x86_64 | amd64) arch="x86_64" ;;
  aarch64 | arm64) arch="aarch64" ;;
  *) echo "sor: unsupported architecture $(uname -m)" >&2; exit 1 ;;
esac

if [ -n "${SOR_VERSION:-}" ]; then
  base="https://github.com/$repo/releases/download/v$SOR_VERSION"
else
  base="https://github.com/$repo/releases/latest/download"
fi
asset="sor-$arch-$os.tar.gz"

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
curl -fsSL "$base/$asset" -o "$tmp/$asset"
curl -fsSL "$base/SHA256SUMS" -o "$tmp/SHA256SUMS"

expected="$(grep " $asset\$" "$tmp/SHA256SUMS" | cut -d' ' -f1)"
if command -v sha256sum >/dev/null 2>&1; then
  actual="$(sha256sum "$tmp/$asset" | cut -d' ' -f1)"
else
  actual="$(shasum -a 256 "$tmp/$asset" | cut -d' ' -f1)"
fi
if [ -z "$expected" ] || [ "$expected" != "$actual" ]; then
  echo "sor: checksum mismatch for $asset" >&2
  exit 1
fi

tar -xzf "$tmp/$asset" -C "$tmp"
mkdir -p "$dest"
install -m 755 "$tmp/sor" "$dest/sor"
echo "sor installed in $dest/sor"
case ":$PATH:" in
  *":$dest:"*) ;;
  *) echo "Add $dest to your PATH to run it." ;;
esac

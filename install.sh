#!/bin/sh
# Install or update to the latest sor release (Linux, macOS).
#   curl -fsSL https://github.com/Khara-AI/sync-optimized-rules/releases/latest/download/install.sh | sh
# Add `-s -- --plugin` to install the Claude Code plugin too, `-s -- --uninstall`
# to remove sor (with `--plugin`, the plugin as well).
# SOR_INSTALL_DIR overrides the target folder (default ~/.local/bin),
# SOR_VERSION pins a release (e.g. 0.1.0), SOR_MARKETPLACE installs the plugin
# from another source (a local checkout).
set -eu

repo="Khara-AI/sync-optimized-rules"
dest="${SOR_INSTALL_DIR:-$HOME/.local/bin}"
marketplace="${SOR_MARKETPLACE:-$repo}"

plugin=""
uninstall=""
for arg in "$@"; do
  case "$arg" in
    --plugin) plugin=1 ;;
    --uninstall) uninstall=1 ;;
    *) echo "sor: unknown option $arg (use --plugin, --uninstall)" >&2; exit 1 ;;
  esac
done

need_claude() {
  if ! command -v claude >/dev/null 2>&1; then
    echo "sor: claude was not found. Install Claude Code, then run:" >&2
    echo "  claude plugin $1" >&2
    exit 1
  fi
}

if [ -n "$uninstall" ]; then
  if [ -e "$dest/sor" ]; then
    rm "$dest/sor"
    echo "sor removed from $dest."
  else
    echo "sor is not installed in $dest."
  fi
  if [ -n "$plugin" ]; then
    need_claude "uninstall sor@sync-optimized-rules"
    claude plugin uninstall sor@sync-optimized-rules
    claude plugin marketplace remove sync-optimized-rules
  fi
  echo "Your configuration is left in ${XDG_CONFIG_HOME:-$HOME/.config}/sync-optimized-rules."
  exit 0
fi
if [ -n "$plugin" ]; then
  need_claude "install sor --marketplace $repo"
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

if [ -n "$plugin" ]; then
  claude plugin install sor --marketplace "$marketplace"
  echo "In Claude Code, run /reload-plugins (or restart it), then /sor:sor."
fi

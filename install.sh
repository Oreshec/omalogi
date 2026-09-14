#!/usr/bin/env bash
# Install Omalogi for the current user:
#
#   curl -fsSL https://raw.githubusercontent.com/elberacasa/omalogi/main/install.sh | bash
#
# Downloads the latest release, checks its SHA-256, installs the binary to ~/.local/bin,
# installs the udev rule that lets your user reach the mouse (sudo, once), and runs
# `omalogi setup` for the shell plugin, bar indicator and daemon. Run it again to update.
#
# OMALOGI_VERSION=v0.1.0 pins a release; OMALOGI_BIN_DIR changes where the binary goes.
set -euo pipefail

REPO=elberacasa/omalogi
VERSION=${OMALOGI_VERSION:-latest}
BIN_DIR=${OMALOGI_BIN_DIR:-$HOME/.local/bin}
RULE=/etc/udev/rules.d/70-omalogi.rules

say() { printf '\033[1m==>\033[0m %s\n' "$*"; }
die() { printf 'omalogi install: %s\n' "$*" >&2; exit 1; }

[ "$(id -u)" -ne 0 ] || die "run this as your user, not root; it asks for sudo once, for the udev rule"
[ "$(uname -s)" = Linux ] || die "Omalogi runs on Linux"
case "$(uname -m)" in
  x86_64) target=x86_64-unknown-linux-gnu ;;
  *) die "there is no prebuilt binary for $(uname -m) yet; install from the AUR (omalogi) or with cargo" ;;
esac
for tool in curl tar sha256sum install; do
  command -v "$tool" >/dev/null 2>&1 || die "needs $tool"
done

if [ "$VERSION" = latest ]; then
  base="https://github.com/$REPO/releases/latest/download"
else
  base="https://github.com/$REPO/releases/download/$VERSION"
fi
name="omalogi-$target"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

say "Downloading $name.tar.gz"
curl -fsSL --proto '=https' --tlsv1.2 --retry 3 --retry-delay 2 -o "$tmp/$name.tar.gz" "$base/$name.tar.gz"
curl -fsSL --proto '=https' --tlsv1.2 --retry 3 --retry-delay 2 -o "$tmp/$name.tar.gz.sha256" "$base/$name.tar.gz.sha256"
(cd "$tmp" && sha256sum --check --status "$name.tar.gz.sha256") \
  || die "the download does not match its checksum; nothing was installed"
tar -xzf "$tmp/$name.tar.gz" -C "$tmp"

install -Dm755 "$tmp/$name/omalogi" "$BIN_DIR/omalogi"
say "Installed $("$BIN_DIR/omalogi" --version) to $BIN_DIR"

if [ -f /usr/lib/udev/rules.d/70-omalogi.rules ]; then
  say "The udev rule is already installed by a package"
elif ! cmp -s "$tmp/$name/70-omalogi.rules" "$RULE"; then
  say "Installing the udev rule, so your user can reach the mouse without root (sudo)"
  sudo install -Dm644 "$tmp/$name/70-omalogi.rules" "$RULE"
  sudo udevadm control --reload-rules
  sudo udevadm trigger --subsystem-match=hidraw --action=change
fi

case ":$PATH:" in
  *":$BIN_DIR:"*) ;;
  *) say "Add $BIN_DIR to your PATH to run omalogi by name" ;;
esac

if [ -d "$HOME/.config/omarchy" ] || command -v omarchy-shell >/dev/null 2>&1; then
  "$BIN_DIR/omalogi" setup
else
  say "Omarchy was not found, so the shell plugin was skipped; run \`omalogi setup\` once it is installed"
fi

say "Done. With the mouse plugged in, \`omalogi info\` shows it."

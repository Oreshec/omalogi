#!/usr/bin/env bash
# Install or update Omalogi's helper for the current user.
#
# After `omarchy plugin add https://github.com/elberacasa/omalogi --enable`, Omalogi runs
# this script from its plugin folder when the helper is missing or too old. To run it
# yourself: bash ~/.config/omarchy/plugins/io.github.elberacasa.omalogi/install.sh
#
# Downloads the release binary from this repository's GitHub releases, checks its
# SHA-256, installs it to ~/.local/bin, installs the udev rule that lets your user reach
# the mouse (sudo, once), and runs `omalogi setup` for the bar indicator and daemon.
#
# OMALOGI_VERSION=v0.1.0 pins a release; OMALOGI_BIN_DIR changes where the binary goes.
set -euo pipefail

VERSION=${OMALOGI_VERSION:-latest}
BIN_DIR=${OMALOGI_BIN_DIR:-$HOME/.local/bin}
RULE=/etc/udev/rules.d/70-omalogi.rules
PACKAGED_RULE=/usr/lib/udev/rules.d/70-omalogi.rules

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
  base="https://github.com/elberacasa/omalogi/releases/latest/download"
else
  base="https://github.com/elberacasa/omalogi/releases/download/$VERSION"
fi
name="omalogi-$target"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

say "Downloading $name.tar.gz"
# A stalled download host otherwise hangs the installer for good: give up on a connection
# after 15 s and a transfer after 5 minutes, and retry either (the binary is ~3 MB).
fetch() {
  curl -fsSL --proto '=https' --tlsv1.2 --connect-timeout 15 --max-time 300 \
    --retry 3 --retry-delay 2 --retry-all-errors -o "$1" "$2" \
    || die "could not download $2; check your connection and try again"
}
fetch "$tmp/$name.tar.gz" "$base/$name.tar.gz"
fetch "$tmp/$name.tar.gz.sha256" "$base/$name.tar.gz.sha256"
(cd "$tmp" && sha256sum --check --status "$name.tar.gz.sha256") \
  || die "the download does not match its checksum; nothing was installed"
tar -xzf "$tmp/$name.tar.gz" -C "$tmp"

install -Dm755 "$tmp/$name/omalogi" "$BIN_DIR/omalogi"
say "Installed $("$BIN_DIR/omalogi" --version) to $BIN_DIR"

# A rule in /etc/udev/rules.d takes precedence over one of the same name in /usr/lib, so
# a current copy in /etc is enough; one in /usr/lib (from a package) is only trusted
# while it matches this release, since older rules miss newer mice and receivers.
rule="$tmp/$name/70-omalogi.rules"
if cmp -s "$rule" "$RULE"; then
  say "The udev rule is up to date"
elif [ ! -f "$RULE" ] && cmp -s "$rule" "$PACKAGED_RULE"; then
  say "The udev rule is up to date (installed by a package)"
else
  if [ -f "$RULE" ] || [ -f "$PACKAGED_RULE" ]; then
    say "Updating the udev rule, which covers more mice in this release (sudo)"
  else
    say "Installing the udev rule, so your user can reach the mouse without root (sudo)"
  fi
  sudo install -Dm644 "$rule" "$RULE"
  sudo udevadm control --reload-rules
  sudo udevadm trigger --subsystem-match=hidraw --action=change
fi

case ":$PATH:" in
  *":$BIN_DIR:"*) ;;
  *) say "Add $BIN_DIR to your PATH to run omalogi by name" ;;
esac

if [ -d "$HOME/.config/omarchy" ] || command -v omarchy-shell >/dev/null 2>&1; then
  # `omalogi setup` changes your bar and user services, so show exactly what and ask first.
  say "omalogi setup would make these changes:"
  "$BIN_DIR/omalogi" setup --dry-run
  answer=n
  if { exec 3</dev/tty; } 2>/dev/null; then
    printf '\033[1m==>\033[0m Apply them? [Y/n] ' >/dev/tty
    read -r answer <&3 || answer=n
    exec 3<&-
    answer=${answer:-y}
  fi
  case "$answer" in
    [Yy]*) "$BIN_DIR/omalogi" setup ;;
    *) say "Skipped. Run \`omalogi setup\` when you want the bar indicator and automatic switching" ;;
  esac
else
  say "Omarchy was not found, so the shell plugin was skipped; run \`omalogi setup\` once it is installed"
fi

if [ -d "$HOME/.config/omarchy" ] || command -v omarchy-shell >/dev/null 2>&1; then
  say "Done. Open Omalogi from the bar or the Omarchy menu to set up your mouse."
else
  say "Done. With the mouse plugged in, \`omalogi info\` shows it."
fi

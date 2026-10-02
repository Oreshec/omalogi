#!/usr/bin/env bash
# Install or update Omalogi's helper for the current user.
#
# After `omarchy plugin add https://github.com/elberacasa/omalogi --enable`, Omalogi runs
# this script from its plugin folder when the helper is missing or too old. To run it
# yourself: bash ~/.config/omarchy/plugins/io.github.elberacasa.omalogi/install.sh
#
# Downloads one exact helper release, checks its archive against the SHA-256 recorded
# below, installs the binary to ~/.local/bin and the udev rule that lets your user reach
# the mouse (sudo, once), and runs `omalogi setup` for the bar indicator and daemon.
#
# OMALOGI_VERSION with OMALOGI_SHA256 installs another release instead; both are needed,
# since a checksum published next to a download only catches a broken transfer.
# OMALOGI_BIN_DIR changes where the binary goes.
set -euo pipefail

# The helper this plugin installs: its release and the SHA-256 of its x86_64 archive.
# Both are part of the reviewed source, so the helper installed is the one reviewed with
# this plugin, never whatever release is newest. The release's own .sha256 file is not
# used. Updated by the release steps in CONTRIBUTING.md.
HELPER_VERSION=0.3.5
HELPER_SHA256=68bdf19d71518712335de0b5c5900ee2a2b76b314a2520df5492e6b3252529c4

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

version=$HELPER_VERSION
sha256=$HELPER_SHA256
if [ -n "${OMALOGI_VERSION:-}" ] || [ -n "${OMALOGI_SHA256:-}" ]; then
  [ -n "${OMALOGI_VERSION:-}" ] && [ -n "${OMALOGI_SHA256:-}" ] \
    || die "set OMALOGI_VERSION and OMALOGI_SHA256 together, or neither"
  version=${OMALOGI_VERSION#v}
  sha256=$OMALOGI_SHA256
fi
case "$sha256" in
  *[!0-9a-f]* | "") die "the SHA-256 must be 64 lowercase hex digits" ;;
esac
[ ${#sha256} -eq 64 ] || die "the SHA-256 must be 64 lowercase hex digits"

name="omalogi-$target"
url="https://github.com/elberacasa/omalogi/releases/download/v$version/$name.tar.gz"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

say "Downloading Omalogi $version ($name.tar.gz)"
# A stalled download host otherwise hangs the installer for good: give up on a connection
# after 15 s and a transfer after 5 minutes, and retry either (the binary is ~3 MB).
fetch() {
  curl -fsSL --proto '=https' --tlsv1.2 --connect-timeout 15 --max-time 300 \
    --retry 3 --retry-delay 2 --retry-all-errors -o "$1" "$2" \
    || die "could not download $2; check your connection and try again"
}
fetch "$tmp/$name.tar.gz" "$url"
printf '%s  %s\n' "$sha256" "$tmp/$name.tar.gz" | sha256sum --check --status \
  || die "the download does not match the recorded SHA-256 for $version; nothing was installed"
tar -xzf "$tmp/$name.tar.gz" -C "$tmp"
reported=$("$tmp/$name/omalogi" --version)
[ "$reported" = "omalogi $version" ] \
  || die "the archive for $version holds $reported; nothing was installed"

install -Dm755 "$tmp/$name/omalogi" "$BIN_DIR/omalogi"
say "Installed $reported to $BIN_DIR"

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

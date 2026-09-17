#!/usr/bin/env bash
# Remove Omalogi from this user's system: the reverse of install.sh and `omalogi setup`.
#
# To run it: bash ~/.config/omarchy/plugins/io.github.elberacasa.omalogi/uninstall.sh
#
# Stops and disables the daemon, removes its user unit, the helper in ~/.local/bin and
# the udev rule install.sh added, or an early manual copy no package owns (sudo), then
# removes the plugin with
# `omarchy plugin remove`, which also takes it off the bar. Shows the plan and asks first.
#
# Kept: the profiles on your mouse (uninstalling changes nothing on it), backups and
# acceptances in ~/.local/state/omalogi, and rules in ~/.config/omalogi.
#
# --dry-run shows the plan without changing anything; --yes skips the question, for the
# overlay's Uninstall, which asks first. OMALOGI_BIN_DIR matches install.sh.
set -euo pipefail

PLUGIN_ID=io.github.elberacasa.omalogi
BIN_DIR=${OMALOGI_BIN_DIR:-$HOME/.local/bin}
RULE=/etc/udev/rules.d/70-omalogi.rules
PACKAGED_RULE=/usr/lib/udev/rules.d/70-omalogi.rules
CONFIG_HOME=${XDG_CONFIG_HOME:-$HOME/.config}
STATE_HOME=${XDG_STATE_HOME:-$HOME/.local/state}
UNIT=$CONFIG_HOME/systemd/user/omalogi.service
PLUGIN_DIR=$CONFIG_HOME/omarchy/plugins/$PLUGIN_ID

say() { printf '\033[1m==>\033[0m %s\n' "$*"; }
die() { printf 'omalogi uninstall: %s\n' "$*" >&2; exit 1; }

dry_run=false
assume_yes=false
for arg in "$@"; do
  case "$arg" in
    --dry-run) dry_run=true ;;
    --yes) assume_yes=true ;;
    *) die "unknown option $arg; use --dry-run or --yes" ;;
  esac
done

[ "$(id -u)" -ne 0 ] || die "run this as your user, not root; it asks for sudo once, for the udev rule"

# What install.sh and `omalogi setup` left, and only that.
steps=()
if systemctl --user is-enabled --quiet omalogi.service 2>/dev/null \
  || systemctl --user is-active --quiet omalogi.service 2>/dev/null; then
  steps+=("daemon")
fi
if [ -f "$UNIT" ] && grep -q 'omalogi daemon' "$UNIT"; then
  steps+=("unit")
fi
if [ -x "$BIN_DIR/omalogi" ] && "$BIN_DIR/omalogi" --version 2>/dev/null | grep -q '^omalogi '; then
  steps+=("binary")
fi
if [ -f "$RULE" ]; then
  steps+=("rule")
fi
# A copy in /usr/lib is the package's to remove, unless no package owns it (a manual
# install from the README of an early version).
if [ -f "$PACKAGED_RULE" ] && command -v pacman >/dev/null 2>&1 \
  && ! pacman -Qo "$PACKAGED_RULE" >/dev/null 2>&1; then
  steps+=("unowned-rule")
fi
if [ -d "$PLUGIN_DIR" ] && command -v omarchy >/dev/null 2>&1; then
  steps+=("plugin")
fi

if [ ${#steps[@]} -eq 0 ]; then
  say "Nothing to remove: Omalogi is not installed for this user"
  exit 0
fi

say "Uninstalling Omalogi would:"
for step in "${steps[@]}"; do
  case "$step" in
    daemon) echo "    stop and disable omalogi.service" ;;
    unit) echo "    remove $UNIT" ;;
    binary) echo "    remove $BIN_DIR/omalogi" ;;
    rule) echo "    remove $RULE (sudo)" ;;
    unowned-rule) echo "    remove $PACKAGED_RULE, which no package owns (sudo)" ;;
    plugin) echo "    remove the plugin and its bar indicator (omarchy plugin remove $PLUGIN_ID)" ;;
  esac
done
echo "  and keep your mouse's profiles, backups in $STATE_HOME/omalogi and rules in $CONFIG_HOME/omalogi."
if command -v pacman >/dev/null 2>&1 && pacman -Qq omalogi >/dev/null 2>&1; then
  echo "  The omalogi package stays installed; remove it with: sudo pacman -R omalogi"
fi

if $dry_run; then
  say "Dry run: nothing was changed"
  exit 0
fi

answer=n
if $assume_yes; then
  answer=y
elif { exec 3</dev/tty; } 2>/dev/null; then
  printf '\033[1m==>\033[0m Uninstall? [y/N] ' >/dev/tty
  read -r answer <&3 || answer=n
  exec 3<&-
fi
case "$answer" in
  [Yy]*) ;;
  *) say "Cancelled; nothing was changed"; exit 0 ;;
esac

for step in "${steps[@]}"; do
  case "$step" in
    daemon)
      systemctl --user disable --now omalogi.service
      say "Stopped and disabled omalogi.service"
      ;;
    unit)
      rm -f "$UNIT"
      systemctl --user daemon-reload
      say "Removed $UNIT"
      ;;
    binary)
      rm -f "$BIN_DIR/omalogi"
      say "Removed $BIN_DIR/omalogi"
      ;;
    rule | unowned-rule)
      path=$RULE
      [ "$step" = rule ] || path=$PACKAGED_RULE
      sudo rm -f "$path"
      sudo udevadm control --reload-rules
      say "Removed $path"
      ;;
    plugin)
      # Last: it deletes the folder this script may be running from.
      omarchy plugin remove "$PLUGIN_ID"
      say "Removed the plugin"
      ;;
  esac
done

say "Omalogi is uninstalled. Your mouse keeps its onboard profiles."

#!/usr/bin/env bash
# Check the helper release install.sh pins: its archive must exist and match the
# recorded SHA-256. With --release, the pin must also name the version this tree is, so
# main never holds a plugin that installs another helper.
set -euo pipefail

cd "$(dirname "$0")/.."

die() { printf 'check-helper-pin: %s\n' "$*" >&2; exit 1; }

release=false
case "${1:-}" in
  "") ;;
  --release) release=true ;;
  *) die "unknown option $1; the only option is --release" ;;
esac

pinned() { sed -n "s/^$1=//p" install.sh; }
version=$(pinned HELPER_VERSION)
sha256=$(pinned HELPER_SHA256)
if [ -z "$version" ] || [ -z "$sha256" ]; then
  die "install.sh pins no HELPER_VERSION or HELPER_SHA256"
fi

if $release; then
  cargo=$(sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml | head -n 1)
  manifest=$(sed -n 's/^ *"version": "\(.*\)",$/\1/p' manifest.json)
  [ "$version" = "$cargo" ] || die "install.sh pins $version, but Cargo.toml is $cargo"
  [ "$version" = "$manifest" ] || die "install.sh pins $version, but manifest.json is $manifest"
fi

name=omalogi-x86_64-unknown-linux-gnu
url="https://github.com/elberacasa/omalogi/releases/download/v$version/$name.tar.gz"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
curl -fsSL --proto '=https' --tlsv1.2 --connect-timeout 15 --max-time 300 \
  --retry 3 --retry-delay 2 --retry-all-errors -o "$tmp/$name.tar.gz" "$url" \
  || die "could not download $url"
actual=$(sha256sum "$tmp/$name.tar.gz" | cut -d ' ' -f 1)
[ "$actual" = "$sha256" ] || die "v$version's archive is $actual, but install.sh pins $sha256"

echo "install.sh pins omalogi $version, and its archive matches the recorded SHA-256"

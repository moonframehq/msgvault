#!/usr/bin/env bash
# Puts APP in a compressed disk image at DMG beside an Applications
# shortcut, so installing is dragging one onto the other.
set -euo pipefail
[[ $# == 2 ]] || { echo "usage: make-dmg.sh APP DMG" >&2; exit 2; }
app=$1
dmg=$2
[[ -d $app/Contents/MacOS ]] || { echo "not an app bundle: $app" >&2; exit 1; }
[[ ! -e $dmg ]] || { echo "refusing to overwrite $dmg" >&2; exit 1; }

stage=$(mktemp -d)
trap 'rm -rf -- "$stage"' EXIT
# ditto keeps the bundle's signature, symlinks and extended attributes.
ditto "$app" "$stage/$(basename "$app")"
ln -s /Applications "$stage/Applications"
hdiutil create -quiet -volname msgvault -srcfolder "$stage" -fs HFS+ -format UDZO "$dmg"
hdiutil verify -quiet "$dmg"
echo "$dmg"

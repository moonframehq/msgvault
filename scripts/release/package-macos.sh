#!/usr/bin/env bash
# Packages the macOS release in OUT_DIR from one executable per
# architecture: a universal msgvault.app (the app's own `bundle` command:
# the binary, its app folder and Moonframe's toolbox with its Wasm providers
# prebuilt, signed ad hoc), msgvault.app.tar for the signing job, and the
# disk image users download, msgvault-moonframe-VERSION-universal-apple-darwin.dmg.
# Developer ID signing and notarization happen later, in
# scripts/release/sign-macos.sh.
set -euo pipefail
[[ $# -ge 2 ]] || { echo "usage: package-macos.sh OUT_DIR BINARY..." >&2; exit 2; }
[[ $(uname -s) == Darwin ]] || { echo "macOS required" >&2; exit 1; }
out=$1
shift
version=$(scripts/release/version.sh)
mkdir -p "$out"
out=$(cd "$out" && pwd)

work=$(mktemp -d)
trap 'rm -rf -- "$work"' EXIT
lipo -create "$@" -output "$work/msgvault-moonframe"
lipo -info "$work/msgvault-moonframe"

# Outside the checkout's target/, the binary runs the app compiled into it,
# which is what it bundles. It runs from here so cargo builds the Wasm
# providers with the pinned toolchain.
"$work/msgvault-moonframe" bundle "$work/bundle" >/dev/null
app="$work/bundle/msgvault.app"
codesign --verify --strict "$app"
# The bundled executable must find the app inside the bundle and load every
# module of it, headlessly, on each architecture this Mac can run.
for arch in $(lipo -archs "$app/Contents/MacOS/msgvault-moonframe"); do
    if arch "-$arch" /usr/bin/true 2>/dev/null; then
        MSGVAULT_DEMO=1 arch "-$arch" "$app/Contents/MacOS/msgvault-moonframe" --check --no-user
    else
        echo "skipping the $arch check: this Mac cannot run it" >&2
    fi
done

tar -C "$work/bundle" -cf "$out/msgvault.app.tar" msgvault.app
scripts/release/make-dmg.sh "$app" "$out/msgvault-moonframe-$version-universal-apple-darwin.dmg"

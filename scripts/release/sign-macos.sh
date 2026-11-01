#!/usr/bin/env bash
# Signs the packaged msgvault.app (APP_TAR, from package-macos.sh) with a
# Developer ID under the hardened runtime, notarizes and staples it, then
# builds, signs, notarizes and staples the disk image in OUT_DIR. It never
# launches the app. Credentials come from the environment (the release
# environment's secrets) and live in a throwaway keychain.
set +x
set -euo pipefail
set -E
trap 'printf "sign-macos.sh: failed at line %d\n" "$LINENO" >&2' ERR
[[ $# == 2 ]] || { echo "usage: sign-macos.sh APP_TAR OUT_DIR" >&2; exit 2; }
[[ $(uname -s) == Darwin ]] || { echo "macOS required" >&2; exit 1; }
for name in MACOS_CERTIFICATE_P12_BASE64 MACOS_CERTIFICATE_PASSWORD MACOS_SIGNING_IDENTITY \
    APPLE_API_PRIVATE_KEY APPLE_API_KEY_ID APPLE_API_ISSUER_ID; do
    [[ -n ${!name:-} ]] || { echo "required environment variable: $name" >&2; exit 1; }
done
[[ $MACOS_SIGNING_IDENTITY == 'Developer ID Application: '* ]] \
    || { echo "a Developer ID Application identity is required" >&2; exit 1; }
version=$(scripts/release/version.sh)
entitlements=$(pwd)/assets/macos/msgvault.entitlements
mkdir -p "$2"
dmg=$(cd "$2" && pwd)/msgvault-moonframe-$version-universal-apple-darwin.dmg
[[ ! -e $dmg ]] || { echo "refusing to overwrite $dmg" >&2; exit 1; }

umask 077
tmp=$(mktemp -d)
keychain=$tmp/signing.keychain-db
original_keychains=()
while IFS= read -r line; do
    [[ $line =~ \"(.*)\" ]] && original_keychains+=("${BASH_REMATCH[1]}")
done < <(security list-keychains -d user)
cleanup() {
    local status=$?
    trap - EXIT
    security list-keychains -d user -s "${original_keychains[@]}" || status=1
    [[ ! -e $keychain ]] || security delete-keychain "$keychain" || status=1
    rm -rf -- "$tmp"
    exit "$status"
}
trap cleanup EXIT

password=$(openssl rand -hex 32)
printf '%s' "$MACOS_CERTIFICATE_P12_BASE64" | base64 -D > "$tmp/certificate.p12"
printf '%s\n' "$APPLE_API_PRIVATE_KEY" > "$tmp/AuthKey.p8"
unset MACOS_CERTIFICATE_P12_BASE64 APPLE_API_PRIVATE_KEY
security create-keychain -p "$password" "$keychain"
# codesign looks identities up in the user search list, not --keychain.
security list-keychains -d user -s "${original_keychains[@]}" "$keychain"
security set-keychain-settings -lut 21600 "$keychain"
security unlock-keychain -p "$password" "$keychain"
security import "$tmp/certificate.p12" -k "$keychain" -P "$MACOS_CERTIFICATE_PASSWORD" -T /usr/bin/codesign >/dev/null
unset MACOS_CERTIFICATE_PASSWORD
security set-key-partition-list -S apple-tool:,apple:,codesign: -s -k "$password" "$keychain" >/dev/null
unset password
rm "$tmp/certificate.p12"

mkdir "$tmp/bundle"
tar -C "$tmp/bundle" -xf "$1"
app=$tmp/bundle/msgvault.app
[[ $(plutil -extract CFBundleShortVersionString raw "$app/Contents/Info.plist") == "$version" ]] \
    || { echo "app version does not match Cargo.toml" >&2; exit 1; }
sign() {
    codesign --force --sign "$MACOS_SIGNING_IDENTITY" --keychain "$keychain" --timestamp "$@"
}
sign --options runtime --entitlements "$entitlements" "$app/Contents/MacOS/msgvault-moonframe"
sign --options runtime --entitlements "$entitlements" "$app"
codesign --verify --deep --strict --verbose=2 "$app"

# Apple's notary service drops connections now and then; retry those. A
# rejected submission still fails: jq checks the status.
apple() {
    local attempt
    for attempt in 1 2 3 4 5; do
        xcrun "$@" && return 0
        echo "xcrun $1 failed (attempt $attempt of 5); retrying in 30s" >&2
        sleep 30
    done
    return 1
}
notarize() {
    apple notarytool submit "$1" --key "$tmp/AuthKey.p8" --key-id "$APPLE_API_KEY_ID" \
        --issuer "$APPLE_API_ISSUER_ID" --wait --output-format json > "$tmp/notary.json"
    jq -e '.status == "Accepted"' "$tmp/notary.json" >/dev/null \
        || { cat "$tmp/notary.json" >&2; echo "notarization was not accepted" >&2; exit 1; }
}
ditto -c -k --keepParent "$app" "$tmp/msgvault.zip"
notarize "$tmp/msgvault.zip"
apple stapler staple "$app"
spctl --assess --type execute --verbose=2 "$app"

scripts/release/make-dmg.sh "$app" "$tmp/msgvault.dmg" >/dev/null
sign "$tmp/msgvault.dmg"
notarize "$tmp/msgvault.dmg"
apple stapler staple "$tmp/msgvault.dmg"
spctl --assess --type open --context context:primary-signature --verbose=2 "$tmp/msgvault.dmg"
chmod 644 "$tmp/msgvault.dmg"
mv "$tmp/msgvault.dmg" "$dmg"
echo "$dmg"

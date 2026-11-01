#!/usr/bin/env bash
# Packages a Linux or Windows release in OUT_DIR: BINARY (which carries its
# app and Moonframe's toolbox inside it) with the license and readme, in
# msgvault-moonframe-VERSION-TARGET.tar.gz (Linux) or .zip (Windows).
set -euo pipefail
[[ $# == 3 ]] || { echo "usage: package-archive.sh TARGET BINARY OUT_DIR" >&2; exit 2; }
target=$1
binary=$2
out=$3
version=$(scripts/release/version.sh)
name="msgvault-moonframe-$version-$target"
mkdir -p "$out"
out=$(cd "$out" && pwd)

work=$(mktemp -d)
trap 'rm -rf -- "$work"' EXIT
root="$work/$name"
mkdir -p "$root"
cp "$binary" "$root/"
cp LICENSE README.md "$root/"
# Outside the checkout, the binary must run the app compiled into it and
# load every module of it, headlessly.
MSGVAULT_DEMO=1 "$root/$(basename "$binary")" --check --no-user

case $target in
*-windows-*)
    # Git Bash on the Windows runners has Python but no zip.
    python=$(command -v python3 || command -v python)
    (cd "$work" && "$python" -c 'import shutil, sys; shutil.make_archive(sys.argv[1], "zip", ".", sys.argv[2])' "$out/$name" "$name")
    echo "$out/$name.zip"
    ;;
*)
    tar -C "$work" -czf "$out/$name.tar.gz" "$name"
    echo "$out/$name.tar.gz"
    ;;
esac

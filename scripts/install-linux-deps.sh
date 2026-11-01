#!/usr/bin/env bash
# The system libraries GPUI builds against on Ubuntu 24.04 (x86_64 or
# ARM64): X11 and Wayland windows, fonts, and sound.
set -euo pipefail
# A developer and a CI runner need sudo; root in a container has none.
if [[ $EUID -eq 0 ]]; then sudo=(); else sudo=(sudo); fi
"${sudo[@]}" apt-get update
"${sudo[@]}" apt-get install --no-install-recommends -y \
    build-essential pkg-config libxkbcommon-dev libxkbcommon-x11-dev libxcb1-dev \
    libwayland-dev libfreetype6-dev libfontconfig1-dev fonts-dejavu-core libasound2-dev \
    unzip

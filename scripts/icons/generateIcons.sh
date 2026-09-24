#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
source_svg="$root/assets/icon/porthmos.svg"
output_dir="$root/assets/icon/png"
sizes=(16 32 48 64 128 256 512 1024)

if command -v rsvg-convert >/dev/null 2>&1; then
    render() { rsvg-convert --width "$1" --height "$1" --output "$2" "$source_svg"; }
elif command -v resvg >/dev/null 2>&1; then
    render() { resvg --width "$1" --height "$1" "$source_svg" "$2"; }
else
    echo "generateIcons: neither rsvg-convert nor resvg is installed." >&2
    echo "Install one of them, for example:" >&2
    echo "  Debian/Ubuntu: sudo apt install librsvg2-bin" >&2
    echo "  Fedora:        sudo dnf install librsvg2-tools" >&2
    echo "  macOS:         brew install librsvg" >&2
    echo "  Any platform:  cargo install resvg" >&2
    exit 1
fi

mkdir -p "$output_dir"
for size in "${sizes[@]}"; do
    render "$size" "$output_dir/porthmos-$size.png"
    echo "wrote assets/icon/png/porthmos-$size.png"
done

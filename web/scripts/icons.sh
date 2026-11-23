#!/usr/bin/env bash
# The home-screen icons, drawn from static/brand.svg (needs rsvg-convert). The tile goes full
# bleed: iOS and Android cut their own corners, and paint a transparent one black.
set -euo pipefail
cd "$(dirname "$0")/../static"
full=$(mktemp -t iris-icon).svg
sed 's/ rx="11" fill="#0E5E68"/ fill="#0E5E68"/' brand.svg > "$full"
grep -q 'width="48" height="48" fill="#0E5E68"' "$full"
rsvg-convert -w 180 -h 180 "$full" -o apple-touch-icon.png
rsvg-convert -w 192 -h 192 "$full" -o icons/icon-192.png
rsvg-convert -w 512 -h 512 "$full" -o icons/icon-512.png
rm -f "$full"

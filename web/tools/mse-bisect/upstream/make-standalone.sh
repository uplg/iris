#!/usr/bin/env bash
# Builds repro-standalone.html: the same page with the three fragmented MP4s
# embedded as base64, so it runs as a single file with no server and no sibling
# files. That is the form to attach to a bug report.
set -euo pipefail
cd "$(dirname "$0")"

out=repro-standalone.html
{
  printf '<script>\nglobalThis.EMBEDDED_FILES = {\n'
  for f in A-idr-start.mp4 B-cra-clean.mp4 C-cra-with-rasl.mp4; do
    printf '  "%s": "%s",\n' "$f" "$(base64 < "$f" | tr -d '\n')"
  done
  printf '};\n</script>\n'
  cat repro.html
} > "$out"

printf '%s  %s bytes\n' "$out" "$(wc -c < "$out" | tr -d ' ')"

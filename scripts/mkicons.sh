#!/bin/sh
set -eu
command -v rsvg-convert >/dev/null || { echo "rsvg-convert missing: brew install librsvg" >&2; exit 1; }

out="${1:-dist}"
set_dir="$out/oriel.iconset"
rm -rf "$set_dir"
mkdir -p "$set_dir"

for s in 16 32 128 256 512; do
  rsvg-convert -w "$s" -h "$s" assets/icon.svg -o "$set_dir/icon_${s}x${s}.png"
  d=$((s * 2))
  rsvg-convert -w "$d" -h "$d" assets/icon.svg -o "$set_dir/icon_${s}x${s}@2x.png"
done
iconutil -c icns "$set_dir" -o "$out/oriel.icns"

rsvg-convert -w 18 -h 18 assets/menubar-template.svg -o "$out/MenubarTemplate.png"
rsvg-convert -w 36 -h 36 assets/menubar-template.svg -o "$out/MenubarTemplate@2x.png"

echo "wrote $out/oriel.icns + menubar template pngs"

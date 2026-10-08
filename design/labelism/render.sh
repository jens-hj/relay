#!/usr/bin/env bash
# Render concept pages to PNG.
#   ./render.sh            -> soft/ (default theme) and high-contrast/
#   ./render.sh soft 01-workspace.html
cd "$(dirname "$0")"
shot() { # theme-dir hash file
  mkdir -p "$1"
  /nix/store/agamzskapmh5018k98bqfwrrb738bgpv-chromium-153.0.8010.52/bin/chromium --headless=new --disable-gpu --hide-scrollbars --allow-file-access-from-files --force-device-scale-factor=1 \
    --window-size=1600,1000 --screenshot="$PWD/$1/${3%.html}.png" "file://$PWD/$3#$2" 2>/dev/null
}
themes=("${1:-soft high-contrast}"); shift 2>/dev/null || true
files=("$@"); [ ${#files[@]} -eq 0 ] && files=(0*.html)
for t in ${themes[@]}; do
  for f in "${files[@]}"; do shot "$t" "$([ "$t" = high-contrast ] && echo hc)" "$f"; done
done

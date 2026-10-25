#!/usr/bin/env bash
# Compiles maps/<name>.map to assets/maps/<name>.bsp with ericw-tools.
# For the engine's own .lvl format, use mapc instead:
#
#   cargo run -p mapc -- maps/<name>.map assets/maps/<name>.lvl
#
#   ERICW_TOOLS=/path/to/ericw-tools/build tools/compile-map.sh test
#
# qbsp flags:
#   -wrbrushes   keep each brush's planes in a BSPX "BRUSHLIST" lump; the
#                engine collides against those, not Quake's fixed hulls
#   -notex -path assets
#                textures are the engine's own assets/textures/<name>.png;
#                the .bsp stores only their names and sizes, no pixels
#   -missing_textures_as_zero_size
#                keep the names of textures with no PNG (e.g. "trigger")
#
# Compiled maps are committed, so CI and other machines need no tools.
set -euo pipefail

name=${1:?usage: compile-map.sh <map name, without .map>}
tools=${ERICW_TOOLS:?set ERICW_TOOLS to the ericw-tools build directory}
root=$(cd "$(dirname "$0")/.." && pwd)

# Build in a scratch directory: the tools leave .prt and .log files.
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
cp "$root/maps/$name.map" "$work/"

"$tools/qbsp/qbsp" -wrbrushes -notex -path "$root/assets" -missing_textures_as_zero_size \
    "$work/$name.map" "$work/$name.bsp"
"$tools/vis/vis" "$work/$name.bsp"
"$tools/light/light" "$work/$name.bsp"

mkdir -p "$root/assets/maps"
cp "$work/$name.bsp" "$root/assets/maps/$name.bsp"
echo "wrote assets/maps/$name.bsp"

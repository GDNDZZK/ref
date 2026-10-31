#!/usr/bin/env bash
# Review aid: scripts/contact-sheet.sh OUT.jpg IMAGE... puts the images in a labelled grid (ImageMagick).
out="$1"; shift
montage "$@" -label '%t' -tile 2x -geometry 900x+6+6 -background '#222' -fill white "$out"

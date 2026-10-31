#!/usr/bin/env bash
# Review aid: scripts/check-video.sh out/film.en.mp4 [SECONDS_BETWEEN_SAMPLES]
# Prints the stream facts, samples frames into .cache/frames/<name>/ and lists the ones that
# look empty (standard deviation of the pixels near zero). Needs ffmpeg and ImageMagick.
set -euo pipefail
video="$1"; step="${2:-2}"
name=$(basename "$video" .mp4)
dir=".cache/frames/$name"
rm -rf "$dir"; mkdir -p "$dir"
ffprobe -v error -select_streams v:0 -show_entries stream=width,height,r_frame_rate,pix_fmt,color_space,nb_frames,duration -of csv=p=0 "$video"
echo "audio streams: $(ffprobe -v error -select_streams a -show_entries stream=index -of csv=p=0 "$video" | wc -l)"
ffmpeg -v error -i "$video" -vf "fps=1/$step,scale=640:-1" "$dir/f%03d.png"
for frame in "$dir"/f*.png; do
  deviation=$(magick "$frame" -colorspace Gray -format '%[fx:standard_deviation*1000]' info:)
  printf '%s %.0f\n' "$(basename "$frame")" "$deviation"
done | awk '$2 < 15 { print "low contrast:", $1, $2 }'
montage "$dir"/f*.png -tile 4x -geometry 480x+3+3 -background '#000' "$dir.jpg"

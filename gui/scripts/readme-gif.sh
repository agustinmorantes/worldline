#!/usr/bin/env sh
# Turns the clips from readme-media.mjs into the README's GIFs (needs ffmpeg).
set -e
cd "$(dirname "$0")/../../docs/media"
for clip in *.webm; do
  ffmpeg -loglevel error -y -ss 0.8 -i "$clip" \
    -vf "fps=15,scale=960:-1:flags=lanczos,split[a][b];[a]palettegen=stats_mode=diff[p];[b][p]paletteuse=dither=bayer:bayer_scale=4" \
    "${clip%.webm}.gif"
  rm "$clip"
done

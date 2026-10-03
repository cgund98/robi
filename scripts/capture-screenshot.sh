#!/usr/bin/env bash
#
# Capture a README asset for Robi, which is a GUI app (not a terminal, so
# charmbracelet/vhs does not apply the way it does for gopi).
#
#   scratch/capture-screenshot.sh            # one still -> assets/summarize-repo.png
#   scratch/capture-screenshot.sh video 8    # an 8s clip  -> assets/summarize-repo.mp4
#
# A still: run `pnpm tauri dev`, get the app to the state you want, then run this
# and click the Robi window (or press space over it).
#
# A clip: same, but this starts recording for the given number of seconds. On
# macOS 14+ `screencapture -v` records the selected region/window to a .mov; on
# older versions use the QuickTime route noted at the bottom.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT_DIR="$ROOT/assets"
mkdir -p "$OUT_DIR"

mode="${1:-still}"
png="$OUT_DIR/summarize-repo.png"

case "$mode" in
  still)
    echo "Click the Robi window (or hover it and press space) to capture it..."
    # -o drops the window shadow; -w is window-selection mode.
    screencapture -o -w "$png"
    echo "Wrote $png"
    echo
    echo "Trim with any image tool if needed, e.g. sips to resize:"
    echo "  sips --resampleWidth 1200 \"$png\""
    ;;

  video)
    secs="${2:-8}"
    mov="$OUT_DIR/summarize-repo.mov"
    gif="$OUT_DIR/summarize-repo.gif"
    echo "Click the Robi window; recording ${secs}s to $mov ..."
    # -v is interactive video capture; the selection prompt still appears.
    screencapture -v -V "$secs" -w "$mov"
    echo "Wrote $mov"
    if command -v ffmpeg >/dev/null 2>&1; then
      # Palette two-pass gives a clean GIF without banding.
      ffmpeg -y -i "$mov" \
        -vf "fps=12,scale=1200:-1:flags=lanczos,split[s0][s1];[s0]palettegen[p];[s1][p]paletteuse" \
        "$gif"
      echo "Wrote $gif"
    else
      echo "ffmpeg not found; leaving the .mov. Install it with 'brew install ffmpeg'."
    fi
    ;;

  *)
    echo "usage: $0 [still|video [seconds]]" >&2
    exit 2
    ;;
esac

cat <<'NOTE'

Alternatives for a GUI capture on macOS:
  - QuickTime Player: File > New Screen Recording, select the Robi window, then
    File > Export as GIF (or export .mov and run the ffmpeg command above).
  - CleanShot X / Kap: window capture with a rounded frame and a published link.
  - Keep the asset small: 1200px wide, under ~2 MB, so the README stays fast.
NOTE

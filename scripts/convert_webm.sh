#!/bin/bash
# Convert WebM (with transparency) to PNG frame sequence
# Usage: ./convert_webm.sh input.webm [fps] [output_dir]

set -e

if [ -z "$1" ]; then
    echo "Usage: $0 <input.webm> [fps] [output_dir]"
    echo ""
    echo "Arguments:"
    echo "  input.webm   - Input WebM file with alpha channel"
    echo "  fps          - Frame rate (default: 24)"
    echo "  output_dir   - Output directory (default: derived from input filename)"
    echo ""
    echo "Examples:"
    echo "  $0 talking.webm              # 24fps, outputs to assets/talking/"
    echo "  $0 idle.webm 30              # 30fps, outputs to assets/idle/"
    echo "  $0 video.webm 24 my_frames   # 24fps, outputs to assets/my_frames/"
    exit 1
fi

INPUT="$1"
FPS="${2:-24}"
BASENAME=$(basename "$INPUT" .webm)
BASENAME=$(basename "$BASENAME" .mp4)  # Also handle .mp4
OUTPUT_DIR="${3:-assets/$BASENAME}"

# Check if input file exists
if [ ! -f "$INPUT" ]; then
    echo "Error: Input file '$INPUT' not found"
    exit 1
fi

# Check if ffmpeg is installed
if ! command -v ffmpeg &> /dev/null; then
    echo "Error: ffmpeg is not installed"
    echo "Install with: brew install ffmpeg"
    exit 1
fi

# Create output directory
mkdir -p "$OUTPUT_DIR"

echo "Converting: $INPUT"
echo "FPS: $FPS"
echo "Output: $OUTPUT_DIR/"
echo ""

# Extract frames with transparency preserved
# -c:v libvpx-vp9 decodes VP9 with alpha
# -pix_fmt rgba ensures alpha channel is preserved
ffmpeg -i "$INPUT" \
    -vf "fps=$FPS" \
    -pix_fmt rgba \
    "$OUTPUT_DIR/frame_%04d.png" \
    -y

# Count extracted frames
FRAME_COUNT=$(ls -1 "$OUTPUT_DIR"/frame_*.png 2>/dev/null | wc -l | tr -d ' ')

echo ""
echo "Done! Extracted $FRAME_COUNT frames to $OUTPUT_DIR/"
echo ""
echo "Add to ui.toml:"
echo ""
echo "[skin]"
echo "path = \"$OUTPUT_DIR\""
echo "fps = $FPS"

#!/bin/bash
# Convert video to PNG frame sequence with optional chromakey
# Usage: ./convert_webm.sh input.mp4 [fps] [output_dir] [max_size] [green_color]

set -e

if [ -z "$1" ]; then
    echo "Usage: $0 <input> [fps] [output_dir] [max_size] [green_color]"
    echo ""
    echo "Arguments:"
    echo "  input        - Input video file (WebM with alpha, or MP4 with green screen)"
    echo "  fps          - Frame rate (default: 24)"
    echo "  output_dir   - Output directory (default: derived from input filename)"
    echo "  max_size     - Max width/height in pixels (default: 1024, GPU limit is 2048)"
    echo "  green_color  - Hex color for chromakey removal (e.g., 00ff00 for pure green)"
    echo "                 Leave empty for WebM with existing alpha channel"
    echo ""
    echo "Examples:"
    echo "  $0 talking.webm                         # WebM with alpha, 24fps"
    echo "  $0 video.mp4 24 out 1024 00ff00         # MP4 with green screen removal"
    echo "  $0 video.mp4 24 out 512 00ff00          # Smaller frames (512px max)"
    exit 1
fi

INPUT="$1"
FPS="${2:-24}"
BASENAME=$(basename "$INPUT" .webm)
BASENAME=$(basename "$BASENAME" .mp4)
OUTPUT_DIR="${3:-assets/$BASENAME}"
MAX_SIZE="${4:-1024}"
GREEN_COLOR="${5:-}"

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
echo "Max size: ${MAX_SIZE}px"
echo "Output: $OUTPUT_DIR/"

if [ -n "$GREEN_COLOR" ]; then
    echo "Chromakey: #$GREEN_COLOR"
    # Build filter with chromakey removal
    # Using conservative values: similarity=0.1, blend=0.1 for clean edges
    FILTER="fps=$FPS,chromakey=0x${GREEN_COLOR}:0.1:0.1,scale=w='min($MAX_SIZE,iw)':h='min($MAX_SIZE,ih)':force_original_aspect_ratio=decrease"

    echo ""
    ffmpeg -i "$INPUT" \
        -vf "$FILTER" \
        -pix_fmt rgba \
        "$OUTPUT_DIR/frame_%04d.png" \
        -y
else
    echo "Mode: Alpha passthrough (WebM)"
    echo ""
    # Extract frames with transparency preserved
    # -c:v libvpx-vp9 decodes VP9 with alpha
    ffmpeg -c:v libvpx-vp9 -i "$INPUT" \
        -vf "fps=$FPS,scale=w='min($MAX_SIZE,iw)':h='min($MAX_SIZE,ih)':force_original_aspect_ratio=decrease" \
        -pix_fmt rgba \
        "$OUTPUT_DIR/frame_%04d.png" \
        -y
fi

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

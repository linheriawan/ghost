#!/bin/bash
# Convert video to PNG frame sequence with optional chromakey
# Usage: ./mp4_fs.sh input.mp4 [fps] [output_dir] [max_size]

set -e

INPUT="$1"
FPS="${2:-24}"
OUTPUT_DIR="${3:-assets/$BASENAME}"
MAX_SIZE="${4:-512}"
GREEN_COLOR="${5:-'00FF00'}"

mkdir -p "$OUTPUT_DIR"

echo "Converting: $INPUT"
echo "FPS: $FPS"
echo "Max size: ${MAX_SIZE}px"
echo "Output: $OUTPUT_DIR/"

ffmpeg -i "$INPUT" \
    -vf "fps=$FPS, chromakey=0x$GREEN_COLOR:0.1:0.2, despill=type=green, format=rgba, scale=w='min($MAX_SIZE,iw)':h='min($MAX_SIZE,ih)':force_original_aspect_ratio=decrease " \
    -pix_fmt rgba \
    "$OUTPUT_DIR/frame_%04d.png" \
    -y

cargo run -- --path $OUTPUT_DIR
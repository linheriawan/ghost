# FFMPEG
```bash
# transparent png -> png
ffmpeg -i ../ghost/assets/xiao-Mei_0.png \
  -vf "chromakey=0x00FF00:0.1:0.3, despill=type=green, lutrgb=g='val*0.9',format=rgba" \
  -pix_fmt rgba \
  ../ghost/assets/xiao-Mei1.png

# transparent mp4 -> webm
ffmpeg -i ../ghost/assets/xiaoMei_talk0.mp4 -an \
  -vf "chromakey=0x274535:0.15:0.3, chromakey=0x00ff00:0.15:0.3, chromakey=0x00ee00:0.15:0.3" \        
  -c:v libvpx-vp9 \
  -pix_fmt yuva420p \
  -auto-alt-ref 0 \
  -q:v 30 \
  ../ghost/assets/xiaoMei_talk0.webm

# to frame sequence webm -> frame sequence
./convert_webm.sh ../../ghost/assets/rin_talk.webm 24 ../../ghost/assets/persona/rin/talk

# mp4 -> frame sequence
ffmpeg -i ../ghost/assets/sasha_idle.mp4 \                         
-vf "fps=24,chromakey=0x00FF00:0.1:0.2, despill=type=green, format=rgba,scale=w='min(512,iw)':h='min(512,ih)':force_original_aspect_ratio=decrease " \
-pix_fmt rgba \
../ghost/assets/persona/sasha/idle/frame_%04d.png

# remove transparent pixel
cargo run -- --path ../ghost/assets/persona/minAh/idle/
```

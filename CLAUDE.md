# GHOST APP
## main-app

## ghost-ui

## drawing callout (ghost-callout) that depends on ghost-ui:

add different type of callout bubble(scream,talk,think), set the position of the arrow and the callout, how long it displayed, render the text like the way lyrics does/or like a streaming                 
1. Separation of concerns: ghost-ui = window management, ghost-callout = UI elements
2. Optional dependency: Users who just want shaped windows don't need text rendering overhead
3. Complexity: Callouts have many features (text animation, shapes, timing)
4. Reusability: Could be used with other window systems later  

### Architecture

```bash
ghost/                 
├── ghost-ui/          # Window, rendering, skin
├── ghost-callout/     # Callout bubbles, text animation
│   └── depends on ghost-ui
└── src/main.rs        # Uses both
```

### ghost-callout API Design
```rust
use ghost_callout::{Callout, CalloutType, ArrowPosition, TextAnimation};

// Callout types         
pub enum CalloutType {   
    Talk,   // ╭─────╮  
            // │     │  
            // ╰──╲──╯  <- rounded with tail       

    Think,   // ○ ○ ○    <- cloud with bubbles      
            // (  °°°  )

    Scream,  // /\/\/\/\  <- jagged/spiky edges     
            // \  !!!  /
} 

// Arrow/tail position   
pub enum ArrowPosition { 
    Bottom(f32),  // 0.0 = left, 0.5 = center, 1.0 = right                     
    Top(f32),            
    Left(f32),           
    Right(f32),          
} 

// Text animation styles 
pub enum TextAnimation { 
    Instant,                    // Show all at once 
    Typewriter { cps: f32 },    // Characters per second
    WordByWord { wps: f32 },    // Words per second (like lyrics)              
    Stream { cps: f32 },        // Like AI streaming response                  
} 

// Configuration         
let callout = Callout::new() 
    .with_type(CalloutType::Talk)                   
    .with_position(100.0, -50.0)  // Relative to ghost window                  
    .with_arrow(ArrowPosition::Bottom(0.3))         
    .with_max_width(200.0) 
    .with_text_animation(TextAnimation::Typewriter { cps: 30.0 })              
    .with_duration(Duration::from_secs(5))  // Auto-hide after 5s              
    .with_style(CalloutStyle {                      
        background: [1.0, 1.0, 1.0, 0.95],          
        text_color: [0.0, 0.0, 0.0, 1.0],           
        font_size: 16.0, 
        padding: 12.0,   
        border_radius: 8.0,
    });

// Usage in app
callout.say("Hello! I'm your assistant.");           
callout.think("Hmm, let me consider...");
callout.scream("WATCH OUT!");

// Update in event loop (for animations)               
callout.update(delta_time);
```

### Implementation Plan
┌──────┬─────────────────────────────────────────────────────────┐
│ Step │Description                                              │  
├──────┼─────────────────────────────────────────────────────────┤
│ 1    │ Add text rendering to ghost-ui (glyphon)                │  
├──────┼─────────────────────────────────────────────────────────┤
│ 2    │ Create ghost-callout crate structure                    │
├──────┼─────────────────────────────────────────────────────────┤
│ 3    │ Implement shape rendering (rounded rect, clouds, spiky) │
├──────┼─────────────────────────────────────────────────────────┤
│ 4    │ Implement arrow/tail rendering                          │
├──────┼─────────────────────────────────────────────────────────┤
│ 5    │ Implement text animation system                         │
├──────┼─────────────────────────────────────────────────────────┤
│ 6    │ Add timing/duration control                             │
└──────┴─────────────────────────────────────────────────────────┘

Dependencies
# ghost-callout/Cargo.toml
[dependencies]        
ghost-ui = { path = "../ghost-ui" } 
glyphon = "0.5"          # Text rendering
cosmic-text = "0.11"     # Text layout


#### FFMPEG
```bash
# transparent png

ffmpeg -i xiao-Mei_0.png \
  -vf "chromakey=0x00FF00:0.1:0.3, despill=type=green, lutrgb=g='val*0.9',format=rgba" \
  -pix_fmt rgba \
  xiao-Mei1.png

# transparent webm
ffmpeg -i xiaoMei_talk0.mp4 -an \
  -vf "chromakey=0x274535:0.15:0.3, chromakey=0x00ff00:0.15:0.3, chromakey=0x00ee00:0.15:0.3" \        
  -c:v libvpx-vp9 \
  -pix_fmt yuva420p \
  -auto-alt-ref 0 \
  -q:v 30 \
  xiaoMei_talk0.webm

# to frame sequence
  ./convert_webm.sh ../assets/rin_talk.webm 24 ../assets/rin/talk
```

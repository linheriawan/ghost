# GHOST APP
## main-app
┌─────────────────────────────┐
│ base layer                  │
│    ┌───────────┐   ┌───┐    │
│    │   Skin    │   | b |    │
│    │           │   └───┘    │
│    │    ┌───────────┐       │
│    └────│  Layer ┌───┐      │
|         |        | a |      |
|         └────────└───┘      |
└─────────────────────────────┘

- Base Layer will be container for rendering the UI on a transparent window. 
- skin is behind-most layer of stil image or a frame sequence animation.
- layer will be stacked on top of other layer
- a and b is form element, it is front-most element
- all components(skin,layers,elements) is opaque, not individually transparent.
- on window lost focus transparency should be applied to components as a whole (layer cant see-through skin)

## chat-window
┌────────────────────────────┐┌────────────────────────────┐
│                            ││ [][][] Chat-window Title   │
│                            │├────────────────────────────┘
│                            ││┌─────────────────────────┐ │
│                            │││ chat bubble area        │ │
│       main window          │││                         │ │
│  (the transparent window)  ││└─────────────────────────┘ │
│                            ││┌─────────────┐ ┌────────┐  │
│                            │││ input text  │ │ button │  │
│                            ││└─────────────┘ └────────┘  │
└────────────────────────────┘└────────────────────────────┘

## windows

### main-window
style: transparent no title-bar,draggabe,resize-not-allowed
behaviour: always-on-top, transparent on lost focus

### chat-window
style: opaque with title-bar,draggabe,resize
behaviour: glued to/follow main-window edge, bring-to-front on main-window receive focus

### setting-window
style: opaque with title-bar,draggabe,resize
behaviour: follow default OS behaviour for window

## ghost-ui

Reusable crate for transparent windows, rendering, skin management, and UI elements.

### Architecture

```bash
ghost/
├── ghost-ui/              # Window, rendering, skin, elements
│   └── src/
│       ├── window/        # Window creation & management
│       │   ├── mod.rs     # GhostWindow, event loops, traits
│       │   ├── config.rs  # WindowConfig, CalloutWindowConfig
│       │   └── platform.rs # Platform-specific (macOS, Windows, Linux)
│       ├── elements/      # UI components
│       │   ├── mod.rs     # Widget trait, Origin, coordinates
│       │   ├── button.rs  # Button element
│       │   └── callout/   # Callout bubbles (merged from ghost-callout)
│       │       ├── mod.rs     # Callout builder, state machine
│       │       ├── types.rs   # CalloutType, ArrowPosition, TextAnimation
│       │       ├── shape.rs   # Shape rendering (talk, think, scream)
│       │       └── text.rs    # Text animation (typewriter, word-by-word)
│       ├── renderer/      # GPU rendering pipelines
│       ├── skin.rs        # Static skin
│       ├── animated_skin.rs # Frame sequence animation
│       ├── layer.rs       # Layer system
│       └── icon.rs        # Tray/dock icons
├── src/                   # Main application
│   ├── main.rs            # Entry point, event loop setup
│   ├── config.rs          # App configuration (ui.toml)
│   ├── actions.rs         # Button click handlers
│   ├── tray.rs            # System tray
│   ├── ui.rs              # UI factory helpers
│   └── windows/           # Window implementations
│       ├── mod.rs
│       ├── main_window.rs     # Main skin window + buttons
│       ├── callout_window.rs  # Callout bubble window
│       └── chat_window.rs     # Chat interface (egui)
└── gassetsmaker/          # Asset processing tool
```

## Callout API (ghost_ui::elements::callout)

Callout bubbles support different types (Talk/Think/Scream), text animation styles, and auto-hide timing.

```rust
use ghost_ui::{Callout, CalloutType, ArrowPosition, TextAnimation};

let callout = Callout::new()
    .with_position(0.0, 0.0)
    .with_max_width(200.0)
    .with_text_animation(TextAnimation::Typewriter { cps: 30.0 })
    .with_duration(Duration::from_secs(5))
    .with_style(CalloutStyle { .. });

callout.say("Hello!");
callout.think("Hmm...");
callout.scream("WATCH OUT!");
callout.update(delta_time);
```



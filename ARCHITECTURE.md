# Ghost Architecture Goals

## Core Principles

1. **Single Responsibility**: Each module has ONE clear job
2. **Standardized Coordinates**: Origin at TOP-LEFT, positions relative to parent
3. **Composability**: Parent/child/sibling relationships
4. **Event Handlers as Properties**: Elements define callback properties, windows implement handlers

---

## Module Structure

```
ghost/
├── ghost-window/                    # Window management + elements + layout
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs
│       │
│       ├── window/                  # Window creation & management
│       │   ├── mod.rs
│       │   ├── config.rs            # WindowConfig, WindowKind
│       │   ├── standard.rs          # Standard window (with decorations)
│       │   ├── transparent.rs       # Transparent/borderless window
│       │   └── platform.rs          # Platform-specific (macOS, Windows)
│       │
│       ├── elements/                # UI components (definitions only)
│       │   ├── mod.rs
│       │   ├── element.rs           # Element trait
│       │   ├── panel.rs
│       │   ├── button.rs
│       │   ├── label.rs
│       │   ├── text_input.rs
│       │   ├── image.rs
│       │   └── dropdown.rs
│       │
│       ├── layout/                  # Positioning system
│       │   ├── mod.rs
│       │   ├── types.rs             # Rect, Point, Size, Edges
│       │   ├── config.rs            # LayoutConfig, SizeMode, Anchor
│       │   ├── node.rs              # LayoutNode (tree structure)
│       │   └── strategies/
│       │       ├── mod.rs
│       │       ├── absolute.rs
│       │       ├── anchored.rs
│       │       ├── stack.rs
│       │       └── flex.rs
│       │
│       ├── skin/                    # Skin with layout support
│       │   ├── mod.rs
│       │   ├── skin.rs              # Static skin (single image)
│       │   ├── animated.rs          # Animated skin (frame sequence)
│       │   └── layout.rs            # Skin positioning within window
│       │
│       └── renderer/                # GPU rendering
│           ├── mod.rs
│           ├── context.rs           # RenderContext
│           ├── sprite.rs            # Sprite/image rendering
│           ├── shape.rs             # Shapes (rect, rounded rect)
│           └── text.rs              # Text rendering
│
├── ghost-callout/                   # Keep as-is (callout bubbles)
│
└── src/                             # Main application
    ├── main.rs                      # Entry point, event loop only
    ├── config.rs                    # App configuration (ui.toml)
    │
    └── windows/                     # Window implementations (event handlers here)
        ├── mod.rs
        ├── main_window.rs           # Main skin window + buttons
        ├── chat_window.rs           # Chat interface
        └── callout_window.rs        # Callout bubble window
```

---

## Module 1: ghost-window

### 1.1 Window Submodule

**Responsibility**: Create and manage OS windows

```rust
// ghost-window/src/window/config.rs
pub struct WindowConfig {
    pub kind: WindowKind,
    pub size: Size,
    pub position: Option<Position>,
    pub title: String,
    pub resizable: bool,
    pub draggable: bool,
    pub always_on_top: bool,
    pub decorations: bool,
    pub transparent: bool,
    pub click_through: bool,
    pub alpha_hit_test: bool,
}

pub enum WindowKind {
    Standard,      // With title bar and borders
    Transparent,   // Borderless, transparent background
}
```

### 1.2 Elements Submodule

**Responsibility**: Define UI components with **properties only** (no business logic)

Event handlers are **properties** (callbacks), not implementations:

```rust
// ghost-window/src/elements/button.rs
pub struct Button {
    pub id: ElementId,
    pub label: String,
    pub style: ButtonStyle,
    pub size: Size,

    // Event handler PROPERTIES (just callbacks, not implementations)
    pub on_click: Option<Box<dyn Fn() + Send>>,
    pub on_hover: Option<Box<dyn Fn(bool) + Send>>,

    // Internal state
    state: ButtonState,
}

impl Button {
    pub fn new(label: impl Into<String>) -> Self { ... }

    // Builder pattern for setting properties
    pub fn on_click(mut self, handler: impl Fn() + Send + 'static) -> Self {
        self.on_click = Some(Box::new(handler));
        self
    }
}
```

```rust
// ghost-window/src/elements/element.rs
pub trait Element {
    fn id(&self) -> ElementId;
    fn intrinsic_size(&self) -> Size;
    fn min_size(&self) -> Size { Size::ZERO }
    fn max_size(&self) -> Size { Size::MAX }

    // State update (animation, etc.)
    fn update(&mut self, delta: f32) -> bool { false }

    // Event dispatch - calls the property callbacks
    fn dispatch_event(&mut self, event: &ElementEvent) -> bool;

    // Rendering
    fn prepare(&mut self, ctx: &mut RenderContext, bounds: Rect);
    fn render<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>);
}
```

### 1.3 Layout Submodule

**Responsibility**: Position and size components

```rust
// ghost-window/src/layout/types.rs
pub struct Point { pub x: f32, pub y: f32 }
pub struct Size { pub width: f32, pub height: f32 }
pub struct Rect { pub origin: Point, pub size: Size }
pub struct Edges { pub left: f32, pub right: f32, pub top: f32, pub bottom: f32 }

// ghost-window/src/layout/config.rs
pub enum SizeMode {
    Fixed(f32),
    MatchParent,
    WrapContent,
    Percent(f32),
    Flex(f32),
}

pub enum Anchor {
    TopLeft, TopCenter, TopRight,
    CenterLeft, Center, CenterRight,
    BottomLeft, BottomCenter, BottomRight,
}

pub struct LayoutConfig {
    pub size: LayoutSize,
    pub position: LayoutPosition,
    pub padding: Edges,
    pub margin: Edges,
}

pub enum LayoutPosition {
    Absolute { x: f32, y: f32 },
    Anchored { anchor: Anchor, offset: Point },
    // For containers
    Stack { direction: Direction, spacing: f32, align: Align },
    Flex { direction: Direction, justify: Justify, align: Align, gap: f32 },
}
```

### 1.4 Skin Submodule

**Responsibility**: Load and render skin images/animations with layout support

```rust
// ghost-window/src/skin/mod.rs
pub struct Skin {
    texture: Texture,
    size: Size,
    layout: SkinLayout,
}

pub struct AnimatedSkin {
    frames: Vec<Texture>,
    current_frame: usize,
    fps: f32,
    elapsed: f32,
    state: AnimationState,
    layout: SkinLayout,
}

// ghost-window/src/skin/layout.rs
pub struct SkinLayout {
    pub anchor: Anchor,           // Where in window to anchor
    pub offset: Point,            // Offset from anchor
    pub scale_mode: ScaleMode,    // How to scale skin
}

pub enum ScaleMode {
    None,                         // Original size
    Fit,                          // Fit within window, maintain aspect
    Fill,                         // Fill window, maintain aspect (may crop)
    Stretch,                      // Stretch to fill (distorts)
}
```

---

## Module 2: windows/ (in main app)

**Responsibility**: Implement event handlers, business logic, window composition

This is where the actual behavior is defined:

```rust
// src/windows/main_window.rs
pub struct MainWindow {
    window: Window,
    skin: AnimatedSkin,
    ui: LayoutNode,
    callout_sender: Sender<CalloutCommand>,
}

impl MainWindow {
    pub fn new(config: &Config, event_loop: &EventLoop) -> Self {
        // Create transparent window
        let window = Window::new(event_loop, WindowConfig {
            kind: WindowKind::Transparent,
            draggable: true,
            always_on_top: true,
            ..Default::default()
        });

        // Load skin with layout
        let skin = AnimatedSkin::load(&config.skin.path)
            .with_layout(SkinLayout {
                anchor: Anchor::Center,
                offset: Point::ZERO,
                scale_mode: ScaleMode::None,
            });

        // Build UI with EVENT HANDLERS IMPLEMENTED HERE
        let ui = LayoutNode::new(Panel::new())
            .add_child(
                Button::new("Greet")
                    .on_click(|| {
                        // ACTUAL HANDLER IMPLEMENTATION HERE
                        self.callout_sender.send(CalloutCommand::Say("Hello!"));
                    }),
                LayoutConfig::anchored(Anchor::BottomLeft, Point::new(10.0, 10.0)),
            );

        Self { window, skin, ui, callout_sender }
    }

    pub fn update(&mut self, delta: f32) {
        self.skin.update(delta);
        self.ui.update(delta);
    }

    pub fn on_window_event(&mut self, event: WindowEvent) {
        // Convert window event to element event and dispatch
        match event {
            WindowEvent::CursorMoved { position, .. } => {
                let pos = Point::new(position.x as f32, position.y as f32);
                self.ui.dispatch_event(&ElementEvent::MouseMove(pos));
            }
            WindowEvent::MouseInput { state, button, .. } => {
                // ... dispatch to ui
            }
            // ...
        }
    }

    pub fn render(&mut self, ctx: &mut RenderContext) {
        // 1. Render skin (background)
        self.skin.render(ctx);

        // 2. Render UI elements on top
        self.ui.render(ctx);
    }
}
```

```rust
// src/windows/chat_window.rs
pub struct ChatWindow {
    window: Window,
    ui: LayoutNode,
    messages: Vec<Message>,
    input_text: String,
}

impl ChatWindow {
    pub fn new(config: &Config, event_loop: &EventLoop) -> Self {
        let window = Window::new(event_loop, WindowConfig {
            kind: WindowKind::Standard,
            decorations: true,
            resizable: true,
            size: Size::new(400.0, 500.0),
            ..Default::default()
        });

        // Build chat UI
        let ui = LayoutNode::new(
            Panel::new().background(Color::WHITE),
            LayoutConfig {
                size: LayoutSize::match_parent(),
                position: LayoutPosition::Flex {
                    direction: Direction::Vertical,
                    justify: Justify::SpaceBetween,
                    align: Align::Stretch,
                    gap: 0.0,
                },
                ..Default::default()
            },
        )
        .add_child(
            // Message list (scrollable)
            ScrollView::new(),
            LayoutConfig {
                size: LayoutSize { width: SizeMode::MatchParent, height: SizeMode::Flex(1.0) },
                ..Default::default()
            },
        )
        .add_child(
            // Input at bottom
            TextInput::new()
                .placeholder("Type a message...")
                .on_change(|text| {
                    // HANDLER IMPLEMENTED HERE
                    self.input_text = text;
                })
                .on_submit(|| {
                    // HANDLER IMPLEMENTED HERE
                    self.send_message();
                }),
            LayoutConfig {
                size: LayoutSize { width: SizeMode::MatchParent, height: SizeMode::Fixed(40.0) },
                ..Default::default()
            },
        );

        Self { window, ui, messages: vec![], input_text: String::new() }
    }
}
```

---

## Module 3: main.rs

**Responsibility**: Event loop only - dispatch events and render

```rust
// src/main.rs
fn main() {
    env_logger::init();

    let config = Config::load("ui.toml");
    let event_loop = EventLoop::new();

    // Create windows
    let mut main_window = MainWindow::new(&config, &event_loop);
    let mut chat_window = ChatWindow::new(&config, &event_loop);
    let mut callout_window = CalloutWindow::new(&config, &event_loop);

    let mut last_frame = Instant::now();

    // Event loop - just dispatch and render
    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Poll;  // Continuous for animation

        match event {
            Event::MainEventsCleared => {
                let now = Instant::now();
                let delta = now.duration_since(last_frame).as_secs_f32();
                last_frame = now;

                // Update all windows
                main_window.update(delta);
                chat_window.update(delta);
                callout_window.update(delta);

                // Request redraws
                main_window.request_redraw();
                chat_window.request_redraw();
                callout_window.request_redraw();
            }

            Event::RedrawRequested(id) => {
                if id == main_window.window_id() {
                    main_window.render();
                } else if id == chat_window.window_id() {
                    chat_window.render();
                } else if id == callout_window.window_id() {
                    callout_window.render();
                }
            }

            Event::WindowEvent { window_id, event } => {
                if window_id == main_window.window_id() {
                    main_window.on_window_event(event);
                } else if window_id == chat_window.window_id() {
                    chat_window.on_window_event(event);
                } else if window_id == callout_window.window_id() {
                    callout_window.on_window_event(event);
                }
            }

            _ => {}
        }
    });
}
```

---

## Coordinate System Standard

```
ALL COORDINATES: Origin at TOP-LEFT

Window Coordinates:
┌─────────────────────────────┐
│(0,0)                   (w,0)│
│                             │
│         Window              │
│                             │
│(0,h)                   (w,h)│
└─────────────────────────────┘

Element Coordinates (relative to parent):
┌─────────────────────────────┐
│ Parent                      │
│  ┌─────────┐                │
│  │(0,0)    │ Child at       │
│  │  Child  │ parent-relative│
│  └─────────┘ position       │
└─────────────────────────────┘

Skin Coordinates (within window):
┌─────────────────────────────┐
│ Window                      │
│    ┌───────────┐            │
│    │   Skin    │            │
│    │ positioned│            │
│    │ by layout │            │
│    └───────────┘            │
└─────────────────────────────┘
```

---

## Summary

| Location | Responsibility |
|----------|---------------|
| `ghost-window/window/` | Create windows, set properties |
| `ghost-window/elements/` | Define components, callback **properties** |
| `ghost-window/layout/` | Position components, size modes |
| `ghost-window/skin/` | Load images/animation, skin layout |
| `ghost-window/renderer/` | GPU rendering |
| `src/windows/` | **Implement** event handlers, business logic |
| `src/main.rs` | Event loop only |

**Key Distinction**:
- `ghost-window/elements/` defines `on_click: Option<Box<dyn Fn()>>` as a **property**
- `src/windows/` sets that property with **actual implementation**: `.on_click(|| self.do_something())`

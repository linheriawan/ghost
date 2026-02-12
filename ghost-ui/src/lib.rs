//! # ghost-ui
//!
//! A reusable crate for creating shaped transparent windows with wgpu.
//!
//! ## Features
//! - Transparent, borderless windows
//! - PNG skin support for custom window shapes
//! - Cross-platform (macOS, Windows, Linux)
//! - Always-on-top and click-through options
//! - Draggable windows
//! - Alpha-based hit testing (clicks on transparent areas pass through)
//! - Focus-based opacity (opaque when focused, transparent when not)
//! - System tray and dock icon helpers
//!
//! ## Example
//!
//! ```no_run
//! use ghost_ui::{EventLoop, GhostWindowBuilder, icon, skin};
//!
//! fn main() {
//!     let event_loop = EventLoop::new();
//!
//!     // Load skin data (can be done before window creation)
//!     let skin_data = skin("assets/character.png").unwrap();
//!
//!     // Set up icons
//!     let mut app_icon = icon("assets/icon.png").unwrap();
//!     app_icon.setup_all().expect("Failed to setup icons");
//!
//!     // Create window
//!     let window = GhostWindowBuilder::new()
//!         .with_size(skin_data.width(), skin_data.height())
//!         .with_always_on_top(true)
//!         .with_draggable(true)
//!         .with_skin_data(&skin_data)
//!         .with_opacity_focused(1.0)    // Opaque when focused
//!         .with_opacity_unfocused(0.5)  // Semi-transparent when unfocused
//!         .build(&event_loop)
//!         .expect("Failed to create window");
//!
//!     ghost_ui::run(window, event_loop);
//! }
//! ```

pub mod animated_skin;
pub mod icon;
pub mod layer;
mod renderer;
mod skin;
pub mod elements;
mod window;

// Icon helpers
pub use icon::{icon, icon_bytes, AppIcon, IconError};

// Skin helpers
pub use skin::{skin, skin_bytes, Skin, SkinData, SkinError};

// Animated skin
pub use animated_skin::{AnimatedSkin, Animation, AnimationState, PersonaMeta, PlayMode};

// Renderer
pub use renderer::{ButtonRenderer, Renderer, RendererError, SpritePipeline};

// Layer system
pub use layer::{Layer, LayerAnchor, LayerConfig, LayerRenderer, TextAlign, TextVAlign};

// Window
pub use window::{
    run, run_with_app, run_with_app_and_callout, run_with_app_callout_and_extra,
    CalloutApp, CalloutWindowConfig, ExtraWindow,
    GhostApp, GhostEvent, GhostWindow, GhostWindowBuilder, GpuResources, WindowConfig, WindowError,
};

// Elements system
pub use elements::{Button, ButtonId, ButtonState, ButtonStyle, Origin, Widget};

// Callout (merged from ghost-callout)
pub use elements::callout::{Callout, CalloutBuilder, CalloutShapeRenderer, CalloutTextAnimator, CalloutTextRenderer};
pub use elements::callout::shape::CalloutShape;
pub use elements::callout::text::{TextAnimator, TextRenderer as CalloutTextRendererFull};
pub use elements::callout::types::{ArrowPosition, CalloutStyle, CalloutType, TextAnimation, CalloutTiming};

// Re-export commonly used types
pub use tao::event_loop::EventLoop;
pub use tray_icon::menu::Menu as TrayMenu;

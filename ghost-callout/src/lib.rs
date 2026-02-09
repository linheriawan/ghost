//! # ghost-callout
//!
//! A crate for rendering callout bubbles with text animation for ghost-ui.
//!
//! ## Features
//! - Different callout types (Talk, Think, Scream)
//! - Configurable arrow/tail position
//! - Text animation (typewriter, word-by-word, streaming)
//! - Timing and duration control
//!
//! ## Example
//!
//! ```no_run
//! use ghost_callout::{Callout, CalloutType, ArrowPosition, TextAnimation};
//! use std::time::Duration;
//!
//! let callout = Callout::new()
//!     .with_type(CalloutType::Talk)
//!     .with_position(100.0, -50.0)
//!     .with_arrow(ArrowPosition::Bottom(0.3))
//!     .with_max_width(200.0)
//!     .with_text_animation(TextAnimation::Typewriter { cps: 30.0 })
//!     .with_duration(Duration::from_secs(5));
//! ```

mod callout;
mod shape;
mod text;
mod types;

pub use callout::{Callout, CalloutBuilder};
pub use shape::{CalloutShape, ShapeRenderer};
pub use text::{TextAnimator, TextRenderer};
pub use types::{ArrowPosition, CalloutStyle, CalloutType, TextAnimation};

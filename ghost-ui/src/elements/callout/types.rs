//! Core types for callout configuration

use std::time::Duration;

/// Type of callout bubble shape
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CalloutType {
    /// Standard speech bubble with rounded corners and a tail
    /// ```text
    /// ╭─────────╮
    /// │  Hello  │
    /// ╰──╲──────╯
    /// ```
    Talk,

    /// Thought bubble with cloud-like edges and bubble trail
    /// ```text
    ///   ○ ○ ○
    /// (  ...  )
    /// ```
    Think,

    /// Exclamation bubble with jagged/spiky edges
    /// ```text
    /// /\/\/\/\/\
    /// \  !!!   /
    /// \/\/\/\/\/
    /// ```
    Scream,
}

impl Default for CalloutType {
    fn default() -> Self {
        Self::Talk
    }
}

/// Position of the arrow/tail on the callout bubble
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ArrowPosition {
    /// Arrow at bottom edge, f32 is position from left (0.0) to right (1.0)
    Bottom(f32),
    /// Arrow at top edge, f32 is position from left (0.0) to right (1.0)
    Top(f32),
    /// Arrow at left edge, f32 is position from top (0.0) to bottom (1.0)
    Left(f32),
    /// Arrow at right edge, f32 is position from top (0.0) to bottom (1.0)
    Right(f32),
    /// No arrow (for floating callouts)
    None,
}

impl Default for ArrowPosition {
    fn default() -> Self {
        Self::Bottom(0.5)
    }
}

impl ArrowPosition {
    /// Get the normalized position value (0.0 to 1.0)
    pub fn position(&self) -> Option<f32> {
        match self {
            Self::Bottom(p) | Self::Top(p) | Self::Left(p) | Self::Right(p) => Some(*p),
            Self::None => None,
        }
    }

    /// Check if this is a horizontal arrow (top or bottom)
    pub fn is_horizontal(&self) -> bool {
        matches!(self, Self::Top(_) | Self::Bottom(_))
    }

    /// Check if this is a vertical arrow (left or right)
    pub fn is_vertical(&self) -> bool {
        matches!(self, Self::Left(_) | Self::Right(_))
    }
}

/// Text animation style
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TextAnimation {
    /// Show all text immediately
    Instant,

    /// Reveal text character by character (like a typewriter)
    Typewriter {
        /// Characters per second
        cps: f32,
    },

    /// Reveal text word by word (like song lyrics)
    WordByWord {
        /// Words per second
        wps: f32,
    },

    /// Stream text like an AI response (with variable pacing)
    Stream {
        /// Base characters per second
        cps: f32,
    },
}

impl Default for TextAnimation {
    fn default() -> Self {
        Self::Instant
    }
}

impl TextAnimation {
    /// Create a typewriter animation with the given characters per second
    pub fn typewriter(cps: f32) -> Self {
        Self::Typewriter { cps }
    }

    /// Create a word-by-word animation with the given words per second
    pub fn word_by_word(wps: f32) -> Self {
        Self::WordByWord { wps }
    }

    /// Create a streaming animation with the given characters per second
    pub fn stream(cps: f32) -> Self {
        Self::Stream { cps }
    }
}

/// Visual style for the callout
#[derive(Debug, Clone, PartialEq)]
pub struct CalloutStyle {
    /// Background color [r, g, b, a] in 0.0-1.0 range
    pub background: [f32; 4],
    /// Text color [r, g, b, a] in 0.0-1.0 range
    pub text_color: [f32; 4],
    /// Border color [r, g, b, a] in 0.0-1.0 range
    pub border_color: [f32; 4],
    /// Border width in pixels
    pub border_width: f32,
    /// Font size in pixels
    pub font_size: f32,
    /// Padding inside the callout in pixels
    pub padding: f32,
    /// Corner radius for rounded rectangles
    pub border_radius: f32,
    /// Shadow blur radius (0 for no shadow)
    pub shadow_blur: f32,
    /// Shadow offset [x, y]
    pub shadow_offset: [f32; 2],
    /// Shadow color [r, g, b, a]
    pub shadow_color: [f32; 4],
}

impl Default for CalloutStyle {
    fn default() -> Self {
        Self {
            background: [1.0, 1.0, 1.0, 0.95],
            text_color: [0.0, 0.0, 0.0, 1.0],
            border_color: [0.0, 0.0, 0.0, 0.2],
            border_width: 1.0,
            font_size: 18.0, // Standard readable size for callouts
            padding: 14.0,
            border_radius: 10.0,
            shadow_blur: 4.0,
            shadow_offset: [2.0, 2.0],
            shadow_color: [0.0, 0.0, 0.0, 0.2],
        }
    }
}

impl CalloutStyle {
    /// Create a dark theme style
    pub fn dark() -> Self {
        Self {
            background: [0.1, 0.1, 0.1, 0.95],
            text_color: [1.0, 1.0, 1.0, 1.0],
            border_color: [1.0, 1.0, 1.0, 0.2],
            ..Default::default()
        }
    }

    /// Create a style with custom colors
    pub fn with_colors(background: [f32; 4], text: [f32; 4]) -> Self {
        Self {
            background,
            text_color: text,
            ..Default::default()
        }
    }
}

/// Configuration for callout timing
#[derive(Debug, Clone, PartialEq)]
pub struct CalloutTiming {
    /// How long the callout stays visible (None = until manually hidden)
    pub duration: Option<Duration>,
    /// Delay before showing the callout
    pub delay: Duration,
    /// Fade in duration
    pub fade_in: Duration,
    /// Fade out duration
    pub fade_out: Duration,
}

impl Default for CalloutTiming {
    fn default() -> Self {
        Self {
            duration: None,
            delay: Duration::ZERO,
            fade_in: Duration::from_millis(150),
            fade_out: Duration::from_millis(150),
        }
    }
}

impl CalloutTiming {
    /// Create timing with a specific duration
    pub fn with_duration(duration: Duration) -> Self {
        Self {
            duration: Some(duration),
            ..Default::default()
        }
    }

    /// Create timing that stays visible until hidden
    pub fn persistent() -> Self {
        Self {
            duration: None,
            ..Default::default()
        }
    }
}

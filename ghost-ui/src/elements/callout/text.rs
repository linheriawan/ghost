//! Text rendering and animation for callouts

use glyphon::{
    Attrs, Buffer, Color, Family, FontSystem, Metrics, Resolution, Shaping,
    SwashCache, TextArea, TextAtlas, TextBounds, TextRenderer as GlyphonTextRenderer,
};
use wgpu::{Device, MultisampleState, Queue, RenderPass, TextureFormat};

use super::types::{CalloutStyle, TextAnimation};

/// Text animator that handles progressive text reveal
pub struct TextAnimator {
    /// Full text content
    full_text: String,
    /// Current visible character count
    visible_chars: usize,
    /// Animation style
    animation: TextAnimation,
    /// Elapsed time since animation started
    elapsed: f32,
    /// Whether animation is complete
    is_complete: bool,
    /// Word boundaries for word-by-word animation
    word_boundaries: Vec<usize>,
}

impl TextAnimator {
    /// Create a new text animator
    pub fn new(text: impl Into<String>, animation: TextAnimation) -> Self {
        let full_text = text.into();
        let word_boundaries = Self::compute_word_boundaries(&full_text);
        let visible_chars = match animation {
            TextAnimation::Instant => full_text.chars().count(),
            _ => 0,
        };
        let is_complete = matches!(animation, TextAnimation::Instant);

        Self {
            full_text,
            visible_chars,
            animation,
            elapsed: 0.0,
            is_complete,
            word_boundaries,
        }
    }

    /// Compute word boundaries in the text
    fn compute_word_boundaries(text: &str) -> Vec<usize> {
        let mut boundaries = Vec::new();
        let mut in_word = false;
        let mut char_count = 0;

        for c in text.chars() {
            char_count += 1;
            if c.is_whitespace() {
                if in_word {
                    boundaries.push(char_count - 1);
                    in_word = false;
                }
            } else {
                in_word = true;
            }
        }

        // Add final boundary
        if in_word {
            boundaries.push(char_count);
        }

        boundaries
    }

    /// Update the animation with delta time
    pub fn update(&mut self, delta_seconds: f32) {
        if self.is_complete {
            return;
        }

        self.elapsed += delta_seconds;
        let total_chars = self.full_text.chars().count();

        match self.animation {
            TextAnimation::Instant => {
                self.visible_chars = total_chars;
                self.is_complete = true;
            }
            TextAnimation::Typewriter { cps } => {
                self.visible_chars = (self.elapsed * cps) as usize;
                if self.visible_chars >= total_chars {
                    self.visible_chars = total_chars;
                    self.is_complete = true;
                }
            }
            TextAnimation::WordByWord { wps } => {
                // word_count is the number of words to show
                let word_count = (self.elapsed * wps) as usize;
                if word_count == 0 {
                    self.visible_chars = 0;
                } else if word_count > self.word_boundaries.len() {
                    self.visible_chars = total_chars;
                    self.is_complete = true;
                } else {
                    // word_count is 1-indexed (1 = show first word)
                    self.visible_chars = self.word_boundaries[word_count - 1];
                }
            }
            TextAnimation::Stream { cps } => {
                // Variable speed streaming (faster on spaces, slower on punctuation)
                let mut actual_chars = 0;
                let mut time_consumed = 0.0;

                for (i, c) in self.full_text.chars().enumerate() {
                    let char_duration = if c.is_whitespace() {
                        0.5 / cps // Faster for spaces
                    } else if c == '.' || c == ',' || c == '!' || c == '?' {
                        2.0 / cps // Slower for punctuation
                    } else {
                        1.0 / cps
                    };

                    time_consumed += char_duration;
                    if time_consumed <= self.elapsed {
                        actual_chars = i + 1;
                    } else {
                        break;
                    }
                }

                self.visible_chars = actual_chars.min(total_chars);
                if self.visible_chars >= total_chars {
                    self.is_complete = true;
                }
            }
        }
    }

    /// Get the currently visible text
    pub fn visible_text(&self) -> &str {
        let mut end = 0;
        for (i, (idx, _)) in self.full_text.char_indices().enumerate() {
            if i >= self.visible_chars {
                break;
            }
            end = idx + self.full_text[idx..].chars().next().map(|c| c.len_utf8()).unwrap_or(0);
        }
        &self.full_text[..end]
    }

    /// Get the full text
    pub fn full_text(&self) -> &str {
        &self.full_text
    }

    /// Check if animation is complete
    pub fn is_complete(&self) -> bool {
        self.is_complete
    }

    /// Reset the animation
    pub fn reset(&mut self) {
        self.elapsed = 0.0;
        self.visible_chars = match self.animation {
            TextAnimation::Instant => self.full_text.chars().count(),
            _ => 0,
        };
        self.is_complete = matches!(self.animation, TextAnimation::Instant);
    }

    /// Set new text and reset animation
    pub fn set_text(&mut self, text: impl Into<String>) {
        self.full_text = text.into();
        self.word_boundaries = Self::compute_word_boundaries(&self.full_text);
        self.reset();
    }

    /// Skip to end of animation
    pub fn skip(&mut self) {
        self.visible_chars = self.full_text.chars().count();
        self.is_complete = true;
    }

    /// Get animation progress (0.0 to 1.0)
    pub fn progress(&self) -> f32 {
        let total = self.full_text.chars().count();
        if total == 0 {
            1.0
        } else {
            self.visible_chars as f32 / total as f32
        }
    }
}

/// Text renderer using glyphon
pub struct TextRenderer {
    font_system: FontSystem,
    swash_cache: SwashCache,
    atlas: TextAtlas,
    renderer: GlyphonTextRenderer,
    buffer: Buffer,
    line_height: f32,
}

impl TextRenderer {
    /// Create a new text renderer
    pub fn new(device: &Device, queue: &Queue, format: TextureFormat) -> Self {
        let mut font_system = FontSystem::new();
        let swash_cache = SwashCache::new();
        let mut atlas = TextAtlas::new(device, queue, format);
        let renderer = GlyphonTextRenderer::new(&mut atlas, device, MultisampleState::default(), None);
        let mut buffer = Buffer::new(&mut font_system, Metrics::new(16.0, 20.0));
        buffer.set_size(&mut font_system, 200.0, 100.0);

        Self {
            font_system,
            swash_cache,
            atlas,
            renderer,
            buffer,
            line_height: 20.0,
        }
    }

    /// Set the text content and style
    pub fn set_text(&mut self, text: &str, style: &CalloutStyle, max_width: f32) {
        self.set_text_scaled(text, style, max_width, 1.0);
    }

    /// Set the text content and style with scale factor applied to font metrics
    pub fn set_text_scaled(&mut self, text: &str, style: &CalloutStyle, max_width: f32, scale_factor: f32) {
        let scaled_font_size = style.font_size * scale_factor;
        let line_height = scaled_font_size * 1.2;
        self.line_height = line_height;
        let metrics = Metrics::new(scaled_font_size, line_height);
        self.buffer.set_metrics(&mut self.font_system, metrics);
        self.buffer.set_size(&mut self.font_system, max_width, f32::MAX);

        let attrs = Attrs::new().family(Family::SansSerif);
        self.buffer.set_text(&mut self.font_system, text, attrs, Shaping::Advanced);
    }

    /// Get the computed text bounds
    pub fn bounds(&mut self) -> (f32, f32) {
        // Calculate bounds from layout runs
        let mut max_width: f32 = 0.0;
        let mut line_count = 0;

        for run in self.buffer.layout_runs() {
            max_width = max_width.max(run.line_w);
            line_count += 1;
        }

        let total_height = line_count as f32 * self.line_height;
        (max_width, total_height)
    }

    /// Prepare for rendering
    /// scale_factor is the display's DPI scale (1.0 for standard, 2.0 for Retina)
    pub fn prepare(
        &mut self,
        device: &Device,
        queue: &Queue,
        position: [f32; 2],
        style: &CalloutStyle,
        viewport: [u32; 2],
        scale_factor: f32,
    ) {
        let color = Color::rgba(
            (style.text_color[0] * 255.0) as u8,
            (style.text_color[1] * 255.0) as u8,
            (style.text_color[2] * 255.0) as u8,
            (style.text_color[3] * 255.0) as u8,
        );

        let text_area = TextArea {
            buffer: &self.buffer,
            left: position[0],
            top: position[1],
            scale: scale_factor,
            bounds: TextBounds {
                left: position[0] as i32,
                top: position[1] as i32,
                right: viewport[0] as i32,
                bottom: viewport[1] as i32,
            },
            default_color: color,
        };

        self.renderer
            .prepare(
                device,
                queue,
                &mut self.font_system,
                &mut self.atlas,
                Resolution {
                    width: viewport[0],
                    height: viewport[1],
                },
                [text_area],
                &mut self.swash_cache,
            )
            .expect("Failed to prepare text");
    }

    /// Render the text
    pub fn render<'a>(&'a self, render_pass: &mut RenderPass<'a>) {
        self.renderer.render(&self.atlas, render_pass).expect("Failed to render text");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_text_animator_instant() {
        let animator = TextAnimator::new("Hello World", TextAnimation::Instant);
        assert_eq!(animator.visible_text(), "Hello World");
        assert!(animator.is_complete());
    }

    #[test]
    fn test_text_animator_typewriter() {
        let mut animator = TextAnimator::new("Hello", TextAnimation::Typewriter { cps: 10.0 });
        assert_eq!(animator.visible_text(), "");
        assert!(!animator.is_complete());

        animator.update(0.1); // Should show 1 character
        assert_eq!(animator.visible_text(), "H");

        animator.update(0.4); // Should show 5 characters total
        assert_eq!(animator.visible_text(), "Hello");
        assert!(animator.is_complete());
    }

    #[test]
    fn test_text_animator_word_by_word() {
        let mut animator = TextAnimator::new("Hello World Test", TextAnimation::WordByWord { wps: 2.0 });
        assert_eq!(animator.visible_text(), "");

        animator.update(0.5); // Should show first word
        assert_eq!(animator.visible_text(), "Hello");

        animator.update(0.5); // Should show two words
        assert_eq!(animator.visible_text(), "Hello World");
    }

    #[test]
    fn test_text_animator_skip() {
        let mut animator = TextAnimator::new("Hello World", TextAnimation::Typewriter { cps: 1.0 });
        assert!(!animator.is_complete());

        animator.skip();
        assert_eq!(animator.visible_text(), "Hello World");
        assert!(animator.is_complete());
    }
}

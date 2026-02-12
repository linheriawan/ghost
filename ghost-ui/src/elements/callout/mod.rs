//! Main callout implementation

pub mod shape;
pub mod text;
pub mod types;

use std::time::Duration;

use wgpu::{Device, Queue, RenderPass, TextureFormat};

use shape::{CalloutShape, ShapeRenderer};
use text::{TextAnimator, TextRenderer};
use types::{ArrowPosition, CalloutStyle, CalloutTiming, CalloutType, TextAnimation};

// Re-exports
pub use shape::ShapeRenderer as CalloutShapeRenderer;
pub use text::{TextAnimator as CalloutTextAnimator, TextRenderer as CalloutTextRenderer};

/// A callout bubble with text and animation
pub struct Callout {
    /// Callout type (Talk, Think, Scream)
    callout_type: CalloutType,
    /// Position relative to parent [x, y]
    position: [f32; 2],
    /// Arrow position and direction
    arrow: ArrowPosition,
    /// Maximum width before text wraps
    max_width: f32,
    /// Visual style
    style: CalloutStyle,
    /// Timing configuration
    timing: CalloutTiming,
    /// Text animation style
    text_animation: TextAnimation,

    // Runtime state
    /// Text animator
    text_animator: Option<TextAnimator>,
    /// Shape renderer
    shape_renderer: Option<ShapeRenderer>,
    /// Text renderer
    text_renderer: Option<TextRenderer>,
    /// Current shape
    shape: Option<CalloutShape>,
    /// Current visibility state
    visibility: VisibilityState,
    /// Elapsed time since creation
    elapsed: f32,
    /// Whether the callout is visible
    is_visible: bool,
    /// Current scale factor (for DPI scaling)
    scale_factor: f32,
    /// Whether shape needs regeneration (when scale factor changes)
    needs_shape_regen: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum VisibilityState {
    Hidden,
    FadingIn { progress: f32 },
    Visible,
    FadingOut { progress: f32 },
}

impl Callout {
    /// Create a new callout with default settings
    pub fn new() -> Self {
        Self {
            callout_type: CalloutType::default(),
            position: [0.0, 0.0],
            arrow: ArrowPosition::default(),
            max_width: 200.0,
            style: CalloutStyle::default(),
            timing: CalloutTiming::default(),
            text_animation: TextAnimation::default(),
            text_animator: None,
            shape_renderer: None,
            text_renderer: None,
            shape: None,
            visibility: VisibilityState::Hidden,
            elapsed: 0.0,
            is_visible: false,
            scale_factor: 1.0,
            needs_shape_regen: true,
        }
    }

    /// Set the callout type
    pub fn with_type(mut self, callout_type: CalloutType) -> Self {
        self.callout_type = callout_type;
        self
    }

    /// Set the position relative to parent
    pub fn with_position(mut self, x: f32, y: f32) -> Self {
        self.position = [x, y];
        self
    }

    /// Set the arrow position
    pub fn with_arrow(mut self, arrow: ArrowPosition) -> Self {
        self.arrow = arrow;
        self
    }

    /// Set the maximum width
    pub fn with_max_width(mut self, width: f32) -> Self {
        self.max_width = width;
        self
    }

    /// Set the visual style
    pub fn with_style(mut self, style: CalloutStyle) -> Self {
        self.style = style;
        self
    }

    /// Set the text animation style
    pub fn with_text_animation(mut self, animation: TextAnimation) -> Self {
        self.text_animation = animation;
        self
    }

    /// Set the duration (auto-hide after this time)
    pub fn with_duration(mut self, duration: Duration) -> Self {
        self.timing.duration = Some(duration);
        self
    }

    /// Set the delay before showing
    pub fn with_delay(mut self, delay: Duration) -> Self {
        self.timing.delay = delay;
        self
    }

    /// Set the timing configuration
    pub fn with_timing(mut self, timing: CalloutTiming) -> Self {
        self.timing = timing;
        self
    }

    /// Initialize GPU resources
    pub fn init(&mut self, device: &Device, queue: &Queue, format: TextureFormat) {
        self.shape_renderer = Some(ShapeRenderer::new(device, format));
        self.text_renderer = Some(TextRenderer::new(device, queue, format));
    }

    /// Say something (talk bubble)
    pub fn say(&mut self, text: impl Into<String>) {
        self.callout_type = CalloutType::Talk;
        self.show_text(text);
    }

    /// Think something (thought bubble)
    pub fn think(&mut self, text: impl Into<String>) {
        self.callout_type = CalloutType::Think;
        self.show_text(text);
    }

    /// Scream something (exclamation bubble)
    pub fn scream(&mut self, text: impl Into<String>) {
        self.callout_type = CalloutType::Scream;
        self.show_text(text);
    }

    /// Show text with current settings
    fn show_text(&mut self, text: impl Into<String>) {
        let text = text.into();
        self.text_animator = Some(TextAnimator::new(text, self.text_animation));
        self.elapsed = 0.0;
        self.is_visible = true;
        self.visibility = if self.timing.delay.is_zero() {
            if self.timing.fade_in.is_zero() {
                VisibilityState::Visible
            } else {
                VisibilityState::FadingIn { progress: 0.0 }
            }
        } else {
            VisibilityState::Hidden
        };

        // Mark shape for regeneration (will happen in prepare() with correct scale_factor)
        self.needs_shape_regen = true;
    }

    /// Regenerate the callout shape based on current text and scale factor
    fn regenerate_shape(&mut self, scale_factor: f32) {
        // Calculate content size based on text with scale factor
        let text_height = if let (Some(ref mut text_renderer), Some(ref animator)) =
            (&mut self.text_renderer, &self.text_animator)
        {
            // Set text to get bounds - use scaled font size for accurate bounds
            text_renderer.set_text_scaled(
                animator.full_text(),
                &self.style,
                self.max_width - 2.0 * self.style.padding * scale_factor,
                scale_factor,
            );
            let (_, h) = text_renderer.bounds();
            h.max(20.0 * scale_factor) // Minimum height scaled
        } else {
            50.0 * scale_factor // Default height when no text renderer
        };

        // Scale width and padding for the shape
        let width = self.max_width * scale_factor;
        let height = text_height + 2.0 * self.style.padding * scale_factor;

        // Create scaled style for shape
        let mut scaled_style = self.style.clone();
        scaled_style.padding *= scale_factor;
        scaled_style.border_radius *= scale_factor;
        scaled_style.border_width *= scale_factor;

        self.shape = Some(CalloutShape::new(
            self.callout_type,
            width,
            height,
            self.arrow,
            &scaled_style,
        ));

        self.scale_factor = scale_factor;
        self.needs_shape_regen = false;
    }

    /// Hide the callout
    pub fn hide(&mut self) {
        if self.is_visible {
            self.visibility = if self.timing.fade_out.is_zero() {
                self.is_visible = false;
                VisibilityState::Hidden
            } else {
                VisibilityState::FadingOut { progress: 0.0 }
            };
        }
    }

    /// Update the callout (call every frame)
    pub fn update(&mut self, delta_seconds: f32) {
        if !self.is_visible && self.visibility == VisibilityState::Hidden {
            return;
        }

        self.elapsed += delta_seconds;

        // Handle delay
        if self.elapsed < self.timing.delay.as_secs_f32() {
            return;
        }

        // Update visibility state
        match self.visibility {
            VisibilityState::Hidden => {
                // Start fading in after delay
                self.visibility = if self.timing.fade_in.is_zero() {
                    VisibilityState::Visible
                } else {
                    VisibilityState::FadingIn { progress: 0.0 }
                };
            }
            VisibilityState::FadingIn { progress } => {
                let fade_duration = self.timing.fade_in.as_secs_f32();
                let new_progress = progress + delta_seconds / fade_duration;
                if new_progress >= 1.0 {
                    self.visibility = VisibilityState::Visible;
                } else {
                    self.visibility = VisibilityState::FadingIn {
                        progress: new_progress,
                    };
                }
            }
            VisibilityState::Visible => {
                // Check if we should start fading out
                if let Some(duration) = self.timing.duration {
                    let visible_time = self.elapsed - self.timing.delay.as_secs_f32();
                    if visible_time >= duration.as_secs_f32() {
                        self.visibility = if self.timing.fade_out.is_zero() {
                            self.is_visible = false;
                            VisibilityState::Hidden
                        } else {
                            VisibilityState::FadingOut { progress: 0.0 }
                        };
                    }
                }
            }
            VisibilityState::FadingOut { progress } => {
                let fade_duration = self.timing.fade_out.as_secs_f32();
                let new_progress = progress + delta_seconds / fade_duration;
                if new_progress >= 1.0 {
                    self.visibility = VisibilityState::Hidden;
                    self.is_visible = false;
                } else {
                    self.visibility = VisibilityState::FadingOut {
                        progress: new_progress,
                    };
                }
            }
        }

        // Update text animation
        if let Some(ref mut animator) = self.text_animator {
            animator.update(delta_seconds);
        }
    }

    /// Get current opacity based on visibility state
    pub fn opacity(&self) -> f32 {
        match self.visibility {
            VisibilityState::Hidden => 0.0,
            VisibilityState::FadingIn { progress } => progress,
            VisibilityState::Visible => 1.0,
            VisibilityState::FadingOut { progress } => 1.0 - progress,
        }
    }

    /// Check if the callout is visible
    pub fn is_visible(&self) -> bool {
        self.is_visible && self.visibility != VisibilityState::Hidden
    }

    /// Check if any animation is currently running (text or visibility fade)
    pub fn is_animating(&self) -> bool {
        // Text animation in progress
        let text_animating = self.text_animator
            .as_ref()
            .map(|a| !a.is_complete())
            .unwrap_or(false);

        // Visibility fade in progress
        let visibility_animating = matches!(
            self.visibility,
            VisibilityState::FadingIn { .. } | VisibilityState::FadingOut { .. }
        );

        text_animating || visibility_animating
    }

    /// Check if text animation is complete
    pub fn is_text_complete(&self) -> bool {
        self.text_animator
            .as_ref()
            .map(|a| a.is_complete())
            .unwrap_or(true)
    }

    /// Skip text animation
    pub fn skip_animation(&mut self) {
        if let Some(ref mut animator) = self.text_animator {
            animator.skip();
        }
    }

    /// Get the visible text
    pub fn visible_text(&self) -> &str {
        self.text_animator
            .as_ref()
            .map(|a| a.visible_text())
            .unwrap_or("")
    }

    /// Prepare for rendering
    /// scale_factor is the display's DPI scale (1.0 for standard, 2.0 for Retina)
    pub fn prepare(&mut self, device: &Device, queue: &Queue, viewport: [f32; 2], scale_factor: f32) {
        if !self.is_visible() {
            return;
        }

        // Regenerate shape if scale factor changed or shape needs regeneration
        if self.needs_shape_regen || (self.scale_factor - scale_factor).abs() > 0.01 {
            self.regenerate_shape(scale_factor);
        }

        // Prepare shape with scaled position
        if let (Some(ref mut shape_renderer), Some(ref shape)) =
            (&mut self.shape_renderer, &self.shape)
        {
            let scaled_position = [
                self.position[0] * scale_factor,
                self.position[1] * scale_factor,
            ];
            shape_renderer.prepare(device, queue, shape, scaled_position, viewport);
        }

        // Prepare text with scale factor for proper DPI rendering
        if let (Some(ref mut text_renderer), Some(ref animator)) =
            (&mut self.text_renderer, &self.text_animator)
        {
            let scaled_padding = self.style.padding * scale_factor;
            let text_position = [
                self.position[0] * scale_factor + scaled_padding,
                self.position[1] * scale_factor + scaled_padding,
            ];
            // Text is already scaled in set_text_scaled during regenerate_shape,
            // now just update visible text and prepare for rendering
            text_renderer.set_text_scaled(
                animator.visible_text(),
                &self.style,
                self.max_width * scale_factor - 2.0 * scaled_padding,
                scale_factor,
            );
            text_renderer.prepare(
                device,
                queue,
                text_position,
                &self.style,
                [viewport[0] as u32, viewport[1] as u32],
                1.0, // Scale is already applied to font metrics
            );
        }
    }

    /// Render the callout
    pub fn render<'a>(&'a self, render_pass: &mut RenderPass<'a>) {
        if !self.is_visible() {
            return;
        }

        // Render shape first
        if let Some(ref shape_renderer) = self.shape_renderer {
            shape_renderer.render(render_pass);
        }

        // Render text on top
        if let Some(ref text_renderer) = self.text_renderer {
            text_renderer.render(render_pass);
        }
    }

    /// Get position
    pub fn position(&self) -> [f32; 2] {
        self.position
    }

    /// Set position
    pub fn set_position(&mut self, x: f32, y: f32) {
        self.position = [x, y];
    }

    /// Get the bounding box of the callout
    pub fn bounds(&self) -> Option<[f32; 4]> {
        self.shape.as_ref().map(|s| {
            let b = s.bounds();
            [
                self.position[0] + b[0],
                self.position[1] + b[1],
                b[2],
                b[3],
            ]
        })
    }
}

impl Default for Callout {
    fn default() -> Self {
        Self::new()
    }
}

/// Builder for creating callouts with a fluent API
pub struct CalloutBuilder {
    callout: Callout,
}

impl CalloutBuilder {
    /// Create a new callout builder
    pub fn new() -> Self {
        Self {
            callout: Callout::new(),
        }
    }

    /// Set the callout type
    pub fn callout_type(mut self, callout_type: CalloutType) -> Self {
        self.callout.callout_type = callout_type;
        self
    }

    /// Set the position
    pub fn position(mut self, x: f32, y: f32) -> Self {
        self.callout.position = [x, y];
        self
    }

    /// Set the arrow position
    pub fn arrow(mut self, arrow: ArrowPosition) -> Self {
        self.callout.arrow = arrow;
        self
    }

    /// Set the maximum width
    pub fn max_width(mut self, width: f32) -> Self {
        self.callout.max_width = width;
        self
    }

    /// Set the style
    pub fn style(mut self, style: CalloutStyle) -> Self {
        self.callout.style = style;
        self
    }

    /// Set the text animation
    pub fn text_animation(mut self, animation: TextAnimation) -> Self {
        self.callout.text_animation = animation;
        self
    }

    /// Set the duration
    pub fn duration(mut self, duration: Duration) -> Self {
        self.callout.timing.duration = Some(duration);
        self
    }

    /// Set the delay
    pub fn delay(mut self, delay: Duration) -> Self {
        self.callout.timing.delay = delay;
        self
    }

    /// Set the timing
    pub fn timing(mut self, timing: CalloutTiming) -> Self {
        self.callout.timing = timing;
        self
    }

    /// Build the callout
    pub fn build(self) -> Callout {
        self.callout
    }

    /// Build and initialize with GPU resources
    pub fn build_with_gpu(mut self, device: &Device, queue: &Queue, format: TextureFormat) -> Callout {
        self.callout.init(device, queue, format);
        self.callout
    }
}

impl Default for CalloutBuilder {
    fn default() -> Self {
        Self::new()
    }
}

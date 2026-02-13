╭──────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────╮
│ Plan to implement                                                                                                                            │
│                                                                                                                                              │
│ Improve ghost-ui Widget System                                                                                                               │
│                                                                                                                                              │
│ Context                                                                                                                                      │
│                                                                                                                                              │
│ The ghost-ui crate's widget system currently only renders flat colored rectangles for buttons. Text labels are stored but never drawn.       │
│ Border-radius exists in style but is ignored. There are no image-based buttons or label widgets.                                             │
│                                                                                                                                              │
│ Goal: Add ButtonImage, improve Button (text + rounded corners), add Label, add MarqueeLabel (scrolling text). All rendering stays in the     │
│ existing single wgpu render pass on the transparent window.                                                                                  │
│                                                                                                                                              │
│ Architecture                                                                                                                                 │
│                                                                                                                                              │
│ Replace ButtonRenderer with a new WidgetRenderer that has three sub-systems:                                                                 │
│                                                                                                                                              │
│ 1. Solid pipeline - SDF rounded rectangles (Button bg, Label bg)                                                                             │
│ 2. Image pipeline - Textured quads with brightness modifier (ButtonImage)                                                                    │
│ 3. Text pipeline - glyphon text (Button labels, Label text, MarqueeLabel scrolling text)                                                     │
│                                                                                                                                              │
│ Render Order (unchanged structure, widgets replace buttons)                                                                                  │
│                                                                                                                                              │
│ Clear(TRANSPARENT)                                                                                                                           │
│ -> sprite_pipeline.render()         [skin/character]                                                                                         │
│ -> app.render_layers()              [layers + layer text]                                                                                    │
│ -> widget_renderer.render()         [solid bg -> images -> text]                                                                             │
│                                                                                                                                              │
│ Files to Create                                                                                                                              │
│ ┌─────────────────────────────────────────┬──────────────────────────────────────────┐                                                       │
│ │                  File                   │                 Purpose                  │                                                       │
│ ├─────────────────────────────────────────┼──────────────────────────────────────────┤                                                       │
│ │ ghost-ui/src/renderer/widget.wgsl       │ SDF rounded rect shader                  │                                                       │
│ ├─────────────────────────────────────────┼──────────────────────────────────────────┤                                                       │
│ │ ghost-ui/src/renderer/widget_image.wgsl │ Textured quad + brightness shader        │                                                       │
│ ├─────────────────────────────────────────┼──────────────────────────────────────────┤                                                       │
│ │ ghost-ui/src/renderer/widget.rs         │ WidgetRenderer (replaces ButtonRenderer) │                                                       │
│ ├─────────────────────────────────────────┼──────────────────────────────────────────┤                                                       │
│ │ ghost-ui/src/elements/button_image.rs   │ ButtonImage element                      │                                                       │
│ ├─────────────────────────────────────────┼──────────────────────────────────────────┤                                                       │
│ │ ghost-ui/src/elements/label.rs          │ Label + LabelStyle + FontStyle           │                                                       │
│ ├─────────────────────────────────────────┼──────────────────────────────────────────┤                                                       │
│ │ ghost-ui/src/elements/marquee_label.rs  │ MarqueeLabel (scrolling text)            │                                                       │
│ └─────────────────────────────────────────┴──────────────────────────────────────────┘                                                       │
│ Files to Modify                                                                                                                              │
│ ┌──────────────────────────────┬─────────────────────────────────────────────────────────────────────────────────────────────┐               │
│ │             File             │                                           Changes                                           │               │
│ ├──────────────────────────────┼─────────────────────────────────────────────────────────────────────────────────────────────┤               │
│ │ ghost-ui/src/elements/mod.rs │ Add new modules, re-export new types                                                        │               │
│ ├──────────────────────────────┼─────────────────────────────────────────────────────────────────────────────────────────────┤               │
│ │ ghost-ui/src/renderer/mod.rs │ Add mod widget, export WidgetRenderer                                                       │               │
│ ├──────────────────────────────┼─────────────────────────────────────────────────────────────────────────────────────────────┤               │
│ │ ghost-ui/src/lib.rs          │ Re-export new types                                                                         │               │
│ ├──────────────────────────────┼─────────────────────────────────────────────────────────────────────────────────────────────┤               │
│ │ ghost-ui/src/window/mod.rs   │ Add widget accessors to GhostApp, replace ButtonRenderer with WidgetRenderer in event loops │               │
│ └──────────────────────────────┴─────────────────────────────────────────────────────────────────────────────────────────────┘               │
│ Step 1: SDF Rounded Rectangle Shader                                                                                                         │
│                                                                                                                                              │
│ Create ghost-ui/src/renderer/widget.wgsl                                                                                                     │
│                                                                                                                                              │
│ Vertex carries rect_min, rect_max, corner_radius as attributes so the fragment shader evaluates the SDF per-pixel. Outputs premultiplied     │
│ alpha (matching SpritePipeline).                                                                                                             │
│                                                                                                                                              │
│ // Vertex: position, color, rect_min, rect_max, corner_radius                                                                                │
│ // Fragment: SDF rounded rect with smoothstep anti-aliasing                                                                                  │
│ // Output: premultiplied alpha (rgb * alpha, alpha)                                                                                          │
│                                                                                                                                              │
│ Vertex layout (48 bytes):                                                                                                                    │
│ - position: [f32; 2] - pixel coords                                                                                                          │
│ - color: [f32; 4] - RGBA                                                                                                                     │
│ - rect_min: [f32; 2] - top-left of rect (pixels)                                                                                             │
│ - rect_max: [f32; 2] - bottom-right of rect (pixels)                                                                                         │
│ - corner_radius: f32 - CSS-like border-radius                                                                                                │
│ - _padding: f32                                                                                                                              │
│                                                                                                                                              │
│ Step 2: Image Button Shader                                                                                                                  │
│                                                                                                                                              │
│ Create ghost-ui/src/renderer/widget_image.wgsl                                                                                               │
│                                                                                                                                              │
│ Based on sprite.wgsl but adds a brightness field to uniforms:                                                                                │
│ - Normal: brightness = 1.0                                                                                                                   │
│ - Hover: brightness = 1.15 (brighter)                                                                                                        │
│ - Pressed: brightness = 0.9 (darker)                                                                                                         │
│                                                                                                                                              │
│ Fragment: clamp(color.rgb * brightness, 0, 1) then premultiply alpha.                                                                        │
│                                                                                                                                              │
│ Step 3: WidgetRenderer                                                                                                                       │
│                                                                                                                                              │
│ Create ghost-ui/src/renderer/widget.rs                                                                                                       │
│                                                                                                                                              │
│ pub struct WidgetRenderer {                                                                                                                  │
│     // Solid pipeline (SDF rounded rects)                                                                                                    │
│     solid_pipeline: RenderPipeline,                                                                                                          │
│     solid_bind_group: BindGroup,                                                                                                             │
│     solid_vertex_buffer: Option<Buffer>,                                                                                                     │
│     solid_index_buffer: Option<Buffer>,                                                                                                      │
│     solid_index_count: u32,                                                                                                                  │
│                                                                                                                                              │
│     // Image pipeline (textured quads with brightness)                                                                                       │
│     image_pipeline: RenderPipeline,                                                                                                          │
│     image_bind_group_layout: BindGroupLayout,                                                                                                │
│     image_sampler: Sampler,                                                                                                                  │
│     image_draw_calls: Vec<ImageDrawCall>,                                                                                                    │
│                                                                                                                                              │
│     // Text pipeline (glyphon - same pattern as LayerRenderer)                                                                               │
│     font_system: FontSystem,                                                                                                                 │
│     swash_cache: SwashCache,                                                                                                                 │
│     text_atlas: Option<TextAtlas>,                                                                                                           │
│     text_renderer: Option<GlyphonTextRenderer>,                                                                                              │
│     text_buffers: Vec<Buffer>,                                                                                                               │
│ }                                                                                                                                            │
│                                                                                                                                              │
│ Key methods:                                                                                                                                 │
│ - new(device, queue, format) - create all pipelines                                                                                          │
│ - prepare(device, queue, buttons, button_images, labels, marquees, viewport, scale_factor):                                                  │
│   a. Generate SDF rounded-rect vertices for Button/Label backgrounds                                                                         │
│   b. Create bind groups for each ButtonImage (texture + brightness uniform)                                                                  │
│   c. Shape and position text for all text-bearing widgets                                                                                    │
│ - render(render_pass):                                                                                                                       │
│   a. Draw solid backgrounds                                                                                                                  │
│   b. Draw image buttons                                                                                                                      │
│   c. Draw text                                                                                                                               │
│                                                                                                                                              │
│ Reuses from existing code:                                                                                                                   │
│ - SpritePipeline bind group layout pattern for image pipeline (from renderer/sprite.rs)                                                      │
│ - glyphon text rendering pattern (from layer.rs LayerRenderer)                                                                               │
│ - Vertex generation pattern (from renderer/button.rs ButtonRenderer)                                                                         │
│                                                                                                                                              │
│ Step 4: ButtonImage Element                                                                                                                  │
│                                                                                                                                              │
│ Create ghost-ui/src/elements/button_image.rs                                                                                                 │
│                                                                                                                                              │
│ pub struct ButtonImage {                                                                                                                     │
│     id: ButtonId,                                                                                                                            │
│     position: [f32; 2],                                                                                                                      │
│     size: [f32; 2],                                                                                                                          │
│     state: ButtonState,                                                                                                                      │
│     origin: Origin,                                                                                                                          │
│     visible: bool,                                                                                                                           │
│     skin_data: SkinData,                                                                                                                     │
│     skin: Option<Skin>,  // GPU texture, created on init_gpu()                                                                               │
│ }                                                                                                                                            │
│                                                                                                                                              │
│ - brightness() returns 1.0 / 1.15 / 0.9 based on state                                                                                       │
│ - init_gpu(device, queue) creates Skin from SkinData                                                                                         │
│ - Implements Widget trait (same hit-testing as Button via contains_point)                                                                    │
│ - Size defaults to image dimensions, can be overridden                                                                                       │
│                                                                                                                                              │
│ Step 5: Label Element                                                                                                                        │
│                                                                                                                                              │
│ Create ghost-ui/src/elements/label.rs                                                                                                        │
│                                                                                                                                              │
│ pub struct LabelStyle {                                                                                                                      │
│     pub background: [f32; 4],    // RGBA background                                                                                          │
│     pub text_color: [f32; 4],                                                                                                                │
│     pub font_size: f32,                                                                                                                      │
│     pub font_style: FontStyle,   // Normal, Bold, Italic                                                                                     │
│     pub border_radius: f32,      // CSS-like                                                                                                 │
│ }                                                                                                                                            │
│                                                                                                                                              │
│ pub struct Label {                                                                                                                           │
│     position: [f32; 2],                                                                                                                      │
│     size: [f32; 2],                                                                                                                          │
│     text: String,                                                                                                                            │
│     style: LabelStyle,                                                                                                                       │
│     origin: Origin,                                                                                                                          │
│     visible: bool,                                                                                                                           │
│ }                                                                                                                                            │
│                                                                                                                                              │
│ Non-interactive. Rendered as: rounded-rect background + text overlay.                                                                        │
│                                                                                                                                              │
│ Step 6: MarqueeLabel Element                                                                                                                 │
│                                                                                                                                              │
│ Create ghost-ui/src/elements/marquee_label.rs                                                                                                │
│                                                                                                                                              │
│ pub struct MarqueeLabel {                                                                                                                    │
│     label: Label,           // composition                                                                                                   │
│     scroll_speed: f32,      // pixels/sec                                                                                                    │
│     scroll_offset: f32,     // current offset                                                                                                │
│     text_width: f32,        // measured after text shaping                                                                                   │
│     gap: f32,               // gap between repeated text                                                                                     │
│ }                                                                                                                                            │
│                                                                                                                                              │
│ - update(delta) advances scroll_offset, wraps when offset > text_width + gap                                                                 │
│ - WidgetRenderer clips text to label bounds using glyphon TextBounds                                                                         │
│ - Text position offset by -scroll_offset for scrolling effect                                                                                │
│                                                                                                                                              │
│ Step 7: Wire into GhostApp Trait                                                                                                             │
│                                                                                                                                              │
│ Modify ghost-ui/src/window/mod.rs                                                                                                            │
│                                                                                                                                              │
│ Add to GhostApp trait (all with default empty implementations):                                                                              │
│ fn button_images(&self) -> Vec<&ButtonImage> { vec![] }                                                                                      │
│ fn button_images_mut(&mut self) -> Vec<&mut ButtonImage> { vec![] }                                                                          │
│ fn labels(&self) -> Vec<&Label> { vec![] }                                                                                                   │
│ fn marquee_labels(&self) -> Vec<&MarqueeLabel> { vec![] }                                                                                    │
│ fn marquee_labels_mut(&mut self) -> Vec<&mut MarqueeLabel> { vec![] }                                                                        │
│                                                                                                                                              │
│ In event loop functions (run_with_app, run_with_app_and_callout, run_with_app_callout_and_extra):                                            │
│ - Replace ButtonRenderer with WidgetRenderer                                                                                                 │
│ - Add hover/press/release handling for ButtonImage (same pattern as Button)                                                                  │
│ - Call marquee.update(delta) in MainEventsCleared                                                                                            │
│ - Pass all widget types to widget_renderer.prepare()                                                                                         │
│                                                                                                                                              │
│ Step 8: Update Exports                                                                                                                       │
│                                                                                                                                              │
│ Modify ghost-ui/src/elements/mod.rs:                                                                                                         │
│ mod button_image;                                                                                                                            │
│ mod label;                                                                                                                                   │
│ mod marquee_label;                                                                                                                           │
│ pub use button_image::ButtonImage;                                                                                                           │
│ pub use label::{Label, LabelId, LabelStyle, FontStyle};                                                                                      │
│ pub use marquee_label::MarqueeLabel;                                                                                                         │
│                                                                                                                                              │
│ Modify ghost-ui/src/lib.rs:                                                                                                                  │
│ pub use elements::{ButtonImage, Label, LabelId, LabelStyle, FontStyle, MarqueeLabel};                                                        │
│ pub use renderer::WidgetRenderer;                                                                                                            │
│                                                                                                                                              │
│ Modify ghost-ui/src/renderer/mod.rs:                                                                                                         │
│ mod widget;                                                                                                                                  │
│ pub use widget::WidgetRenderer;                                                                                                              │
│                                                                                                                                              │
│ Execution Order                                                                                                                              │
│                                                                                                                                              │
│ 1. Create widget.wgsl (SDF shader)                                                                                                           │
│ 2. Create widget_image.wgsl (brightness shader)                                                                                              │
│ 3. Create renderer/widget.rs (WidgetRenderer with solid pipeline only)                                                                       │
│ 4. Test: existing Buttons render with rounded corners via WidgetRenderer                                                                     │
│ 5. Add text pipeline to WidgetRenderer (Button labels render)                                                                                │
│ 6. Create elements/button_image.rs + image pipeline in WidgetRenderer                                                                        │
│ 7. Create elements/label.rs + rendering                                                                                                      │
│ 8. Create elements/marquee_label.rs + scrolling                                                                                              │
│ 9. Update GhostApp trait + event loops                                                                                                       │
│ 10. Update elements/mod.rs, renderer/mod.rs, lib.rs exports                                                                                  │
│ 11. Deprecate ButtonRenderer (keep for backward compat)                                                                                      │
│                                                                                                                                              │
│ Key Design Decisions                                                                                                                         │
│                                                                                                                                              │
│ - One image per ButtonImage, shader does brightness effects (not separate images per state)                                                  │
│ - CSS-like border-radius via SDF in fragment shader (user sets value, shader handles rendering)                                              │
│ - Premultiplied alpha throughout (matching SpritePipeline for transparent window)                                                            │
│ - glyphon for all text (same stack as LayerRenderer)                                                                                         │
│ - WidgetRenderer replaces ButtonRenderer but keeps backward compat via deprecation                                                           │
│                                                                                                                                              │
│ Verification                                                                                                                                 │
│                                                                                                                                              │
│ After each phase, cargo check must pass. Final verification:                                                                                 │
│ - Button renders with rounded corners and visible text label                                                                                 │
│ - ButtonImage renders PNG, brightens on hover, darkens on click                                                                              │
│ - Label renders background + text                                                                                                            │
│ - MarqueeLabel text scrolls horizontally                                                                                                     │
│ - All widgets work on the transparent window with correct alpha blending                                                                     │ 
╰──────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────╯

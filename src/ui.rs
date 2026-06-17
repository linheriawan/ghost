//! UI factory — create and configure widget instances.
//!
//! Buttons are defined in code (make_btn); ui.toml can override their position/size/style.
//! Layers, images, labels, and marquees each have a matching factory.

use ghost_ui::{
    Button, ButtonId, ButtonImage, ButtonStyle, Label, LabelId, LabelStyle,
    Layer, LayerAnchor, LayerConfig, MarqueeLabel, Origin, TextAlign, TextVAlign,
};

use crate::config::{Config, LayerConfig as CfgLayer};

fn name_hash(name: &str) -> u32 {
    name.bytes().fold(0u32, |acc, b| acc.wrapping_mul(31).wrapping_add(b as u32))
}

/// Stable ButtonId from a name string.
pub fn get_button_id(name: &str) -> ButtonId {
    ButtonId::new(name_hash(name))
}

/// Stable LabelId from a name string.
pub fn get_label_id(name: &str) -> LabelId {
    LabelId::new(name_hash(name))
}

/// Build a Button with code defaults; a matching config entry overrides position/size/style.
pub fn make_btn(
    id: &str,
    label: &str,
    pos: [f32; 2],
    size: [f32; 2],
    style: ButtonStyle,
    config: &Config,
) -> Button {
    let cfg   = config.buttons.iter().find(|c| c.id == id);
    let pos   = cfg.map(|c| c.position).unwrap_or(pos);
    let size  = cfg.map(|c| c.size).unwrap_or(size);
    let style = cfg.map(|c| match c.style.as_str() {
        "primary" => ButtonStyle::primary(),
        "light"   => ButtonStyle::light(),
        _         => ButtonStyle::default(),
    }).unwrap_or(style);
    Button::new(get_button_id(id), label)
        .with_position(pos[0], pos[1])
        .with_size(size[0], size[1])
        .with_style(style)
        .with_origin(Origin::BottomLeft)
}

/// Build a Layer from a config entry; logs and returns None on load failure.
pub fn make_layer(cfg: &CfgLayer, skin_width: u32, skin_height: u32) -> Option<Layer> {
    let ghost_cfg = LayerConfig {
        anchor:      LayerAnchor::from_str(&cfg.anchor),
        offset:      cfg.offset,
        size:        cfg.size,
        text:        cfg.text.clone(),
        text_color:  cfg.text_color,
        font_size:   cfg.font_size,
        z_order:     cfg.z_order,
        text_align:  TextAlign::from_str(&cfg.text_align),
        text_valign: TextVAlign::from_str(&cfg.text_valign),
        text_offset: cfg.text_offset,
        text_padding:cfg.text_padding,
    };
    match Layer::from_path(&cfg.path, ghost_cfg) {
        Ok(mut layer) => {
            layer.calculate_position(skin_width, skin_height);
            log::info!("Loaded layer: {} at {:?}", cfg.path, layer.position());
            Some(layer)
        }
        Err(e) => {
            log::error!("Failed to load layer '{}': {}", cfg.path, e);
            None
        }
    }
}

/// Build a ButtonImage from an image file; logs and returns None on load failure.
pub fn make_btn_image(id: &str, path: &str, pos: [f32; 2]) -> Option<ButtonImage> {
    match ButtonImage::from_path(get_button_id(id), path) {
        Ok(img) => Some(img.with_position(pos[0], pos[1]).with_origin(Origin::BottomLeft)),
        Err(e) => {
            log::error!("Failed to load button image '{}': {}", path, e);
            None
        }
    }
}

/// Build a Label.
pub fn make_label(
    id: &str,
    text: &str,
    pos: [f32; 2],
    size: [f32; 2],
    style: LabelStyle,
) -> Label {
    Label::new(get_label_id(id), text)
        .with_position(pos[0], pos[1])
        .with_size(size[0], size[1])
        .with_style(style)
        .with_origin(Origin::BottomLeft)
}

/// Build a MarqueeLabel (scrolling text).
pub fn make_marquee(
    id: &str,
    text: &str,
    pos: [f32; 2],
    size: [f32; 2],
    style: LabelStyle,
    speed: f32,
) -> MarqueeLabel {
    MarqueeLabel::new(get_label_id(id), text)
        .with_position(pos[0], pos[1])
        .with_size(size[0], size[1])
        .with_style(style)
        .with_scroll_speed(speed)
        .with_origin(Origin::BottomLeft)
}

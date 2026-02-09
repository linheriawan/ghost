//! UI layer - buttons, layout, visual elements

use ghost_ui::{Button, ButtonId, ButtonStyle, Origin};

use crate::config::ButtonConfig;

/// Button identifiers (dynamic based on config)
pub fn button_id_from_string(s: &str) -> ButtonId {
    // Simple hash for string -> u32
    let hash = s.bytes().fold(0u32, |acc, b| acc.wrapping_mul(31).wrapping_add(b as u32));
    ButtonId::new(hash)
}

/// Create buttons from configuration
pub fn create_buttons_from_config(configs: &[ButtonConfig]) -> Vec<Button> {
    configs
        .iter()
        .map(|cfg| {
            let id = button_id_from_string(&cfg.id);
            let style = match cfg.style.as_str() {
                "primary" => ButtonStyle::primary(),
                "light" => ButtonStyle::light(),
                _ => ButtonStyle::default(),
            };

            Button::new(id, &cfg.label)
                .with_position(cfg.position[0], cfg.position[1])
                .with_size(cfg.size[0], cfg.size[1])
                .with_style(style)
                .with_origin(Origin::BottomLeft)
        })
        .collect()
}

/// Get button ID by name
pub fn get_button_id(name: &str) -> ButtonId {
    button_id_from_string(name)
}

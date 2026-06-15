//! Skin loading from config — shared by main_window and any other window that needs a persona skin.

use std::sync::mpsc::Receiver;

use ghost_ui::{skin, AnimatedSkin, PersonaMeta, SkinData};

use crate::config::SkinConfig;

/// Everything that comes out of loading a skin from config.
pub struct SkinBundle {
    pub width: u32,
    pub height: u32,
    /// Set when skin is an animation directory (already loaded).
    pub animated: Option<AnimatedSkin>,
    /// Set when skin is a .persona.zip (loaded in background thread).
    pub persona: Option<PersonaMeta>,
    /// Background receiver for async zip loading.
    pub load_rx: Option<Receiver<AnimatedSkin>>,
    /// Set for static (non-animated) skins — pass to GhostWindowBuilder.
    pub static_data: Option<SkinData>,
}

/// Load a skin from config. Handles zip, animated directory, and static image.
pub fn load(config: &SkinConfig) -> SkinBundle {
    if config.path.ends_with(".zip") {
        let meta = AnimatedSkin::load_meta_from_zip(&config.path).unwrap_or_else(|e| {
            log::error!("Failed to load persona meta '{}': {}", config.path, e);
            panic!("Could not load persona meta");
        });
        let dims = meta.still_image.as_ref().map(|s| s.dimensions()).unwrap_or((200, 200));
        log::info!(
            "Loaded persona meta '{}': {}x{}, loading: \"{}\"",
            meta.name, dims.0, dims.1, meta.loading_text
        );

        let zip_path = config.path.clone();
        let fps = config.fps;
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            log::info!("Background: loading animation frames from zip...");
            match AnimatedSkin::from_zip(&zip_path, fps) {
                Ok((skin, _meta)) => {
                    log::info!("Background: animation loading complete");
                    let _ = tx.send(skin);
                }
                Err(e) => log::error!("Background: failed to load animation: {}", e),
            }
        });

        SkinBundle {
            width: dims.0,
            height: dims.1,
            animated: None,
            persona: Some(meta),
            load_rx: Some(rx),
            static_data: None,
        }
    } else if config.animated {
        let animated = AnimatedSkin::from_directory(&config.path, config.fps).unwrap_or_else(|e| {
            log::error!("Failed to load animated skin '{}': {}", config.path, e);
            panic!("Could not load animated skin");
        });
        let dims = animated.dimensions().unwrap_or((200, 200));
        log::info!("Loaded animated skin: {}x{} at {}fps", dims.0, dims.1, config.fps);
        SkinBundle {
            width: dims.0,
            height: dims.1,
            animated: Some(animated),
            persona: None,
            load_rx: None,
            static_data: None,
        }
    } else {
        let data = skin(&config.path).unwrap_or_else(|e| {
            log::error!("Failed to load skin '{}': {}", config.path, e);
            panic!("Could not load skin image");
        });
        let (w, h) = (data.width(), data.height());
        SkinBundle {
            width: w,
            height: h,
            animated: None,
            persona: None,
            load_rx: None,
            static_data: Some(data),
        }
    }
}

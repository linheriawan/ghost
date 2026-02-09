//! Ghost - Desktop mascot with callout bubbles

mod actions;
mod app;
mod config;
mod ui;

use ghost_ui::{icon_bytes, skin, EventLoop, GhostWindowBuilder};

fn main() {
    // Initialize logging
    env_logger::init();

    // --- 1. LOAD CONFIGURATION ---
    let config = config::Config::load_default().unwrap_or_else(|e| {
        log::error!("Failed to load ui.toml: {}", e);
        log::info!("Using default configuration");
        // Provide a sensible default
        panic!("Please create ui.toml configuration file");
    });

    log::info!("Loaded configuration from ui.toml");
    log::info!("Skin: {}", config.skin.path);
    log::info!("Callout anchor: {}", config.callout.anchor);
    log::info!("Buttons: {}", config.buttons.len());

    let event_loop = EventLoop::new();

    // --- 2. SETUP ICONS (tray + dock) ---
    let mut app_icon = icon_bytes(include_bytes!("../assets/icon.png"));
    if let Err(e) = app_icon.setup_all() {
        log::error!("Failed to setup icons: {}", e);
    }

    // --- 3. LOAD SKIN FROM CONFIG ---
    let skin_data = skin(&config.skin.path).unwrap_or_else(|e| {
        log::error!("Failed to load skin '{}': {}", config.skin.path, e);
        panic!("Could not load skin image");
    });

    // --- 4. CALCULATE WINDOW LAYOUT ---
    // Window needs to be larger than skin to accommodate callout
    let layout = config.calculate_window_layout(skin_data.width(), skin_data.height());

    // --- 5. CREATE GHOST WINDOW ---
    let window = GhostWindowBuilder::new()
        .with_size(layout.window_width, layout.window_height)
        .with_skin_offset(layout.skin_offset)
        .with_always_on_top(true)
        .with_draggable(true)
        .with_click_through(false)
        .with_alpha_hit_test(true)
        .with_opacity_focused(1.0)
        .with_opacity_unfocused(0.7)
        .with_title("Ghost")
        .with_skin_data(&skin_data)
        .build(&event_loop)
        .expect("Failed to create ghost window");

    // --- 6. CREATE APP FROM CONFIG ---
    // Pass skin offset so app can position callout relative to skin
    let app = app::App::new(config, skin_data.width(), skin_data.height(), layout.skin_offset);

    log::info!("Ghost app started");

    // Run with custom app handler
    ghost_ui::run_with_app(window, event_loop, app);
}

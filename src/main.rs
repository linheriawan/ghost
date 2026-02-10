//! Ghost - Desktop mascot with callout bubbles

mod actions;
mod app;
mod callout_app;
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

    // --- 4. CREATE CALLOUT CHANNEL ---
    let (callout_sender, callout_receiver) = callout_app::create_callout_channel();

    // --- 5. CALCULATE CALLOUT WINDOW POSITION AND SIZE ---
    let callout_offset = callout_app::calculate_callout_offset(&config, skin_data.width(), skin_data.height());
    let callout_size = callout_app::calculate_callout_size(&config);

    log::info!("Callout offset: {:?}, size: {:?}", callout_offset, callout_size);

    // --- 6. CREATE MAIN GHOST WINDOW ---
    let main_window = GhostWindowBuilder::new()
        .with_size(skin_data.width(), skin_data.height())
        .with_always_on_top(true)
        .with_draggable(true)
        .with_click_through(false)
        .with_alpha_hit_test(true)
        .with_opacity_focused(1.0)
        .with_opacity_unfocused(0.7)
        .with_title("Ghost")
        .with_skin_data(&skin_data)
        .build(&event_loop)
        .expect("Failed to create main window");

    // --- 7. CREATE CALLOUT WINDOW ---
    let callout_window = GhostWindowBuilder::new()
        .with_size(callout_size.0, callout_size.1)
        .with_always_on_top(true)
        .with_draggable(false) // Callout follows main window
        .with_click_through(true) // Clicks pass through
        .with_alpha_hit_test(false)
        .with_opacity_focused(1.0)
        .with_opacity_unfocused(1.0)
        .with_title("Ghost Callout")
        .build(&event_loop)
        .expect("Failed to create callout window");

    // --- 8. CREATE APPS ---
    let main_app = app::App::new(config.clone(), skin_data.width(), skin_data.height(), callout_sender);
    let callout_window_app = callout_app::CalloutWindowApp::new(&config, callout_receiver);

    log::info!("Ghost app started with linked callout window");

    // Run with linked callout window
    ghost_ui::run_with_app_and_callout(
        main_window,
        callout_window,
        callout_offset,
        event_loop,
        main_app,
        callout_window_app,
    );
}

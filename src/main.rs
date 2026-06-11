//! Ghost - Desktop mascot with callout bubbles

mod actions;
mod brain;
mod bus;
mod config;
mod coordinator;
mod tray;
mod ui;
mod vars;
mod windows;

use ghost_ui::{skin, AnimatedSkin, EventLoop, GhostWindowBuilder};

fn main() {
    // Initialize logging
    env_logger::init();

    // --- 1. LOAD CONFIGURATION ---
    let config = config::Config::load_default().unwrap_or_else(|e| {
        log::error!("Failed to load ui.toml: {}", e);
        panic!("Please create ui.toml configuration file");
    });

    log::info!("Loaded configuration from ui.toml");
    log::info!("Skin: {}", config.skin.path);
    log::info!("Animated: {}", config.skin.animated);
    log::info!("Callout anchor: {}", config.callout.anchor);
    log::info!("Buttons: {}", config.buttons.len());

    // --- 2. CREATE EVENT LOOP ---
    let event_loop = EventLoop::new();

    // --- 3. CREATE SHARED STATE ---
    let ghost_state = vars::GhostState::new();

    // --- 4. CREATE ALL CHANNELS + SPAWN BRAIN ---
    let bus = bus::AppBus::create(&config);

    // --- 5. SETUP TRAY ---
    let tray_components = tray::setup_tray("assets/icon.png");

    // --- 6. LOAD SKIN FROM CONFIG ---
    let is_zip = config.skin.path.ends_with(".zip");
    let (skin_width, skin_height, animated_skin, persona_meta, skin_load_rx) = if is_zip {
        let meta = AnimatedSkin::load_meta_from_zip(&config.skin.path)
            .unwrap_or_else(|e| {
                log::error!("Failed to load persona meta '{}': {}", config.skin.path, e);
                panic!("Could not load persona meta");
            });
        let dims = meta.still_image.as_ref()
            .map(|s| s.dimensions())
            .unwrap_or((200, 200));
        log::info!(
            "Loaded persona meta '{}': {}x{}, loading: \"{}\"",
            meta.name, dims.0, dims.1, meta.loading_text
        );

        let zip_path = config.skin.path.clone();
        let fps = config.skin.fps;
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            log::info!("Background: loading animation frames from zip...");
            match AnimatedSkin::from_zip(&zip_path, fps) {
                Ok((skin, _meta)) => {
                    log::info!("Background: animation loading complete");
                    let _ = tx.send(skin);
                }
                Err(e) => {
                    log::error!("Background: failed to load animation: {}", e);
                }
            }
        });

        (dims.0, dims.1, None, Some(meta), Some(rx))
    } else if config.skin.animated {
        let animated = AnimatedSkin::from_directory(&config.skin.path, config.skin.fps)
            .unwrap_or_else(|e| {
                log::error!("Failed to load animated skin '{}': {}", config.skin.path, e);
                panic!("Could not load animated skin");
            });
        let dims = animated.dimensions().unwrap_or((200, 200));
        log::info!("Loaded animated skin: {}x{} at {}fps", dims.0, dims.1, config.skin.fps);
        (dims.0, dims.1, Some(animated), None, None)
    } else {
        let skin_data = skin(&config.skin.path).unwrap_or_else(|e| {
            log::error!("Failed to load skin '{}': {}", config.skin.path, e);
            panic!("Could not load skin image");
        });
        (skin_data.width(), skin_data.height(), None, None, None)
    };

    // --- 7. CREATE CHAT WINDOW ---
    let chat_size = [config.chat.size[0], skin_height];
    let assistant_name = persona_meta.as_ref().map(|m| m.nick.clone());
    let chat_win = windows::chat_window::ChatWindow::new(
        &event_loop,
        bus.chat_rx,
        None,
        chat_size,
        assistant_name.clone(),
        ghost_state.clone(),
        &config.chat,
        bus.brain_tx.clone(),
        bus.brain_rx,
    );
    log::info!("Chat window created (hidden) with size {:?}", chat_size);

    // --- 8. CREATE LOG WINDOW (experiment) ---
    let log_win = windows::log_window::LogWindow::new(
        &event_loop,
        bus.log_rx,
        None,
        chat_size,
        assistant_name,
        ghost_state.clone(),
        &config.chat,
        None, // log window does not use brain yet
        None,
    );
    log::info!("Log window created (hidden)");

    // --- 9. CREATE CALLOUT CHANNEL + WINDOW ---
    let callout_offset = windows::callout_window::calculate_callout_offset(&config, skin_width, skin_height);
    let callout_size = windows::callout_window::calculate_callout_size(&config);
    log::info!("Callout offset: {:?}, size: {:?}", callout_offset, callout_size);

    let callout_window = GhostWindowBuilder::new()
        .with_size(callout_size.0, callout_size.1)
        .with_always_on_top(true)
        .with_draggable(false)
        .with_click_through(true)
        .with_alpha_hit_test(false)
        .with_opacity_focused(1.0)
        .with_opacity_unfocused(1.0)
        .with_title("Ghost Callout")
        .build(&event_loop)
        .expect("Failed to create callout window");

    // --- 10. CREATE MAIN GHOST WINDOW ---
    let mut window_builder = GhostWindowBuilder::new()
        .with_size(skin_width, skin_height)
        .with_always_on_top(true)
        .with_draggable(true)
        .with_click_through(false)
        .with_alpha_hit_test(true)
        .with_opacity_focused(1.0)
        .with_opacity_unfocused(0.7)
        .with_title("Ghost");

    if !config.skin.animated && !is_zip {
        let skin_data = skin(&config.skin.path).unwrap();
        window_builder = window_builder.with_skin_data(&skin_data);
    }

    let main_window = window_builder
        .build(&event_loop)
        .expect("Failed to create main window");

    // --- SET STARTUP POSITION ---
    if let Some(monitor) = main_window.window().current_monitor() {
        let mon_size = monitor.size();
        let win_size = main_window.window().outer_size();
        let (start_x, start_y) = config.window.calculate_position(
            mon_size.width,
            mon_size.height,
            win_size.width,
            win_size.height,
        );
        main_window.set_position(start_x, start_y);
        log::info!(
            "Main window startup position: ({}, {}) [{}] on {}x{} monitor, window {}x{} physical",
            start_x, start_y, config.window.position,
            mon_size.width, mon_size.height,
            win_size.width, win_size.height
        );
    }

    // --- 11. CONTROLLER WINDOW (experiment) ---
    let ctrl_skin_data = skin(&config.skin.path).unwrap();
    let _ctrl_window = GhostWindowBuilder::new()
        .with_size(skin_width, skin_height)
        .with_always_on_top(true)
        .with_draggable(true)
        .with_click_through(false)
        .with_alpha_hit_test(true)
        .with_opacity_focused(1.0)
        .with_opacity_unfocused(0.7)
        .with_title("Controller: Ghost")
        .with_skin_data(&ctrl_skin_data)
        .build(&event_loop)
        .expect("Failed to create controller window");

    // --- 12. CREATE APPS ---
    let main_app = windows::main_window::App::new(
        config.clone(),
        skin_width,
        skin_height,
        bus.callout_tx,
        animated_skin,
        persona_meta,
        skin_load_rx,
        ghost_state.clone(),
    );

    // Coordinator wraps main_app and owns tray event handling.
    let coordinator = coordinator::Coordinator::new(
        main_app,
        tray_components.menu_ids,
        bus.chat_tx,
    );

    let callout_window_app = windows::callout_window::CalloutWindowApp::new(&config, bus.callout_rx);

    log::info!("Ghost app started");

    // --- 13. COMPUTE SNAP OFFSETS ---
    let chat_offset = config.chat.calculate_offset_with_size(skin_width, skin_height, chat_size);
    log::info!("Chat window offset: {:?}", chat_offset);

    let scale_factor = main_window.window().scale_factor();
    let scaled_extra_offset = [
        (chat_offset[0] as f64 * scale_factor) as i32,
        (chat_offset[1] as f64 * scale_factor) as i32,
    ];
    ghost_state.set_snap_config(vars::SnapConfig { scaled_extra_offset });

    if let Some((x, y)) = main_window.outer_position() {
        ghost_state.set_main_pos(x, y);
    }

    // --- 14. RUN ---
    ghost_ui::run_with_app_callout_and_extra(
        main_window,
        callout_window,
        callout_offset,
        event_loop,
        coordinator,
        callout_window_app,
        Some(chat_win),
    );
}

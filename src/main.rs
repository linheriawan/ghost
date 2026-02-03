use ghost_ui::{icon_bytes, skin_bytes, EventLoop, GhostWindowBuilder};

fn main() {
    // Initialize logging
    env_logger::init();

    let event_loop = EventLoop::new();

    // --- 1. SETUP ICONS (tray + dock) ---
    let mut app_icon = icon_bytes(include_bytes!("../assets/icon.png"));
    if let Err(e) = app_icon.setup_all() {
        log::error!("Failed to setup icons: {}", e);
    }

    // --- 2. LOAD SKIN ---
    let skin_data = skin_bytes(include_bytes!("../assets/Tania.png"))
        .expect("Failed to load skin");

    // --- 3. CREATE GHOST WINDOW ---
    let window = GhostWindowBuilder::new()
        .with_size(skin_data.width(), skin_data.height())
        .with_always_on_top(true)
        .with_draggable(true)
        .with_click_through(false)
        .with_alpha_hit_test(true) // Clicks on transparent areas pass through
        .with_opacity_focused(1.0) // Fully visible when focused
        .with_opacity_unfocused(0.5) // Semi-transparent when unfocused
        .with_title("Ghost")
        .with_skin_data(&skin_data)
        .build(&event_loop)
        .expect("Failed to create ghost window");

    // Run the event loop
    ghost_ui::run(window, event_loop);
}

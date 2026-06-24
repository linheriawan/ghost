//! Ghost - Desktop mascot with callout bubbles

mod actions;
mod brain;
mod bus;
mod config;
mod coordinator;
mod runner;
mod skin;
mod tray;
mod ui;
mod vars;
mod windows;

use ghost_ui::{EventLoop, GhostWindowBuilder};

fn main() {
    env_logger::init();

    // --- 1. CONFIGURATION ---
    let config = config::Config::load_default().unwrap_or_else(|e| {
        log::error!("Failed to load ui.toml: {}", e);
        panic!("Please create ui.toml configuration file");
    });
    log::info!("Skin: {}, animated: {}, buttons: {}", config.skin.path, config.skin.animated, config.buttons.len());

    // --- 2. EVENT LOOP + SHARED STATE ---
    let event_loop = EventLoop::new();
    let ghost_state = vars::GhostState::new();

    // --- 3. CHANNELS + BRAIN ---
    let mut bus = bus::AppBus::create(&config);

    // --- 4. TRAY ---
    let tray_components = tray::setup_tray("assets/icon.png");

    // --- 5. LOAD SKIN ---
    let skin_bundle = skin::load(&config.skin);
    let (skin_width, skin_height) = (skin_bundle.width, skin_bundle.height);
    let assistant_name = skin_bundle.persona.as_ref().map(|m| m.nick.clone());

    println!("\x1b[1;105;34m ASSISTANT {:?} \x1b[0m", &assistant_name);

    // --- 6. EXTRA WINDOWS ---
    let chat_size = [config.chat.size[0], skin_height];
    let chat_win = windows::chat_window::ChatWindow::new(
        &event_loop, &mut bus, chat_size, assistant_name.clone(), ghost_state.clone(), &config.chat,
    );
    let log_win = windows::log_window::LogWindow::new(
        &event_loop, &mut bus, chat_size, assistant_name.clone(), ghost_state.clone(), &config.chat,
    );
    let ctrl_win = windows::control_window::ControlWindow::new(&event_loop, &mut bus);

    // --- 7. CALLOUT WINDOW ---
    let callout_win = windows::callout_window::CalloutWindow::new(&config, &event_loop, &mut bus);

    // --- 8. MAIN GHOST WINDOW ---
    let mut window_builder = GhostWindowBuilder::new()
        .with_size(skin_width, skin_height)
        .with_always_on_top(true)
        .with_draggable(true)
        .with_click_through(false)
        .with_alpha_hit_test(true)
        .with_opacity_focused(1.0)
        .with_opacity_unfocused(0.7)
        .with_title("Ghost");
    if let Some(ref data) = skin_bundle.static_data {
        window_builder = window_builder.with_skin_data(data);
    }
    let main_window = window_builder.build(&event_loop).expect("Failed to create main window");

    if let Some(monitor) = main_window.window().current_monitor() {
        let mon = monitor.size();
        dbg!(&mon);
        let win = main_window.window().outer_size();
        dbg!(&win);
        let (x, y) = config.window.calculate_position(mon.width, mon.height, win.width, win.height);
        main_window.set_position(x, y);
        println!("Main window at ({}, {}) [{}]", &x, &y, &config.window.position);
    }

    // --- 9. APPS ---
    let main_app = windows::main_window::App::new(config.clone(), skin_bundle, &mut bus, ghost_state.clone());
    let coordinator = coordinator::Coordinator::new(main_app, tray_components.menu_ids, bus.senders());

    // --- 10. SNAP CONFIG ---
    let chat_offset = config.chat.calculate_offset_with_size(skin_width, skin_height, chat_size);
    let scale_factor = main_window.window().scale_factor();
    ghost_state.set_snap_config(vars::SnapConfig {
        scaled_extra_offset: [
            (chat_offset[0] as f64 * scale_factor) as i32,
            (chat_offset[1] as f64 * scale_factor) as i32,
        ],
    });
    if let Some((x, y)) = main_window.outer_position() { ghost_state.set_main_pos(x, y); }

    // --- 11. RUN ---
    runner::run(
        main_window,
        event_loop,
        coordinator,
        vec![
            Box::new(callout_win) as Box<dyn ghost_ui::ExtraWindow>,
            Box::new(chat_win) as Box<dyn ghost_ui::ExtraWindow>,
            Box::new(log_win) as Box<dyn ghost_ui::ExtraWindow>,
            Box::new(ctrl_win) as Box<dyn ghost_ui::ExtraWindow>,
        ],
    );
}

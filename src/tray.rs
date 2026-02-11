use std::path::Path;
use tray_icon::menu::{MenuEvent, MenuId};
use tray_icon::{
    menu::{Menu, MenuItem, PredefinedMenuItem, Submenu},
    TrayIcon, TrayIconBuilder,
};

/// Menu item IDs for handling events
pub struct MenuIds {
    pub open_chat: MenuId,
    pub talk: MenuId,
    pub idle: MenuId,
    pub quit: MenuId,
}

pub struct TrayComponents {
    /// Keep tray icon alive (dropping it removes the icon)
    #[allow(dead_code)]
    pub tray_icon: TrayIcon,
    pub menu_ids: MenuIds,
}

/// Commands that can be sent from tray menu
#[derive(Debug, Clone)]
pub enum TrayCommand {
    OpenChat,
    SetState(String), // "idle", "talk", etc.
    Quit,
}

pub fn setup_tray(icon_path: &str) -> TrayComponents {
    let tray_menu = Menu::new();

    // 1. Create a Submenu for animation states
    let state_submenu = Submenu::new("State", true);
    let talk_item = MenuItem::new("Talk", true, None);
    let idle_item = MenuItem::new("Idle", true, None);

    // Store IDs before moving items
    let talk_id = talk_item.id().clone();
    let idle_id = idle_item.id().clone();

    state_submenu
        .append_items(&[&talk_item, &idle_item])
        .unwrap();

    // 2. Main Menu Items
    let open_chat_item = MenuItem::new("Open Chat Window", true, None);
    let quit_item = MenuItem::new("Quit", true, None);

    let open_chat_id = open_chat_item.id().clone();
    let quit_id = quit_item.id().clone();

    // 3. Assemble everything into the main menu
    tray_menu
        .append_items(&[
            &open_chat_item,
            &PredefinedMenuItem::separator(),
            &state_submenu,
            &PredefinedMenuItem::separator(),
            &quit_item,
        ])
        .unwrap();

    // 4. Build the Icon
    let icon = load_icon(Path::new(icon_path));

    let tray_icon = TrayIconBuilder::new()
        .with_menu(Box::new(tray_menu))
        .with_tooltip("Ghost")
        .with_icon(icon)
        .build()
        .unwrap();

    let menu_ids = MenuIds {
        open_chat: open_chat_id,
        talk: talk_id,
        idle: idle_id,
        quit: quit_id,
    };

    TrayComponents { tray_icon, menu_ids }
}

/// Check for menu events and return command if any
pub fn poll_menu_event(menu_ids: &MenuIds) -> Option<TrayCommand> {
    if let Ok(event) = MenuEvent::receiver().try_recv() {
        if event.id == menu_ids.open_chat {
            return Some(TrayCommand::OpenChat);
        } else if event.id == menu_ids.talk {
            return Some(TrayCommand::SetState("talk".to_string()));
        } else if event.id == menu_ids.idle {
            return Some(TrayCommand::SetState("idle".to_string()));
        } else if event.id == menu_ids.quit {
            return Some(TrayCommand::Quit);
        }
    }
    None
}

fn load_icon(path: &Path) -> tray_icon::Icon {
    let image = image::open(path)
        .expect("Failed to open icon path")
        .into_rgba8();
    let (width, height) = image.dimensions();
    let rgba = image.into_raw();
    
    tray_icon::Icon::from_rgba(rgba, width, height).expect("Failed to create icon")
} // <-- Ensure this brace is here to close the function!
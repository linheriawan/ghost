// src/tray.rs
use tray_icon::{
    menu::{Menu, MenuEvent, MenuEventReceiver, CheckMenuItem,MenuItem, MenuId}, 
    TrayIconBuilder, TrayIcon
};

pub struct TrayHandler {
    _tray_icon: TrayIcon,
    menu_event_receiver: MenuEventReceiver,
    eq_item: CheckMenuItem,
    pl_item: CheckMenuItem,
    quit_item: MenuItem,
}

impl TrayHandler {
    pub fn new() -> Self {
        let tray_menu = Menu::new();
        let eq_item = CheckMenuItem::new("Equalizer", true, true, None);
        let pl_item = CheckMenuItem::new("Playlist", true, true, None);
        let quit_item = tray_icon::menu::MenuItem::new("Quit", true, None);

        tray_menu.append_items(&[
            &eq_item,
            &pl_item,
            &tray_icon::menu::PredefinedMenuItem::separator(),
            &quit_item,
        ]).unwrap();

        let tray_icon = TrayIconBuilder::new()
            .with_menu(Box::new(tray_menu))
            .with_icon(load_icon())
            .build()
            .unwrap();

        Self { 
            _tray_icon: tray_icon,
            menu_event_receiver: MenuEvent::receiver().clone(),
            eq_item,
            pl_item,
            quit_item,
        }
    }

    // pub fn update(&self) -> Option<MenuId> {
        // if let Ok(event) = self.menu_event_receiver.try_recv() {
        //     return Some(event.id);
        // }
        // None
    // }
    pub fn update(&self, ctx: &egui::Context) -> Option<MenuId> {
        if let Ok(event) = self.menu_event_receiver.try_recv() {
            // Force the GUI to wake up because we have an external event
            ctx.request_repaint(); 
            return Some(event.id);
        }
        None
    }

    // Fixed: Added .clone() to convert &MenuId to MenuId
    pub fn eq_id(&self) -> MenuId { self.eq_item.id().clone() }
    pub fn pl_id(&self) -> MenuId { self.pl_item.id().clone() }
    pub fn quit_id(&self) -> MenuId { self.quit_item.id().clone() }

    pub fn sync_states(&self, eq_visible: bool, pl_visible: bool) {
        self.eq_item.set_checked(eq_visible);
        self.pl_item.set_checked(pl_visible);
    }
}

fn load_icon() -> tray_icon::Icon {
    let path = std::path::Path::new("assets/icon.png");
    let image = image::open(path).expect("Icon not found").into_rgba8();
    let (width, height) = image.dimensions();
    tray_icon::Icon::from_rgba(image.into_raw(), width, height).unwrap()
}
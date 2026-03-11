// src/tray.rs
use tray_item::{TrayItem, IconSource};
use crate::Message;
use fltk::app;

pub fn setup_tray(s: app::Sender<Message>) -> TrayItem {
    // Wrap the string in IconSource
    let path = std::path::Path::new("assets/icon.png");
    if !path.exists() {
        println!("Warning: Icon not found at {:?}", path.canonicalize());
    }
    let icon = IconSource::Resource("assets/icon.png");
    
    let mut tray = TrayItem::new("Rustamp", icon).unwrap();

    tray.add_menu_item("Toggle Equalizer", move || {
        s.send(Message::ToggleEQ);
    }).unwrap();

    tray.add_menu_item("Toggle Playlist", move || {
        s.send(Message::TogglePL);
    }).unwrap();

    tray.add_menu_item("Quit", move || {
        s.send(Message::Quit);
    }).unwrap();

    tray
}
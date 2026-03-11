use fltk::{prelude::*, *};

pub struct PlaylistWindow {
    pub win: window::Window,
}

impl PlaylistWindow {
    pub fn new() -> Self {
        let mut win = window::Window::default()
            .with_size(300, 150)
            .with_label("Playlist");
        
        win.set_border(false);
        win.set_color(enums::Color::from_rgb(40, 40, 40));
        
        frame::Frame::new(0, 0, 300, 30, "PLAYLIST")
            .with_align(enums::Align::Center);

        let mut pack = group::Pack::new(10, 30, 280, 160, "");
        pack.set_spacing(5);
        
        for i in 1..6 {
            // Widgets inside a Pack only care about the height (25) 
            // the Pack handles their X and Y.
            let mut btn = button::Button::default().with_size(280, 25);
            btn.set_label(&format!("{:02}. Rust_Beats_Vol_{}.mp3", i, i));
            btn.set_label_color(enums::Color::White);
            btn.set_frame(enums::FrameType::FlatBox);
            btn.set_color(enums::Color::from_rgb(50, 50, 50));
            btn.set_label_size(12);
        }
        
        pack.end();
        win.end();
        Self { win }
    }
}
// src/equaliser.rs
use fltk::{prelude::*, *};

pub struct EqualiserWindow {
    pub win: window::Window,
}

impl EqualiserWindow {
    pub fn new() -> Self {
        let mut win = window::Window::default()
            .with_size(300, 150)
            .with_label("Equalizer");
        
        // Remove title bar
        win.set_border(false);
        
        win.set_color(enums::Color::from_rgb(40, 40, 40));
        
        let mut frame = frame::Frame::new(0, 0, 300, 30, "EQUALIZER");
        frame.set_label_color(enums::Color::White);

        win.end();
        Self { win }
    }
}
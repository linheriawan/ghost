use fltk::{prelude::*, *};
use crate::Message;

pub struct PlayerWindow {
    pub win: window::Window,
}

impl PlayerWindow {
    pub fn new(s: app::Sender<Message>) -> Self {
        let mut win = window::Window::default()
            .with_size(300, 100);
        // win.set_type(window::WindowType::Tooltip); 
        // win.set_type(window::WindowType::Toplevel); 
        // Remove macOS title bar for that Winamp look
        win.set_border(false);
        win.set_color(enums::Color::from_rgb(40, 40, 40));

        let mut btn_eq = button::Button::new(20, 40, 60, 30, "EQ");
        btn_eq.emit(s, Message::ToggleEQ);

        let mut btn_pl = button::Button::new(90, 40, 60, 30, "PL");
        btn_pl.emit(s, Message::TogglePL);

        let mut btn_quit = button::Button::new(220, 40, 60, 30, "Quit");
        btn_quit.emit(s, Message::Quit);

        // Make the window draggable despite having no border
        let mut x = 0;
        let mut y = 0;
        win.handle(move |w, ev| match ev {
            enums::Event::Push => {
                let coords = app::event_coords();
                x = coords.0;
                y = coords.1;
                true
            }
            enums::Event::Drag => {
                let new_x = app::event_x_root() - x;
                let new_y = app::event_y_root() - y;
                w.set_pos(new_x, new_y);
                
                // Force a redraw/wake to ensure the main loop 
                // sees the movement immediately for snapping
                app::awake(); 
                true
            }
            _ => false,
        });

        win.end();
        Self { win }
    }
}
use fltk::{prelude::*, *};
mod player;
mod equaliser;
mod playlist;
mod tray;

#[derive(Clone, Copy)]
pub enum Message {
    ToggleEQ,
    TogglePL,
    Quit,
}

fn main() {
    let app = app::App::default();
    let (s, r) = app::channel::<Message>();

    // 1. Create Windows
    let mut player_win = player::PlayerWindow::new(s);
    let mut eq_win = equaliser::EqualiserWindow::new();
    let mut pl_win = playlist::PlaylistWindow::new();

    // 2. Setup Tray
    let _tray = tray::setup_tray(s);

    player_win.win.show();
    eq_win.win.show();
    pl_win.win.show();

    // 3. Main Event Loop
    while app.wait() {
        if let Some(msg) = r.recv() {
            match msg {
                Message::ToggleEQ => {
                    if eq_win.win.visible() { eq_win.win.hide(); } 
                    else { eq_win.win.show(); }
                }
                Message::TogglePL => {
                    if pl_win.win.visible() { pl_win.win.hide(); } 
                    else { pl_win.win.show(); }
                },
                Message::Quit => app.quit(),
            }
        }

        if player_win.win.shown() {
            // 1. Position EQ below Player
            if eq_win.win.visible() {
                eq_win.win.set_pos(player_win.win.x(), player_win.win.y() + player_win.win.h()+2);
            }
            
            // 2. Position Playlist below Equalizer
            if pl_win.win.visible() {
                // If EQ is hidden, the Playlist "jumps up" to the Player. 
                // If EQ is shown, Playlist sticks to EQ.
                let anchor_y = if eq_win.win.visible() {
                    eq_win.win.y() + eq_win.win.h()
                } else {
                    player_win.win.y() + player_win.win.h()
                };
                
                pl_win.win.set_pos(player_win.win.x(), anchor_y+2);
            }
        }
    }
    app.run().unwrap();
}
use eframe::egui;
mod player;
mod equaliser;
mod playlist;
mod tray;

pub struct WinampApp {
    pub show_eq: bool,
    pub show_playlist: bool,
    pub main_pos: egui::Pos2,
    pub tray_handler: tray::TrayHandler,
}

impl WinampApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        Self {
            show_eq: true,
            show_playlist: true,
            main_pos: egui::pos2(200.0, 200.0),
            tray_handler: tray::TrayHandler::new(),
        }
    }
}

// src/main.rs

impl eframe::App for WinampApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        ctx.request_repaint();
        // --- THIS IS THE SYSTRAY EVENT HANDLER ---
        if let Some(clicked_id) = self.tray_handler.update(ctx) {
            // Check if the Equalizer item was clicked in the tray
            if clicked_id == self.tray_handler.eq_id() {
                self.show_eq = !self.show_eq;
                println!("Tray toggled EQ: {}", self.show_eq);
            } 
            // Check if the Playlist item was clicked in the tray
            else if clicked_id == self.tray_handler.pl_id() {
                self.show_playlist = !self.show_playlist;
                println!("Tray toggled Playlist: {}", self.show_playlist);
            }
            if clicked_id == self.tray_handler.quit_id() {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }

        // Keep the tray checkmarks in sync with the actual variables
        self.tray_handler.sync_states(self.show_eq, self.show_playlist);

        // --- WINDOW RENDERING ---
        
        // Main Player
        player::show(ctx, self, _frame);

        // Only show if the boolean is true
        if self.show_eq {
            equaliser::show(ctx, self);
        }

        if self.show_playlist {
            playlist::show(ctx, self);
        }
    }
}

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_always_on_top() // Keeps windows grouped together visually
            .with_inner_size([300.0, 150.0])
            .with_decorations(false), // Winamp style: no title bar
        ..Default::default()
    };
    
    eframe::run_native(
        "Winamp Rust",
        options,
        Box::new(|cc| Box::new(WinampApp::new(cc))),
    )
}
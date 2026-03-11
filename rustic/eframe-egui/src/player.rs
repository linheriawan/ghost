// src/player.rs
use eframe::egui;
use crate::WinampApp;

pub fn show(ctx: &egui::Context, app: &mut WinampApp, _frame: &mut eframe::Frame) {
    egui::CentralPanel::default().show(ctx, |ui| {
        ui.heading("📻 RUSTAMP");
        
        // Custom Drag Area (Since decorations are off)
        let title_bar_response = ui.interact(ui.max_rect(), ui.id().with("drag"), egui::Sense::drag());
        if title_bar_response.dragged() {
            ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
        }

        ui.horizontal(|ui| {
            if ui.button("EQ").clicked() { app.show_eq = !app.show_eq; }
            if ui.button("PL").clicked() { app.show_playlist = !app.show_playlist; }
        });

        // Track position for syncing
        if let Some(pos) = ctx.input(|i| i.viewport().outer_rect) {
            app.main_pos = pos.min;
        }
        
        if ui.button("Quit").clicked() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    });
}
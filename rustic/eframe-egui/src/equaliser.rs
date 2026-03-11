// src/equaliser.rs
use eframe::egui;
use crate::WinampApp;

pub fn show(ctx: &egui::Context, app: &mut WinampApp) {
    // Relative positioning: 155 pixels below main window
    let eq_pos = egui::pos2(app.main_pos.x, app.main_pos.y + 150.0);

    ctx.show_viewport_immediate(
        egui::ViewportId::from_hash_of("equalizer_window"),
        egui::ViewportBuilder::default()
            .with_title("Equalizer")
            .with_inner_size([300.0, 100.0])
            .with_position(eq_pos)
            .with_decorations(false),
        |ctx, _class| {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.label("EQUALIZER");
                ui.add(egui::Slider::new(&mut 0.5, 0.0..=1.0));
                
                // Allow dragging this window too
                if ui.interact(ui.max_rect(), ui.id(), egui::Sense::drag()).dragged() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
                }
            });
        },
    );
}
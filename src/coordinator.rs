use ghost_ui::{GhostApp, GhostEvent, GpuResources, Skin};

use crate::bus::AppSenders;
use crate::tray::{self, MenuIds, TrayCommand};
use crate::windows::chat_window::ChatWindowCommand;
use crate::windows::control_window::ControlWindowCommand;
use crate::windows::log_window::LogWindowCommand;
use crate::windows::main_window;

/// Owns application-level concerns (tray, quit signal) and delegates all
/// visual/rendering work to the inner main_window::App.
pub struct Coordinator {
    pub inner: main_window::App,
    menu_ids: MenuIds,
    bus: AppSenders,
    should_quit: bool,
}

impl Coordinator {
    pub fn new(inner: main_window::App, menu_ids: MenuIds, bus: AppSenders) -> Self {
        Self { inner, menu_ids, bus, should_quit: false }
    }

}

impl GhostApp for Coordinator {
    fn init_gpu(&mut self, gpu: GpuResources<'_>) { self.inner.init_gpu(gpu); }

    fn update(&mut self, delta: f32) {
        self.inner.update(delta);
        if let Some(cmd) = tray::poll_menu_event(&self.menu_ids) {
            match cmd {
                TrayCommand::OpenChat => { let _ = self.bus.chat_tx.send(ChatWindowCommand::Show); }
                TrayCommand::OpenCtrl => { let _ = self.bus.ctrl_tx.send(ControlWindowCommand::Show); }
                TrayCommand::OpenLog  => { let _ = self.bus.log_tx.send(LogWindowCommand::Show); }
                TrayCommand::SetState(s) => {
                    if let Some(ref mut skin) = self.inner.animated_skin {
                        let state: ghost_ui::AnimationState = ghost_ui::AnimationState::from_str(&s);
                        if skin.has_state(state) { skin.set_state(state); }
                    }
                }
                TrayCommand::Quit => { self.should_quit = true; }
            }
        }
    }
    fn should_quit(&self) -> bool { self.should_quit }
    fn on_event(&mut self, event: GhostEvent) { self.inner.on_event(event); }
    fn buttons(&self) -> Vec<&ghost_ui::Button> { self.inner.buttons() }
    fn buttons_mut(&mut self) -> Vec<&mut ghost_ui::Button> { self.inner.buttons_mut() }
    fn current_skin(&self) -> Option<&Skin> { self.inner.current_skin() }
    fn target_fps(&self) -> f32 { self.inner.target_fps() }

    fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        viewport: [f32; 2],
        scale_factor: f32,
        opacity: f32,
    ) {
        self.inner.prepare(device, queue, viewport, scale_factor, opacity);
    }

    fn hit_test(&self, x: f32, y: f32) -> bool { self.inner.hit_test(x, y) }

    fn render_layers<'a>(
        &'a mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        viewport: [f32; 2],
        render_pass: &mut wgpu::RenderPass<'a>,
    ) {
        self.inner.render_layers(device, queue, viewport, render_pass);
    }
}

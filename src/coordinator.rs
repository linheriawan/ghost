use ghost_ui::{GhostApp, GhostEvent, GpuResources, Skin};

use crate::tray::{self, MenuIds, TrayCommand};
use crate::windows::chat_window::{ChatSender, ChatWindowCommand};
use crate::windows::main_window;

/// Owns application-level concerns (tray, quit signal) and delegates all
/// visual/rendering work to the inner main_window::App.
pub struct Coordinator {
    pub inner: main_window::App,
    menu_ids: MenuIds,
    chat_tx: ChatSender,
    should_quit: bool,
}

impl Coordinator {
    pub fn new(inner: main_window::App, menu_ids: MenuIds, chat_tx: ChatSender) -> Self {
        Self {
            inner,
            menu_ids,
            chat_tx,
            should_quit: false,
        }
    }

    fn poll_tray(&mut self) {
        let Some(cmd) = tray::poll_menu_event(&self.menu_ids) else {
            return;
        };
        match cmd {
            TrayCommand::OpenChat => {
                let _ = self.chat_tx.send(ChatWindowCommand::Show);
                log::info!("Tray: open chat");
            }
            TrayCommand::OpenCtrl | TrayCommand::OpenLog => {
                // TODO: wire log_tx / ctrl_tx from AppBus once those windows
                // have their own command senders.
                log::info!("Tray: open ctrl/log (not yet wired)");
            }
            TrayCommand::SetState(s) => {
                self.inner.set_animation_state(&s);
            }
            TrayCommand::Quit => {
                log::info!("Tray: quit");
                self.should_quit = true;
            }
        }
    }
}

impl GhostApp for Coordinator {
    fn init_gpu(&mut self, gpu: GpuResources<'_>) {
        self.inner.init_gpu(gpu);
    }

    fn update(&mut self, delta: f32) {
        self.inner.update(delta);
        self.poll_tray();
    }

    fn should_quit(&self) -> bool {
        self.should_quit
    }

    fn on_event(&mut self, event: GhostEvent) {
        self.inner.on_event(event);
    }

    fn buttons(&self) -> Vec<&ghost_ui::Button> {
        self.inner.buttons()
    }

    fn buttons_mut(&mut self) -> Vec<&mut ghost_ui::Button> {
        self.inner.buttons_mut()
    }

    fn current_skin(&self) -> Option<&Skin> {
        self.inner.current_skin()
    }

    fn target_fps(&self) -> f32 {
        self.inner.target_fps()
    }

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

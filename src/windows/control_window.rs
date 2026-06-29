//! Control window — ghost_ui window via GhostFollower<ControlApp>.

use std::sync::mpsc::{channel, Receiver, Sender};

use ghost_ui::{skin, ButtonStyle, GhostApp, GhostEvent, GhostFollower, GhostWindowBuilder};
use tao::event_loop::EventLoop;

use super::callout_window::CalloutSender;
use crate::bus::AppBus;
use crate::ui;

#[derive(Debug)]
pub enum ControlWindowCommand { Show, Hide, Toggle }

pub type ControlSender = Sender<ControlWindowCommand>;
pub type ControlReceiver = Receiver<ControlWindowCommand>;
pub fn create_control_channel() -> (ControlSender, ControlReceiver) { channel() }

/// GhostFollower handles all ExtraWindow boilerplate; only app logic lives here.
pub struct ControlApp {
    receiver: ControlReceiver,
    #[allow(dead_code)]
    callout_tx: CalloutSender,
    widgets: ui::Widgets,
    pending_visible: Option<bool>,
    is_visible: bool,
}

impl ControlApp {
    fn new(receiver: ControlReceiver, callout_tx: CalloutSender) -> Self {
        let widgets = ui::Widgets::new(vec![
            ui::AnyWidget::ButtonImage(ui::make_btn_image("win_close", "assets/dot-r.png", [10.0, 20.0]).unwrap()),
            ui::AnyWidget::ButtonImage(ui::make_btn_image("win_min",   "assets/dot-y.png", [45.0, 20.0]).unwrap()),
            ui::AnyWidget::ButtonImage(ui::make_btn_image("win_max",   "assets/dot-g.png", [80.0, 20.0]).unwrap()),
            ui::AnyWidget::Button(ui::mk_btn("greet", "Greet", [10.0, 10.0], [60.0, 28.0], ButtonStyle::primary())),
            ui::AnyWidget::Label(ui::make_label("status_bar", "A text in the bottom", [10.0, 60.0], [160.0, 24.0], ghost_ui::LabelStyle::with_background())),
            ui::AnyWidget::Marquee(ui::make_marquee("announcement", "this is a marquee scrolling text", [10.0, 90.0], [160.0, 24.0], ghost_ui::LabelStyle::with_background(), 30.0)),
        ]);
        Self { receiver, callout_tx, widgets, pending_visible: None, is_visible: false }
    }
}

impl GhostApp for ControlApp {
    fn update(&mut self, _delta: f32) {
        while let Ok(cmd) = self.receiver.try_recv() {
            self.pending_visible = Some(match cmd {
                ControlWindowCommand::Show => true,
                ControlWindowCommand::Hide => false,
                ControlWindowCommand::Toggle => !self.is_visible,
            });
        }
    }
    fn take_visibility(&mut self) -> Option<bool> { self.pending_visible.take() }
    fn on_visible_changed(&mut self, visible: bool) { self.is_visible = visible; }
    fn on_event(&mut self, _event: GhostEvent) {}

    fn widget_list(&self) -> &[ghost_ui::AnyWidget] { &self.widgets.0 }
    fn widgets_mut(&mut self) -> Option<&mut Vec<ghost_ui::AnyWidget>> { Some(&mut self.widgets.0) }
}

pub type ControlWindow = GhostFollower<ControlApp>;

pub fn create_control_window(event_loop: &EventLoop<()>, bus: &mut AppBus) -> ControlWindow {
    let receiver = bus.ctrl_rx.take().expect("ctrl_rx already consumed");
    let callout_tx = bus.callout_tx.clone();
    let skin_data = skin("assets/quad_arrow.png").unwrap_or_else(|e| {
        log::error!("Failed to load control skin: {}", e);
        panic!("Could not load assets/quad_arrow.png");
    });
    let window = GhostWindowBuilder::new()
        .with_size(312, 180)
        .with_always_on_top(true)
        .with_draggable(true)
        .with_click_through(false)
        .with_alpha_hit_test(true)
        .with_opacity_focused(1.0)
        .with_opacity_unfocused(0.7)
        .with_title("Ghost Controller")
        .with_skin_data(&skin_data)
        .build(event_loop)
        .expect("Failed to create controller window");
    GhostFollower::new(window, ControlApp::new(receiver, callout_tx))
}

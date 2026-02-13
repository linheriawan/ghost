use parking_lot::RwLock;
use std::sync::Arc;

// -- Shared types --

#[derive(Clone, Copy, Debug, Default)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum WinState {
    Focused,
    Behind,
    NotRunning,
}

#[derive(Clone, Debug)]
pub struct PersonaState {
    pub name: String,
    pub animation: String,
    pub opacity: f32,
}

#[derive(Clone, Debug)]
pub struct WindowState {
    pub rect: Rect,
    pub opacity: f32,
    pub state: WinState,
    pub visible: bool,
    pub snapped: bool,
    /// Last position we programmatically set (for snap/unsnap detection)
    pub last_set_pos: (i32, i32),
    /// Suppress false unsnap when OS fires Moved events during resize
    pub just_resized: bool,
}

#[derive(Clone, Debug)]
pub struct SnapConfig {
    /// Scaled extra window offset from main window (physical pixels)
    pub scaled_extra_offset: [i32; 2],
}

// -- Inner state (the actual data) --

struct Inner {
    pub persona: PersonaState,
    pub main_window: WindowState,
    pub chat_window: WindowState,
    pub callout_window: WindowState,
    pub snap_config: SnapConfig,
}

// -- GhostState (thread-safe shared handle) --

#[derive(Clone)]
pub struct GhostState(Arc<RwLock<Inner>>);

impl GhostState {
    pub fn new() -> Self {
        Self(Arc::new(RwLock::new(Inner {
            persona: PersonaState {
                name: String::new(),
                animation: "idle".into(),
                opacity: 1.0,
            },
            main_window: WindowState {
                rect: Rect::default(),
                opacity: 1.0,
                state: WinState::NotRunning,
                visible: true,
                snapped: false,
                last_set_pos: (0, 0),
                just_resized: false,
            },
            chat_window: WindowState {
                rect: Rect::default(),
                opacity: 1.0,
                state: WinState::NotRunning,
                visible: false,
                snapped: true,
                last_set_pos: (0, 0),
                just_resized: false,
            },
            callout_window: WindowState {
                rect: Rect::default(),
                opacity: 1.0,
                state: WinState::NotRunning,
                visible: false,
                snapped: true,
                last_set_pos: (0, 0),
                just_resized: false,
            },
            snap_config: SnapConfig {
                scaled_extra_offset: [0, 0],
            },
        })))
    }

    // -- Persona --

    pub fn persona(&self) -> PersonaState {
        self.0.read().persona.clone()
    }

    pub fn set_persona_animation(&self, animation: &str) {
        self.0.write().persona.animation = animation.to_string();
    }

    pub fn set_persona_opacity(&self, opacity: f32) {
        self.0.write().persona.opacity = opacity;
    }

    // -- Main window --

    pub fn main_window(&self) -> WindowState {
        self.0.read().main_window.clone()
    }

    pub fn main_pos(&self) -> (i32, i32) {
        let r = self.0.read().main_window.rect;
        (r.x, r.y)
    }

    pub fn main_size(&self) -> (u32, u32) {
        let r = self.0.read().main_window.rect;
        (r.width, r.height)
    }

    pub fn set_main_pos(&self, x: i32, y: i32) {
        let mut inner = self.0.write();
        inner.main_window.rect.x = x;
        inner.main_window.rect.y = y;
    }

    pub fn set_main_size(&self, w: u32, h: u32) {
        let mut inner = self.0.write();
        inner.main_window.rect.width = w;
        inner.main_window.rect.height = h;
    }

    pub fn set_main_state(&self, state: WinState) {
        self.0.write().main_window.state = state;
    }

    pub fn set_main_opacity(&self, opacity: f32) {
        self.0.write().main_window.opacity = opacity;
    }

    // -- Chat window --

    pub fn chat_window(&self) -> WindowState {
        self.0.read().chat_window.clone()
    }

    pub fn chat_visible(&self) -> bool {
        self.0.read().chat_window.visible
    }

    pub fn chat_snapped(&self) -> bool {
        self.0.read().chat_window.snapped
    }

    pub fn set_chat_visible(&self, v: bool) {
        self.0.write().chat_window.visible = v;
    }

    pub fn set_chat_snapped(&self, v: bool) {
        self.0.write().chat_window.snapped = v;
    }

    pub fn set_chat_pos(&self, x: i32, y: i32) {
        let mut inner = self.0.write();
        inner.chat_window.rect.x = x;
        inner.chat_window.rect.y = y;
    }

    pub fn chat_last_set_pos(&self) -> (i32, i32) {
        self.0.read().chat_window.last_set_pos
    }

    pub fn set_chat_last_set_pos(&self, x: i32, y: i32) {
        self.0.write().chat_window.last_set_pos = (x, y);
    }

    pub fn chat_just_resized(&self) -> bool {
        self.0.read().chat_window.just_resized
    }

    pub fn set_chat_just_resized(&self, v: bool) {
        self.0.write().chat_window.just_resized = v;
    }

    // -- Callout window --

    pub fn callout_visible(&self) -> bool {
        self.0.read().callout_window.visible
    }

    pub fn set_callout_visible(&self, v: bool) {
        self.0.write().callout_window.visible = v;
    }

    // -- Snap config --

    pub fn snap_config(&self) -> SnapConfig {
        self.0.read().snap_config.clone()
    }

    pub fn set_snap_config(&self, config: SnapConfig) {
        self.0.write().snap_config = config;
    }

    pub fn set_main_focused(&self, focused: bool) {
        self.0.write().main_window.state = if focused {
            WinState::Focused
        } else {
            WinState::Behind
        };
    }
}

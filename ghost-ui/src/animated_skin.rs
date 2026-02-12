//! Animated skin support with frame sequences and state management

use std::collections::HashMap;
use std::io::Read;
use std::path::Path;
use wgpu::{Device, Queue};

use serde::Deserialize;

use crate::skin::{Skin, SkinData, SkinError};

/// Persona manifest matching config.toml format
#[derive(Deserialize)]
struct CharacterManifest {
    character: CharacterInfoManifest,
}

#[derive(Deserialize)]
struct CharacterInfoManifest {
    name: String,
    nick: String,
    animationstate: Vec<String>,
    image: String,
    loading: String,
}

/// Metadata extracted from a persona package
#[derive(Debug, Clone)]
pub struct PersonaMeta {
    pub name: String,
    pub nick: String,
    pub still_image: Option<SkinData>,
    pub loading_text: String,
}

/// Animation playback mode
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PlayMode {
    /// Loop the animation forever
    Loop,
    /// Play once and stop at last frame
    Once,
    /// Play once and hide when done
    OnceAndHide,
    /// Ping-pong between first and last frame
    PingPong,
}

/// A single animation (sequence of frames)
pub struct Animation {
    /// Frame data (loaded from disk)
    frames: Vec<SkinData>,
    /// GPU textures for each frame
    textures: Vec<Option<Skin>>,
    /// Frames per second
    fps: f32,
    /// Playback mode
    pub play_mode: PlayMode,
    /// Current frame index
    current_frame: usize,
    /// Time accumulator for frame timing
    time_accumulator: f32,
    /// Direction for ping-pong mode (1 = forward, -1 = backward)
    direction: i32,
    /// Whether animation has finished (for Once mode)
    finished: bool,
}

impl Animation {
    /// Load an animation from a directory of PNG frames
    /// Frames should be named frame_0001.png, frame_0002.png, etc.
    pub fn from_directory(dir: impl AsRef<Path>, fps: f32) -> Result<Self, SkinError> {
        let dir = dir.as_ref();
        let mut frames = Vec::new();
        let mut frame_num = 1;

        loop {
            let frame_path = dir.join(format!("frame_{:04}.png", frame_num));
            if !frame_path.exists() {
                break;
            }

            let skin_data = SkinData::from_path(&frame_path)?;
            frames.push(skin_data);
            frame_num += 1;
        }

        if frames.is_empty() {
            return Err(SkinError::NotFound(format!(
                "No frames found in directory: {}",
                dir.display()
            )));
        }

        log::info!(
            "Loaded animation: {} frames at {}fps from {}",
            frames.len(),
            fps,
            dir.display()
        );

        Ok(Self {
            textures: (0..frames.len()).map(|_| None).collect(),
            frames,
            fps,
            play_mode: PlayMode::Loop,
            current_frame: 0,
            time_accumulator: 0.0,
            direction: 1,
            finished: false,
        })
    }

    /// Create an animation from pre-loaded frame data
    pub fn from_frames(frames: Vec<SkinData>, fps: f32) -> Result<Self, SkinError> {
        if frames.is_empty() {
            return Err(SkinError::NotFound("No frames provided".to_string()));
        }

        log::info!("Created animation from {} frames at {}fps", frames.len(), fps);

        Ok(Self {
            textures: (0..frames.len()).map(|_| None).collect(),
            frames,
            fps,
            play_mode: PlayMode::Loop,
            current_frame: 0,
            time_accumulator: 0.0,
            direction: 1,
            finished: false,
        })
    }

    /// Initialize GPU resources for all frames
    pub fn init_gpu(&mut self, device: &Device, queue: &Queue) {
        for (i, frame_data) in self.frames.iter().enumerate() {
            if self.textures[i].is_none() {
                match Skin::from_skin_data(frame_data, device, queue) {
                    Ok(skin) => {
                        self.textures[i] = Some(skin);
                    }
                    Err(e) => {
                        log::error!("Failed to create texture for frame {}: {}", i, e);
                    }
                }
            }
        }
    }

    /// Update animation timing
    pub fn update(&mut self, delta: f32) {
        if self.finished || self.frames.is_empty() {
            return;
        }

        self.time_accumulator += delta;
        let frame_duration = 1.0 / self.fps;

        while self.time_accumulator >= frame_duration {
            self.time_accumulator -= frame_duration;
            self.advance_frame();
        }
    }

    /// Advance to the next frame based on play mode
    fn advance_frame(&mut self) {
        let frame_count = self.frames.len();
        if frame_count <= 1 {
            return;
        }

        match self.play_mode {
            PlayMode::Loop => {
                self.current_frame = (self.current_frame + 1) % frame_count;
            }
            PlayMode::Once | PlayMode::OnceAndHide => {
                if self.current_frame < frame_count - 1 {
                    self.current_frame += 1;
                } else {
                    self.finished = true;
                }
            }
            PlayMode::PingPong => {
                let next = self.current_frame as i32 + self.direction;
                if next < 0 {
                    self.direction = 1;
                    self.current_frame = 1;
                } else if next >= frame_count as i32 {
                    self.direction = -1;
                    self.current_frame = frame_count - 2;
                } else {
                    self.current_frame = next as usize;
                }
            }
        }
    }

    /// Get the current frame's skin for rendering
    pub fn current_skin(&self) -> Option<&Skin> {
        if self.finished && self.play_mode == PlayMode::OnceAndHide {
            return None;
        }
        self.textures.get(self.current_frame)?.as_ref()
    }

    /// Reset animation to the beginning
    pub fn reset(&mut self) {
        self.current_frame = 0;
        self.time_accumulator = 0.0;
        self.direction = 1;
        self.finished = false;
    }

    /// Check if animation has finished (for Once mode)
    pub fn is_finished(&self) -> bool {
        self.finished
    }

    /// Get frame dimensions
    pub fn dimensions(&self) -> Option<(u32, u32)> {
        self.frames.first().map(|f| f.dimensions())
    }

    /// Get the number of frames
    pub fn frame_count(&self) -> usize {
        self.frames.len()
    }

    /// Get current frame index
    pub fn current_frame_index(&self) -> usize {
        self.current_frame
    }

    /// Set the playback mode
    pub fn set_play_mode(&mut self, mode: PlayMode) {
        self.play_mode = mode;
    }
}

/// Animation state identifier
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AnimationState {
    Idle,
    Talking,
    Thinking,
    Happy,
    Sad,
    Custom(u32),
}

impl AnimationState {
    /// Parse state from string
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "idle" => Self::Idle,
            "talking" | "talk" => Self::Talking,
            "thinking" | "think" => Self::Thinking,
            "happy" => Self::Happy,
            "sad" => Self::Sad,
            _ => Self::Idle,
        }
    }
}

/// Animated skin with multiple animation states
pub struct AnimatedSkin {
    /// Map of animation states to animations
    animations: HashMap<AnimationState, Animation>,
    /// Current animation state
    current_state: AnimationState,
    /// Default/fallback state
    default_state: AnimationState,
    /// Whether GPU resources are initialized
    gpu_initialized: bool,
}

impl AnimatedSkin {
    /// Create a new animated skin
    pub fn new() -> Self {
        Self {
            animations: HashMap::new(),
            current_state: AnimationState::Idle,
            default_state: AnimationState::Idle,
            gpu_initialized: false,
        }
    }

    /// Load animations from a base directory
    /// Expected structure:
    /// base_dir/
    ///   idle/frame_0001.png, frame_0002.png, ...
    ///   talking/frame_0001.png, ...
    ///   etc.
    pub fn from_directory(base_dir: impl AsRef<Path>, fps: f32) -> Result<Self, SkinError> {
        let base_dir = base_dir.as_ref();
        let mut skin = Self::new();

        // Try to load common animation states
        let states = [
            ("idle", AnimationState::Idle),
            ("talking", AnimationState::Talking),
            ("talk", AnimationState::Talking),
            ("thinking", AnimationState::Thinking),
            ("think", AnimationState::Thinking),
            ("happy", AnimationState::Happy),
            ("sad", AnimationState::Sad),
        ];

        for (dir_name, state) in states {
            let state_dir = base_dir.join(dir_name);
            if state_dir.exists() && state_dir.is_dir() {
                match Animation::from_directory(&state_dir, fps) {
                    Ok(anim) => {
                        skin.add_animation(state, anim);
                    }
                    Err(e) => {
                        log::warn!("Could not load animation '{}': {}", dir_name, e);
                    }
                }
            }
        }

        if skin.animations.is_empty() {
            return Err(SkinError::NotFound(format!(
                "No animations found in: {}",
                base_dir.display()
            )));
        }

        // Set the first available state as default
        if let Some(state) = skin.animations.keys().next().copied() {
            skin.current_state = state;
            skin.default_state = state;
        }

        Ok(skin)
    }

    /// Load a single animation as the idle state (for simple use cases)
    pub fn from_single_animation(dir: impl AsRef<Path>, fps: f32) -> Result<Self, SkinError> {
        let anim = Animation::from_directory(dir, fps)?;
        let mut skin = Self::new();
        skin.add_animation(AnimationState::Idle, anim);
        Ok(skin)
    }

    /// Add an animation for a state
    pub fn add_animation(&mut self, state: AnimationState, animation: Animation) {
        self.animations.insert(state, animation);
    }

    /// Initialize GPU resources for all animations
    pub fn init_gpu(&mut self, device: &Device, queue: &Queue) {
        if self.gpu_initialized {
            return;
        }

        for anim in self.animations.values_mut() {
            anim.init_gpu(device, queue);
        }

        self.gpu_initialized = true;
        log::info!("AnimatedSkin GPU initialized with {} states", self.animations.len());
    }

    /// Update the current animation
    pub fn update(&mut self, delta: f32) {
        if let Some(anim) = self.animations.get_mut(&self.current_state) {
            anim.update(delta);

            // If animation finished and it's a one-shot, return to default
            if anim.is_finished() && anim.play_mode != PlayMode::Loop {
                self.set_state(self.default_state);
            }
        }
    }

    /// Get the current frame's skin for rendering
    pub fn current_skin(&self) -> Option<&Skin> {
        self.animations
            .get(&self.current_state)
            .and_then(|a| a.current_skin())
    }

    /// Set the current animation state
    pub fn set_state(&mut self, state: AnimationState) {
        if self.current_state != state && self.animations.contains_key(&state) {
            // Reset the new animation
            if let Some(anim) = self.animations.get_mut(&state) {
                anim.reset();
            }
            self.current_state = state;
            log::debug!("Animation state changed to: {:?}", state);
        }
    }

    /// Get the current animation state
    pub fn current_state(&self) -> AnimationState {
        self.current_state
    }

    /// Set the default/fallback state
    pub fn set_default_state(&mut self, state: AnimationState) {
        if self.animations.contains_key(&state) {
            self.default_state = state;
        }
    }

    /// Get dimensions of the animation frames
    pub fn dimensions(&self) -> Option<(u32, u32)> {
        self.animations
            .get(&self.current_state)
            .and_then(|a| a.dimensions())
    }

    /// Check if a state exists
    pub fn has_state(&self, state: AnimationState) -> bool {
        self.animations.contains_key(&state)
    }

    /// Get available states
    pub fn available_states(&self) -> Vec<AnimationState> {
        self.animations.keys().copied().collect()
    }

    /// Load only metadata (manifest + still image) from a .persona.zip — fast, no frame loading.
    pub fn load_meta_from_zip(path: impl AsRef<Path>) -> Result<PersonaMeta, SkinError> {
        let file = std::fs::File::open(path.as_ref())
            .map_err(|e| SkinError::IoError(e))?;
        let mut archive = zip::ZipArchive::new(file)
            .map_err(|e| SkinError::NotFound(format!("Invalid zip: {}", e)))?;

        // Read config.toml
        let manifest: CharacterManifest = {
            let mut config_file = archive.by_name("config.toml")
                .map_err(|e| SkinError::NotFound(format!("No config.toml in zip: {}", e)))?;
            let mut s = String::new();
            config_file.read_to_string(&mut s)
                .map_err(|e| SkinError::IoError(e))?;
            toml::from_str(&s)
                .map_err(|e| SkinError::NotFound(format!("Invalid config.toml: {}", e)))?
        };

        // Load still image
        let image_name = manifest.character.image
            .strip_prefix("./")
            .unwrap_or(&manifest.character.image)
            .to_string();

        let still_image = if let Ok(mut img_file) = archive.by_name(&image_name) {
            let mut buf = Vec::new();
            img_file.read_to_end(&mut buf)
                .map_err(|e| SkinError::IoError(e))?;
            Some(SkinData::from_bytes(&buf)?)
        } else {
            log::warn!("Still image '{}' not found in zip", image_name);
            None
        };

        Ok(PersonaMeta {
            name: manifest.character.name,
            nick: manifest.character.nick,
            still_image,
            loading_text: manifest.character.loading,
        })
    }

    /// Load a full animated skin from a .persona.zip file.
    /// Returns (AnimatedSkin, PersonaMeta).
    pub fn from_zip(path: impl AsRef<Path>, fps: f32) -> Result<(Self, PersonaMeta), SkinError> {
        let file = std::fs::File::open(path.as_ref())
            .map_err(|e| SkinError::IoError(e))?;
        let mut archive = zip::ZipArchive::new(file)
            .map_err(|e| SkinError::NotFound(format!("Invalid zip: {}", e)))?;

        // Read config.toml
        let manifest: CharacterManifest = {
            let mut config_file = archive.by_name("config.toml")
                .map_err(|e| SkinError::NotFound(format!("No config.toml in zip: {}", e)))?;
            let mut s = String::new();
            config_file.read_to_string(&mut s)
                .map_err(|e| SkinError::IoError(e))?;
            toml::from_str(&s)
                .map_err(|e| SkinError::NotFound(format!("Invalid config.toml: {}", e)))?
        };

        // Load still image
        let image_name = manifest.character.image
            .strip_prefix("./")
            .unwrap_or(&manifest.character.image)
            .to_string();

        let still_image = if let Ok(mut img_file) = archive.by_name(&image_name) {
            let mut buf = Vec::new();
            img_file.read_to_end(&mut buf)
                .map_err(|e| SkinError::IoError(e))?;
            Some(SkinData::from_bytes(&buf)?)
        } else {
            None
        };

        let meta = PersonaMeta {
            name: manifest.character.name.clone(),
            nick: manifest.character.nick.clone(),
            still_image,
            loading_text: manifest.character.loading.clone(),
        };

        // Collect all file names from the archive first
        let file_names: Vec<String> = (0..archive.len())
            .filter_map(|i| archive.by_index(i).ok().map(|f| f.name().to_string()))
            .collect();

        // Load animation states
        let mut skin = Self::new();
        for state_name in &manifest.character.animationstate {
            // Find matching frame entries
            let prefix = format!("{}/frame_", state_name);
            let mut frame_entries: Vec<&str> = file_names
                .iter()
                .filter(|name| name.starts_with(&prefix) && name.ends_with(".png"))
                .map(|s| s.as_str())
                .collect();
            frame_entries.sort();

            let mut frames = Vec::new();
            for entry_name in &frame_entries {
                let mut file = archive.by_name(entry_name)
                    .map_err(|e| SkinError::NotFound(format!("Failed to read {}: {}", entry_name, e)))?;
                let mut buf = Vec::new();
                file.read_to_end(&mut buf)
                    .map_err(|e| SkinError::IoError(e))?;
                frames.push(SkinData::from_bytes(&buf)?);
            }

            if !frames.is_empty() {
                let state = AnimationState::from_str(state_name);
                let anim = Animation::from_frames(frames, fps)?;
                skin.add_animation(state, anim);
                log::info!("Loaded state '{}' with {} frames from zip", state_name, frame_entries.len());
            }
        }

        if skin.animations.is_empty() {
            return Err(SkinError::NotFound("No animation states found in zip".to_string()));
        }

        // Set the first available state as default
        if let Some(state) = skin.animations.keys().next().copied() {
            skin.current_state = state;
            skin.default_state = state;
        }

        Ok((skin, meta))
    }

    /// Play a one-shot animation and return to default when done
    pub fn play_once(&mut self, state: AnimationState) {
        if let Some(anim) = self.animations.get_mut(&state) {
            anim.reset();
            anim.set_play_mode(PlayMode::Once);
        }
        self.set_state(state);
    }
}

impl Default for AnimatedSkin {
    fn default() -> Self {
        Self::new()
    }
}

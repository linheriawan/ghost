//! Skin loading and texture management

use std::path::Path;

use image::{DynamicImage, GenericImageView};
use thiserror::Error;
use wgpu::{Device, Queue, Texture, TextureView};

#[derive(Error, Debug)]
pub enum SkinError {
    #[error("Failed to load image: {0}")]
    ImageLoadError(#[from] image::ImageError),
    #[error("Failed to read file: {0}")]
    IoError(#[from] std::io::Error),
}

/// Skin data that can be loaded before GPU initialization.
/// Use this for runtime skin loading/switching.
#[derive(Clone)]
pub struct SkinData {
    bytes: Vec<u8>,
    width: u32,
    height: u32,
}

impl SkinData {
    /// Load skin data from a file path.
    pub fn from_path(path: impl AsRef<Path>) -> Result<Self, SkinError> {
        let bytes = std::fs::read(path)?;
        Self::from_bytes(&bytes)
    }

    /// Load skin data from bytes.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, SkinError> {
        let img = image::load_from_memory(bytes)?;
        let (width, height) = img.dimensions();
        Ok(Self {
            bytes: bytes.to_vec(),
            width,
            height,
        })
    }

    /// Get the raw bytes.
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Get the width in pixels.
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Get the height in pixels.
    pub fn height(&self) -> u32 {
        self.height
    }

    /// Get dimensions as (width, height).
    pub fn dimensions(&self) -> (u32, u32) {
        (self.width, self.height)
    }
}

/// A skin that can be rendered onto a ghost window.
pub struct Skin {
    // Keep texture alive - the texture_view references it
    #[allow(dead_code)]
    texture: Texture,
    texture_view: TextureView,
    width: u32,
    height: u32,
    /// Alpha channel data for hit testing (row-major, top-to-bottom)
    alpha_data: Vec<u8>,
}

impl Skin {
    /// Load a skin from a PNG file path.
    pub fn from_png(path: &Path, device: &Device, queue: &Queue) -> Result<Self, SkinError> {
        let bytes = std::fs::read(path)?;
        Self::from_png_bytes(&bytes, device, queue)
    }

    /// Load a skin from PNG bytes (useful for embedded assets).
    pub fn from_png_bytes(bytes: &[u8], device: &Device, queue: &Queue) -> Result<Self, SkinError> {
        let img = image::load_from_memory(bytes)?;
        Self::from_image(img, device, queue)
    }

    /// Create a skin from SkinData.
    pub fn from_skin_data(data: &SkinData, device: &Device, queue: &Queue) -> Result<Self, SkinError> {
        Self::from_png_bytes(&data.bytes, device, queue)
    }

    /// Create a skin from a dynamic image.
    pub fn from_image(img: DynamicImage, device: &Device, queue: &Queue) -> Result<Self, SkinError> {
        let rgba = img.to_rgba8();
        let (width, height) = img.dimensions();

        // Extract alpha channel for hit testing
        let alpha_data: Vec<u8> = rgba.pixels().map(|p| p.0[3]).collect();

        let texture_size = wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        };

        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Skin Texture"),
            size: texture_size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });

        queue.write_texture(
            wgpu::ImageCopyTexture {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &rgba,
            wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(4 * width),
                rows_per_image: Some(height),
            },
            texture_size,
        );

        let texture_view = texture.create_view(&wgpu::TextureViewDescriptor::default());

        Ok(Self {
            texture,
            texture_view,
            width,
            height,
            alpha_data,
        })
    }

    /// Get the width of the skin in pixels.
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Get the height of the skin in pixels.
    pub fn height(&self) -> u32 {
        self.height
    }

    /// Get the texture view for rendering.
    pub fn texture_view(&self) -> &TextureView {
        &self.texture_view
    }

    /// Check if a point hits a non-transparent pixel.
    ///
    /// Returns true if the pixel at (x, y) has alpha > threshold.
    /// Coordinates are in skin space (0,0 is top-left of the skin).
    pub fn hit_test(&self, x: f32, y: f32, alpha_threshold: u8) -> bool {
        let px = x as u32;
        let py = y as u32;

        if px >= self.width || py >= self.height {
            return false;
        }

        let index = (py * self.width + px) as usize;
        if index >= self.alpha_data.len() {
            return false;
        }

        self.alpha_data[index] > alpha_threshold
    }

    /// Get alpha value at a specific pixel.
    pub fn alpha_at(&self, x: u32, y: u32) -> Option<u8> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let index = (y * self.width + x) as usize;
        self.alpha_data.get(index).copied()
    }
}

/// Convenience function to load skin data from a file path.
///
/// Returns (SkinData) which contains bytes, width, and height.
/// Use this for runtime skin loading.
///
/// # Example
/// ```no_run
/// let skin_data = ghost_ui::skin("assets/character.png").unwrap();
/// println!("Skin size: {}x{}", skin_data.width(), skin_data.height());
/// ```
pub fn skin(path: impl AsRef<Path>) -> Result<SkinData, SkinError> {
    SkinData::from_path(path)
}

/// Convenience function to load skin data from bytes.
///
/// # Example
/// ```no_run
/// let skin_data = ghost_ui::skin_bytes(include_bytes!("../assets/character.png")).unwrap();
/// ```
pub fn skin_bytes(bytes: &[u8]) -> Result<SkinData, SkinError> {
    SkinData::from_bytes(bytes)
}

//! Icon and tray helpers

use std::path::Path;
use thiserror::Error;
use tray_icon::{menu::Menu, Icon as TrayIconImage, TrayIcon, TrayIconBuilder};

#[derive(Error, Debug)]
pub enum IconError {
    #[error("Failed to load image: {0}")]
    ImageLoadError(#[from] image::ImageError),
    #[error("Failed to create icon: {0}")]
    IconCreationError(#[from] tray_icon::BadIcon),
    #[error("Failed to build tray icon: {0}")]
    TrayBuildError(#[from] tray_icon::Error),
    #[error("Failed to read file: {0}")]
    IoError(#[from] std::io::Error),
}

/// Load an icon from PNG bytes.
pub fn load_icon(bytes: &[u8]) -> Result<TrayIconImage, IconError> {
    let img = image::load_from_memory(bytes)?;
    let rgba = img.to_rgba8();
    let dimensions = image::GenericImageView::dimensions(&img);
    let icon = TrayIconImage::from_rgba(rgba.into_raw(), dimensions.0, dimensions.1)?;
    Ok(icon)
}

/// Load an icon from a file path.
pub fn load_icon_from_path(path: impl AsRef<Path>) -> Result<TrayIconImage, IconError> {
    let bytes = std::fs::read(path)?;
    load_icon(&bytes)
}

/// Create a system tray icon with a menu.
pub fn create_tray(icon: TrayIconImage, menu: Menu) -> Result<TrayIcon, IconError> {
    let tray = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_icon(icon)
        .build()?;
    Ok(tray)
}

/// Create a system tray icon with a default empty menu.
pub fn create_tray_simple(icon: TrayIconImage) -> Result<TrayIcon, IconError> {
    create_tray(icon, Menu::new())
}

/// Set the application dock icon on macOS.
///
/// Note: This only works reliably when running as a bundled .app.
/// When running via `cargo run`, the dock shows the default terminal/cargo icon.
/// For development, the tray icon will still work correctly.
#[cfg(target_os = "macos")]
pub fn set_dock_icon(bytes: &[u8]) -> Result<(), IconError> {
    use cocoa::appkit::{NSApp, NSImage};
    use cocoa::base::{id, nil};
    use cocoa::foundation::NSData;
    use objc::msg_send;
    use objc::sel;
    use objc::sel_impl;

    let img = image::load_from_memory(bytes)?;
    let rgba = img.to_rgba8();
    let raw_data = rgba.into_raw();

    unsafe {
        let app = NSApp();

        // Create NSData from the raw bytes
        let data: id = NSData::dataWithBytes_length_(
            nil,
            raw_data.as_ptr() as *const std::ffi::c_void,
            raw_data.len() as u64,
        );

        // Create NSImage from the data
        let ns_image: id = NSImage::initWithData_(NSImage::alloc(nil), data);

        if ns_image != nil {
            // Set the application icon
            let _: () = msg_send![app, setApplicationIconImage: ns_image];
        }
    }

    Ok(())
}

#[cfg(not(target_os = "macos"))]
pub fn set_dock_icon(_bytes: &[u8]) -> Result<(), IconError> {
    // No-op on non-macOS platforms
    Ok(())
}

/// Helper struct to manage application icons.
pub struct AppIcon {
    icon_bytes: Vec<u8>,
    tray_icon: Option<TrayIcon>,
}

impl AppIcon {
    /// Create a new AppIcon from PNG bytes.
    pub fn new(bytes: &[u8]) -> Self {
        Self {
            icon_bytes: bytes.to_vec(),
            tray_icon: None,
        }
    }

    /// Create a new AppIcon from a file path.
    pub fn from_path(path: impl AsRef<Path>) -> Result<Self, IconError> {
        let bytes = std::fs::read(path)?;
        Ok(Self::new(&bytes))
    }

    /// Set up the system tray icon.
    pub fn setup_tray(&mut self) -> Result<&TrayIcon, IconError> {
        let icon = load_icon(&self.icon_bytes)?;
        let tray = create_tray_simple(icon)?;
        self.tray_icon = Some(tray);
        Ok(self.tray_icon.as_ref().unwrap())
    }

    /// Set up the system tray icon with a custom menu.
    pub fn setup_tray_with_menu(&mut self, menu: Menu) -> Result<&TrayIcon, IconError> {
        let icon = load_icon(&self.icon_bytes)?;
        let tray = create_tray(icon, menu)?;
        self.tray_icon = Some(tray);
        Ok(self.tray_icon.as_ref().unwrap())
    }

    /// Set the dock icon (macOS only).
    ///
    /// Note: Only works when running as a bundled .app, not via `cargo run`.
    pub fn setup_dock(&self) -> Result<(), IconError> {
        set_dock_icon(&self.icon_bytes)
    }

    /// Set up both tray and dock icons.
    pub fn setup_all(&mut self) -> Result<&TrayIcon, IconError> {
        // Try to set dock icon, but don't fail if it doesn't work
        if let Err(e) = self.setup_dock() {
            log::warn!("Could not set dock icon (normal when running via cargo run): {}", e);
        }
        self.setup_tray()
    }

    /// Get a reference to the tray icon if it exists.
    pub fn tray(&self) -> Option<&TrayIcon> {
        self.tray_icon.as_ref()
    }
}

/// Convenience function to create an AppIcon from a file path.
///
/// # Example
/// ```no_run
/// let mut app_icon = ghost_ui::icon("assets/icon.png").unwrap();
/// app_icon.setup_all().unwrap();
/// ```
pub fn icon(path: impl AsRef<Path>) -> Result<AppIcon, IconError> {
    AppIcon::from_path(path)
}

/// Convenience function to create an AppIcon from embedded bytes.
///
/// # Example
/// ```no_run
/// let mut app_icon = ghost_ui::icon_bytes(include_bytes!("../assets/icon.png"));
/// app_icon.setup_all().unwrap();
/// ```
pub fn icon_bytes(bytes: &[u8]) -> AppIcon {
    AppIcon::new(bytes)
}

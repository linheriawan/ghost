use std::fs;
use std::io::{Read, Write};
use std::path::PathBuf;

use clap::{Parser, Subcommand};
use glob::glob;
use image::Rgba;
use serde::Deserialize;

/// Persona manifest matching config.toml format
#[derive(Deserialize)]
struct CharacterManifest {
    character: CharacterInfo,
}

#[derive(Deserialize)]
struct CharacterInfo {
    name: String,
    nick: String,
    animationstate: Vec<String>,
    image: String,
    loading: String,
}

#[derive(Parser)]
#[command(name = "gassetsmaker", about = "Ghost asset processing tool")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Clean semi-transparent pixels from PNG frames (hard cut alpha)
    Clean {
        /// Path to directory containing PNG frames
        path: PathBuf,
    },
    /// Pack a persona directory into a .persona.zip
    Pack {
        /// Path to persona directory (must contain config.toml)
        path: PathBuf,
    },
    /// Show info about a .persona.zip file
    Info {
        /// Path to .persona.zip file
        path: PathBuf,
    },
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Commands::Clean { path } => cmd_clean(&path),
        Commands::Pack { path } => cmd_pack(&path),
        Commands::Info { path } => cmd_info(&path),
    }
}

fn cmd_clean(folder: &PathBuf) {
    let clean_path = folder.to_string_lossy();
    let clean_path = clean_path.trim_end_matches('/');
    let pattern = format!("{}/*.png", clean_path);

    println!("Cleaning frames in: {}", pattern);

    for entry in glob(&pattern).expect("Failed to read glob pattern") {
        if let Ok(path) = entry {
            let mut img = image::open(&path).expect("Failed to load").into_rgba8();
            let (width, height) = img.dimensions();
            let mut changed = false;

            for x in 0..width {
                for y in 0..height {
                    let pixel = img.get_pixel(x, y);
                    if pixel.0[3] < 255 {
                        img.put_pixel(x, y, Rgba([0, 0, 0, 0]));
                        changed = true;
                    }
                }
            }

            if changed {
                img.save(&path).unwrap();
                println!("Fixed: {:?}", path.file_name().unwrap());
            }
        }
    }
}

fn cmd_pack(persona_dir: &PathBuf) {
    // Read and parse config.toml
    let config_path = persona_dir.join("config.toml");
    let config_str = fs::read_to_string(&config_path).unwrap_or_else(|e| {
        eprintln!("Error reading {}: {}", config_path.display(), e);
        std::process::exit(1);
    });
    let manifest: CharacterManifest = toml::from_str(&config_str).unwrap_or_else(|e| {
        eprintln!("Error parsing config.toml: {}", e);
        std::process::exit(1);
    });

    let nick = &manifest.character.nick;
    let output_name = format!("{}.persona.zip", nick.to_lowercase());
    let output_path = persona_dir.parent().unwrap_or(persona_dir).join(&output_name);

    println!("Packing persona '{}' ({})...", manifest.character.name, nick);

    let file = fs::File::create(&output_path).unwrap_or_else(|e| {
        eprintln!("Error creating {}: {}", output_path.display(), e);
        std::process::exit(1);
    });

    let mut zip = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Stored);

    // Add config.toml
    zip.start_file("config.toml", options).unwrap();
    zip.write_all(config_str.as_bytes()).unwrap();
    println!("  + config.toml");

    // Add still image
    let image_path = persona_dir.join(&manifest.character.image);
    if image_path.exists() {
        let image_filename = &manifest.character.image;
        // Strip leading ./ if present
        let image_filename = image_filename.strip_prefix("./").unwrap_or(image_filename);
        let mut buf = Vec::new();
        fs::File::open(&image_path)
            .unwrap()
            .read_to_end(&mut buf)
            .unwrap();
        zip.start_file(image_filename, options).unwrap();
        zip.write_all(&buf).unwrap();
        println!("  + {} ({} bytes)", image_filename, buf.len());
    } else {
        eprintln!(
            "Warning: still image '{}' not found",
            image_path.display()
        );
    }

    // Add animation state frames
    for state_name in &manifest.character.animationstate {
        let state_dir = persona_dir.join(state_name);
        if !state_dir.exists() || !state_dir.is_dir() {
            eprintln!("Warning: state directory '{}' not found", state_dir.display());
            continue;
        }

        let mut frame_num = 1u32;
        loop {
            let frame_filename = format!("frame_{:04}.png", frame_num);
            let frame_path = state_dir.join(&frame_filename);
            if !frame_path.exists() {
                break;
            }

            let zip_entry = format!("{}/{}", state_name, frame_filename);
            let mut buf = Vec::new();
            fs::File::open(&frame_path)
                .unwrap()
                .read_to_end(&mut buf)
                .unwrap();
            zip.start_file(&zip_entry, options).unwrap();
            zip.write_all(&buf).unwrap();
            frame_num += 1;
        }
        let count = frame_num - 1;
        println!("  + {}/  ({} frames)", state_name, count);
    }

    zip.finish().unwrap();
    println!("Created: {}", output_path.display());
}

fn cmd_info(zip_path: &PathBuf) {
    let file = fs::File::open(zip_path).unwrap_or_else(|e| {
        eprintln!("Error opening {}: {}", zip_path.display(), e);
        std::process::exit(1);
    });

    let mut archive = zip::ZipArchive::new(file).unwrap_or_else(|e| {
        eprintln!("Error reading zip: {}", e);
        std::process::exit(1);
    });

    // Read config.toml from zip
    let config_str = {
        let mut config_file = archive.by_name("config.toml").unwrap_or_else(|e| {
            eprintln!("No config.toml in archive: {}", e);
            std::process::exit(1);
        });
        let mut s = String::new();
        config_file.read_to_string(&mut s).unwrap();
        s
    };

    let manifest: CharacterManifest = toml::from_str(&config_str).unwrap_or_else(|e| {
        eprintln!("Error parsing config.toml: {}", e);
        std::process::exit(1);
    });

    println!("Persona: {} ({})", manifest.character.name, manifest.character.nick);
    println!("Still image: {}", manifest.character.image);
    println!("Loading text: \"{}\"", manifest.character.loading);
    println!("States:");

    for state_name in &manifest.character.animationstate {
        let prefix = format!("{}/frame_", state_name);
        let count = (0..archive.len())
            .filter(|&i| {
                archive
                    .by_index(i)
                    .map(|f| f.name().starts_with(&prefix))
                    .unwrap_or(false)
            })
            .count();
        println!("  {}: {} frames", state_name, count);
    }

    let mut total_size: u64 = 0;
    for i in 0..archive.len() {
        if let Ok(f) = archive.by_index(i) {
            total_size += f.size();
        }
    }
    println!("Total uncompressed: {:.1} MB", total_size as f64 / 1_048_576.0);
}

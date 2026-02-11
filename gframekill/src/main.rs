use image::{GenericImage, Rgba};
use glob::glob;
use std::env;

fn main() {
    // Collect command line arguments: [executable, --path, folder/path/]
    let args: Vec<String> = env::args().collect();
    
    // Find the index of "--path" and get the value after it
    let path_arg = args.iter().position(|r| r == "--path")
        .and_then(|i| args.get(i + 1));

    let folder = match path_arg {
        Some(p) => p,
        None => {
            println!("Usage: cargo run --path ../ghost/assets/your/folder/");
            return;
        }
    };

    // Clean up path and add the glob pattern
    let clean_path = folder.trim_end_matches('/');
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
                    // The "Hard Cut" logic you liked
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
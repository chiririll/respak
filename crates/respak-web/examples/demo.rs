//! Writes a small synthetic game folder to look at the web UI with (no game files needed):
//! `cargo run -p respak-web --example demo -- target/demo`, then
//! `cargo run -p respak-web -- --dir target/demo`.

use std::path::Path;

use respak_respawn_testing::{build_uimg_pak, SpriteSpec, TexSpec, UimgSpec};

const RGBA8: u16 = 31;

/// A colourful `w`x`h` gradient with a checker overlay.
fn gradient(w: u16, h: u16) -> Vec<u8> {
    let mut pixels = Vec::new();
    for y in 0..h {
        for x in 0..w {
            let check = ((x / 8) + (y / 8)) % 2 == 0;
            let r = (x as f32 / w as f32 * 255.0) as u8;
            let g = (y as f32 / h as f32 * 255.0) as u8;
            let b = if check { 220 } else { 90 };
            pixels.extend([r, g, b, 255]);
        }
    }
    pixels
}

fn sprite(path: &'static str, hash: u32, [x, y, w, h]: [u32; 4], atlas: (u32, u32)) -> SpriteSpec {
    SpriteSpec {
        path: Some(path),
        hash,
        uv: [
            x as f32 / atlas.0 as f32,
            y as f32 / atlas.1 as f32,
            w as f32 / atlas.0 as f32,
            h as f32 / atlas.1 as f32,
        ],
        layout: [0.0, 0.0, 1.0, 1.0, 0.0, 0.0, 1.0, 1.0],
        render: (w as u16, h as u16),
    }
}

fn main() -> std::io::Result<()> {
    let dir = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "target/demo".into());
    let root = Path::new(&dir).join("game");
    std::fs::create_dir_all(root.join("paks/Win64"))?;
    std::fs::create_dir_all(root.join("scripts"))?;
    std::fs::write(
        root.join("README.txt"),
        "A synthetic game folder for the respak web UI.\n",
    )?;
    std::fs::write(root.join("scripts/init.cfg"), "name \"demo\"\n")?;

    let textures = [
        TexSpec::new(0xA11CE, 9, 128, 128, RGBA8, 1, gradient(128, 128)),
        TexSpec::new(0xB0B, 9, 64, 64, RGBA8, 1, gradient(64, 64)),
        TexSpec::new(0xA71A5, 9, 128, 64, RGBA8, 1, gradient(128, 64)),
    ];
    let atlas = (128, 64);
    let sprites = vec![
        sprite("ui/icons/health", 1, [4, 4, 32, 32], atlas),
        sprite("ui/icons/shield", 2, [44, 4, 32, 32], atlas),
        sprite("ui/icons/ammo", 3, [84, 4, 40, 24], atlas),
        sprite("ui/hud/frame", 4, [4, 40, 120, 20], atlas),
    ];
    let atlases = [UimgSpec {
        guid: 0x1111,
        version: 10,
        width: 128,
        height: 64,
        atlas_guid: 0xA71A5,
        sprites,
        names: true,
    }];
    std::fs::write(
        root.join("paks/Win64/common.rpak"),
        build_uimg_pak(8, &textures, &atlases),
    )?;

    std::fs::write(
        Path::new(&dir).join("respak.toml"),
        "root = \"game\"\nout = \"out\"\n",
    )?;
    println!("demo game written to {}", root.display());
    Ok(())
}

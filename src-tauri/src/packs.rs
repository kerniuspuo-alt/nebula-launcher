use serde::Serialize;
use std::fs::File;
use std::io::{Read, Write};
use std::path::PathBuf;
use zip::write::SimpleFileOptions;
use zip::{ZipArchive, ZipWriter};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MixResult {
    pub output: String,
    pub files: usize,
}

/// Overlay pack B on pack A. Later pack wins on path conflicts. Writes a new zip.
#[tauri::command]
pub fn mix_resource_packs(pack_a: String, pack_b: String, output: String) -> Result<MixResult, String> {
    let mut map: std::collections::BTreeMap<String, Vec<u8>> = std::collections::BTreeMap::new();
    ingest(&pack_a, &mut map)?;
    ingest(&pack_b, &mut map)?;
    if let Some(parent) = PathBuf::from(&output).parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let file = File::create(&output).map_err(|e| e.to_string())?;
    let mut writer = ZipWriter::new(file);
    let opts = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    let files = map.len();
    for (name, bytes) in map {
        writer.start_file(name, opts).map_err(|e| e.to_string())?;
        writer.write_all(&bytes).map_err(|e| e.to_string())?;
    }
    writer.finish().map_err(|e| e.to_string())?;
    Ok(MixResult { output, files })
}

fn ingest(path: &str, map: &mut std::collections::BTreeMap<String, Vec<u8>>) -> Result<(), String> {
    let file = File::open(path).map_err(|e| format!("{path}: {e}"))?;
    let mut zip = ZipArchive::new(file).map_err(|e| e.to_string())?;
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).map_err(|e| e.to_string())?;
        if entry.is_dir() {
            continue;
        }
        let name = entry.name().to_string();
        let mut buf = Vec::new();
        entry.read_to_end(&mut buf).map_err(|e| e.to_string())?;
        map.insert(name, buf);
    }
    Ok(())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Schematic {
    pub title: String,
    pub prompt: String,
    pub palette: Vec<String>,
    pub layers: Vec<Vec<String>>,
}

/// Heuristic blueprint generator (not a trained model). Maps keywords to block palettes and a layered grid.
#[tauri::command]
pub fn generate_schematic(prompt: String) -> Schematic {
    let p = prompt.to_ascii_lowercase();
    let (title, palette) = if p.contains("steampunk") || p.contains("airship") {
        (
            "Steampunk dock",
            vec!["spruce_planks", "copper_block", "oxidized_copper", "oak_fence", "lantern", "chain"],
        )
    } else if p.contains("castle") || p.contains("keep") {
        (
            "Stone keep",
            vec!["stone_bricks", "cobblestone", "oak_planks", "glass_pane", "torch"],
        )
    } else if p.contains("modern") || p.contains("house") {
        (
            "Modern house",
            vec!["white_concrete", "gray_concrete", "glass", "oak_planks", "light_gray_wool"],
        )
    } else if p.contains("farm") {
        (
            "Starter farm",
            vec!["farmland", "oak_log", "oak_leaves", "water", "oak_fence", "wheat"],
        )
    } else {
        (
            "Sketch",
            vec!["oak_planks", "cobblestone", "glass", "torch"],
        )
    };
    let w = 12usize;
    let layers = (0..6)
        .map(|y| {
            (0..w * w)
                .map(|i| {
                    let x = i % w;
                    let z = i / w;
                    let edge = x == 0 || z == 0 || x == w - 1 || z == w - 1;
                    if y == 0 {
                        palette[0].to_string()
                    } else if edge && y < 4 {
                        palette[1.min(palette.len() - 1)].to_string()
                    } else if y == 4 && edge {
                        palette.last().unwrap_or(&"torch").to_string()
                    } else {
                        "air".into()
                    }
                })
                .collect()
        })
        .collect();
    Schematic {
        title: title.into(),
        prompt,
        palette: palette.into_iter().map(|s| s.to_string()).collect(),
        layers,
    }
}

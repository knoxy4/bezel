//! Loads `assets/atlas/hero.atlas` (+ `hero.png`): sprite names -> ids for extract, frames for the renderer.
//! Line format (written by tools/pack_atlas.py): `<name> <x> <y> <w> <h> <pivot_x> <pivot_y>`; `<name>~f` is the
//! mirrored copy and becomes frame 1 of the same sprite.

use std::collections::HashMap;
use std::path::Path;

use bezel_render_api::{AtlasDesc, AtlasFrame, SpriteId};

#[derive(Default)]
pub struct Sprites {
    ids: HashMap<String, SpriteId>,
}

impl Sprites {
    pub fn get(&self, name: &str) -> Option<SpriteId> {
        self.ids.get(name).copied()
    }

    pub fn len(&self) -> usize {
        self.ids.len()
    }

    /// `None` if the atlas isn't there (extract falls back to shapes).
    pub fn load(dir: &Path) -> Option<(Sprites, AtlasDesc)> {
        let text = std::fs::read_to_string(dir.join("hero.atlas")).ok()?;
        let mut ids: HashMap<String, SpriteId> = HashMap::new();
        let mut frames = Vec::new();
        for line in text.lines() {
            let p: Vec<&str> = line.split_whitespace().collect();
            if p.len() != 7 {
                continue;
            }
            let (name, frame) = match p[0].strip_suffix("~f") {
                Some(base) => (base, 1u16),
                None => (p[0], 0u16),
            };
            let n = ids.len() as u32;
            let id = *ids.entry(name.to_string()).or_insert(SpriteId(n));
            let num = |i: usize| p[i].parse::<u32>().unwrap_or(0);
            let fl = |i: usize| p[i].parse::<f32>().unwrap_or(0.5);
            frames.push(AtlasFrame { id, frame, rect: [num(1), num(2), num(3), num(4)], pivot: [fl(5), fl(6)] });
        }
        if ids.is_empty() {
            return None;
        }
        Some((Sprites { ids }, AtlasDesc { image: dir.join("hero.png"), frames }))
    }
}

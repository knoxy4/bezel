//! Pure layout: zones, canopy modes, occlusion grid, platforms. (ADR-0001 §2.2, §8)
//!
//! W1 scope: the occlusion grid and platforms. Zones, canopy, fallback and policy land later.

pub mod occlusion;
pub mod platforms;

pub use occlusion::{CellDelta, OcclusionGrid};
pub use platforms::Platform;

/// Art scale = max(1, round(dpi / 96)) (ADR §2.1).
pub fn art_scale_for(dpi: u32) -> u8 {
    let s = (dpi + 48) / 96;
    s.clamp(1, u8::MAX as u32) as u8
}

#[cfg(test)]
mod tests {
    use super::art_scale_for;

    #[test]
    fn art_scale_rounds() {
        assert_eq!(art_scale_for(0), 1);
        assert_eq!(art_scale_for(96), 1);
        assert_eq!(art_scale_for(120), 1); // 125%
        assert_eq!(art_scale_for(144), 2); // 150% rounds half up
        assert_eq!(art_scale_for(192), 2);
        assert_eq!(art_scale_for(288), 3);
    }
}

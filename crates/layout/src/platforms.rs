//! Windows as terrain (D24): a window's top edge is a platform wherever the cells directly above it are bare.

use bezel_core::{ExtWindowId, MonitorId, Px};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Platform {
    pub window: ExtWindowId,
    pub monitor: MonitorId,
    pub y: Px,
    pub x0: Px,
    pub x1: Px,
}

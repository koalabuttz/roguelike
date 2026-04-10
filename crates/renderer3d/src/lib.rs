#![cfg_attr(not(feature = "std"), no_std)]

pub mod color_map;
pub mod framebuffer;
pub mod geometry;
pub mod math;
pub mod pipeline;

#[cfg(feature = "std")]
pub mod rasterizer;

#[cfg(feature = "std")]
pub mod scene;

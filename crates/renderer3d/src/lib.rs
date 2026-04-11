#![cfg_attr(not(feature = "std"), no_std)]

#[cfg(all(not(feature = "std"), feature = "alloc"))]
extern crate alloc;

pub mod color_map;
pub mod font;
pub mod framebuffer;
pub mod geometry;
pub mod math;
pub mod pipeline;

#[cfg(any(feature = "std", feature = "alloc"))]
pub mod rasterizer;

#[cfg(any(feature = "std", feature = "alloc"))]
pub mod scene;

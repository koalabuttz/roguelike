#![cfg_attr(not(feature = "std"), no_std)]

pub mod framebuffer;
pub mod math;
pub mod pipeline;

#[cfg(feature = "std")]
pub mod rasterizer;

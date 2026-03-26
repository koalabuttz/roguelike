#![cfg_attr(not(feature = "std"), no_std)]

// --- Always compiled (no_std compatible) ---
pub mod rules;
pub mod tier_compact;
pub mod tier_micro;

pub mod command;

#[cfg(test)]
mod mem_override_tests;

// --- Standard-tier engine (std feature implies rand + serde) ---
#[cfg(feature = "std")]
pub mod ai;
#[cfg(feature = "std")]
pub mod combat;
#[cfg(feature = "std")]
pub mod data;
#[cfg(feature = "std")]
pub mod entity;
#[cfg(feature = "std")]
pub mod exploration_graph;
#[cfg(feature = "std")]
pub mod fov;
#[cfg(feature = "std")]
pub mod game;
#[cfg(feature = "std")]
pub mod game_step;
#[cfg(feature = "std")]
pub mod item;
#[cfg(feature = "std")]
pub mod look;
#[cfg(feature = "std")]
pub mod map;
#[cfg(feature = "std")]
pub mod menu;
#[cfg(feature = "std")]
pub mod message_history;
#[cfg(feature = "std")]
pub mod message_log;
#[cfg(feature = "std")]
pub mod pathfinding;
#[cfg(feature = "std")]
pub mod platform;
#[cfg(feature = "std")]
pub mod saves;
#[cfg(feature = "std")]
pub mod seed_code;
#[cfg(feature = "std")]
pub mod settings;
#[cfg(feature = "std")]
pub mod spawn;
#[cfg(feature = "std")]
pub mod spectate;
#[cfg(feature = "std")]
pub mod types;

// --- Feature-gated extensions (each implies std transitively) ---
#[cfg(feature = "dev-tools")]
pub mod analytics;
#[cfg(feature = "dev-tools")]
pub mod dev_tools;
#[cfg(feature = "data-files")]
pub mod help;
#[cfg(feature = "dev-tools")]
pub mod scenario;

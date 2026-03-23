//! Pure game rules shared by all capability tiers.
//!
//! This module contains constants and pure functions with zero state
//! interaction. Everything here is `no_std` compatible and used by all
//! platforms — from C64 to PC.

pub mod balance;
pub mod color;
pub mod damage;
pub mod direction;
pub mod health;
pub mod items;
pub mod message;
pub mod monster_table;
pub mod seed_code;
pub mod tiles;

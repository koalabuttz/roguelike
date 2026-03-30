#![no_std]
#![no_main]

mod display;
mod input;
mod palette;

use gba::prelude::*;
use roguelike_core::rules::color::GameColor;

use crate::display::*;
use crate::palette::*;

#[panic_handler]
fn panic_handler(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}

/// Draw a static test screen to verify the full display pipeline:
/// font loading, palette colors, both BG layers, and layer transparency.
fn draw_test_screen() {
    // --- BG0: Map layer test pattern ---

    // Fill a region with floor dots (DarkGrey)
    let floor_pb = GameColor::DarkGrey as u16;
    for y in 2..15 {
        for x in 1..29 {
            write_map_tile(x, y, b'.', floor_pb);
        }
    }

    // Structural walls around the room (White)
    let wall_pb = GameColor::White as u16;
    for x in 0..30 {
        write_map_tile(x, 1, b'#', wall_pb);
        write_map_tile(x, 15, b'#', wall_pb);
    }
    for y in 1..16 {
        write_map_tile(0, y, b'#', wall_pb);
        write_map_tile(29, y, b'#', wall_pb);
    }

    // Player '@' in green
    write_map_tile(15, 8, b'@', GameColor::Green as u16);

    // Monsters
    write_map_tile(20, 6, b'g', GameColor::Green as u16);
    write_map_tile(10, 10, b'o', GameColor::Red as u16);
    write_map_tile(25, 12, b'T', GameColor::DarkGreen as u16);

    // Items
    write_map_tile(12, 5, b'!', GameColor::Yellow as u16);
    write_map_tile(18, 11, b'/', GameColor::Yellow as u16);
    write_map_tile(7, 9, b'[', GameColor::Yellow as u16);

    // Stairs
    write_map_tile(27, 14, b'>', GameColor::Cyan as u16);

    // Corpse
    write_map_tile(22, 8, b'%', GameColor::DarkRed as u16);

    // Explored-but-not-visible area (dimmed)
    for x in 1..10 {
        write_map_tile(x, 16, b'.', PALBANK_DIM);
        write_map_tile(x + 10, 16, b'#', PALBANK_DIM);
    }

    // Title text at top
    write_map_string(10, 0, "ROGUELIKE GBA", GameColor::White as u16);

    // --- BG1: HUD layer ---

    // Status bar row (white on dark blue background)
    // Fill entire row with spaces first to get the blue background
    for x in 0..30 {
        write_hud_tile(x, STATUS_ROW, b' ', PALBANK_STATUS);
    }
    write_hud_string(0, STATUS_ROW, "HP:20/20 ATK:5 DEF:2 D:1", PALBANK_STATUS);

    // Message log
    write_hud_string(0, MSG_ROW, "You see a Goblin.", PALBANK_MSG);
    write_hud_string(0, MSG_ROW + 1, "The Orc hits you for 3 damage.", PALBANK_MSG);
}

#[no_mangle]
extern "C" fn main() -> ! {
    init_display();
    draw_test_screen();

    loop {
        // Busy-wait for VBlank
        while VCOUNT.read() < 160 {}
        while VCOUNT.read() >= 160 {}
    }
}

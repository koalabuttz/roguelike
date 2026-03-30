#![no_std]
#![no_main]

use gba::prelude::*;

// Prove roguelike-core links: import a compact tier type
use roguelike_core::tier_compact::types::MAP_WIDTH;

#[panic_handler]
fn panic_handler(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}

#[no_mangle]
extern "C" fn main() -> ! {
    // Dark green backdrop — proves we booted
    BACKDROP_COLOR.write(Color::from_rgb(0, 12, 0));

    // Mode 0, no backgrounds enabled yet — just backdrop color
    DISPCNT.write(DisplayControl::new());

    // Prove the core link worked at compile time
    let _map_w: i32 = MAP_WIDTH;

    loop {
        // Busy-wait for VBlank (scanline 160+)
        while VCOUNT.read() < 160 {}
        while VCOUNT.read() >= 160 {}
    }
}

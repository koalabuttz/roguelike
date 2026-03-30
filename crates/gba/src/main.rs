#![no_std]
#![no_main]

mod display;
mod format;
mod game_loop;
mod input;
mod palette;
mod render;

#[panic_handler]
fn panic_handler(info: &core::panic::PanicInfo) -> ! {
    // Show panic on screen — write to BG0 screenblock directly
    // (display::init_display may not have been called yet)
    use gba::prelude::*;

    // Try to show location
    if let Some(loc) = info.location() {
        let mut buf = [b' '; 30];
        let mut p = 0;
        p = format::write_str(&mut buf, p, "PANIC ");
        // Write line number
        p = format::write_u16(&mut buf, p, loc.line() as u16);
        if let Ok(s) = core::str::from_utf8(&buf[..p]) {
            display::write_map_string(0, 0, s, 4); // red
        }
    } else {
        display::write_map_string(0, 0, "PANIC (no location)", 4);
    }
    loop {}
}

#[no_mangle]
extern "C" fn main() -> ! {
    display::init_display();
    game_loop::run()
}

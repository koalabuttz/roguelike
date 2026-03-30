#![no_std]
#![no_main]

mod debug;
mod display;
mod format;
mod game_loop;
mod input;
mod inventory_ui;
mod palette;
mod render;
#[cfg(feature = "dev")]
mod stack_check;

#[panic_handler]
fn panic_handler(info: &core::panic::PanicInfo) -> ! {
    // Show panic on screen — write to BG0 screenblock directly
    // (display::init_display may not have been called yet)

    // Line 0: "PANIC {line_number}"
    if let Some(loc) = info.location() {
        let mut buf = [b' '; 30];
        let mut p = 0;
        p = format::write_str(&mut buf, p, "PANIC ");
        p = format::write_u16(&mut buf, p, loc.line() as u16);
        if let Ok(s) = core::str::from_utf8(&buf[..p]) {
            display::write_map_string(0, 0, s, 4); // red
        }
    } else {
        display::write_map_string(0, 0, "PANIC (no location)", 4);
    }

    // Lines 1-2: SP and LR register values for post-mortem addr2line
    let sp: u32;
    let lr: u32;
    unsafe {
        core::arch::asm!("mov {}, sp", out(reg) sp, options(nomem, nostack));
        core::arch::asm!("mov {}, lr", out(reg) lr, options(nomem, nostack));
    }
    {
        let mut buf = [b' '; 30];
        let mut p = 0;
        p = format::write_str(&mut buf, p, "SP:");
        p = format::write_hex(&mut buf, p, sp);
        if let Ok(s) = core::str::from_utf8(&buf[..p]) {
            display::write_map_string(0, 1, s, 4);
        }
    }
    {
        let mut buf = [b' '; 30];
        let mut p = 0;
        p = format::write_str(&mut buf, p, "LR:");
        p = format::write_hex(&mut buf, p, lr);
        if let Ok(s) = core::str::from_utf8(&buf[..p]) {
            display::write_map_string(0, 2, s, 4);
        }
    }

    // Log full panic info to mGBA console (no-op on hardware/other emulators)
    debug::debug_log_fatal!("{}", info);

    // If GDB is attached via mGBA, break here for inspection.
    // 0xBE00 is the Thumb bkpt encoding — undefined instruction on real ARM7TDMI
    // but recognized by mGBA's GDB stub as a software breakpoint.
    #[cfg(feature = "dev")]
    unsafe {
        core::arch::asm!(".inst 0xBE00", options(nomem, nostack));
    }

    loop {}
}

#[no_mangle]
extern "C" fn main() -> ! {
    #[cfg(feature = "dev")]
    stack_check::init_canary();

    display::init_display();
    debug::debug_log!("Display initialized");

    game_loop::run()
}

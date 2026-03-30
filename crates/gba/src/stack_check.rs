//! Stack overflow detection via canary words in IWRAM.
//!
//! Places a known magic value at a fixed IWRAM address between BSS and the
//! stack top. The GBA stack grows downward from 0x0300_7F00; BSS ends at
//! ~0x0300_0060. The canary at 0x0300_6000 triggers if the stack exceeds
//! ~8 KB, which is generous for this project.
//!
//! Call `init_canary()` once at startup, then `check_canary()` per frame.

const CANARY_ADDR: *mut u32 = 0x0300_6000 as *mut u32;
const CANARY_MAGIC: u32 = 0xDEAD_BEEF;
const CANARY_COUNT: usize = 4;

/// Write magic values to the canary region. Call before any deep stack usage.
pub fn init_canary() {
    for i in 0..CANARY_COUNT {
        unsafe {
            core::ptr::write_volatile(CANARY_ADDR.add(i), CANARY_MAGIC);
        }
    }
}

/// Returns true if the canary is intact (no stack overflow detected).
pub fn check_canary() -> bool {
    for i in 0..CANARY_COUNT {
        if unsafe { core::ptr::read_volatile(CANARY_ADDR.add(i)) } != CANARY_MAGIC {
            return false;
        }
    }
    true
}

// Message log — fixed circular buffer.
//
// The Rust version uses Vec<String> with unlimited history. On C64 we
// store the last 4 messages in a flat [u8; 160] buffer (4 × 40 chars).
// Only the 2 most recent are rendered on screen (rows 23-24).

use crate::c64;
use crate::entity;

const MSG_WIDTH: usize = 40;
const MSG_COUNT: usize = 4;

static mut MSGS: [[u8; MSG_WIDTH]; MSG_COUNT] = [[b' '; MSG_WIDTH]; MSG_COUNT];
static mut MSG_LENS: [u8; MSG_COUNT] = [0; MSG_COUNT];
static mut MSG_HEAD: u8 = 0; // next write position (circular)
static mut MSG_TOTAL: u16 = 0;

/// Clear all messages.
pub fn reset() {
    unsafe {
        for row in MSGS.iter_mut() {
            for ch in row.iter_mut() { *ch = b' '; }
        }
        for l in MSG_LENS.iter_mut() { *l = 0; }
        MSG_HEAD = 0;
        MSG_TOTAL = 0;
    }
}

/// Add a raw message (ASCII bytes, max 40 chars).
pub fn add(text: &[u8]) {
    unsafe {
        let slot = MSG_HEAD as usize;
        // Clear slot
        for ch in MSGS[slot].iter_mut() { *ch = b' '; }
        // Copy text
        let len = if text.len() > MSG_WIDTH { MSG_WIDTH } else { text.len() };
        for i in 0..len {
            MSGS[slot][i] = text[i];
        }
        MSG_LENS[slot] = len as u8;
        MSG_HEAD = ((slot + 1) % MSG_COUNT) as u8;
        MSG_TOTAL += 1;
    }
}

/// Get the Nth most recent message (0 = newest).
/// Returns the message bytes and length, or empty if not enough messages.
pub fn recent(n: u8) -> (&'static [u8; MSG_WIDTH], u8) {
    unsafe {
        if (n as u16) >= MSG_TOTAL || n >= MSG_COUNT as u8 {
            // Return empty
            static EMPTY: [u8; MSG_WIDTH] = [b' '; MSG_WIDTH];
            return (&EMPTY, 0);
        }
        let idx = ((MSG_HEAD as usize + MSG_COUNT - 1 - n as usize) % MSG_COUNT) as usize;
        (&MSGS[idx], MSG_LENS[idx])
    }
}

// --- Message formatting helpers ---
// These build messages by copying byte slices into a scratch buffer,
// since we can't use format!() in no_std without alloc.

static mut SCRATCH: [u8; MSG_WIDTH] = [0; MSG_WIDTH];

fn scratch_clear() {
    unsafe { for ch in SCRATCH.iter_mut() { *ch = b' '; } }
}

fn scratch_copy(pos: usize, text: &[u8]) -> usize {
    let mut p = pos;
    for &ch in text {
        if p >= MSG_WIDTH { break; }
        unsafe { SCRATCH[p] = ch; }
        p += 1;
    }
    p
}

fn scratch_num(pos: usize, val: u8) -> usize {
    let mut p = pos;
    if val >= 100 {
        if p < MSG_WIDTH { unsafe { SCRATCH[p] = b'0' + val / 100; } p += 1; }
    }
    if val >= 10 {
        if p < MSG_WIDTH { unsafe { SCRATCH[p] = b'0' + (val / 10) % 10; } p += 1; }
    }
    if p < MSG_WIDTH { unsafe { SCRATCH[p] = b'0' + val % 10; } p += 1; }
    p
}

fn scratch_submit() {
    unsafe { add(&SCRATCH); }
}

/// "{attacker} hits {defender} for {damage}."
pub fn add_hit_msg(attacker: u8, defender: u8, damage: u8) {
    scratch_clear();
    let mut p = 0;
    p = scratch_copy(p, entity::name(attacker));
    p = scratch_copy(p, b" hit ");
    p = scratch_copy(p, entity::name(defender));
    p = scratch_copy(p, b" for ");
    p = scratch_num(p, damage);
    let _ = scratch_copy(p, b".");
    scratch_submit();
}

/// "{attacker} attacks {defender} but no damage."
pub fn add_miss_msg(attacker: u8, defender: u8) {
    scratch_clear();
    let mut p = 0;
    p = scratch_copy(p, entity::name(attacker));
    p = scratch_copy(p, b" hit ");
    p = scratch_copy(p, entity::name(defender));
    let _ = scratch_copy(p, b": no damage.");
    scratch_submit();
}

/// "{name} is dead!"
pub fn add_death_msg(idx: u8) {
    scratch_clear();
    let mut p = 0;
    p = scratch_copy(p, entity::name(idx));
    let _ = scratch_copy(p, b" is dead!");
    scratch_submit();
}

/// "The {name} notices you!"
pub fn add_notice_msg(idx: u8) {
    scratch_clear();
    let mut p = 0;
    p = scratch_copy(p, b"The ");
    p = scratch_copy(p, entity::name(idx));
    let _ = scratch_copy(p, b" notices you!");
    scratch_submit();
}

/// Render the 2 most recent messages to screen rows 23 and 24.
pub fn render(msg_row_0: u8) {
    // Row 0: second most recent (dimmer)
    let (msg1, _) = recent(1);
    for i in 0..MSG_WIDTH {
        c64::draw_char(i as u8, msg_row_0, c64::to_screen_code(msg1[i]), c64::COLOR_GREY);
    }
    // Row 1: most recent (bright)
    let (msg0, _) = recent(0);
    for i in 0..MSG_WIDTH {
        c64::draw_char(i as u8, msg_row_0 + 1, c64::to_screen_code(msg0[i]), c64::COLOR_WHITE);
    }
}

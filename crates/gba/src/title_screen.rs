//! "Neon Dungeon" title screen — 80s-aesthetic with GBA hardware effects.
//!
//! Features: dungeon map backdrop on BG0 with neon palettes, mosaic entrance,
//! palette-animated title text, flickering torches, OAM cursor menu,
//! base36 seed entry roller.

use gba::prelude::*;

use roguelike_core::rules::seed_code;
use roguelike_core::tier_compact::map::{
    CompactMap, TILE_FLOOR, TILE_STAIRS_DOWN, TILE_STRUCTURAL, TILE_WALL,
};
use roguelike_core::tier_compact::prng::LfsrRng32;
use roguelike_core::tier_compact::types::{MAP_HEIGHT, MAP_WIDTH};

use crate::display;
use crate::palette;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Neon palbank for animated title + selected menu item.
const PALBANK_NEON: u16 = 15;
/// Palbank for dungeon walls (dim cyan glow).
const PALBANK_WALL: u16 = 10; // reuse Cyan
/// Palbank for dungeon floor (very dark).
const PALBANK_FLOOR_DIM: u16 = 3; // reuse DarkGrey
/// Palbank for torch flame (animated).
const PALBANK_FLAME: u16 = 4; // reuse Red palbank, animate fg
/// Palbank for static torch shaft/base.
const PALBANK_SHAFT: u16 = 2; // Grey
/// Palbank for dim cyan decorative line.
const PALBANK_LINE: u16 = 11; // DIM

/// Title row.
const TITLE_ROW: usize = 5;
/// Menu start row.
const MENU_ROW: usize = 8;
/// Title text (spaced).
const TITLE_TEXT: &str = "R O G U E L I K E";
/// Title X position (centered: (30 - 17) / 2 = 6.5, round to 7).
const TITLE_X: usize = 7;
/// Neon cycle period in frames (~4 seconds at 60fps).
const NEON_PERIOD: u16 = 240;

/// Sine lookup table (64 entries, range 0..=31) for smooth neon interpolation.
/// sin(i * 2π / 64) mapped to 0..31.
const SINE_LUT: [u8; 64] = [
    16, 17, 19, 21, 22, 24, 25, 27, 28, 29, 30, 30, 31, 31, 31, 31,
    31, 31, 31, 31, 30, 30, 29, 28, 27, 25, 24, 22, 21, 19, 17, 16,
    15, 14, 12, 10,  9,  7,  6,  4,  3,  2,  1,  1,  0,  0,  0,  0,
     0,  0,  0,  0,  1,  1,  2,  3,  4,  6,  7,  9, 10, 12, 14, 15,
];

/// Torch flame colors: orange, yellow, red (cycled per-frame).
const FLAME_COLORS: [Color; 6] = [
    Color::from_rgb(31, 16, 0),  // orange
    Color::from_rgb(31, 24, 0),  // yellow-orange
    Color::from_rgb(31, 31, 4),  // bright yellow
    Color::from_rgb(31, 20, 0),  // orange
    Color::from_rgb(28, 8, 0),   // dark orange
    Color::from_rgb(31, 12, 0),  // red-orange
];

/// Base36 character set for seed roller.
const BASE36: &[u8; 36] = b"0123456789abcdefghijklmnopqrstuvwxyz";

/// Fixed seed for the backdrop dungeon.
const BACKDROP_SEED: u32 = 0xDEAD_CAFE;

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// What the player chose on the title screen.
pub enum TitleAction {
    /// Start a new game with a timer-derived random seed.
    NewGame,
    /// Start a game with a specific seed.
    Seed(u32),
    /// Continue from a saved game.
    Continue,
}

/// Run the title screen. Blocks until the player picks an action.
/// `has_save` controls whether "Continue" appears in the menu.
#[inline(never)]
pub fn run_title(has_save: bool) -> TitleAction {
    // Clear both layers — gameplay HUD/map may still be visible.
    display::clear_hud();

    setup_neon_palettes();
    generate_backdrop();

    // Darken BG0 so the dungeon map is a subtle backdrop, not competing with text.
    BLDCNT.write(
        BlendControl::new()
            .with_mode(ColorEffectMode::Darken)
            .with_target1_bg0(true),
    );
    BLDY.write(8); // 8/16 = 50% brightness decrease

    // Enable mosaic on BG0 for entrance effect.
    let bg0 = BG0CNT.read();
    BG0CNT.write(bg0.with_mosaic(true));

    // OAM cursor setup.
    crate::cursor::init();

    let action = run_main_loop(has_save);

    // Cleanup: restore gameplay state.
    cleanup();
    action
}

// ---------------------------------------------------------------------------
// Main loop
// ---------------------------------------------------------------------------

fn run_main_loop(has_save: bool) -> TitleAction {
    let menu_count: u8 = if has_save { 4 } else { 3 };
    // Menu item indices: with save = [Continue, New Game, Enter Seed, Settings]
    //                    no save   = [New Game, Enter Seed, Settings]
    let mut frame: u16 = 0;
    let mut sel: u8 = 0;
    let mut entrance_done = false;

    loop {
        display::vblank_wait();

        // -- Entrance animation --
        if !entrance_done {
            // Mosaic: de-pixelate BG0 over 20 frames (level 10 → 0).
            let mosaic_level = if frame < 20 { 10 - (frame / 2) as u16 } else { 0 };
            MOSAIC.write(
                Mosaic::new()
                    .with_bg_h_extra(mosaic_level)
                    .with_bg_v_extra(mosaic_level),
            );

            // Title letters appear one per frame starting at frame 8.
            let letters_shown = if frame >= 8 {
                ((frame - 8) as usize).min(TITLE_TEXT.len())
            } else {
                0
            };
            if letters_shown > 0 {
                let partial = &TITLE_TEXT[..letters_shown];
                display::write_hud_string(TITLE_X, TITLE_ROW, partial, PALBANK_NEON);
            }

            // Decorative line appears at frame 18.
            if frame == 18 {
                for x in TITLE_X..(TITLE_X + TITLE_TEXT.len()) {
                    display::write_hud_tile(x, TITLE_ROW + 1, 0xC4, PALBANK_LINE); // ─
                }
            }

            // Torches appear after title text finishes (frame 26).
            if frame == 26 {
                render_torches();
            }

            // Menu fades in at frame 28.
            if frame == 28 {
                render_menu(sel, has_save);
            }

            if frame >= 34 {
                entrance_done = true;
                // Disable mosaic.
                let bg0 = BG0CNT.read();
                BG0CNT.write(bg0.with_mosaic(false));
                MOSAIC.write(Mosaic::new());
            }
        }

        // -- Per-frame animation (always runs) --
        animate_palettes(frame);

        // Background scroll (smooth diagonal drift — 1px per axis every 8 frames).
        if frame % 8 == 0 {
            let scroll = frame / 8;
            BG0HOFS.write(scroll & 0x1FF);
            BG0VOFS.write(scroll & 0x1FF);
        }

        // Cursor animation.
        if entrance_done {
            crate::cursor::update(MENU_X - 2, MENU_ROW + sel as usize * MENU_SPACING, frame, 0);
        }

        frame = frame.wrapping_add(1);

        // -- Input (only after entrance) --
        if !entrance_done {
            continue;
        }

        if let Some(input) = read_title_input() {
            match input {
                TitleInput::Up => {
                    if sel > 0 {
                        sel -= 1;
                        render_menu(sel, has_save);
                    }
                }
                TitleInput::Down => {
                    if sel < menu_count - 1 {
                        sel += 1;
                        render_menu(sel, has_save);
                    }
                }
                TitleInput::Confirm => {
                    // Resolve menu index to action. With save: 0=Continue,
                    // 1=New, 2=Seed, 3=Settings. Without: 0=New, 1=Seed, 2=Settings.
                    let action_idx = if has_save { sel } else { sel + 1 };
                    match action_idx {
                        0 => return TitleAction::Continue,
                        1 => return TitleAction::NewGame,
                        2 => {
                            if let Some(seed) = run_seed_entry(frame) {
                                return TitleAction::Seed(seed);
                            }
                            // Cancelled — redraw menu.
                            display::clear_hud();
                            display::write_hud_string(
                                TITLE_X, TITLE_ROW, TITLE_TEXT, PALBANK_NEON,
                            );
                            for x in TITLE_X..(TITLE_X + TITLE_TEXT.len()) {
                                display::write_hud_tile(x, TITLE_ROW + 1, 0xC4, PALBANK_LINE);
                            }
                            render_torches();
                            render_menu(sel, has_save);
                        }
                        3 => {
                            crate::settings_menu::run_settings();
                            // Restore title screen state after settings closes.
                            // Settings cleanup resets BLDCNT and OAM.
                            crate::cursor::init();
                            BLDCNT.write(
                                BlendControl::new()
                                    .with_mode(ColorEffectMode::Darken)
                                    .with_target1_bg0(true),
                            );
                            BLDY.write(8);
                            display::clear_hud();
                            display::write_hud_string(
                                TITLE_X, TITLE_ROW, TITLE_TEXT, PALBANK_NEON,
                            );
                            for x in TITLE_X..(TITLE_X + TITLE_TEXT.len()) {
                                display::write_hud_tile(x, TITLE_ROW + 1, 0xC4, PALBANK_LINE);
                            }
                            render_torches();
                            render_menu(sel, has_save);
                        }
                        _ => {}
                    }
                }
                TitleInput::Cancel => {} // No action on B at title
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Dungeon backdrop
// ---------------------------------------------------------------------------

fn generate_backdrop() {
    // Generate a dungeon map on the stack and render to BG0.
    let mut map = CompactMap::new(MAP_WIDTH, MAP_HEIGHT);
    let mut rng = LfsrRng32::new(BACKDROP_SEED);
    let _ = map.generate(&mut rng);

    // Write the 32×32 screenblock region (wrapping viewport).
    for sy in 0..20u16 {
        for sx in 0..30u16 {
            let mx = sx as i32;
            let my = sy as i32;
            let tile = if map.in_bounds(mx, my) {
                map.tile_at(mx, my)
            } else {
                TILE_WALL
            };
            let (glyph, pal) = match tile {
                TILE_FLOOR => (b'.', PALBANK_FLOOR_DIM),
                TILE_STAIRS_DOWN => (b'>', PALBANK_WALL),
                TILE_STRUCTURAL => (b'#', PALBANK_WALL),
                _ => (b' ', 0), // TILE_WALL = empty/black
            };
            display::write_map_tile(sx as usize, sy as usize, glyph, pal);
        }
    }

    // Also fill the rest of the 32-wide screenblock for scroll wrapping.
    let sb = TEXT_SCREENBLOCKS.get_frame(30).unwrap();
    let empty = TextEntry::new();
    for sy in 0..32 {
        let row = sb.get_row(sy).unwrap();
        for sx in 30..32 {
            row.index(sx).write(empty);
        }
    }
    for sy in 20..32 {
        let row = sb.get_row(sy).unwrap();
        for sx in 0..30 {
            // Continue the map for scroll wrapping.
            let mx = sx as i32;
            let my = sy as i32;
            let tile = if map.in_bounds(mx, my) {
                map.tile_at(mx, my)
            } else {
                TILE_WALL
            };
            let (glyph, pal) = match tile {
                TILE_FLOOR => (b'.', PALBANK_FLOOR_DIM),
                TILE_STAIRS_DOWN => (b'>', PALBANK_WALL),
                TILE_STRUCTURAL => (b'#', PALBANK_WALL),
                _ => (b' ', 0),
            };
            let entry = TextEntry::new()
                .with_tile(glyph as u16)
                .with_palbank(pal);
            row.index(sx as usize).write(entry);
        }
    }
}

// ---------------------------------------------------------------------------
// Palette animation
// ---------------------------------------------------------------------------

fn setup_neon_palettes() {
    // Palbank 15 (NEON): animated — set initial color.
    let pal = bg_palbank(PALBANK_NEON as usize);
    pal.index(0).write(Color::from_rgb(0, 0, 0)); // bg transparent
    pal.index(1).write(Color::from_rgb(31, 0, 20)); // start at hot pink

    // Dungeon wall palbank override: dim cyan glow.
    let wall_pal = bg_palbank(PALBANK_WALL as usize);
    wall_pal.index(1).write(Color::from_rgb(0, 14, 18));

    // Dungeon floor: very dark blue.
    let floor_pal = bg_palbank(PALBANK_FLOOR_DIM as usize);
    floor_pal.index(1).write(Color::from_rgb(3, 3, 8));
}

fn animate_palettes(frame: u16) {
    // Neon title: interpolate between hot pink (31,0,20) and cyan (0,25,31).
    let idx = ((frame as u32 * 64 / NEON_PERIOD as u32) % 64) as usize;
    let t = SINE_LUT[idx]; // 0..31

    // Lerp: color = pink * (31-t)/31 + cyan * t/31
    // Do all math in u32 (ARM7-native), cast to u16 only for the MMIO write.
    let t32 = t as u32;
    let r = (31 * (31 - t32)) / 31;
    let g = (25 * t32) / 31;
    let b = (20 * (31 - t32) + 31 * t32) / 31;

    let neon_color = Color::from_rgb(r.min(31) as u16, g.min(31) as u16, b.min(31) as u16);
    bg_palbank(PALBANK_NEON as usize)
        .index(1)
        .write(neon_color);

    // Torch flames: cycle through FLAME_COLORS.
    let flame_idx = (frame as usize / 4) % FLAME_COLORS.len();
    let flame_idx2 = ((frame as usize + 2) / 4) % FLAME_COLORS.len(); // offset for right torch
    bg_palbank(PALBANK_FLAME as usize)
        .index(1)
        .write(FLAME_COLORS[flame_idx]);

    // Right torch uses a slightly different phase — write to the flame tiles directly
    // (both torches share the same palbank, so we use a single color — the phase offset
    // still creates subtle variation since the frames aren't synchronized with rendering).
    let _ = flame_idx2; // Both torches share palbank; accept same color for simplicity.
}

// ---------------------------------------------------------------------------
// Torches
// ---------------------------------------------------------------------------

fn render_torches() {
    // Torches flank the title. Title is at TITLE_X=6..23 (17 chars).
    // Left torch at column 4, right at column 24 — symmetric.
    // Base (╨) sits one row below the title text for visual grounding.
    let base = TITLE_ROW + 1;
    let lx = TITLE_X - 2;
    let rx = TITLE_X + TITLE_TEXT.len() + 1;

    // Left torch (4 tiles tall: flame, shaft, shaft, base).
    display::write_hud_tile(lx, base - 3, b'*', PALBANK_FLAME);
    display::write_hud_tile(lx, base - 2, 0xB3, PALBANK_SHAFT); // │
    display::write_hud_tile(lx, base - 1, 0xB3, PALBANK_SHAFT); // │
    display::write_hud_tile(lx, base, 0xD0, PALBANK_SHAFT);     // ╨

    // Right torch.
    display::write_hud_tile(rx, base - 3, b'*', PALBANK_FLAME);
    display::write_hud_tile(rx, base - 2, 0xB3, PALBANK_SHAFT);
    display::write_hud_tile(rx, base - 1, 0xB3, PALBANK_SHAFT);
    display::write_hud_tile(rx, base, 0xD0, PALBANK_SHAFT);
}

// ---------------------------------------------------------------------------
// Menu
// ---------------------------------------------------------------------------

const MENU_ITEMS_SAVE: [&str; 4] = ["Continue", "New Game", "Enter Seed", "Settings"];
const MENU_ITEMS_NO_SAVE: [&str; 3] = ["New Game", "Enter Seed", "Settings"];
/// Menu X: centered-ish between torches, left-justified.
const MENU_X: usize = 10;
/// Vertical spacing between menu items (every other row).
const MENU_SPACING: usize = 2;

fn render_menu(sel: u8, has_save: bool) {
    let count = if has_save { 4usize } else { 3 };
    for i in 0..count {
        let label = if has_save {
            MENU_ITEMS_SAVE[i]
        } else {
            MENU_ITEMS_NO_SAVE[i]
        };
        let row = MENU_ROW + i * MENU_SPACING;
        let pal = if i as u8 == sel {
            PALBANK_NEON
        } else {
            2 // Grey
        };
        for x in MENU_X..29 {
            display::write_hud_tile(x, row, b' ', 0);
        }
        display::write_hud_string(MENU_X, row, label, pal);
    }
    // Clear any leftover row from a previous 4-item render when now showing 3.
    if !has_save {
        let row = MENU_ROW + 3 * MENU_SPACING;
        for x in MENU_X..29 {
            display::write_hud_tile(x, row, b' ', 0);
        }
    }
}

// ---------------------------------------------------------------------------
// Input
// ---------------------------------------------------------------------------

enum TitleInput {
    Up,
    Down,
    Confirm,
    Cancel,
}

fn read_title_input() -> Option<TitleInput> {
    let pressed = !KEYINPUT.read().to_u16() & 0x03FF;

    // Simple edge detection via static state.
    static mut PREV: u16 = 0;
    let prev = unsafe { PREV };
    let edges = pressed & !prev;
    unsafe { PREV = pressed };

    if edges & (1 << 6) != 0 {
        return Some(TitleInput::Up);
    }
    if edges & (1 << 7) != 0 {
        return Some(TitleInput::Down);
    }
    if edges & (1 << 0) != 0 {
        return Some(TitleInput::Confirm);
    }
    if edges & (1 << 1) != 0 {
        return Some(TitleInput::Cancel);
    }

    None
}

// ---------------------------------------------------------------------------
// Seed entry roller
// ---------------------------------------------------------------------------

/// Run the base36 seed entry roller. Returns Some(seed) on confirm, None on cancel.
fn run_seed_entry(mut frame: u16) -> Option<u32> {
    const NUM_CHARS: usize = 7; // 7 base36 chars covers full u32 range
    let mut chars = [0u8; NUM_CHARS]; // indices into BASE36 (all start at '0')
    let mut cursor: usize = 0;

    // Clear HUD for seed entry and hide menu cursor.
    display::clear_hud();
    crate::cursor::hide();
    render_torches();

    // "ENTER SEED CODE" = 15 chars, centered between torches (cols 5-25).
    // (25 - 5 - 15) / 2 + 5 + 1 = 8. Underline matches.
    display::write_hud_string(8, TITLE_ROW, "ENTER SEED CODE", PALBANK_NEON);
    for x in 8..(8 + 15) {
        display::write_hud_tile(x, TITLE_ROW + 1, 0xC4, PALBANK_LINE); // ─
    }

    render_seed_display(&chars, cursor);
    //                   "U/D:char   D-pad:move"  (21 chars, centered: (30-21)/2 = 4)
    //                   "A:confirm   B:cancel"   (19 chars, centered: (30-19)/2 = 5)
    display::write_hud_string(5, 13, "U/D:char   D-pad:move", 2);
    display::write_hud_string(5, 14, "A:confirm  B:cancel", 2);

    // Wait one frame to consume the A press that entered this screen.
    display::vblank_wait();
    let _ = read_seed_input();

    loop {
        display::vblank_wait();
        frame = frame.wrapping_add(1);
        animate_palettes(frame);

        // Background scroll continues.
        if frame % 8 == 0 {
            let scroll = frame / 8;
            BG0HOFS.write(scroll & 0x1FF);
            BG0VOFS.write(scroll & 0x1FF);
        }

        if let Some(input) = read_seed_input() {
            match input {
                SeedInput::Up => {
                    chars[cursor] = (chars[cursor] + 1) % 36;
                    render_seed_display(&chars, cursor);
                }
                SeedInput::Down => {
                    chars[cursor] = if chars[cursor] == 0 { 35 } else { chars[cursor] - 1 };
                    render_seed_display(&chars, cursor);
                }
                SeedInput::Left => {
                    if cursor > 0 {
                        cursor -= 1;
                        render_seed_display(&chars, cursor);
                    }
                }
                SeedInput::Right => {
                    if cursor < NUM_CHARS - 1 {
                        cursor += 1;
                        render_seed_display(&chars, cursor);
                    }
                }
                SeedInput::Confirm => {
                    // Decode the base36 string, stripping leading zeros.
                    let mut buf = [0u8; NUM_CHARS];
                    for i in 0..NUM_CHARS {
                        buf[i] = BASE36[chars[i] as usize];
                    }
                    // Find first non-zero character.
                    let start = buf.iter().position(|&b| b != b'0').unwrap_or(NUM_CHARS - 1);
                    match seed_code::decode_from_bytes(&buf[start..]) {
                        Ok(seed) if seed > 0 && seed <= 0xFFFF_FFFF => {
                            return Some(seed as u32);
                        }
                        _ => {
                            // Invalid or zero seed — flash error.
                            display::write_hud_string(8, 10, "Invalid seed!", 4);
                            for _ in 0..30 {
                                display::vblank_wait();
                            }
                            for x in 0..30 {
                                display::write_hud_tile(x, 10, b' ', 0);
                            }
                        }
                    }
                }
                SeedInput::Cancel => {
                    return None;
                }
            }
        }
    }
}

fn render_seed_display(chars: &[u8; 7], cursor: usize) {
    // 7 chars at 2-wide spacing = 14 tiles, plus 2 brackets = 16.
    // Centered between torches (cols 5-25): (20-16)/2 + 5 + 1 = 8. Bracket at 8, first char at 9.
    let base_x = 9;
    let row = 8;

    // Clear rows.
    for x in 0..30 {
        display::write_hud_tile(x, row, b' ', 0);
        display::write_hud_tile(x, row + 1, b' ', 0);
    }

    // Brackets.
    display::write_hud_tile(base_x - 1, row, b'[', palette::PALBANK_DIM);
    display::write_hud_tile(base_x + 7 * 2, row, b']', palette::PALBANK_DIM);

    // Characters.
    for i in 0..7 {
        let ch = BASE36[chars[i] as usize];
        let pal = if i == cursor { PALBANK_NEON } else { 1 };
        display::write_hud_tile(base_x + i * 2, row, ch, pal);
    }

    // Cursor indicator below.
    display::write_hud_tile(base_x + cursor * 2, row + 1, b'^', PALBANK_NEON);
}

enum SeedInput {
    Up,
    Down,
    Left,
    Right,
    Confirm,
    Cancel,
}

fn read_seed_input() -> Option<SeedInput> {
    let pressed = !KEYINPUT.read().to_u16() & 0x03FF;

    static mut PREV: u16 = 0;
    let prev = unsafe { PREV };
    let edges = pressed & !prev;
    unsafe { PREV = pressed };

    if edges & (1 << 6) != 0 { return Some(SeedInput::Up); }
    if edges & (1 << 7) != 0 { return Some(SeedInput::Down); }
    if edges & (1 << 5) != 0 { return Some(SeedInput::Left); }
    if edges & (1 << 4) != 0 { return Some(SeedInput::Right); }
    if edges & (1 << 0) != 0 { return Some(SeedInput::Confirm); }
    if edges & (1 << 1) != 0 { return Some(SeedInput::Cancel); }

    None
}

// ---------------------------------------------------------------------------
// Cleanup
// ---------------------------------------------------------------------------

fn cleanup() {
    crate::cursor::hide();
    crate::cursor::disable_obj_layer();

    // Reset scroll, mosaic, and blend.
    BG0HOFS.write(0);
    BG0VOFS.write(0);
    BG1HOFS.write(0);
    MOSAIC.write(Mosaic::new());
    BLDCNT.write(BlendControl::new());
    BLDY.write(0);
    let bg0 = BG0CNT.read();
    BG0CNT.write(bg0.with_mosaic(false));

    // Restore gameplay palettes.
    palette::init_palette();

    // Clear both layers.
    display::clear_hud();
    let sb = TEXT_SCREENBLOCKS.get_frame(30).unwrap();
    let empty = TextEntry::new();
    for y in 0..32 {
        let row = sb.get_row(y).unwrap();
        for x in 0..32 {
            row.index(x).write(empty);
        }
    }
}

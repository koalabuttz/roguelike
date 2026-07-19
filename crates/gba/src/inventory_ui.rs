//! GBA inventory modal — full-screen overlay with hardware polish.
//!
//! Features: OAM sprite cursor with bob animation, BG0 darken blend,
//! BG1 slide-in/out transition, per-item palette colors, detail panel,
//! hybrid action model (A=primary instant, L=secondary submenu).

use gba::prelude::*;

use roguelike_core::command::GameCommand;
use roguelike_core::rules::game_view::GameView;
use roguelike_core::rules::items::{
    attack_bonus, color, defense_bonus, defense_boost, heal_amount, is_armor, is_consumable,
    is_weapon, name, ItemKind,
};

use crate::display;
use crate::format;
use crate::input::{self, InventoryInput, SubmenuInput};
use crate::palette::{PALBANK_DIM, PALBANK_MSG, PALBANK_STATUS};

/// Palbank for the selected/highlighted item — bright yellow foreground.
/// We don't use PALBANK_SEL (inverse video) because BG palette index 0
/// is always transparent on GBA, making the "yellow background" invisible.
const PALBANK_SEL: u16 = 8; // GameColor::Yellow

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// First row of the item list.
const LIST_ROW_START: usize = 2;

/// Row for the detail panel separator.
const DETAIL_SEP_ROW: usize = 15;
/// Row for item detail name + stats.
const DETAIL_ROW: usize = 16;
/// Row for action hints.
const HINTS_ROW: usize = 18;

/// Slide transition: frames and pixels per frame.
const SLIDE_FRAMES: u8 = 6;
const SLIDE_PX_PER_FRAME: u16 = 40;

/// Submenu X position (tile column).
const SUBMENU_X: usize = 20;

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

enum InvMode {
    SlideIn { frame: u8 },
    Browse,
    Submenu { sel: u8 },
    CombineSelect { target_slot: u8 },
    SlideOut { frame: u8 },
}

struct InvState {
    mode: InvMode,
    cursor: u8,
    frame_counter: u16,
    needs_redraw: bool,
}

// ---------------------------------------------------------------------------
// Cleanup
// ---------------------------------------------------------------------------

fn cleanup() {
    crate::cursor::hide();
    crate::cursor::disable_obj_layer();
    crate::menu::disable_dim();
    BG1HOFS.write(0);
    display::clear_hud();
}

// ---------------------------------------------------------------------------
// Item list helpers
// ---------------------------------------------------------------------------

fn equip_count(state: &impl GameView) -> u8 {
    state.equipment().weapon.is_some() as u8 + state.equipment().armor.is_some() as u8
}

fn total_items(state: &impl GameView) -> u8 {
    equip_count(state) + state.inventory().len() as u8
}

/// Clamp cursor to valid range after items change.
fn clamp_cursor(state: &impl GameView, cursor: &mut u8) {
    let total = total_items(state);
    if total == 0 {
        *cursor = 0;
    } else if *cursor >= total {
        *cursor = total - 1;
    }
}

/// Number of screen rows consumed by equipped items (including blank separator).
fn equip_rows(state: &impl GameView) -> usize {
    let ec = equip_count(state) as usize;
    if ec > 0 { ec + 1 } else { 0 }
}

/// Compute the first inventory item index to display, given cursor position.
/// Equipped items are always visible; only inventory items scroll.
fn inv_scroll(state: &impl GameView, cursor: u8) -> usize {
    let ec = equip_count(state);
    if cursor < ec {
        return 0;
    }
    let inv_visible = (DETAIL_SEP_ROW - LIST_ROW_START).saturating_sub(equip_rows(state));
    if inv_visible == 0 {
        return 0;
    }
    let inv_cursor = (cursor - ec) as usize;
    inv_cursor.saturating_sub(inv_visible - 1)
}

/// Convert a logical cursor position to the screen row it occupies.
fn cursor_screen_row(state: &impl GameView, cursor: u8) -> usize {
    let ec = equip_count(state);
    if cursor < ec {
        LIST_ROW_START + cursor as usize
    } else {
        let er = equip_rows(state);
        let scroll = inv_scroll(state, cursor);
        let inv_cursor = (cursor - ec) as usize;
        LIST_ROW_START + er + (inv_cursor - scroll)
    }
}

/// Get the item kind and whether it's equipped, for a given visual cursor position.
fn item_at_cursor(state: &impl GameView, cursor: u8) -> Option<CursorItem> {
    let ec = equip_count(state);
    if cursor < ec {
        // Equipped item
        let mut idx = 0u8;
        if let Some(kind) = state.equipment().weapon {
            if cursor == idx {
                return Some(CursorItem::Equipped { kind, is_weapon: true });
            }
            idx += 1;
        }
        if let Some(kind) = state.equipment().armor {
            if cursor == idx {
                return Some(CursorItem::Equipped { kind, is_weapon: false });
            }
        }
        None
    } else {
        let inv_idx = (cursor - ec) as usize;
        state
            .inventory()
            .nth_occupied(inv_idx)
            .map(|(slot_idx, slot)| CursorItem::Inventory {
                slot_idx: slot_idx as u8,
                kind: slot.kind,
            })
    }
}

enum CursorItem {
    Equipped { kind: ItemKind, is_weapon: bool },
    Inventory { slot_idx: u8, kind: ItemKind },
}

// ---------------------------------------------------------------------------
// Actions
// ---------------------------------------------------------------------------

/// A-button: context-smart primary action.
fn primary_action(state: &impl GameView, cursor: u8) -> Option<GameCommand> {
    match item_at_cursor(state, cursor)? {
        CursorItem::Equipped { is_weapon: true, .. } => Some(GameCommand::UnequipWeapon),
        CursorItem::Equipped { is_weapon: false, .. } => Some(GameCommand::UnequipArmor),
        CursorItem::Inventory { slot_idx, kind, .. } => {
            if is_consumable(kind) {
                Some(GameCommand::UseItem(slot_idx))
            } else {
                Some(GameCommand::EquipItem(slot_idx))
            }
        }
    }
}

/// Number of submenu options for the item at cursor.
fn submenu_count(state: &impl GameView, cursor: u8) -> u8 {
    let ec = equip_count(state);
    if cursor < ec { 1 } else { 2 } // Equipped: [Drop]. Inventory: [Drop, Combine].
}

/// Execute a submenu option. Returns true if the inventory modal should continue.
fn execute_submenu(state: &mut impl GameView, cursor: u8, sel: u8, mode: &mut InvMode) {
    let ec = equip_count(state);
    if cursor < ec {
        // Equipped item — only option is Drop (sel=0)
        if let Some(item) = item_at_cursor(state, cursor) {
            match item {
                CursorItem::Equipped { is_weapon: true, .. } => {
                    state.step_view(GameCommand::DropEquippedWeapon);
                }
                CursorItem::Equipped { is_weapon: false, .. } => {
                    state.step_view(GameCommand::DropEquippedArmor);
                }
                _ => {}
            }
        }
        *mode = InvMode::Browse;
    } else {
        match sel {
            0 => {
                // Drop
                if let Some(CursorItem::Inventory { slot_idx, .. }) = item_at_cursor(state, cursor) {
                    state.step_view(GameCommand::DropItem(slot_idx));
                }
                *mode = InvMode::Browse;
            }
            1 => {
                // Combine — enter target selection mode
                if let Some(CursorItem::Inventory { slot_idx, .. }) = item_at_cursor(state, cursor) {
                    *mode = InvMode::CombineSelect { target_slot: slot_idx };
                } else {
                    *mode = InvMode::Browse;
                }
            }
            _ => *mode = InvMode::Browse,
        }
    }
}

fn execute_combine(state: &mut impl GameView, target_slot: u8, source_cursor: u8) {
    let ec = equip_count(state);
    if source_cursor < ec {
        return; // Can't combine with equipped items
    }
    let source_inv = (source_cursor - ec) as usize;
    if let Some((source_slot_idx, _)) = state.inventory().nth_occupied(source_inv) {
        let source_slot = source_slot_idx as u8;
        if source_slot != target_slot {
            state.step_view(GameCommand::Combine(target_slot, source_slot));
        }
    }
}

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

fn render_screen(state: &impl GameView, inv: &InvState) {
    display::clear_hud();

    // Title
    display::write_hud_string(1, 0, "INVENTORY", PALBANK_SEL);

    let ec = equip_count(state);
    let mut row = LIST_ROW_START;

    // Equipped items
    if ec > 0 {
        let mut equip_vis: u8 = 0;
        if let Some(kind) = state.equipment().weapon {
            let selected = inv.cursor == equip_vis;
            let pal = if selected { PALBANK_SEL } else { color(kind) as u16 };
            display::write_hud_string(1, row, "W:", pal);
            write_item_name(3, row, kind, pal);
            write_equip_stat_suffix(row, kind, true);
            row += 1;
            equip_vis += 1;
        }
        if let Some(kind) = state.equipment().armor {
            let selected = inv.cursor == equip_vis;
            let pal = if selected { PALBANK_SEL } else { color(kind) as u16 };
            display::write_hud_string(1, row, "A:", pal);
            write_item_name(3, row, kind, pal);
            write_equip_stat_suffix(row, kind, false);
            row += 1;
        }
        row += 1; // blank separator after equipped
    }

    // Inventory items (scrollable)
    let inv_visible = DETAIL_SEP_ROW.saturating_sub(row);
    let scroll = inv_scroll(state, inv.cursor);
    let inv_len = state.inventory().len();
    for vis_idx in 0..inv_visible {
        let item_idx = scroll + vis_idx;
        if let Some((_slot_idx, slot)) = state.inventory().nth_occupied(item_idx) {
            let logical = ec as usize + item_idx;
            let selected = inv.cursor as usize == logical;
            let pal = if selected { PALBANK_SEL } else { color(slot.kind) as u16 };

            let letter = b'a' + item_idx as u8;
            display::write_hud_tile(1, row, letter, pal);
            display::write_hud_tile(2, row, b')', pal);
            write_item_name(3, row, slot.kind, pal);

            // Stack count
            if slot.count > 1 {
                let mut buf = [b' '; 4];
                buf[0] = b'x';
                let p = format::write_u16(&mut buf, 1, slot.count as u16);
                if let Ok(s) = core::str::from_utf8(&buf[..p]) {
                    display::write_hud_string(25, row, s, PALBANK_DIM);
                }
            }
            row += 1;
        } else {
            break;
        }
    }

    // Scroll indicators
    if scroll > 0 {
        display::write_hud_tile(0, LIST_ROW_START + equip_rows(state), 0x1E, PALBANK_DIM);
    }
    if scroll + inv_visible < inv_len {
        display::write_hud_tile(0, DETAIL_SEP_ROW - 1, 0x1F, PALBANK_DIM);
    }

    if total_items(state) == 0 {
        display::write_hud_string(1, row, "Empty", PALBANK_DIM);
    }

    // Separator
    for x in 1..29 {
        display::write_hud_tile(x, DETAIL_SEP_ROW, 0xC4, PALBANK_DIM); // ─
    }

    // Detail panel
    render_detail(state, inv.cursor);

    // Action hints
    render_hints(state, inv);

    // Submenu overlay
    if let InvMode::Submenu { sel } = inv.mode {
        render_submenu(state, inv.cursor, sel);
    }

    // Combine mode indicator
    if let InvMode::CombineSelect { .. } = inv.mode {
        display::write_hud_string(1, 19, "Pick source (A), B=cancel", PALBANK_SEL);
    }
}

fn write_item_name(x: usize, y: usize, kind: ItemKind, pal: u16) {
    let n = name(kind);
    display::write_hud_string(x, y, n, pal);
}

fn write_equip_stat_suffix(row: usize, kind: ItemKind, weapon: bool) {
    let mut buf = [b' '; 8];
    let mut p = 0;
    if weapon {
        let atk = attack_bonus(kind);
        if atk > 0 {
            p = format::write_str(&mut buf, p, "ATK+");
            p = format::write_u16(&mut buf, p, atk as u16);
        }
    } else {
        let def = defense_bonus(kind);
        if def > 0 {
            p = format::write_str(&mut buf, p, "DEF+");
            p = format::write_u16(&mut buf, p, def as u16);
        }
    }
    if p > 0 {
        if let Ok(s) = core::str::from_utf8(&buf[..p]) {
            display::write_hud_string(22, row, s, PALBANK_STATUS);
        }
    }
}

fn render_detail(state: &impl GameView, cursor: u8) {
    let item = match item_at_cursor(state, cursor) {
        Some(i) => i,
        None => return,
    };

    let (kind, show_atk, show_def, show_heal) = match item {
        CursorItem::Equipped { kind, is_weapon } => {
            if is_weapon {
                (kind, true, false, false)
            } else {
                (kind, false, true, false)
            }
        }
        CursorItem::Inventory { kind, .. } => {
            (kind, is_weapon(kind), is_armor(kind), is_consumable(kind))
        }
    };

    // Item name
    display::write_hud_string(1, DETAIL_ROW, name(kind), color(kind) as u16);

    // Stats line
    let mut buf = [b' '; 28];
    let mut p = 0;
    if show_atk {
        let atk = attack_bonus(kind);
        if atk > 0 {
            p = format::write_str(&mut buf, p, "ATK+");
            p = format::write_u16(&mut buf, p, atk as u16);
            buf[p] = b' ';
            p += 1;
        }
    }
    if show_def {
        let def = defense_bonus(kind);
        if def > 0 {
            p = format::write_str(&mut buf, p, "DEF+");
            p = format::write_u16(&mut buf, p, def as u16);
            buf[p] = b' ';
            p += 1;
        }
    }
    if show_heal {
        let heal = heal_amount(kind);
        if heal > 0 {
            p = format::write_str(&mut buf, p, "Heal:");
            p = format::write_u16(&mut buf, p, heal as u16);
        }
        let boost = defense_boost(kind);
        if boost > 0 {
            p = format::write_str(&mut buf, p, "DEF+");
            p = format::write_u16(&mut buf, p, boost as u16);
        }
    }
    if p > 0 {
        if let Ok(s) = core::str::from_utf8(&buf[..p]) {
            display::write_hud_string(1, DETAIL_ROW + 1, s.trim_end(), PALBANK_STATUS);
        }
    }
}

fn render_hints(state: &impl GameView, inv: &InvState) {
    if total_items(state) == 0 {
        display::write_hud_string(1, HINTS_ROW, "B:Close", PALBANK_DIM);
        return;
    }

    if let InvMode::CombineSelect { .. } = inv.mode {
        return; // Combine prompt shown on row 19 instead
    }

    match item_at_cursor(state, inv.cursor) {
        Some(CursorItem::Equipped { .. }) => {
            display::write_hud_string(1, HINTS_ROW, "A:Unequip L:More B:Close", PALBANK_DIM);
        }
        Some(CursorItem::Inventory { kind, .. }) => {
            if is_consumable(kind) {
                display::write_hud_string(1, HINTS_ROW, "A:Use L:More B:Close", PALBANK_DIM);
            } else {
                display::write_hud_string(1, HINTS_ROW, "A:Equip L:More B:Close", PALBANK_DIM);
            }
        }
        None => {
            display::write_hud_string(1, HINTS_ROW, "B:Close", PALBANK_DIM);
        }
    }
}

fn render_submenu(state: &impl GameView, cursor: u8, sel: u8) {
    let ec = equip_count(state);
    let is_equipped = cursor < ec;

    // Position submenu near the cursor's screen row
    let sy = cursor_screen_row(state, cursor).min(display::SCREEN_ROWS - 4);

    if is_equipped {
        let pal = if sel == 0 { PALBANK_SEL } else { PALBANK_MSG };
        display::write_hud_string(SUBMENU_X, sy, "Drop", pal);
    } else {
        let pal0 = if sel == 0 { PALBANK_SEL } else { PALBANK_MSG };
        let pal1 = if sel == 1 { PALBANK_SEL } else { PALBANK_MSG };
        display::write_hud_string(SUBMENU_X, sy, "Drop", pal0);
        display::write_hud_string(SUBMENU_X, sy + 1, "Combine", pal1);
    }
}

// ---------------------------------------------------------------------------
// Main entry point
// ---------------------------------------------------------------------------

/// Run the inventory modal. Blocks until the player closes it.
#[inline(never)]
pub fn run_inventory(state: &mut impl GameView) {
    let mut inv = InvState {
        mode: InvMode::SlideIn { frame: 0 },
        cursor: 0,
        frame_counter: 0,
        needs_redraw: true,
    };

    crate::cursor::init();
    crate::menu::enable_dim();

    // Pre-render content for slide-in
    render_screen(state, &inv);

    loop {
        display::vblank_wait();
        inv.frame_counter = inv.frame_counter.wrapping_add(1);

        match inv.mode {
            InvMode::SlideIn { ref mut frame } => {
                let remaining = SLIDE_FRAMES - *frame;
                let offset = remaining as u16 * SLIDE_PX_PER_FRAME;
                BG1HOFS.write(offset);
                crate::cursor::update(0, cursor_screen_row(state, inv.cursor), inv.frame_counter, offset);
                *frame += 1;
                if *frame >= SLIDE_FRAMES {
                    BG1HOFS.write(0);
                    inv.mode = InvMode::Browse;
                }
            }

            InvMode::SlideOut { ref mut frame } => {
                let offset = (*frame as u16 + 1) * SLIDE_PX_PER_FRAME;
                BG1HOFS.write(offset);
                crate::cursor::update(0, cursor_screen_row(state, inv.cursor), inv.frame_counter, offset);
                *frame += 1;
                if *frame >= SLIDE_FRAMES {
                    break;
                }
            }

            InvMode::Browse => {
                if inv.needs_redraw {
                    render_screen(state, &inv);
                    inv.needs_redraw = false;
                }
                crate::cursor::update(0, LIST_ROW_START + inv.cursor as usize, inv.frame_counter, 0);

                if let Some(input) = input::read_inventory_input() {
                    match input {
                        InventoryInput::Up => {
                            if inv.cursor > 0 {
                                inv.cursor -= 1;
                                inv.needs_redraw = true;
                            }
                        }
                        InventoryInput::Down => {
                            let total = total_items(state);
                            if total > 0 && inv.cursor < total - 1 {
                                inv.cursor += 1;
                                inv.needs_redraw = true;
                            }
                        }
                        InventoryInput::Primary => {
                            if let Some(cmd) = primary_action(state, inv.cursor) {
                                state.step_view(cmd);
                                clamp_cursor(state, &mut inv.cursor);
                                inv.needs_redraw = true;
                                if total_items(state) == 0 {
                                    inv.mode = InvMode::SlideOut { frame: 0 };
                                }
                            }
                        }
                        InventoryInput::Secondary => {
                            if total_items(state) > 0 {
                                inv.mode = InvMode::Submenu { sel: 0 };
                                inv.needs_redraw = true;
                            }
                        }
                        InventoryInput::Close => {
                            inv.mode = InvMode::SlideOut { frame: 0 };
                        }
                    }
                }
            }

            InvMode::Submenu { .. } => {
                // Copy sel out to avoid borrow conflict with &inv in render_screen.
                let mut sel = match inv.mode {
                    InvMode::Submenu { sel } => sel,
                    _ => unreachable!(),
                };

                if inv.needs_redraw {
                    render_screen(state, &inv);
                    inv.needs_redraw = false;
                }
                crate::cursor::update(0, LIST_ROW_START + inv.cursor as usize, inv.frame_counter, 0);

                if let Some(input) = input::read_submenu_input() {
                    let max = submenu_count(state, inv.cursor);
                    match input {
                        SubmenuInput::Up => {
                            if sel > 0 {
                                sel -= 1;
                                inv.needs_redraw = true;
                            }
                        }
                        SubmenuInput::Down => {
                            if sel < max - 1 {
                                sel += 1;
                                inv.needs_redraw = true;
                            }
                        }
                        SubmenuInput::Confirm => {
                            execute_submenu(state, inv.cursor, sel, &mut inv.mode);
                            clamp_cursor(state, &mut inv.cursor);
                            inv.needs_redraw = true;
                        }
                        SubmenuInput::Cancel => {
                            inv.mode = InvMode::Browse;
                            inv.needs_redraw = true;
                        }
                    }
                }

                // Write sel back if still in submenu mode.
                if let InvMode::Submenu { sel: ref mut s } = inv.mode {
                    *s = sel;
                }
            }

            InvMode::CombineSelect { target_slot } => {
                if inv.needs_redraw {
                    render_screen(state, &inv);
                    inv.needs_redraw = false;
                }
                crate::cursor::update(0, LIST_ROW_START + inv.cursor as usize, inv.frame_counter, 0);

                if let Some(input) = input::read_inventory_input() {
                    match input {
                        InventoryInput::Up => {
                            if inv.cursor > 0 {
                                inv.cursor -= 1;
                                inv.needs_redraw = true;
                            }
                        }
                        InventoryInput::Down => {
                            let total = total_items(state);
                            if total > 0 && inv.cursor < total - 1 {
                                inv.cursor += 1;
                                inv.needs_redraw = true;
                            }
                        }
                        InventoryInput::Primary => {
                            execute_combine(state, target_slot, inv.cursor);
                            inv.mode = InvMode::Browse;
                            clamp_cursor(state, &mut inv.cursor);
                            inv.needs_redraw = true;
                        }
                        InventoryInput::Close => {
                            inv.mode = InvMode::Browse;
                            inv.needs_redraw = true;
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    cleanup();
}

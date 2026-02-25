use std::io::Write;

use crossterm::{
    cursor, queue,
    style::{self, Color, SetBackgroundColor, SetForegroundColor},
    terminal,
};

use roguelike_core::game::{GameObservation, GameState};
use roguelike_core::item;
use roguelike_core::map::Tile;
use roguelike_core::platform::Renderer;
use roguelike_core::settings::{ColorPalette, Settings};
use roguelike_core::types::{Coord, GameColor};

#[cfg(all(debug_assertions, feature = "dev-tools"))]
use roguelike_core::dev_tools::OverlayCell;

/// Map a platform-independent `GameColor` to a crossterm terminal color.
pub fn to_crossterm_color(c: GameColor) -> Color {
    match c {
        GameColor::Black => Color::Black,
        GameColor::White => Color::White,
        GameColor::Grey => Color::Grey,
        GameColor::DarkGrey => Color::DarkGrey,
        GameColor::Red => Color::Red,
        GameColor::DarkRed => Color::DarkRed,
        GameColor::Green => Color::Green,
        GameColor::DarkGreen => Color::DarkGreen,
        GameColor::Yellow => Color::Yellow,
        GameColor::DarkBlue => Color::DarkBlue,
        GameColor::Cyan => Color::Cyan,
        GameColor::Rgb(r, g, b) => Color::Rgb { r, g, b },
    }
}

/// Remap a `GameColor` for protanopia (no red cones — red/green confused).
pub fn protanopia_color(c: GameColor) -> Color {
    match c {
        GameColor::Green => Color::Cyan,
        GameColor::DarkGreen => Color::DarkBlue,
        GameColor::Red => Color::Rgb {
            r: 255,
            g: 176,
            b: 0,
        },
        GameColor::DarkRed => Color::Rgb {
            r: 200,
            g: 130,
            b: 0,
        },
        _ => to_crossterm_color(c),
    }
}

/// Remap a `GameColor` for deuteranopia (no green cones — similar confusion).
pub fn deuteranopia_color(c: GameColor) -> Color {
    match c {
        GameColor::Green => Color::Cyan,
        GameColor::DarkGreen => Color::Rgb {
            r: 80,
            g: 80,
            b: 200,
        },
        GameColor::Red => Color::Rgb {
            r: 255,
            g: 140,
            b: 0,
        },
        GameColor::DarkRed => Color::Rgb {
            r: 200,
            g: 100,
            b: 0,
        },
        _ => to_crossterm_color(c),
    }
}

/// Remap a `GameColor` for high-contrast mode (maximum distinctness).
pub fn high_contrast_color(c: GameColor) -> Color {
    match c {
        GameColor::DarkGrey => Color::Rgb {
            r: 120,
            g: 120,
            b: 120,
        },
        GameColor::Green => Color::Cyan,
        GameColor::DarkGreen => Color::Rgb {
            r: 255,
            g: 60,
            b: 60,
        },
        GameColor::DarkRed => Color::Magenta,
        _ => to_crossterm_color(c),
    }
}

/// Map a `GameColor` through the selected palette to a crossterm `Color`.
pub fn palette_color(c: GameColor, palette: ColorPalette) -> Color {
    match palette {
        ColorPalette::Default => to_crossterm_color(c),
        ColorPalette::Protanopia => protanopia_color(c),
        ColorPalette::Deuteranopia => deuteranopia_color(c),
        ColorPalette::HighContrast => high_contrast_color(c),
    }
}

/// Terminal renderer backed by crossterm.
///
/// Generic over the output sink so it works with `Stdout` (terminal),
/// a buffered SSH channel writer, or any `Write` impl. Dimensions are
/// provided explicitly so the renderer doesn't depend on `terminal::size()`.
pub struct CrosstermRenderer<W: Write> {
    out: W,
    width: Coord,
    height: Coord,
}

impl<W: Write> CrosstermRenderer<W> {
    pub fn new(out: W, width: Coord, height: Coord) -> Self {
        Self { out, width, height }
    }

    /// Update the screen dimensions (e.g. on terminal resize or PTY resize).
    pub fn set_size(&mut self, width: Coord, height: Coord) {
        self.width = width;
        self.height = height;
    }

    /// Access the underlying writer (e.g. for direct crossterm operations).
    pub fn writer(&mut self) -> &mut W {
        &mut self.out
    }
}

impl<W: Write> Renderer for CrosstermRenderer<W> {
    fn clear(&mut self) {
        let _ = queue!(
            self.out,
            terminal::Clear(terminal::ClearType::All),
            cursor::MoveTo(0, 0)
        );
    }

    fn draw_char(&mut self, x: Coord, y: Coord, ch: char, fg: GameColor, bg: GameColor) {
        let _ = queue!(
            self.out,
            cursor::MoveTo(x as u16, y as u16),
            SetForegroundColor(to_crossterm_color(fg)),
            SetBackgroundColor(to_crossterm_color(bg)),
            style::Print(ch)
        );
    }

    fn draw_str(&mut self, x: Coord, y: Coord, text: &str, fg: GameColor, bg: GameColor) {
        let _ = queue!(
            self.out,
            cursor::MoveTo(x as u16, y as u16),
            SetForegroundColor(to_crossterm_color(fg)),
            SetBackgroundColor(to_crossterm_color(bg)),
            style::Print(text)
        );
    }

    fn flush(&mut self) {
        let _ = self.out.flush();
    }

    fn screen_size(&self) -> (Coord, Coord) {
        (self.width, self.height)
    }
}

pub fn render<W: Write>(
    w: &mut W,
    state: &GameState,
    screen_width: Coord,
    screen_height: Coord,
    settings: &Settings,
) -> std::io::Result<()> {
    let pal = settings.color_palette;
    render_map(w, state, pal)?;
    render_items(w, state, pal)?;
    render_entities(w, state, settings.show_corpses, pal)?;
    render_status_bar(w, state, screen_width, screen_height, settings)?;
    render_message_log(w, state, screen_width, screen_height, settings)?;

    w.flush()?;
    Ok(())
}

fn render_map<W: Write>(w: &mut W, state: &GameState, pal: ColorPalette) -> std::io::Result<()> {
    let map = &state.map;
    for y in 0..map.height {
        for x in 0..map.width {
            let is_visible = state.visible.contains(&(x, y));
            let is_explored = state.explored.contains(&(x, y));

            if is_visible {
                let idx = map.idx(x, y);
                let tile = map.tiles[idx];
                // Skip filler walls that aren't adjacent to any floor tile.
                if tile == Tile::Wall && !map.structural[idx] {
                    queue!(
                        w,
                        cursor::MoveTo(x as u16, y as u16),
                        SetForegroundColor(palette_color(GameColor::Black, pal)),
                        SetBackgroundColor(palette_color(GameColor::Black, pal)),
                        style::Print(' ')
                    )?;
                    continue;
                }
                let (ch, fg) = match tile {
                    Tile::Floor => ('.', palette_color(GameColor::DarkGrey, pal)),
                    Tile::Wall => ('#', palette_color(GameColor::White, pal)),
                    Tile::StairsDown => ('>', palette_color(GameColor::Cyan, pal)),
                };
                queue!(
                    w,
                    cursor::MoveTo(x as u16, y as u16),
                    SetForegroundColor(fg),
                    SetBackgroundColor(palette_color(GameColor::Black, pal)),
                    style::Print(ch)
                )?;
            } else if is_explored {
                let idx = map.idx(x, y);
                let tile = map.tiles[idx];
                // Skip filler walls in explored area too.
                if tile == Tile::Wall && !map.structural[idx] {
                    queue!(
                        w,
                        cursor::MoveTo(x as u16, y as u16),
                        SetForegroundColor(palette_color(GameColor::Black, pal)),
                        SetBackgroundColor(palette_color(GameColor::Black, pal)),
                        style::Print(' ')
                    )?;
                    continue;
                }
                let ch = match tile {
                    Tile::Floor => '.',
                    Tile::Wall => '#',
                    Tile::StairsDown => '>',
                };
                let dim = match pal {
                    ColorPalette::HighContrast => palette_color(GameColor::Grey, pal),
                    _ => palette_color(GameColor::Rgb(40, 40, 50), pal),
                };
                queue!(
                    w,
                    cursor::MoveTo(x as u16, y as u16),
                    SetForegroundColor(dim),
                    SetBackgroundColor(palette_color(GameColor::Black, pal)),
                    style::Print(ch)
                )?;
            } else {
                queue!(
                    w,
                    cursor::MoveTo(x as u16, y as u16),
                    SetForegroundColor(palette_color(GameColor::Black, pal)),
                    SetBackgroundColor(palette_color(GameColor::Black, pal)),
                    style::Print(' ')
                )?;
            }
        }
    }
    Ok(())
}

fn render_items<W: Write>(w: &mut W, state: &GameState, pal: ColorPalette) -> std::io::Result<()> {
    for it in &state.ground_items {
        if state.visible.contains(&(it.x, it.y)) {
            queue!(
                w,
                cursor::MoveTo(it.x as u16, it.y as u16),
                SetForegroundColor(palette_color(item::item_color(it.kind), pal)),
                SetBackgroundColor(palette_color(GameColor::Black, pal)),
                style::Print(item::item_glyph(it.kind))
            )?;
        }
    }
    Ok(())
}

fn render_entities<W: Write>(
    w: &mut W,
    state: &GameState,
    show_corpses: bool,
    pal: ColorPalette,
) -> std::io::Result<()> {
    if show_corpses {
        for entity in state.entities.iter() {
            if !entity.alive && state.visible.contains(&(entity.x, entity.y)) {
                queue!(
                    w,
                    cursor::MoveTo(entity.x as u16, entity.y as u16),
                    SetForegroundColor(palette_color(GameColor::DarkRed, pal)),
                    SetBackgroundColor(palette_color(GameColor::Black, pal)),
                    style::Print('%')
                )?;
            }
        }
    }

    for entity in state.entities.iter() {
        if entity.alive && state.visible.contains(&(entity.x, entity.y)) {
            queue!(
                w,
                cursor::MoveTo(entity.x as u16, entity.y as u16),
                SetForegroundColor(palette_color(entity.color, pal)),
                SetBackgroundColor(palette_color(GameColor::Black, pal)),
                style::Print(entity.glyph)
            )?;
        }
    }

    Ok(())
}

fn render_status_bar<W: Write>(
    w: &mut W,
    state: &GameState,
    screen_width: Coord,
    screen_height: Coord,
    settings: &Settings,
) -> std::io::Result<()> {
    let player = &state.entities[0];
    let bar_row = (screen_height - 1 - settings.message_log_lines as i32) as u16;

    let bar_width = 16;
    let fill = if player.max_hp > 0 {
        (bar_width as f32 * player.hp as f32 / player.max_hp as f32).round() as i32
    } else {
        0
    };
    let fill = fill.max(0).min(bar_width);
    let empty = bar_width - fill;

    let hp_pct = if player.max_hp > 0 {
        player.hp as f32 / player.max_hp as f32
    } else {
        0.0
    };
    let pal = settings.color_palette;
    let hp_color = if hp_pct > 0.6 {
        palette_color(GameColor::Green, pal)
    } else if hp_pct > 0.3 {
        palette_color(GameColor::Yellow, pal)
    } else {
        palette_color(GameColor::Red, pal)
    };

    let bar_filled: String = "\u{2588}".repeat(fill as usize);
    let bar_empty: String = "\u{2591}".repeat(empty as usize);

    let coord_segment = if settings.show_coordinates {
        format!(" | ({},{})", player.x, player.y)
    } else {
        String::new()
    };

    let explored_segment = if settings.show_explored_pct {
        format!(" | Explored: {}%", state.explored_pct())
    } else {
        String::new()
    };

    let equip_segment = {
        let atk_str = if state.equipment.weapon.is_some() {
            format!("{}+{}", player.attack, state.equipment.attack_bonus())
        } else {
            format!("{}", player.attack)
        };
        let def_str = if state.equipment.armor.is_some() {
            format!("{}+{}", player.defense, state.equipment.defense_bonus())
        } else {
            format!("{}", player.defense)
        };
        format!(" | ATK:{} DEF:{}", atk_str, def_str)
    };

    let hint_segment = if settings.show_keybind_hints {
        " | hjkl/arrows: move | .: wait | Ctrl+P: log | q: quit"
    } else {
        ""
    };

    let depth_segment = format!(" | Depth {}/{}", state.depth, state.target_depth);

    let status = format!(
        " HP [{}{}] {}/{}{}{}{}{}{}",
        bar_filled,
        bar_empty,
        player.hp,
        player.max_hp,
        equip_segment,
        depth_segment,
        coord_segment,
        explored_segment,
        hint_segment
    );
    let truncated: String = status
        .chars()
        .chain(std::iter::repeat(' '))
        .take(screen_width as usize)
        .collect();

    queue!(
        w,
        cursor::MoveTo(0, bar_row),
        SetForegroundColor(hp_color),
        SetBackgroundColor(palette_color(GameColor::DarkBlue, pal)),
        style::Print(truncated)
    )?;

    Ok(())
}

fn render_message_log<W: Write>(
    w: &mut W,
    state: &GameState,
    screen_width: Coord,
    screen_height: Coord,
    settings: &Settings,
) -> std::io::Result<()> {
    let pal = settings.color_palette;
    let message_log_lines = settings.message_log_lines;
    let n = message_log_lines as usize;
    let log_start_row = (screen_height - message_log_lines as i32) as u16;
    let messages = state.log.recent(n);

    for i in 0..message_log_lines as u16 {
        let row = log_start_row + i;
        let msg = messages.get(i as usize).map(|s| s.as_str()).unwrap_or("");
        let line = format!(" {}", msg);
        let truncated: String = line
            .chars()
            .chain(std::iter::repeat(' '))
            .take(screen_width as usize)
            .collect();

        let color =
            if i as usize + messages.len().saturating_sub(n) >= messages.len().saturating_sub(1) {
                palette_color(GameColor::White, pal)
            } else {
                palette_color(GameColor::Grey, pal)
            };

        queue!(
            w,
            cursor::MoveTo(0, row),
            SetForegroundColor(color),
            SetBackgroundColor(palette_color(GameColor::Black, pal)),
            style::Print(truncated)
        )?;
    }

    if state.game_won {
        let msg = "Victory! You conquered the dungeon! Press any key to exit.".to_string();
        let x = (screen_width as usize).saturating_sub(msg.len()) / 2;
        let y = screen_height / 2;
        queue!(
            w,
            cursor::MoveTo(x as u16, y as u16),
            SetForegroundColor(palette_color(GameColor::Yellow, pal)),
            SetBackgroundColor(palette_color(GameColor::Black, pal)),
            style::Print(&msg)
        )?;

        let seed_msg = format!("Seed: {}", state.seed_code());
        let sx = (screen_width as usize).saturating_sub(seed_msg.len()) / 2;
        queue!(
            w,
            cursor::MoveTo(sx as u16, (y + 1) as u16),
            SetForegroundColor(palette_color(GameColor::Grey, pal)),
            SetBackgroundColor(palette_color(GameColor::Black, pal)),
            style::Print(seed_msg)
        )?;
    } else if state.game_over {
        let msg = if settings.player_name.is_empty() {
            "You have been slain... Press any key to exit.".to_string()
        } else {
            format!(
                "{} {} slain... Press any key to exit.",
                settings.player_name,
                settings.pronouns.was_were()
            )
        };
        let x = (screen_width as usize).saturating_sub(msg.len()) / 2;
        let y = screen_height / 2;
        queue!(
            w,
            cursor::MoveTo(x as u16, y as u16),
            SetForegroundColor(palette_color(GameColor::Red, pal)),
            SetBackgroundColor(palette_color(GameColor::Black, pal)),
            style::Print(&msg)
        )?;

        let seed_msg = format!("Seed: {}", state.seed_code());
        let sx = (screen_width as usize).saturating_sub(seed_msg.len()) / 2;
        queue!(
            w,
            cursor::MoveTo(sx as u16, (y + 1) as u16),
            SetForegroundColor(palette_color(GameColor::Grey, pal)),
            SetBackgroundColor(palette_color(GameColor::Black, pal)),
            style::Print(seed_msg)
        )?;
    }

    Ok(())
}

/// Render a `GameObservation` to the terminal — used for non-standard tiers
/// (e.g. micro) that don't have a `GameState` for the full render pipeline.
///
/// Writes the ASCII map, a simple status bar, and recent messages.
/// No color (micro tier doesn't carry tile metadata for palette mapping).
pub fn render_observation<W: Write>(
    w: &mut W,
    obs: &GameObservation,
    screen_width: Coord,
    screen_height: Coord,
) -> std::io::Result<()> {
    queue!(
        w,
        terminal::Clear(terminal::ClearType::All),
        cursor::MoveTo(0, 0)
    )?;

    // Map lines.
    for (y, line) in obs.map_ascii.iter().enumerate() {
        queue!(
            w,
            cursor::MoveTo(0, y as u16),
            SetForegroundColor(Color::White),
            SetBackgroundColor(Color::Black),
            style::Print(line)
        )?;
    }

    // Status bar — 1 row above message log.
    let msg_lines: Coord = 4;
    let bar_row = (screen_height - 1 - msg_lines) as u16;
    let status = format!(
        " HP {}/{} | ATK:{} DEF:{} | Turn {} | Kills {} | Seed: {}",
        obs.player_hp,
        obs.player_max_hp,
        obs.player_atk,
        obs.player_def,
        obs.turn_count,
        obs.kills,
        obs.seed_code,
    );
    let truncated: String = status
        .chars()
        .chain(std::iter::repeat(' '))
        .take(screen_width as usize)
        .collect();
    queue!(
        w,
        cursor::MoveTo(0, bar_row),
        SetForegroundColor(Color::White),
        SetBackgroundColor(Color::DarkBlue),
        style::Print(truncated)
    )?;

    // Recent messages.
    let log_start = (screen_height - msg_lines) as u16;
    for i in 0..msg_lines as usize {
        let row = log_start + i as u16;
        let msg = obs
            .recent_messages
            .get(obs.recent_messages.len().saturating_sub(msg_lines as usize) + i)
            .map(|s| s.as_str())
            .unwrap_or("");
        let line = format!(" {}", msg);
        let truncated: String = line
            .chars()
            .chain(std::iter::repeat(' '))
            .take(screen_width as usize)
            .collect();
        queue!(
            w,
            cursor::MoveTo(0, row),
            SetForegroundColor(Color::Grey),
            SetBackgroundColor(Color::Black),
            style::Print(truncated)
        )?;
    }

    // Game-over / victory overlay.
    if obs.game_won {
        let msg = "Victory! You conquered the dungeon! Press any key to exit.";
        let x = (screen_width as usize).saturating_sub(msg.len()) / 2;
        let y = screen_height / 2;
        queue!(
            w,
            cursor::MoveTo(x as u16, y as u16),
            SetForegroundColor(Color::Yellow),
            SetBackgroundColor(Color::Black),
            style::Print(msg)
        )?;
    } else if obs.game_over {
        let msg = "You have been slain... Press any key to exit.";
        let x = (screen_width as usize).saturating_sub(msg.len()) / 2;
        let y = screen_height / 2;
        queue!(
            w,
            cursor::MoveTo(x as u16, y as u16),
            SetForegroundColor(Color::Red),
            SetBackgroundColor(Color::Black),
            style::Print(msg)
        )?;
    }

    w.flush()?;
    Ok(())
}

#[cfg(all(debug_assertions, feature = "dev-tools"))]
pub fn render_overlay<W: Write>(
    w: &mut W,
    cells: &[OverlayCell],
    pal: ColorPalette,
) -> std::io::Result<()> {
    for cell in cells {
        queue!(
            w,
            cursor::MoveTo(cell.x as u16, cell.y as u16),
            SetForegroundColor(palette_color(cell.color, pal)),
            SetBackgroundColor(palette_color(GameColor::Black, pal)),
            style::Print(cell.ch)
        )?;
    }
    w.flush()?;
    Ok(())
}

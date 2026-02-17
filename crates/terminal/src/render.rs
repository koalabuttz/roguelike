use std::io::{Stdout, Write};

use crossterm::{
    cursor, queue,
    style::{self, Color, SetBackgroundColor, SetForegroundColor},
    terminal,
};

use roguelike_core::game::GameState;
use roguelike_core::map::{self, Tile};
use roguelike_core::platform::Renderer;
use roguelike_core::settings::{ColorPalette, Settings};
use roguelike_core::types::{Coord, GameColor};

#[cfg(all(debug_assertions, feature = "dev-tools"))]
use roguelike_core::dev_tools::OverlayCell;

/// Map a platform-independent `GameColor` to a crossterm terminal color.
fn to_crossterm_color(c: GameColor) -> Color {
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
fn protanopia_color(c: GameColor) -> Color {
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
fn deuteranopia_color(c: GameColor) -> Color {
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

/// Map a `GameColor` through the selected palette to a crossterm `Color`.
fn palette_color(c: GameColor, palette: ColorPalette) -> Color {
    match palette {
        ColorPalette::Default => to_crossterm_color(c),
        ColorPalette::Protanopia => protanopia_color(c),
        ColorPalette::Deuteranopia => deuteranopia_color(c),
    }
}

/// Terminal renderer backed by crossterm.
///
/// Wraps `Stdout` and uses crossterm's queued-write API for efficient
/// batched rendering. Call `flush()` after a frame of draw calls.
pub struct CrosstermRenderer {
    out: Stdout,
}

impl CrosstermRenderer {
    pub fn new(out: Stdout) -> Self {
        Self { out }
    }
}

impl Renderer for CrosstermRenderer {
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
        terminal::size().map_or((80, 24), |(w, h)| (w as Coord, h as Coord))
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
    render_entities(w, state, settings.show_corpses, pal)?;
    render_status_bar(w, state, screen_width, screen_height, settings)?;
    render_message_log(
        w,
        state,
        screen_width,
        screen_height,
        settings.message_log_lines,
        pal,
    )?;

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
                };
                let dim = palette_color(GameColor::Rgb(40, 40, 50), pal);
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
        let floor_count = state.map.known_floor_count();
        let explored_floors = state
            .explored
            .iter()
            .filter(|&&(x, y)| {
                state.map.in_bounds(x, y)
                    && state.map.tiles[state.map.idx(x, y)] == map::Tile::Floor
            })
            .count() as i32;
        let pct = if floor_count > 0 {
            (explored_floors * 100) / floor_count
        } else {
            0
        };
        format!(" | Explored: {}%", pct)
    } else {
        String::new()
    };

    let hint_segment = if settings.show_keybind_hints {
        " | hjkl/arrows: move | .: wait | q: quit"
    } else {
        ""
    };

    let status = format!(
        " HP [{}{}] {}/{}{}{}{}",
        bar_filled,
        bar_empty,
        player.hp,
        player.max_hp,
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
    message_log_lines: u8,
    pal: ColorPalette,
) -> std::io::Result<()> {
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

    if state.game_over {
        let msg = "You have been slain... Press any key to exit.";
        let x = (screen_width as usize).saturating_sub(msg.len()) / 2;
        let y = screen_height / 2;
        queue!(
            w,
            cursor::MoveTo(x as u16, y as u16),
            SetForegroundColor(palette_color(GameColor::Red, pal)),
            SetBackgroundColor(palette_color(GameColor::Black, pal)),
            style::Print(msg)
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

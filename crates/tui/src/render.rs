use std::io::Write;

use crossterm::{
    cursor, queue,
    style::{self, Color, SetBackgroundColor, SetForegroundColor},
    terminal,
};

use roguelike_core::game::GameObservation;
use roguelike_core::platform::Renderer;
use roguelike_core::settings::{ColorPalette, Settings};
use roguelike_core::types::{Coord, GameColor};

use crate::render_source::{RenderSource, TileVisibility};

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

/// Viewport offset returned by `render()` for look-mode cursor offsetting.
pub struct Viewport {
    pub x: Coord,
    pub y: Coord,
}

/// Internal viewport rectangle for sub-function parameters.
struct ViewportRect {
    x: usize,
    y: usize,
    cols: usize,
    rows: usize,
}

pub fn render<W: Write>(
    w: &mut W,
    source: &dyn RenderSource,
    screen_width: Coord,
    screen_height: Coord,
    settings: &Settings,
) -> std::io::Result<Viewport> {
    render_focused(w, source, screen_width, screen_height, settings, None)
}

/// Like `render`, but centers the viewport on `focus` instead of the player.
pub fn render_focused<W: Write>(
    w: &mut W,
    source: &dyn RenderSource,
    screen_width: Coord,
    screen_height: Coord,
    settings: &Settings,
    focus: Option<(Coord, Coord)>,
) -> std::io::Result<Viewport> {
    let pal = settings.color_palette;
    let msg_lines = settings.message_log_lines as Coord;
    let (map_w, map_h) = source.map_size();
    let (fx, fy) = focus.unwrap_or_else(|| source.player_pos());

    // Viewport: center on focus point when map exceeds screen.
    let map_rows = (screen_height - 1 - msg_lines).max(0) as usize;
    let map_cols = screen_width.max(0) as usize;

    let view_y = if (map_h as usize) <= map_rows {
        0usize
    } else {
        let half = map_rows / 2;
        let ideal = (fy as usize).saturating_sub(half);
        ideal.min((map_h as usize) - map_rows)
    };

    let view_x = if (map_w as usize) <= map_cols {
        0usize
    } else {
        let half = map_cols / 2;
        let ideal = (fx as usize).saturating_sub(half);
        ideal.min((map_w as usize) - map_cols)
    };

    let vp = ViewportRect {
        x: view_x,
        y: view_y,
        cols: map_cols,
        rows: map_rows,
    };
    render_map(w, source, &vp, pal)?;
    render_items(w, source, &vp, pal)?;
    render_entities(w, source, &vp, settings.show_corpses, pal)?;
    render_status_bar(w, source, screen_width, screen_height, settings)?;
    render_message_log(w, source, screen_width, screen_height, settings)?;

    w.flush()?;
    Ok(Viewport {
        x: view_x as Coord,
        y: view_y as Coord,
    })
}

fn render_map<W: Write>(
    w: &mut W,
    source: &dyn RenderSource,
    vp: &ViewportRect,
    pal: ColorPalette,
) -> std::io::Result<()> {
    let (map_w, map_h) = source.map_size();
    let bg = palette_color(GameColor::Black, pal);

    for screen_y in 0..vp.rows {
        let world_y = (vp.y + screen_y) as Coord;
        if world_y >= map_h {
            break;
        }
        for screen_x in 0..vp.cols {
            let world_x = (vp.x + screen_x) as Coord;
            if world_x >= map_w {
                break;
            }

            let vis = source.tile_visibility(world_x, world_y);
            match vis {
                TileVisibility::Visible => {
                    let tile = source.tile_at(world_x, world_y);
                    if tile.glyph == '#' && !tile.structural {
                        queue!(
                            w,
                            cursor::MoveTo(screen_x as u16, screen_y as u16),
                            SetForegroundColor(bg),
                            SetBackgroundColor(bg),
                            style::Print(' ')
                        )?;
                    } else {
                        queue!(
                            w,
                            cursor::MoveTo(screen_x as u16, screen_y as u16),
                            SetForegroundColor(palette_color(tile.fg, pal)),
                            SetBackgroundColor(bg),
                            style::Print(tile.glyph)
                        )?;
                    }
                }
                TileVisibility::Explored => {
                    let tile = source.tile_at(world_x, world_y);
                    if tile.glyph == '#' && !tile.structural {
                        queue!(
                            w,
                            cursor::MoveTo(screen_x as u16, screen_y as u16),
                            SetForegroundColor(bg),
                            SetBackgroundColor(bg),
                            style::Print(' ')
                        )?;
                    } else {
                        let dim = match pal {
                            ColorPalette::HighContrast => palette_color(GameColor::Grey, pal),
                            _ => palette_color(GameColor::Rgb(40, 40, 50), pal),
                        };
                        queue!(
                            w,
                            cursor::MoveTo(screen_x as u16, screen_y as u16),
                            SetForegroundColor(dim),
                            SetBackgroundColor(bg),
                            style::Print(tile.glyph)
                        )?;
                    }
                }
                TileVisibility::Unexplored => {
                    queue!(
                        w,
                        cursor::MoveTo(screen_x as u16, screen_y as u16),
                        SetForegroundColor(bg),
                        SetBackgroundColor(bg),
                        style::Print(' ')
                    )?;
                }
            }
        }
    }
    Ok(())
}

fn render_items<W: Write>(
    w: &mut W,
    source: &dyn RenderSource,
    vp: &ViewportRect,
    pal: ColorPalette,
) -> std::io::Result<()> {
    let bg = palette_color(GameColor::Black, pal);
    source.for_each_visible_item(&mut |item| {
        let sx = item.x as usize;
        let sy = item.y as usize;
        if sx >= vp.x && sx < vp.x + vp.cols && sy >= vp.y && sy < vp.y + vp.rows {
            let screen_x = (sx - vp.x) as u16;
            let screen_y = (sy - vp.y) as u16;
            let _ = queue!(
                w,
                cursor::MoveTo(screen_x, screen_y),
                SetForegroundColor(palette_color(item.fg, pal)),
                SetBackgroundColor(bg),
                style::Print(item.glyph)
            );
        }
    });
    Ok(())
}

fn render_entities<W: Write>(
    w: &mut W,
    source: &dyn RenderSource,
    vp: &ViewportRect,
    show_corpses: bool,
    pal: ColorPalette,
) -> std::io::Result<()> {
    let bg = palette_color(GameColor::Black, pal);

    // Collect entities so we can draw corpses first, then living entities on top.
    let mut entities = Vec::new();
    source.for_each_visible_entity(&mut |e| {
        let sx = e.x as usize;
        let sy = e.y as usize;
        if sx >= vp.x && sx < vp.x + vp.cols && sy >= vp.y && sy < vp.y + vp.rows {
            entities.push(e);
        }
    });

    // Corpses first (drawn under living entities).
    if show_corpses {
        for e in entities.iter().filter(|e| !e.alive) {
            let screen_x = (e.x as usize - vp.x) as u16;
            let screen_y = (e.y as usize - vp.y) as u16;
            queue!(
                w,
                cursor::MoveTo(screen_x, screen_y),
                SetForegroundColor(palette_color(e.fg, pal)),
                SetBackgroundColor(bg),
                style::Print(e.glyph)
            )?;
        }
    }

    // Living entities on top.
    for e in entities.iter().filter(|e| e.alive) {
        let screen_x = (e.x as usize - vp.x) as u16;
        let screen_y = (e.y as usize - vp.y) as u16;
        queue!(
            w,
            cursor::MoveTo(screen_x, screen_y),
            SetForegroundColor(palette_color(e.fg, pal)),
            SetBackgroundColor(bg),
            style::Print(e.glyph)
        )?;
    }

    Ok(())
}

fn render_status_bar<W: Write>(
    w: &mut W,
    source: &dyn RenderSource,
    screen_width: Coord,
    screen_height: Coord,
    settings: &Settings,
) -> std::io::Result<()> {
    let (hp, max_hp) = source.player_hp();
    let bar_row = (screen_height - 1 - settings.message_log_lines as i32) as u16;

    let bar_width = 16;
    let fill = if max_hp > 0 {
        (bar_width as f32 * hp as f32 / max_hp as f32).round() as i32
    } else {
        0
    };
    let fill = fill.max(0).min(bar_width);
    let empty = bar_width - fill;

    let hp_pct = if max_hp > 0 {
        hp as f32 / max_hp as f32
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

    let (px, py) = source.player_pos();
    let coord_segment = if settings.show_coordinates {
        format!(" | ({},{})", px, py)
    } else {
        String::new()
    };

    let explored_segment = if settings.show_explored_pct {
        format!(" | Explored: {}%", source.explored_pct())
    } else {
        String::new()
    };

    let (base_atk, atk_bonus) = source.player_atk();
    let (base_def, def_bonus) = source.player_def();
    let equip_segment = {
        let atk_str = if atk_bonus > 0 {
            format!("{}+{}", base_atk, atk_bonus)
        } else {
            format!("{}", base_atk)
        };
        let def_str = if def_bonus > 0 {
            format!("{}+{}", base_def, def_bonus)
        } else {
            format!("{}", base_def)
        };
        format!(" | ATK:{} DEF:{}", atk_str, def_str)
    };

    let hint_segment = if settings.show_keybind_hints {
        " | hjkl/arrows: move | .: wait | Ctrl+P: log | q: quit"
    } else {
        ""
    };

    let (cur_depth, target_depth) = source.depth();
    let depth_segment = format!(" | Depth {}/{}", cur_depth, target_depth);

    let status = format!(
        " HP [{}{}] {}/{}{}{}{}{}{}",
        bar_filled,
        bar_empty,
        hp,
        max_hp,
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
    source: &dyn RenderSource,
    screen_width: Coord,
    screen_height: Coord,
    settings: &Settings,
) -> std::io::Result<()> {
    let pal = settings.color_palette;
    let message_log_lines = settings.message_log_lines;
    let n = message_log_lines as usize;
    let log_start_row = (screen_height - message_log_lines as i32) as u16;
    let messages = source.recent_messages(n);

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

    if source.game_won() {
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

        let seed_msg = format!("Seed: {}", source.seed_code());
        let sx = (screen_width as usize).saturating_sub(seed_msg.len()) / 2;
        queue!(
            w,
            cursor::MoveTo(sx as u16, (y + 1) as u16),
            SetForegroundColor(palette_color(GameColor::Grey, pal)),
            SetBackgroundColor(palette_color(GameColor::Black, pal)),
            style::Print(seed_msg)
        )?;
    } else if source.game_over() {
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

        let seed_msg = format!("Seed: {}", source.seed_code());
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
    msg_lines: Coord,
) -> std::io::Result<()> {
    queue!(
        w,
        terminal::Clear(terminal::ClearType::All),
        cursor::MoveTo(0, 0)
    )?;

    // Status bar — 1 row above message log.
    // Clamp msg_lines so the status bar never goes above the map.
    let msg_lines = msg_lines.min(screen_height.saturating_sub(2));

    // Map viewport — center on the player when the map exceeds the screen.
    let map_rows = (screen_height - 1 - msg_lines) as usize; // rows available for map
    let map_cols = screen_width as usize;
    let map_height = obs.map_ascii.len();
    let map_width = obs.map_ascii.iter().map(|l| l.len()).max().unwrap_or(0);

    // Find the player glyph in the visual map (map_ascii may skip blank rows,
    // so obs.player_y is NOT a valid index into map_ascii).
    let (player_map_y, player_map_x) = obs
        .map_ascii
        .iter()
        .enumerate()
        .find_map(|(y, line)| line.find('@').map(|x| (y, x)))
        .unwrap_or((0, 0));

    let view_y = if map_height <= map_rows {
        0
    } else {
        let half = map_rows / 2;
        let ideal = player_map_y.saturating_sub(half);
        ideal.min(map_height - map_rows)
    };

    let view_x = if map_width <= map_cols {
        0
    } else {
        let half = map_cols / 2;
        let ideal = player_map_x.saturating_sub(half);
        ideal.min(map_width - map_cols)
    };

    for (screen_y, line) in obs.map_ascii[view_y..][..map_rows.min(map_height)]
        .iter()
        .enumerate()
    {
        let slice = if view_x < line.len() {
            let end = (view_x + map_cols).min(line.len());
            &line[view_x..end]
        } else {
            ""
        };
        queue!(
            w,
            cursor::MoveTo(0, screen_y as u16),
            SetForegroundColor(Color::White),
            SetBackgroundColor(Color::Black),
            style::Print(slice)
        )?;
    }
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

/// Render the inventory modal overlay.
///
/// `inventory` is the list of slot descriptions from `GameObservation.inventory`
/// (e.g. `["a) Health Potion (x3)", "b) Short Sword [equipped]"]`).
/// `selected` is the currently highlighted slot index (for action dispatch).
/// `action_hint` shows what the next keypress will do (if a slot is selected).
#[allow(clippy::too_many_arguments)]
pub fn render_inventory<W: Write>(
    w: &mut W,
    inventory: &[String],
    inventory_colors: &[GameColor],
    weapon: Option<&str>,
    armor: Option<&str>,
    screen_width: Coord,
    screen_height: Coord,
    selected: Option<usize>,
    action_hint: &str,
) -> std::io::Result<()> {
    // Count equipped items for box height calculation.
    let equip_lines = weapon.is_some() as usize + armor.is_some() as usize;
    let equip_section = if equip_lines > 0 { equip_lines + 1 } else { 0 }; // +1 for header

    let box_w = (screen_width as usize).min(40);
    let box_h = (screen_height as usize)
        .min(inventory.len() + equip_section + 6)
        .max(8);
    let box_x = (screen_width as usize).saturating_sub(box_w) / 2;
    let box_y = (screen_height as usize).saturating_sub(box_h) / 2;

    // Draw box background.
    let blank: String = " ".repeat(box_w);
    for row in 0..box_h {
        queue!(
            w,
            cursor::MoveTo(box_x as u16, (box_y + row) as u16),
            SetForegroundColor(Color::White),
            SetBackgroundColor(Color::DarkBlue),
            style::Print(&blank)
        )?;
    }

    // Title.
    let title = "INVENTORY";
    let title_x = box_x + (box_w.saturating_sub(title.len())) / 2;
    queue!(
        w,
        cursor::MoveTo(title_x as u16, box_y as u16),
        SetForegroundColor(Color::Cyan),
        SetBackgroundColor(Color::DarkBlue),
        style::Print(title)
    )?;

    let mut content_row = box_y + 2;

    // Equipped items section (non-interactive).
    if equip_lines > 0 {
        queue!(
            w,
            cursor::MoveTo((box_x + 2) as u16, content_row as u16),
            SetForegroundColor(Color::DarkGrey),
            SetBackgroundColor(Color::DarkBlue),
            style::Print("Equipped:")
        )?;
        content_row += 1;
        for (label, name) in [("W: ", weapon), ("A: ", armor)] {
            if let Some(name) = name {
                let display = format!(" {}{}", label, name);
                queue!(
                    w,
                    cursor::MoveTo((box_x + 1) as u16, content_row as u16),
                    SetForegroundColor(Color::Green),
                    SetBackgroundColor(Color::DarkBlue),
                    style::Print(display)
                )?;
                content_row += 1;
            }
        }
        content_row += 1; // blank line separator
    }

    // List items.
    if inventory.is_empty() && equip_lines == 0 {
        queue!(
            w,
            cursor::MoveTo((box_x + 2) as u16, content_row as u16),
            SetForegroundColor(Color::DarkGrey),
            SetBackgroundColor(Color::DarkBlue),
            style::Print("(empty)")
        )?;
    } else {
        let max_items = box_h.saturating_sub(content_row - box_y + 2);
        for (i, item) in inventory.iter().take(max_items).enumerate() {
            let row = content_row + i;
            let fg = if selected == Some(i) {
                Color::Yellow
            } else {
                inventory_colors
                    .get(i)
                    .map(|c| to_crossterm_color(*c))
                    .unwrap_or(Color::White)
            };
            let display: String = format!(" {} ", item)
                .chars()
                .take(box_w.saturating_sub(2))
                .collect();
            queue!(
                w,
                cursor::MoveTo((box_x + 1) as u16, row as u16),
                SetForegroundColor(fg),
                SetBackgroundColor(Color::DarkBlue),
                style::Print(display)
            )?;
        }
    }

    // Footer.
    let footer_y = box_y + box_h - 1;
    let footer: String = if action_hint.is_empty() {
        "[a-z] select  [Esc] close".into()
    } else {
        action_hint.into()
    };
    let footer_x = box_x + (box_w.saturating_sub(footer.len())) / 2;
    queue!(
        w,
        cursor::MoveTo(footer_x as u16, footer_y as u16),
        SetForegroundColor(Color::DarkGrey),
        SetBackgroundColor(Color::DarkBlue),
        style::Print(footer)
    )?;

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

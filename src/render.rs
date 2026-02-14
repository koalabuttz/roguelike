use std::io::Write;

use crossterm::{
    cursor, queue,
    style::{self, Color, SetBackgroundColor, SetForegroundColor},
};

use crate::game::GameState;
use crate::map::Tile;
use crate::types::{Coord, GameColor};

/// Map a platform-independent `GameColor` to a crossterm terminal color.
fn to_crossterm_color(c: GameColor) -> Color {
    match c {
        GameColor::Yellow => Color::Yellow,
        GameColor::Green => Color::Green,
        GameColor::DarkGreen => Color::DarkGreen,
        GameColor::DarkRed => Color::DarkRed,
    }
}

pub fn render<W: Write>(
    w: &mut W,
    state: &GameState,
    screen_width: Coord,
    screen_height: Coord,
) -> std::io::Result<()> {
    render_map(w, state)?;
    render_entities(w, state)?;
    render_status_bar(w, state, screen_width, screen_height)?;
    render_message_log(w, state, screen_width, screen_height)?;

    w.flush()?;
    Ok(())
}

fn render_map<W: Write>(w: &mut W, state: &GameState) -> std::io::Result<()> {
    let map = &state.map;
    for y in 0..map.height {
        for x in 0..map.width {
            let is_visible = state.visible.contains(&(x, y));
            let is_explored = state.explored.contains(&(x, y));

            if is_visible {
                let tile = map.tiles[map.idx(x, y)];
                let (ch, fg) = match tile {
                    Tile::Floor => ('.', Color::DarkGrey),
                    Tile::Wall => ('#', Color::White),
                };
                queue!(
                    w,
                    cursor::MoveTo(x as u16, y as u16),
                    SetForegroundColor(fg),
                    SetBackgroundColor(Color::Black),
                    style::Print(ch)
                )?;
            } else if is_explored {
                let tile = map.tiles[map.idx(x, y)];
                let ch = match tile {
                    Tile::Floor => '.',
                    Tile::Wall => '#',
                };
                let dim = Color::Rgb {
                    r: 40,
                    g: 40,
                    b: 50,
                };
                queue!(
                    w,
                    cursor::MoveTo(x as u16, y as u16),
                    SetForegroundColor(dim),
                    SetBackgroundColor(Color::Black),
                    style::Print(ch)
                )?;
            } else {
                queue!(
                    w,
                    cursor::MoveTo(x as u16, y as u16),
                    SetForegroundColor(Color::Black),
                    SetBackgroundColor(Color::Black),
                    style::Print(' ')
                )?;
            }
        }
    }
    Ok(())
}

fn render_entities<W: Write>(w: &mut W, state: &GameState) -> std::io::Result<()> {
    for entity in state.entities.iter() {
        if !entity.alive && state.visible.contains(&(entity.x, entity.y)) {
            queue!(
                w,
                cursor::MoveTo(entity.x as u16, entity.y as u16),
                SetForegroundColor(Color::DarkRed),
                SetBackgroundColor(Color::Black),
                style::Print('%')
            )?;
        }
    }

    for entity in state.entities.iter() {
        if entity.alive && state.visible.contains(&(entity.x, entity.y)) {
            queue!(
                w,
                cursor::MoveTo(entity.x as u16, entity.y as u16),
                SetForegroundColor(to_crossterm_color(entity.color)),
                SetBackgroundColor(Color::Black),
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
) -> std::io::Result<()> {
    let player = &state.entities[0];
    let bar_row = (screen_height - 5) as u16;

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
    let hp_color = if hp_pct > 0.6 {
        Color::Green
    } else if hp_pct > 0.3 {
        Color::Yellow
    } else {
        Color::Red
    };

    let bar_filled: String = "\u{2588}".repeat(fill as usize);
    let bar_empty: String = "\u{2591}".repeat(empty as usize);

    let status = format!(
        " HP [{}{}] {}/{} | ({},{}) | hjkl/numpad/arrows: move | .: wait | q: quit",
        bar_filled, bar_empty, player.hp, player.max_hp, player.x, player.y
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
        SetBackgroundColor(Color::DarkBlue),
        style::Print(truncated)
    )?;

    Ok(())
}

fn render_message_log<W: Write>(
    w: &mut W,
    state: &GameState,
    screen_width: Coord,
    screen_height: Coord,
) -> std::io::Result<()> {
    let log_start_row = (screen_height - 4) as u16;
    let messages = state.log.recent(4);

    for i in 0..4u16 {
        let row = log_start_row + i;
        let msg = messages.get(i as usize).map(|s| s.as_str()).unwrap_or("");
        let line = format!(" {}", msg);
        let truncated: String = line
            .chars()
            .chain(std::iter::repeat(' '))
            .take(screen_width as usize)
            .collect();

        let color =
            if i as usize + messages.len().saturating_sub(4) >= messages.len().saturating_sub(1) {
                Color::White
            } else {
                Color::Grey
            };

        queue!(
            w,
            cursor::MoveTo(0, row),
            SetForegroundColor(color),
            SetBackgroundColor(Color::Black),
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
            SetForegroundColor(Color::Red),
            SetBackgroundColor(Color::Black),
            style::Print(msg)
        )?;
    }

    Ok(())
}

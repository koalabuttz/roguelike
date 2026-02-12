use std::collections::HashSet;
use std::io::Write;

use crossterm::{
    cursor,
    queue,
    style::{self, Color, SetBackgroundColor, SetForegroundColor},
};

use crate::map::{Map, Tile};

pub fn render<W: Write>(
    w: &mut W,
    map: &Map,
    player_x: i32,
    player_y: i32,
    visible: &HashSet<(i32, i32)>,
    explored: &HashSet<(i32, i32)>,
) -> std::io::Result<()> {
    for y in 0..map.height {
        for x in 0..map.width {
            let is_visible = visible.contains(&(x, y));
            let is_explored = explored.contains(&(x, y));

            if x == player_x && y == player_y {
                queue!(
                    w,
                    cursor::MoveTo(x as u16, y as u16),
                    SetForegroundColor(Color::Yellow),
                    SetBackgroundColor(Color::Black),
                    style::Print("@")
                )?;
            } else if is_visible {
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

    // Status bar
    let status = format!(
        " @ ({},{}) | Arrows/hjkl: move | yubn: diagonals | q: quit",
        player_x, player_y
    );
    let padded = format!("{:<width$}", status, width = map.width as usize);
    queue!(
        w,
        cursor::MoveTo(0, map.height as u16),
        SetForegroundColor(Color::White),
        SetBackgroundColor(Color::DarkBlue),
        style::Print(padded)
    )?;

    w.flush()?;
    Ok(())
}

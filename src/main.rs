mod fov;
mod map;
mod render;

use std::collections::HashSet;
use std::io::stdout;

use crossterm::{
    cursor,
    event::{self, Event, KeyCode, KeyEvent, KeyModifiers},
    execute,
    terminal::{self, EnterAlternateScreen, LeaveAlternateScreen},
};

struct Game {
    map: map::Map,
    player_x: i32,
    player_y: i32,
    fov_radius: i32,
    visible: HashSet<(i32, i32)>,
    explored: HashSet<(i32, i32)>,
}

impl Game {
    fn new(width: i32, height: i32) -> Self {
        let mut map = map::Map::new(width, height);
        let (px, py) = map.generate(30, 4, 10);
        let visible = fov::compute_fov(&map, px, py, 8);
        let explored = visible.clone();

        Game {
            map,
            player_x: px,
            player_y: py,
            fov_radius: 8,
            visible,
            explored,
        }
    }

    fn move_player(&mut self, dx: i32, dy: i32) {
        let new_x = self.player_x + dx;
        let new_y = self.player_y + dy;

        if self.map.is_walkable(new_x, new_y) {
            self.player_x = new_x;
            self.player_y = new_y;
            self.visible = fov::compute_fov(&self.map, self.player_x, self.player_y, self.fov_radius);
            self.explored.extend(&self.visible);
        }
    }
}

fn main() -> std::io::Result<()> {
    let mut stdout = stdout();

    // Enter raw mode and alternate screen
    terminal::enable_raw_mode()?;
    execute!(stdout, EnterAlternateScreen, cursor::Hide)?;

    // Size the map to fit the terminal (leave 1 row for the status bar)
    let (cols, rows) = terminal::size()?;
    let map_height = (rows - 1) as i32;
    let mut game = Game::new(cols as i32, map_height);

    loop {
        render::render(
            &mut stdout,
            &game.map,
            game.player_x,
            game.player_y,
            &game.visible,
            &game.explored,
        )?;

        if let Event::Key(KeyEvent {
            code, modifiers, ..
        }) = event::read()?
        {
            // Ctrl+C always quits
            if modifiers.contains(KeyModifiers::CONTROL) && code == KeyCode::Char('c') {
                break;
            }

            match code {
                // Cardinal movement (arrows + vi keys)
                KeyCode::Up | KeyCode::Char('k') => game.move_player(0, -1),
                KeyCode::Down | KeyCode::Char('j') => game.move_player(0, 1),
                KeyCode::Left | KeyCode::Char('h') => game.move_player(-1, 0),
                KeyCode::Right | KeyCode::Char('l') => game.move_player(1, 0),

                // Diagonal movement (vi keys)
                KeyCode::Char('y') => game.move_player(-1, -1),
                KeyCode::Char('u') => game.move_player(1, -1),
                KeyCode::Char('b') => game.move_player(-1, 1),
                KeyCode::Char('n') => game.move_player(1, 1),

                // Quit
                KeyCode::Char('q') | KeyCode::Esc => break,

                _ => {}
            }
        }
    }

    // Restore terminal
    execute!(stdout, LeaveAlternateScreen, cursor::Show)?;
    terminal::disable_raw_mode()?;

    Ok(())
}

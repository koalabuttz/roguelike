use minifb::{Key, Window, WindowOptions};

use roguelike_core::rules::command::GameCommand;
use roguelike_core::rules::direction::Direction;
use roguelike_core::rules::game_view::GameView;
use roguelike_core::tier_micro::game::MicroGameState;
use roguelike_renderer3d::framebuffer::{Framebuffer, unpack_rgb555};
use roguelike_renderer3d::scene::render_scene;

const WIDTH: usize = 640;
const HEIGHT: usize = 480;

/// Convert RGB555 framebuffer to u32 buffer (0x00RRGGBB) for minifb.
fn fb_to_u32(fb: &Framebuffer, buf: &mut [u32]) {
    for y in 0..fb.height() {
        for x in 0..fb.width() {
            let (r, g, b) = unpack_rgb555(fb.get_pixel(x, y));
            buf[(y * fb.width() + x) as usize] = (r as u32) << 16 | (g as u32) << 8 | b as u32;
        }
    }
}

/// Map minifb key presses to game commands.
fn get_command(window: &Window) -> Option<GameCommand> {
    // Check keys in priority order
    if window.is_key_pressed(Key::Up, minifb::KeyRepeat::Yes) {
        Some(GameCommand::Move(Direction::North))
    } else if window.is_key_pressed(Key::Down, minifb::KeyRepeat::Yes) {
        Some(GameCommand::Move(Direction::South))
    } else if window.is_key_pressed(Key::Left, minifb::KeyRepeat::Yes) {
        Some(GameCommand::Move(Direction::West))
    } else if window.is_key_pressed(Key::Right, minifb::KeyRepeat::Yes) {
        Some(GameCommand::Move(Direction::East))
    } else if window.is_key_pressed(Key::Period, minifb::KeyRepeat::No) {
        Some(GameCommand::Descend)
    } else if window.is_key_pressed(Key::Space, minifb::KeyRepeat::Yes) {
        Some(GameCommand::Wait)
    } else if window.is_key_pressed(Key::G, minifb::KeyRepeat::No) {
        Some(GameCommand::Pickup)
    } else {
        None
    }
}

fn main() {
    let seed: u16 = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(42);

    let mut game = MicroGameState::new_default(seed);
    let mut fb = Framebuffer::new(WIDTH as u32, HEIGHT as u32);
    let mut buf = vec![0u32; WIDTH * HEIGHT];

    let mut window = Window::new(
        &format!("Roguelike 3D - seed {seed}"),
        WIDTH,
        HEIGHT,
        WindowOptions::default(),
    )
    .expect("failed to create window");

    // ~60 FPS update rate (keeps window responsive without burning CPU)
    window.set_target_fps(60);

    let mut frame: u32 = 0;

    // Initial render
    render_scene(&game, &mut fb, frame);
    fb_to_u32(&fb, &mut buf);

    while window.is_open() && !window.is_key_down(Key::Escape) {
        if let Some(cmd) = get_command(&window) {
            game.step_view(cmd);
        }

        // Re-render every frame for torch flicker animation
        render_scene(&game, &mut fb, frame);
        fb_to_u32(&fb, &mut buf);
        frame = frame.wrapping_add(1);

        window.update_with_buffer(&buf, WIDTH, HEIGHT).unwrap();
    }
}

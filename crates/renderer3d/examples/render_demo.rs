use roguelike_core::tier_micro::game::MicroGameState;
use roguelike_renderer3d::framebuffer::Framebuffer;
use roguelike_renderer3d::scene::render_scene;

fn main() {
    let seed: u16 = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(42);

    let width: u32 = std::env::args()
        .nth(2)
        .and_then(|s| s.parse().ok())
        .unwrap_or(640);

    let height: u32 = std::env::args()
        .nth(3)
        .and_then(|s| s.parse().ok())
        .unwrap_or(480);

    let game = MicroGameState::new_default(seed);
    let mut fb = Framebuffer::new(width, height);

    render_scene(&game, &mut fb, 0);

    let path = format!("render_seed{seed}_{width}x{height}.ppm");
    let mut file = std::fs::File::create(&path).expect("failed to create output file");
    fb.write_ppm(&mut file).expect("failed to write PPM");

    eprintln!("Wrote {path}");
}

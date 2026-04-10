use roguelike_core::tier_micro::game::MicroGameState;
use roguelike_renderer3d::framebuffer::Framebuffer;
use roguelike_renderer3d::scene::render_scene;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let ppm = args.iter().any(|a| a == "--ppm");

    let seed: u16 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(42);

    let (default_w, default_h) = if ppm { (640, 480) } else { (80, 48) };

    let width: u32 = args
        .get(2)
        .and_then(|s| s.parse().ok())
        .unwrap_or(default_w);

    let height: u32 = args
        .get(3)
        .and_then(|s| s.parse().ok())
        .unwrap_or(default_h);

    let game = MicroGameState::new_default(seed);
    let mut fb = Framebuffer::new(width, height);

    render_scene(&game, &mut fb, 0);

    if ppm {
        let path = format!("render_seed{seed}_{width}x{height}.ppm");
        let mut file = std::fs::File::create(&path).expect("failed to create output file");
        fb.write_ppm(&mut file).expect("failed to write PPM");
        eprintln!("Wrote {path}");
    } else {
        let stdout = std::io::stdout();
        let mut out = std::io::BufWriter::new(stdout.lock());
        fb.write_half_blocks(&mut out)
            .expect("failed to write terminal output");
    }
}

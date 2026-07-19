fn main() {
    let manifest = std::path::PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let source_path = manifest.join("data/game.toml");
    let source = std::fs::read_to_string(&source_path).expect("read canonical game.toml");
    let data = roguelike_content::parse_game_data(&source)
        .unwrap_or_else(|error| panic!("invalid canonical game.toml: {error}"));
    let generated = roguelike_content::emit_rust(&data);
    let output =
        std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap()).join("game_content.rs");
    std::fs::write(output, generated).expect("write generated game content");
    println!("cargo::rerun-if-changed={}", source_path.display());
}

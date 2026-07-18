//! Pass the GBA boot/linker script to the final link via a build-script
//! link argument. Environment `RUSTFLAGS` replaces config rustflags wholesale,
//! so keeping this mandatory argument in `.cargo/config.toml` is fragile.

fn main() {
    let dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    println!("cargo::rustc-link-arg-bins=-T{dir}/mono_boot.ld");
    println!("cargo::rerun-if-changed=mono_boot.ld");
}

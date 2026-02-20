fn main() {
    // No-op: we load libudev at runtime via dlopen instead of linking at
    // compile time.  This build script exists only to satisfy the `links`
    // key in Cargo.toml.
    println!("cargo:hwdb=false");
}

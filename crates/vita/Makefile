VPK = roguelike-vita.vpk

.PHONY: build run clean

# Build release .vpk via cargo-vita.
# Requires: VITASDK=/usr/local/vitasdk, cargo-vita installed, Rust nightly.
build:
	cargo vita build vpk -- --release

# Run in Vita3K emulator. Install Vita3K and add it to PATH.
# The emulator must be set up with a valid PSP2UPDAT.PUP firmware dump.
run: build
	Vita3K $(VPK)

clean:
	cargo clean
	rm -f $(VPK)

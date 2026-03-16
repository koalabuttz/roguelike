DOCKER_IMAGE = ghcr.io/koalabuttz/rust-mos:54c694f07-4537158-1bd9e164b
DOCKER_PATH = /usr/local/rust-mos/bin:/usr/local/bin:/usr/bin:/bin
PROJECT_ROOT = $(CURDIR)/../..
WORKSPACE_TOML = $(PROJECT_ROOT)/Cargo.toml
DOCKER_RUN = sudo docker run --rm -e PATH=$(DOCKER_PATH) -v $(PROJECT_ROOT):/project -w /project/crates/c64 $(DOCKER_IMAGE)
PRG = target/mos-c64-none/release/roguelike-c64

.PHONY: build run clean size map

# Build with rust-mos Docker image.
# Patches workspace edition to 2021 for rust-mos compatibility (restored after build).
build:
	@sed -i.bak 's/edition = "2024"/edition = "2021"/' $(WORKSPACE_TOML)
	@$(DOCKER_RUN) cargo build --release; rc=$$?; \
		mv $(WORKSPACE_TOML).bak $(WORKSPACE_TOML); \
		exit $$rc

# Generate linker map (builds with -Map flag)
MAP_RUSTFLAGS = -C link-arg=-Tlink.ld -C link-arg=-Wl,--no-check-sections -C link-arg=-Wl,-Map,map.txt
map:
	@sed -i.bak 's/edition = "2024"/edition = "2021"/' $(WORKSPACE_TOML)
	@$(DOCKER_RUN) env RUSTFLAGS="$(MAP_RUSTFLAGS)" cargo build --release; rc=$$?; \
		mv $(WORKSPACE_TOML).bak $(WORKSPACE_TOML); \
		exit $$rc

# Show RAM/hiram usage from linker map
size: map
	@python3 memmap.py map.txt

# Run in VICE emulator (requires x64sc in PATH)
run: build
	x64sc $(PRG)

clean:
	cargo clean

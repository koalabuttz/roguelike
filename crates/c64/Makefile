DOCKER_IMAGE = ghcr.io/koalabuttz/rust-mos:ac2fb2277-4537158-4aaa40e16
DOCKER_PATH = /usr/local/rust-mos/bin:/usr/local/bin:/usr/bin:/bin
PROJECT_ROOT = $(CURDIR)/../..
DOCKER_RUN = sudo docker run --rm -e PATH=$(DOCKER_PATH) -v $(PROJECT_ROOT):/project -w /project/crates/c64 $(DOCKER_IMAGE)
PRG = target/mos-c64-none/release/roguelike-c64

.PHONY: build debug run clean size map profile

# Dev console build — release optimizations + menu-driven dev tools via ← key.
debug:
	@$(DOCKER_RUN) cargo build --release --features dev-console

# Build with rust-mos Docker image.
build:
	@$(DOCKER_RUN) cargo build --release

# Generate linker map (builds with -Map flag)
MAP_RUSTFLAGS = -C link-arg=-Tlink.ld -C link-arg=-Wl,--no-check-sections -C link-arg=-Wl,--allow-multiple-definition -C link-arg=-Wl,-Map,map.txt -Z mir-opt-level=1
map:
	@$(DOCKER_RUN) env RUSTFLAGS="$(MAP_RUSTFLAGS)" cargo build --release

# Show RAM/hiram usage from linker map
size: map
	@python3 memmap.py map.txt

# Run in VICE emulator (requires x64sc in PATH)
run: build
	x64sc $(PRG)

# Profile via VICE remote monitor (requires x64sc in PATH)
PROFILE_TURNS ?= 50
PROFILE_MODE ?= builtin

profile: map
	python3 profile.py map.txt $(PRG) --turns $(PROFILE_TURNS) --mode $(PROFILE_MODE)

clean:
	cargo clean

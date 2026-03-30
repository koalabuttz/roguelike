TARGET = thumbv4t-none-eabi
ELF = target/$(TARGET)/release/roguelike-gba
GBA = roguelike-gba.gba

.PHONY: build dev run run-dev debug gdb clean size

# Build ELF + .gba ROM (release, no debug features)
build:
	cargo +nightly build --release
	agb-gbafix $(ELF) -o $(GBA) -p -t "ROGUELIKE" -c "RGLK" -m "DV"

# Build with dev feature (mGBA logging, stack canary, GDB breakpoints)
dev:
	cargo +nightly build --release --features dev
	agb-gbafix $(ELF) -o $(GBA) -p -t "ROGUELIKE" -c "RGLK" -m "DV"

# Run in mGBA SDL backend (no Qt flicker on Chromebook)
run: build
	mgba -2 $(GBA)

# Run dev build with all mGBA log levels visible
run-dev: dev
	mgba -2 -l 31 $(GBA)

# Launch mGBA with GDB stub on port 2345 (dev build for logging + breakpoints)
debug: dev
	mgba -2 -g $(GBA)

# Connect gdb-multiarch to a running mGBA GDB session (run after `make debug`)
gdb:
	gdb-multiarch -x gba.gdbinit $(ELF)

# Show ROM and section sizes
size: build
	readelf -S $(ELF) | grep -E '(text|rodata|data|ewram|bss|Size)'

clean:
	cargo +nightly clean
	rm -f $(GBA)

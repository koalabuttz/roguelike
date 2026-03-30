TARGET = thumbv4t-none-eabi
ELF = target/$(TARGET)/release/roguelike-gba
GBA = roguelike-gba.gba

.PHONY: build gba run clean size

# Build ELF + .gba ROM
build:
	cargo +nightly build --release
	agb-gbafix $(ELF) -o $(GBA) -p -t "ROGUELIKE" -c "RGLK" -m "DV"

# Run in mGBA SDL backend (no Qt flicker on Chromebook)
run: build
	mgba -2 $(GBA)

# Show ROM and section sizes
size: build
	readelf -S $(ELF) | grep -E '(text|rodata|data|ewram|bss|Size)'

clean:
	cargo +nightly clean
	rm -f $(GBA)

TARGET = thumbv4t-none-eabi
ELF = target/$(TARGET)/release/roguelike-gba
GBA = roguelike-gba.gba

.PHONY: build gba run clean size

# Build ELF (runnable in mGBA directly)
build:
	cargo +nightly build --release

# Build .gba ROM file (reads ELF directly, no separate objcopy)
gba: build
	agb-gbafix $(ELF) -o $(GBA) -t "ROGUELIKE" -c "RGLK" -m "DV"

# Run in mGBA (loads ELF with debug symbols)
run: build
	mgba-qt $(ELF)

# Show ROM and section sizes
size: build
	readelf -S $(ELF) | grep -E '(text|rodata|data|ewram|bss|Size)'

clean:
	cargo +nightly clean

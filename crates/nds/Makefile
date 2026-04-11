TARGET  := armv5te-none-eabi
ELF     := target/$(TARGET)/release/roguelike-nds
ARM7    := arm7/arm7.elf
ROM     := roguelike-nds.nds

.PHONY: build run clean arm7

build: $(ROM)

$(ARM7):
	python3 arm7/gen_arm7.py $(ARM7)

$(ELF): $(shell find src -name '*.rs') Cargo.toml
	cargo +nightly build --release

$(ROM): $(ELF) $(ARM7)
	ndstool -c $(ROM) \
		-9 $(ELF) -r9 0x02000000 -e9 0x02000000 \
		-7 $(ARM7) -r7 0x03800000 -e7 0x03800000

run: $(ROM)
	desmume $(ROM)

clean:
	cargo +nightly clean
	rm -f $(ROM) $(ARM7)

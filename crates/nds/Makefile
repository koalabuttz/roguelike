TARGET  := armv5te-none-eabi
ELF     := target/$(TARGET)/release/roguelike-nds
ARM7    := arm7/arm7.elf
ROM     := roguelike-nds.nds

.PHONY: build run clean

build: $(ROM)

# ARM9 builds first (Rust)
$(ELF): $(shell find src -name '*.rs') Cargo.toml nds9.ld
	cargo +nightly build --release

# ARM7 depends on ARM9 — extracts TOUCH_SHMEM address from the ARM9 ELF
# so the two processors agree on the shared memory location.
$(ARM7): $(ELF)
	$(eval SHMEM := $(shell nm $(ELF) | grep 'TOUCH_SHMEM$$' | head -1 | cut -c1-8))
	python3 arm7/gen_arm7.py $(ARM7) --shmem=0x$(SHMEM)

$(ROM): $(ELF) $(ARM7)
	ndstool -c $(ROM) \
		-9 $(ELF) -r9 0x02000000 -e9 0x02000000 \
		-7 $(ARM7) -r7 0x03800000 -e7 0x03800000

run: $(ROM)
	desmume $(ROM)

clean:
	cargo +nightly clean
	rm -f $(ROM) $(ARM7)

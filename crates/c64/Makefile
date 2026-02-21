DOCKER_IMAGE = mrkits/rust-mos:13f2838f9-334fc98-8f3a80f8
DOCKER_PATH = /usr/local/rust-mos/bin:/usr/local/bin:/usr/bin:/bin
DOCKER_RUN = sudo docker run --rm -e PATH=$(DOCKER_PATH) -v $(CURDIR):/work -w /work $(DOCKER_IMAGE)
PRG = target/mos-c64-none/release/roguelike-c64

.PHONY: build run clean size

# Build with rust-mos Docker image
build:
	$(DOCKER_RUN) cargo build --release

# Show binary size breakdown
size: build
	@echo "--- Binary size ---"
	@ls -la $(PRG)
	@echo ""
	@echo "--- Budget check ---"
	@SIZE=$$(stat -c%s $(PRG) 2>/dev/null || stat -f%z $(PRG)); \
	echo "PRG size: $$SIZE bytes"; \
	if [ $$SIZE -gt 16384 ]; then \
		echo "WARNING: Exceeds 16 KB budget!"; \
	else \
		echo "OK: Within 16 KB budget"; \
	fi

# Run in VICE emulator (requires x64sc in PATH)
run: build
	x64sc $(PRG)

clean:
	cargo clean

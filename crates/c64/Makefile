DOCKER_IMAGE = mrkits/rust-mos:13f2838f9-334fc98-8f3a80f8
DOCKER_PATH = /usr/local/rust-mos/bin:/usr/local/bin:/usr/bin:/bin
PROJECT_ROOT = $(CURDIR)/../..
WORKSPACE_TOML = $(PROJECT_ROOT)/Cargo.toml
DOCKER_RUN = sudo docker run --rm -e PATH=$(DOCKER_PATH) -v $(PROJECT_ROOT):/project -w /project/crates/c64 $(DOCKER_IMAGE)
PRG = target/mos-c64-none/release/roguelike-c64

.PHONY: build run clean size

# Build with rust-mos Docker image.
# Patches workspace edition to 2021 for rust-mos compatibility (restored after build).
build:
	@sed -i.bak 's/edition = "2024"/edition = "2021"/' $(WORKSPACE_TOML)
	@$(DOCKER_RUN) cargo build --release; rc=$$?; \
		mv $(WORKSPACE_TOML).bak $(WORKSPACE_TOML); \
		exit $$rc

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

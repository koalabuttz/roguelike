#!/usr/bin/env python3
"""Generate a minimal ARM7 ELF for the Nintendo DS.

The ARM7 has two jobs:
  1. Initialize the PM (power management IC) via SPI to enable LCD backlights.
  2. Poll the touchscreen controller (TSC) via SPI and store coordinates in
     shared memory for the ARM9 to read.

SPI registers (ARM7 only):
  SPICNT  = 0x040001C0  (control: baud, device select, enable, busy)
  SPIDATA = 0x040001C2  (8-bit read/write data)

SPICNT bit layout (from GBATEK):
  0-1   Baudrate (0=4MHz, 1=2MHz/Touchscr, 2=1MHz/Powerman, 3=512KHz)
  7     Busy Flag / Enable (see note below)
  8-9   Device Select (0=Powerman, 1=Firmware, 2=Touchscr, 3=Reserved)
  11    Chipselect Hold (0=Deselect after transfer, 1=Keep selected)
  15    SPI Bus Enable

Note: GBATEK says bit 7 is "busy (presumably read-only)" and bit 15 is
"enable". However, the proven PM init code below uses bit 7 as enable
and omits bit 15, and this works on real hardware. We follow the same
pattern for the touchscreen to stay consistent.

Note: GBATEK says device 0 = Powerman, but the proven PM init below uses
device select [9:8]=01. This discrepancy is documented but unresolved.
For the touchscreen, we use GBATEK's value [9:8]=10 (device 2). If this
doesn't work on real hardware, try [9:8]=11 then [9:8]=00.

Power Management IC registers:
  PM_CONTROL (reg 0): bit 2 = backlight bottom, bit 3 = backlight top
  Writing 0x0C to PM reg 0 enables both backlights.

Touchscreen controller (TSC2046 / AK4148AVT):
  Accessible via SPI device 2 at 2MHz baudrate.
  Control byte: Start(7) | Channel(6:4) | Mode(3) | SER(2) | PD(1:0)
    Channel 5 = X position, Channel 1 = Y position
    Mode 0 = 12-bit, SER 0 = differential
    PD 01 = ADC on, PD 00 = power down with pen IRQ enabled
  Response: 1 dummy bit + 12 data bits (MSB first) across 2 SPI bytes.

Pen-down detection:
  EXTKEYIN (0x04000136) bit 6: 0=pen touching, 1=pen released.
  Only valid when TSC is in PD mode 0 (pen IRQ enabled).

Shared memory (ARM7 writes, ARM9 reads):
  0x023FFF00: u16 raw_x     (12-bit ADC, 0-4095)
  0x023FFF02: u16 raw_y     (12-bit ADC, 0-4095)
  0x023FFF04: u16 pen_down  (1=touching, 0=not)
"""

import struct
import sys
import io

ENTRY = 0x03800000  # ARM7 WRAM base

# ---------------------------------------------------------------------------
# Parse CLI arguments
# ---------------------------------------------------------------------------
# Usage: gen_arm7.py <output.elf> --shmem=0x<addr>
# The --shmem address is extracted from the ARM9 ELF by the Makefile:
#   nm $(ELF) | grep TOUCH_SHMEM
out_path = "arm7.elf"
shmem_addr = None
for arg in sys.argv[1:]:
    if arg.startswith("--shmem="):
        shmem_addr = int(arg.split("=", 1)[1], 0)
    elif not arg.startswith("-"):
        out_path = arg

if shmem_addr is None:
    print("ERROR: --shmem=0x<addr> is required (address of TOUCH_SHMEM from ARM9 ELF)", file=sys.stderr)
    sys.exit(1)

# ---------------------------------------------------------------------------
# Register and constant addresses
# ---------------------------------------------------------------------------
SPICNT   = 0x040001C0
SPIDATA  = 0x040001C2
IME      = 0x04000208
IE       = 0x04000210
IF       = 0x04000214
HALTCNT  = 0x04000301   # ARM7 only, byte register
IRQ_FLAG = 0x0380FFF8   # BIOS IntrWait check flags (ARM7 WRAM)
IRQ_HAND = 0x0380FFFC   # BIOS user IRQ handler pointer (ARM7 WRAM)
SHMEM    = shmem_addr

# SPICNT values for PM (device 1, 4MHz baud — proven on real hardware)
SPICNT_PM_HOLD = 0x0980   # enable | device_1 | hold
SPICNT_PM_LAST = 0x0180   # enable | device_1 | no hold

# SPICNT values for touchscreen (2MHz baud)
# GBATEK says device 2 = Touchscr, but the proven PM code uses [9:8]=01
# for what GBATEK calls device 0. Device select values tried:
#   [9:8]=10 (0x0A81/0x0281) — GBATEK literal         ✗ did not work
#   [9:8]=11 (0x0B81/0x0381) — offset-by-one theory   ✗ did not work
#   [9:8]=00 (0x0881/0x0081) — zero                    ✗ did not work
# Now trying GBATEK literal + bit 15 (SPI Bus Enable):
SPICNT_TSC_HOLD = 0x8A81  # bus_en | enable | device_2 | hold | 2MHz
SPICNT_TSC_LAST = 0x8281  # bus_en | enable | device_2 | no hold | 2MHz

# TSC control bytes (12-bit differential mode)
TSC_READ_X = 0xD1  # Start | Channel 5 (X) | 12-bit | Diff | PD=01 (ADC on)
TSC_READ_Y = 0x90  # Start | Channel 1 (Y) | 12-bit | Diff | PD=00 (pen IRQ)

# ---------------------------------------------------------------------------
# ARM instruction encoding helpers
# ---------------------------------------------------------------------------
buf = io.BytesIO()
pc_offset = 0  # current write position in the binary


def w32(val):
    """Write a 32-bit little-endian word and advance the position."""
    global pc_offset
    buf.write(struct.pack("<I", val))
    pc_offset += 4


def arm_pc():
    """ARM pipeline: PC reads as current instruction address + 8."""
    return pc_offset + 8


def ldr_rd_pool(rd, pool_addr):
    """Emit `ldr Rd, [pc, #offset]` to load from a literal pool entry."""
    diff = pool_addr - arm_pc()
    if diff >= 0:
        assert diff < 4096, f"Pool offset +{diff} too large"
        w32(0xE59F0000 | (rd << 12) | diff)
    else:
        assert -diff < 4096, f"Pool offset {diff} too large"
        w32(0xE51F0000 | (rd << 12) | (-diff))


def branch(cond, target_offset):
    """Emit a conditional/unconditional branch.

    cond: 0xE = always, 0x1 = NE
    target_offset: byte offset of the target instruction.
    """
    diff = (target_offset - arm_pc()) >> 2  # word offset, signed
    imm24 = diff & 0x00FFFFFF
    w32((cond << 28) | (0b1010 << 24) | imm24)


# ---------------------------------------------------------------------------
# Literal pool (referenced by PC-relative loads from the code below)
# ---------------------------------------------------------------------------

# 0x00: branch over the literal pool → code_start
# We'll fix this up after emitting the pool.
branch_fixup_pos = pc_offset
w32(0)  # placeholder

# Pool entries (addresses are byte offsets in the binary)
POOL_SPICNT   = pc_offset; w32(SPICNT)
POOL_SPIDATA  = pc_offset; w32(SPIDATA)
POOL_IME      = pc_offset; w32(IME)
POOL_IE       = pc_offset; w32(IE)
POOL_IF       = pc_offset; w32(IF)
POOL_HALTCNT  = pc_offset; w32(HALTCNT)
POOL_IRQ_FLAG = pc_offset; w32(IRQ_FLAG)
POOL_IRQ_HAND = pc_offset; w32(IRQ_HAND)
POOL_IRQ_ADDR = pc_offset; w32(0)        # handler address, fixed up later
POOL_SHMEM    = pc_offset; w32(SHMEM)

code_start = pc_offset

# Fix up the branch-over-pool instruction
diff_words = (code_start - (branch_fixup_pos + 8)) >> 2
buf.seek(branch_fixup_pos)
buf.write(struct.pack("<I", 0xEA000000 | (diff_words & 0x00FFFFFF)))
buf.seek(0, 2)  # seek to end

# ---------------------------------------------------------------------------
# Initialization: disable interrupts, load register pointers
# ---------------------------------------------------------------------------

# Disable ARM7 interrupts (IME = 0)
ldr_rd_pool(0, POOL_IME)         # ldr r0, =IME
w32(0xE3A01000)                   # mov r1, #0
w32(0xE5801000)                   # str r1, [r0]

# Load SPI register addresses into callee-save registers
ldr_rd_pool(4, POOL_SPICNT)      # ldr r4, =SPICNT
ldr_rd_pool(5, POOL_SPIDATA)     # ldr r5, =SPIDATA

# ---------------------------------------------------------------------------
# PM init: enable both LCD backlights
# ---------------------------------------------------------------------------

# Step 1: Write PM register address 0 with CS hold
w32(0xE3A00A09)                   # mov r0, #0x09 << 8  → r0 = 0x0900
w32(0xE3800080)                   # orr r0, r0, #0x80   → r0 = 0x0980
w32(0xE1C400B0)                   # strh r0, [r4]       → SPICNT = PM + hold
w32(0xE3A00000)                   # mov r0, #0x00       → register address 0
w32(0xE1C500B0)                   # strh r0, [r5]       → SPIDATA = 0x00

# Busy wait
busy1 = pc_offset
w32(0xE1D400B0)                   # ldrh r0, [r4]
w32(0xE3100080)                   # tst r0, #0x80
w32(0x1AFFFFFC)                   # bne busy1 (-3 instructions)

# Step 2: Write data byte 0x0C (backlights on), release CS
w32(0xE3A00A01)                   # mov r0, #0x01 << 8  → r0 = 0x0100
w32(0xE3800080)                   # orr r0, r0, #0x80   → r0 = 0x0180
w32(0xE1C400B0)                   # strh r0, [r4]       → SPICNT = PM + no hold
w32(0xE3A0000C)                   # mov r0, #0x0C
w32(0xE1C500B0)                   # strh r0, [r5]       → SPIDATA = 0x0C

# Busy wait
busy2 = pc_offset
w32(0xE1D400B0)                   # ldrh r0, [r4]
w32(0xE3100080)                   # tst r0, #0x80
w32(0x1AFFFFFC)                   # bne busy2

# ---------------------------------------------------------------------------
# VBlank IRQ setup: install handler, enable VBlank interrupt, enable IME
# ---------------------------------------------------------------------------
# The DS ARM7 BIOS reads a user IRQ handler pointer from 0x0380FFFC and
# calls it with r0-r3 saved. Our handler acknowledges IF and updates the
# BIOS IntrWait check flags at 0x0380FFF8. After this, HALTCNT halt mode
# will wake on VBlank (~60Hz) instead of requiring a busy-wait delay.

# Install IRQ handler: write irq_handler address to [0x0380FFFC].
# The handler is emitted after the main loop — pool entry fixed up later.
ldr_rd_pool(0, POOL_IRQ_HAND)     # ldr r0, =0x0380FFFC
ldr_rd_pool(1, POOL_IRQ_ADDR)     # ldr r1, =irq_handler (fixup below)
w32(0xE5801000)                    # str r1, [r0]

# Set IE = 1 (VBlank only)
ldr_rd_pool(0, POOL_IE)           # ldr r0, =IE
w32(0xE3A01001)                    # mov r1, #1
w32(0xE5801000)                    # str r1, [r0]

# Acknowledge any pending interrupts (write all-ones to IF)
ldr_rd_pool(0, POOL_IF)           # ldr r0, =IF
w32(0xE3E01000)                    # mvn r1, #0           → r1 = 0xFFFFFFFF
w32(0xE5801000)                    # str r1, [r0]

# Enable IME (must be last — interrupts can fire after this)
ldr_rd_pool(0, POOL_IME)          # ldr r0, =IME
w32(0xE3A01001)                    # mov r1, #1
w32(0xE5801000)                    # str r1, [r0]

# Load HALTCNT address into r8 (used in sleep section)
ldr_rd_pool(8, POOL_HALTCNT)      # ldr r8, =HALTCNT

# ---------------------------------------------------------------------------
# Load touchscreen-related register pointers
# ---------------------------------------------------------------------------

ldr_rd_pool(9, POOL_SHMEM)        # ldr r9, =SHMEM

# Build SPICNT constants for touchscreen in r10/r11 (callee-save, built once)
# r10 = SPICNT_TSC_HOLD = 0x8A81
w32(0xE3A0AC8A)                   # mov r10, #0x8A << 8    → r10 = 0x8A00
w32(0xE38AA081)                   # orr r10, r10, #0x81    → r10 = 0x8A81
# r11 = SPICNT_TSC_LAST = 0x8281
w32(0xE3A0BC82)                   # mov r11, #0x82 << 8    → r11 = 0x8200
w32(0xE38BB081)                   # orr r11, r11, #0x81    → r11 = 0x8281

# ---------------------------------------------------------------------------
# Main loop: read touchscreen via SPI, detect pen from X value, store results
# ---------------------------------------------------------------------------
# EXTKEYIN bit 6 (PENIRQ) is always zero on DSi, so we can't use it for
# pen-down detection. Instead, we detect pen state from the TSC X channel:
#   X > 0x10 → pen touching (valid range ~0x100..0xED0)
#   X ≤ 0x10 → pen released (GBATEK: X=000h when released)

tsc_loop = pc_offset

# ---- Read X coordinate (TSC channel 5) ----

# Transfer 1: send control byte (hold CS)
w32(0xE1C4A0B0)                   # strh r10, [r4]   → SPICNT = TSC + hold
w32(0xE3A000D1)                   # mov r0, #0xD1    → X control byte
w32(0xE1C500B0)                   # strh r0, [r5]    → SPIDATA = 0xD1
busy3 = pc_offset
w32(0xE1D400B0)                   # ldrh r0, [r4]
w32(0xE3100080)                   # tst r0, #0x80
w32(0x1AFFFFFC)                   # bne busy3

# Transfer 2: clock out high byte (hold CS, SPICNT unchanged)
w32(0xE3A00000)                   # mov r0, #0
w32(0xE1C500B0)                   # strh r0, [r5]    → SPIDATA = 0x00 (dummy)
busy4 = pc_offset
w32(0xE1D400B0)                   # ldrh r0, [r4]
w32(0xE3100080)                   # tst r0, #0x80
w32(0x1AFFFFFC)                   # bne busy4
w32(0xE1D560B0)                   # ldrh r6, [r5]    → r6 = high byte

# Transfer 3: clock out low byte (release CS)
w32(0xE1C4B0B0)                   # strh r11, [r4]   → SPICNT = TSC + no hold
w32(0xE3A00000)                   # mov r0, #0
w32(0xE1C500B0)                   # strh r0, [r5]    → SPIDATA = 0x00 (dummy)
busy5 = pc_offset
w32(0xE1D400B0)                   # ldrh r0, [r4]
w32(0xE3100080)                   # tst r0, #0x80
w32(0x1AFFFFFC)                   # bne busy5
w32(0xE1D570B0)                   # ldrh r7, [r5]    → r7 = low byte

# Combine: r2 = ((r6 & 0x7F) << 5) | (r7 >> 3)
w32(0xE206607F)                   # and r6, r6, #0x7F
w32(0xE1A02286)                   # mov r2, r6, lsl #5
w32(0xE18221A7)                   # orr r2, r2, r7, lsr #3

# ---- Read Y coordinate (TSC channel 1, PD=00 to re-enable pen IRQ) ----

# Transfer 1: send control byte (hold CS)
w32(0xE1C4A0B0)                   # strh r10, [r4]   → SPICNT = TSC + hold
w32(0xE3A00090)                   # mov r0, #0x90    → Y control byte (PD=00, pen IRQ)
w32(0xE1C500B0)                   # strh r0, [r5]    → SPIDATA = 0x90
busy6 = pc_offset
w32(0xE1D400B0)                   # ldrh r0, [r4]
w32(0xE3100080)                   # tst r0, #0x80
w32(0x1AFFFFFC)                   # bne busy6

# Transfer 2: clock out high byte (hold CS, SPICNT unchanged)
w32(0xE3A00000)                   # mov r0, #0
w32(0xE1C500B0)                   # strh r0, [r5]    → SPIDATA = 0x00 (dummy)
busy7 = pc_offset
w32(0xE1D400B0)                   # ldrh r0, [r4]
w32(0xE3100080)                   # tst r0, #0x80
w32(0x1AFFFFFC)                   # bne busy7
w32(0xE1D560B0)                   # ldrh r6, [r5]    → r6 = high byte

# Transfer 3: clock out low byte (release CS)
w32(0xE1C4B0B0)                   # strh r11, [r4]   → SPICNT = TSC + no hold
w32(0xE3A00000)                   # mov r0, #0
w32(0xE1C500B0)                   # strh r0, [r5]    → SPIDATA = 0x00 (dummy)
busy8 = pc_offset
w32(0xE1D400B0)                   # ldrh r0, [r4]
w32(0xE3100080)                   # tst r0, #0x80
w32(0x1AFFFFFC)                   # bne busy8
w32(0xE1D570B0)                   # ldrh r7, [r5]    → r7 = low byte

# Combine: r3 = ((r6 & 0x7F) << 5) | (r7 >> 3)
w32(0xE206607F)                   # and r6, r6, #0x7F
w32(0xE1A03286)                   # mov r3, r6, lsl #5
w32(0xE18331A7)                   # orr r3, r3, r7, lsr #3

# ---- Store results and detect pen state from X value ----

w32(0xE1C920B0)                   # strh r2, [r9, #0]  → raw_x
w32(0xE1C930B2)                   # strh r3, [r9, #2]  → raw_y

# Pen detection: X > 0x10 = touching, X <= 0x10 = released.
# EXTKEYIN PENIRQ is always 0 on DSi so we use the ADC value directly.
w32(0xE3520010)                   # cmp r2, #0x10
w32(0xC3A00001)                   # movgt r0, #1       (GT: pen touching)
w32(0xD3A00000)                   # movle r0, #0       (LE: pen released)
w32(0xE1C900B4)                   # strh r0, [r9, #4]  → pen_down

# ---- sleep: halt until VBlank then loop ----

sleep = pc_offset
w32(0xE3A00080)                   # mov r0, #0x80      (HALTCNT value: halt mode)
w32(0xE5C80000)                   # strb r0, [r8]      → HALTCNT = 0x80 (halt)
# CPU sleeps here until VBlank IRQ fires (~60Hz), then resumes.
branch(0xE, tsc_loop)             # b tsc_loop

# ---------------------------------------------------------------------------
# IRQ handler (called by BIOS dispatcher via pointer at 0x0380FFFC)
# ---------------------------------------------------------------------------
# The BIOS saves r0-r3, r12, r14 before calling. We must:
#   1. Read IF (pending interrupts)
#   2. Acknowledge by writing back to IF (write-1-to-clear)
#   3. Update BIOS IntrWait check flags at 0x0380FFF8
#   4. Return via BX LR

irq_handler = pc_offset
ldr_rd_pool(0, POOL_IF)           # ldr r0, =IF
w32(0xE5901000)                   # ldr r1, [r0]       → r1 = pending IRQs
w32(0xE5801000)                   # str r1, [r0]       → acknowledge (write-1-to-clear)
ldr_rd_pool(0, POOL_IRQ_FLAG)     # ldr r0, =0x0380FFF8
w32(0xE5902000)                   # ldr r2, [r0]       → r2 = current flags
w32(0xE1822001)                   # orr r2, r2, r1     → r2 |= pending
w32(0xE5802000)                   # str r2, [r0]       → update BIOS flags
w32(0xE12FFF1E)                   # bx lr

# ---------------------------------------------------------------------------
# Fix up forward references
# ---------------------------------------------------------------------------

# Write the IRQ handler's runtime address into the pool entry.
handler_addr = ENTRY + irq_handler
buf.seek(POOL_IRQ_ADDR)
buf.write(struct.pack("<I", handler_addr))
buf.seek(0, 2)  # back to end

# ---------------------------------------------------------------------------
# Build ELF
# ---------------------------------------------------------------------------

binary = buf.getvalue()

e_ident = b"\x7fELF" + bytes([1, 1, 1, 0]) + b"\x00" * 8
ehdr = struct.pack(
    "<HHIIIIIHHHHHH",
    2,       # ET_EXEC
    40,      # EM_ARM
    1,       # version
    ENTRY,   # entry
    52,      # phoff
    0,       # shoff
    0x200,   # flags
    52,      # ehsize
    32,      # phentsize
    1,       # phnum
    40,      # shentsize
    0,       # shnum
    0,       # shstrndx
)

code_offset = 52 + 32
phdr = struct.pack(
    "<IIIIIIII",
    1,           # PT_LOAD
    code_offset,
    ENTRY,
    ENTRY,
    len(binary),
    len(binary),
    5,           # PF_R | PF_X
    4,
)

with open(out_path, "wb") as f:
    f.write(e_ident + ehdr + phdr + binary)

print(f"Generated {out_path}: {52 + 32 + len(binary)} bytes ({len(binary)} bytes code), entry 0x{ENTRY:08X}")
print(f"ARM7: PM backlights + TSC polling, SHMEM=0x{SHMEM:08X}")

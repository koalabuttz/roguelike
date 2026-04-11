#!/usr/bin/env python3
"""Generate a minimal ARM7 ELF for the Nintendo DS.

The ARM7 must initialize the power management IC (PM) via SPI to enable
the LCD backlights, and then idle forever. Without this, the screens
stay dark on real hardware.

SPI registers (ARM7 only):
  SPICNT  = 0x040001C0  (control: baud, device select, enable, busy)
  SPIDATA = 0x040001C2  (8-bit read/write data)

Power Management IC registers:
  PM_CONTROL (reg 0): bit 0 = sound amp enable, bit 2 = backlight bottom,
                       bit 3 = backlight top
  Writing 0x0C to PM reg 0 enables both backlights.

SPI protocol for PM write:
  1. SPICNT = 0x0080 | (device << 8) | chipselect_hold
     device 1 = Power Management, hold = bit 11
  2. Write register address (bit 7 = 0 for write) to SPIDATA
  3. Wait for not-busy (SPICNT bit 7)
  4. SPICNT = 0x0080 | (device << 8) (no hold = release CS)
  5. Write data byte to SPIDATA
  6. Wait for not-busy
"""

import struct
import sys

ENTRY = 0x03800000  # ARM7 WRAM base

# ARM7 machine code (ARM mode, little-endian)
# Register addresses
SPICNT  = 0x040001C0
SPIDATA = 0x040001C2
POWCNT2 = 0x04000304  # ARM7 power control: bit 0 = speakers, bit 1 = wifi

code = []

def emit(instruction):
    code.append(struct.pack("<I", instruction))

# Helper: encode LDR Rd, =imm via a literal pool at the end
# We'll use a simpler approach: build constants from MOV/ORR instructions

# For this tiny program, we'll hand-assemble the ARM instructions:
arm_code = bytearray()

# We use a PC-relative literal pool approach. The code jumps over the pool.
# Layout:
#   0x00: b init         (branch past literal pool)
#   0x04: .word SPICNT   (literal pool entry 0)
#   0x08: .word SPIDATA  (literal pool entry 1)
#   0x0C: init:          (actual code starts here)
#
# Actually, let's keep it simple — use MOV/MOVT (ARMv6T2+) ... but ARM7 is ARMv4T.
# On ARMv4T, we load constants from a literal pool after the code.

# Simpler approach: put literal pool right after a branch, referenced by PC-relative loads.

# Let's write the binary directly with pc-relative ldr instructions.

# ARM encoding for ldr rd, [pc, #offset]: 0xe59f_X_YYY where X=rd<<12, YYY=offset
# Note: PC reads as current instruction + 8 in ARM mode

import io

buf = io.BytesIO()

def w32(val):
    buf.write(struct.pack("<I", val))

# Code layout:
# 0x00: b skip_pool          ; jump over literal pool
# 0x04: pool_spicnt  = 0x040001C0
# 0x08: pool_spidata = 0x040001C2
# 0x0C: pool_ime     = 0x04000208
# 0x10: skip_pool:

# b skip_pool: offset = (0x10 - 0x00 - 8) / 4 = 2
w32(0xEA000002)   # b +2 (skip 3 words of pool, lands at 0x10)

# Literal pool
w32(SPICNT)        # [0x04]
w32(SPIDATA)       # [0x08]
w32(0x04000208)    # [0x0C] IME register

# 0x10: skip_pool - actual init code starts here

# Disable ARM7 interrupts (IME = 0)
# ldr r0, [pc, #-12]   ; load IME address from pool at 0x0C
# PC at 0x10 reads as 0x18, so offset = 0x0C - 0x18 = -0x0C
w32(0xE51F000C)    # ldr r0, [pc, #-0x0C]  → loads from 0x18-0x0C = 0x0C ✓
w32(0xE3A01000)    # mov r1, #0
w32(0xE5801000)    # str r1, [r0]

# Load SPICNT address into r4
# PC at 0x1C reads as 0x24, need pool at 0x04, offset = 0x04-0x24 = -0x20
w32(0xE51F4020)    # ldr r4, [pc, #-0x20] → loads from 0x24-0x20 = 0x04 ✓

# Load SPIDATA address into r5
# PC at 0x20 reads as 0x28, need pool at 0x08, offset = 0x08-0x28 = -0x20
w32(0xE51F5020)    # ldr r5, [pc, #-0x20] → loads from 0x28-0x20 = 0x08 ✓

# --- Write PM register 0 = 0x0C (enable both backlights) ---

# SPICNT = 0x0180 | (1 << 8) | (1 << 11) = 0x0980
#   bit 7: enable (0x80)
#   bit 8: 8-bit transfer (0x100) — actually bits 8-9 are device select
#   device 1 = power management → bits [9:8] = 01
#   bit 11: CS hold (keep selected for multi-byte transfer)
# SPICNT for PM write with hold: enable | device_PM | hold
#   = 0x0080 | (1 << 8) | (1 << 11) = 0x0080 | 0x0100 | 0x0800 = 0x0980

# Step 1: Write register address (0x00 = PM_CONTROL, bit 7=0 for write)
w32(0xE3A00A09)    # mov r0, #0x09 << 8 → r0 = 0x0900
w32(0xE3800080)    # orr r0, r0, #0x80  → r0 = 0x0980
w32(0xE1C400B0)    # strh r0, [r4]      → SPICNT = 0x0980

w32(0xE3A00000)    # mov r0, #0x00      → register address 0 (PM_CONTROL)
w32(0xE1C500B0)    # strh r0, [r5]      → SPIDATA = 0x00

# Wait for SPI not busy (SPICNT bit 7 goes low when done)
# busy_wait_1:
w32(0xE1D400B0)    # ldrh r0, [r4]      → read SPICNT
w32(0xE3100080)    # tst r0, #0x80      → test busy bit
w32(0x1AFFFFFC)    # bne busy_wait_1    → branch back 3 instructions if busy

# Step 2: Write data byte (0x0C = enable both backlights)
# SPICNT without hold (release CS after this byte):
w32(0xE3A00A01)    # mov r0, #0x01 << 8 → r0 = 0x0100
w32(0xE3800080)    # orr r0, r0, #0x80  → r0 = 0x0180
w32(0xE1C400B0)    # strh r0, [r4]      → SPICNT = 0x0180 (no hold)

w32(0xE3A0000C)    # mov r0, #0x0C      → data: backlights on
w32(0xE1C500B0)    # strh r0, [r5]      → SPIDATA = 0x0C

# Wait for SPI not busy
# busy_wait_2:
w32(0xE1D400B0)    # ldrh r0, [r4]      → read SPICNT
w32(0xE3100080)    # tst r0, #0x80      → test busy bit
w32(0x1AFFFFFC)    # bne busy_wait_2

# --- Idle forever ---
# halt:
w32(0xE3A00000)    # mov r0, #0
w32(0xEE070F90)    # mcr p15, 0, r0, c7, c0, 4  → wait for interrupt (halt)
w32(0xEAFFFFFD)    # b halt

binary = buf.getvalue()

# Build ELF
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

out = sys.argv[1] if len(sys.argv) > 1 else "arm7.elf"
with open(out, "wb") as f:
    f.write(e_ident + ehdr + phdr + binary)

print(f"Generated {out}: {52 + 32 + len(binary)} bytes ({len(binary)} bytes code), entry 0x{ENTRY:08X}")
print(f"ARM7 initializes PM backlights then halts.")

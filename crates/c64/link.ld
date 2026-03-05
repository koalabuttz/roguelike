/* Custom C64 linker script for roguelike-c64.
 *
 * Memory layout:
 *   ram    $0801-$CFFF  ~50 KB  Code, rodata, data, BSS, static stacks
 *   ioram  $D000-$DFFF    4 KB  Overlay code (under I/O, banked in/out via $01)
 *   hiram  $E000-$FFF7  ~8 KB   Game state (.noinit.state)
 *
 * init_hardware() unmaps KERNAL ($E000-$FFFF) and BASIC ($A000-$BFFF)
 * ROMs via CPU port $35.  BASIC's 8 KB is already inside the ram region.
 * KERNAL's 8 KB becomes hiram.  The soft stack is relocated to $FFF8.
 *
 * The .overlay section holds pure-computation functions (FOV, map gen)
 * at VMA $D000 (ioram).  The PRG loads this code into main RAM (LMA
 * follows .bss); init_hardware() copies it to $D000 with I/O banked
 * out.  The LMA space is then reused by .noinit (NOLOAD).
 * This intentional LMA/VMA overlap requires --no-check-sections.
 */

__basic_zp_start = 0x0002;
__basic_zp_end = 0x0090;

MEMORY {
    ram (rw) : ORIGIN = 0x0801, LENGTH = 0xC7FF
    ioram (rwx) : ORIGIN = 0xD000, LENGTH = 0x1000
    hiram (rw) : ORIGIN = 0xE000, LENGTH = 0x1FF8
}

/* Imaginary registers in BASIC zero page area. */
__rc0 = __basic_zp_start;
INCLUDE imag-regs.ld

MEMORY { zp : ORIGIN = __rc31 + 1, LENGTH = __basic_zp_end - (__rc31 + 1) }

INPUT(basic-header.o)
INPUT(unmap-basic.o)

REGION_ALIAS("c_readonly", ram)
REGION_ALIAS("c_writeable", ram)

/* Soft stack starts in main RAM (CRT uses this before KERNAL is
 * unmapped).  init_hardware() relocates it to $FFF8 at startup. */
__stack = 0xD000;

SECTIONS {
    .basic_header : { *(.basic_header) }

    INCLUDE zp.ld
    INCLUDE text.ld
    INCLUDE rodata.ld
    INCLUDE data.ld
    INCLUDE bss.ld

    /* Overlay code (~3.6 KB) placed under I/O at $D000-$DFFF.
     * VMA is in ioram (where the code executes after being copied).
     * LMA is in ram at the current position (right after .bss).
     * Using AT(.) instead of AT> ram to avoid advancing the ram
     * region counter — .noinit (NOLOAD) reuses the same addresses. */
    __overlay_lma_start = .;
    .overlay : AT(__overlay_lma_start) {
        __overlay_vma_start = .;
        *(.overlay .overlay.*)
        __overlay_vma_end = .;
    } > ioram

    /* Game state (~4.5 KB) placed in freed KERNAL region.
     * Must come BEFORE .noinit so the wildcard doesn't claim it.
     * Safe: NOLOAD (not in the PRG) and only accessed during
     * gameplay, after init_hardware() unmaps KERNAL. */
    .noinit.state (NOLOAD) : { *(.noinit.state) } >hiram

    /* Compiler-generated static stacks (~10 KB) stay in ram — too large
     * for hiram's ~3.5 KB remaining after game state. */
    .noinit (NOLOAD) : { INCLUDE noinit-sections.ld } >ram
}

OUTPUT_FORMAT {
    SHORT(ORIGIN(ram))
    TRIM(ram)
}

/* Custom C64 linker script for roguelike-c64.
 *
 * Based on the default mos-c64-none script, with one key change:
 * adds a second RAM region at $E000-$FFF7 (freed KERNAL ROM area).
 *
 * The game unmaps KERNAL in init_hardware() and relocates the soft
 * stack to $FFF8.  This gives the linker ~8 KB of extra space for
 * .noinit (static stacks and imaginary register spills) beyond the
 * main $0801-$CFFF region.
 */

__basic_zp_start = 0x0002;
__basic_zp_end = 0x0090;

MEMORY {
    ram (rw) : ORIGIN = 0x0801, LENGTH = 0xC7FF
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

    /* Game state (~4.5 KB) placed in freed KERNAL region.
     * Must come BEFORE .noinit so the wildcard doesn't claim it.
     * Safe: NOLOAD (not in the PRG) and only accessed during
     * gameplay, after init_hardware() unmaps KERNAL. */
    .noinit.state (NOLOAD) : { *(.noinit.state) } >hiram

    /* Compiler-generated static stacks (~9 KB) stay in main ram. */
    .noinit (NOLOAD) : { INCLUDE noinit-sections.ld } >ram
}

OUTPUT_FORMAT {
    SHORT(ORIGIN(ram))
    TRIM(ram)
}

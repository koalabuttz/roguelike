/* Nintendo DS ARM9 linker script
 *
 * Memory map:
 *   ITCM  — 0x0100_0000, 32 KB (fast instruction RAM, ARM9 only)
 *   Main  — 0x0200_0000,  4 MB (shared between ARM9 and ARM7)
 *   DTCM  — 0x0B00_0000, 16 KB (fast data RAM, ARM9 only — stack lives here)
 *   VRAM  — 0x0680_0000, 656 KB (bank-switched, used for display)
 *
 * The .nds loader places the ARM9 binary at 0x02000000 and jumps to _start.
 */

ENTRY(_start)

MEMORY
{
    itcm  : ORIGIN = 0x01000000, LENGTH = 32K
    main  : ORIGIN = 0x02000000, LENGTH = 4M
    dtcm  : ORIGIN = 0x0B000000, LENGTH = 16K
}

SECTIONS
{
    /* Code and read-only data in main RAM */
    .text : ALIGN(4)
    {
        *(.text._start)    /* entry point first */
        *(.text .text.*)
        *(.rodata .rodata.*)
    } > main

    /* Initialized data */
    .data : ALIGN(4)
    {
        *(.data .data.*)
    } > main

    /* Zero-initialized data */
    .bss (NOLOAD) : ALIGN(4)
    {
        __bss_start = .;
        *(.bss .bss.*)
        *(COMMON)
        . = ALIGN(4);
        __bss_end = .;
    } > main

    /* Heap starts after BSS (grows up) */
    . = ALIGN(8);
    __heap_start = .;

    /* Stack in DTCM (grows down from top) */
    __stack_top = ORIGIN(dtcm) + LENGTH(dtcm);

    /DISCARD/ :
    {
        *(.ARM.exidx .ARM.exidx.*)
        *(.ARM.extab .ARM.extab.*)
        *(.comment)
        *(.debug*)
    }
}

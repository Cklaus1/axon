/* Axon guest kernel linker script.
 *
 * Firecracker loads the kernel at physical address 0x100000 (1 MiB) and
 * jumps to it in 32-bit protected mode.  The Linux boot-protocol setup
 * header is placed at offset 0x1F1 inside the first 512-byte sector that
 * Firecracker inspects; that sector lives in the .boot.setup section below.
 */

OUTPUT_FORMAT(elf64-x86-64)
ENTRY(_start32)

SECTIONS {
    /* ── Boot-protocol setup sector (read by Firecracker at load time) ──── */
    . = 0x100000;

    .boot.setup : {
        KEEP(*(.boot.setup))
    }

    /* ── 32-bit protected-mode entry + 64-bit bootstrap ─────────────────── */
    .text32 : {
        *(.text32)
    }

    /* Align kernel proper to 2 MiB (huge-page boundary). */
    . = ALIGN(0x200000);

    .text : {
        *(.text .text.*)
    }

    .rodata : {
        *(.rodata .rodata.*)
    }

    .data : {
        *(.data .data.*)
    }

    .bss (NOLOAD) : {
        _bss_start = .;
        *(.bss .bss.*)
        *(COMMON)
        _bss_end = .;
    }

    /* Stack — 64 KiB, 16-byte aligned.  BSS region is zeroed by _start32. */
    . = ALIGN(16);
    _stack_bottom = .;
    . += 0x10000;
    _stack_top = .;

    /DISCARD/ : {
        *(.comment)
        *(.eh_frame)
        *(.note*)
    }
}

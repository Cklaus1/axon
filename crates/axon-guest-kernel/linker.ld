/* Axon guest kernel linker script.
 *
 * Firecracker / QEMU -kernel loads our ELF and enters at the ELF entry point
 * (e_entry = _start32) in 32-bit protected mode with ESI = &boot_params.
 * No Linux setup-sector needed — loaders construct their own boot_params.
 */

OUTPUT_FORMAT(elf64-x86-64)
ENTRY(_start32)

SECTIONS {
    /* ── PVH ELF note (QEMU PVH boot — read at load time, not executed) ─── */
    . = 0x100000;

    .note.Xen : {
        KEEP(*(.note.Xen))
    }

    /* ── 32-bit protected-mode entry ─────────────────────────────────────── */
    .text32 : {
        KEEP(*(.text32))
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

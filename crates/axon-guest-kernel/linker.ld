/* Axon guest kernel linker script.
 *
 * Dual entry-point design:
 *   e_entry = _start64_boot  — Firecracker enters here in 64-bit mode.
 *   PVH note (_start32)      — QEMU reads this and enters in 32-bit PM.
 *
 * No Linux setup-sector needed — loaders construct their own boot_params.
 */

OUTPUT_FORMAT(elf64-x86-64)
ENTRY(_start64_boot)

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
        /* _start64_boot MUST be first: it is the ELF e_entry (Firecracker). */
        KEEP(*(.text64_entry))
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

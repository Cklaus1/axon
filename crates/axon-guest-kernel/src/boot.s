/* axon-guest-kernel/src/boot.s
 *
 * 32-bit protected-mode entry for Firecracker / QEMU -kernel.
 *
 * Both loaders enter at our ELF entry point (e_entry = _start32) in 32-bit
 * protected mode with a flat 4 GiB GDT already loaded, interrupts off, and:
 *   ESI = physical address of struct boot_params
 *
 * We:
 *  1. Re-load our own GDT (clean known-good descriptors)
 *  2. Build a minimal PML4/PDPT/PD identity map (first 4 GiB, 2 MiB pages)
 *  3. Enable PAE, CR3, EFER.LME, paging → enter 64-bit long mode
 *  4. Call kernel_main(boot_params_phys: u64)
 */

/* ── PVH ELF note (QEMU -kernel with 64-bit ELF uses XEN_ELFNOTE_PHYS32_ENTRY) ── */
.section ".note.Xen", "a"
.align 4
    .long 4                  /* namesz: "Xen\0" */
    .long 4                  /* descsz: 4-byte 32-bit entry point */
    .long 18                 /* type: XEN_ELFNOTE_PHYS32_ENTRY */
    .ascii "Xen\0"           /* name */
    .long _start32           /* desc: 32-bit protected-mode entry */

/* ── 32-bit protected-mode entry ────────────────────────────────────── */
.section ".text32", "ax"
.code32

/* ── 32-bit protected-mode entry ────────────────────────────────────── */

.global _start32
_start32:
    cli

    /* Detect boot source via EAX magic:
     *   0x2BADB002 = multiboot (EBX = multiboot info, NOT boot_params)
     *   anything else = Firecracker / QEMU Linux protocol (ESI = boot_params)
     */
    cmpl    $0x2BADB002, %eax
    je      _multiboot_entry

    /* Firecracker/Linux: save boot_params (ESI) in EBP. */
    movl    %esi, %ebp
    jmp     _init_gdt

_multiboot_entry:
    /* Multiboot: no boot_params — pass 0 so mmds falls back to open policy. */
    xorl    %ebp, %ebp

_init_gdt:
    /* Load our flat GDT. */
    lgdt    _gdt32_ptr

    /* Reload code segment by far-jump to our 32-bit CS (selector 0x08). */
    ljmp    $0x08, $_pm_flush32
_pm_flush32:
    movw    $0x10, %ax
    movw    %ax, %ds
    movw    %ax, %es
    movw    %ax, %fs
    movw    %ax, %gs
    movw    %ax, %ss

    /* Zero BSS (page-table buffers live here).
     * _bss_start/_bss_end are provided by the linker script. */
    movl    $_bss_start, %edi
    movl    $_bss_end,   %ecx
    subl    %edi, %ecx
    xorl    %eax, %eax
    rep stosl

    /* Build identity page tables (4 GiB, 2 MiB huge pages).
     *
     * Memory layout (all in BSS, already zeroed above):
     *   _pml4  — 4 KiB PML4 table
     *   _pdpt  — 4 KiB PDPT table
     *   _pd    — 4 KiB PD table (512 × 2 MiB entries)
     */

    /* PML4[0] → PDPT  (present, writable, no XD) */
    movl    $_pdpt,  %eax
    orl     $0x3,    %eax    /* P | RW */
    movl    %eax,    (_pml4)

    /* PDPT[0] → PD */
    movl    $_pd,    %eax
    orl     $0x3,    %eax
    movl    %eax,    (_pdpt)

    /* PD[0..511]: each maps a 2 MiB chunk starting at 0x0 */
    movl    $_pd,    %edi     /* PD table write pointer */
    movl    $512,    %ecx     /* loop counter */
    xorl    %esi,    %esi     /* physical base for this entry (starts at 0) */
_pd_loop:
    movl    %esi,    %eax
    orl     $0x83,   %eax    /* P | RW | PS (huge-page) */
    movl    %eax,    (%edi)
    /* upper 32 bits are already zero from the BSS clear */
    addl    $8,      %edi
    addl    $0x200000, %esi
    loopl   _pd_loop

    /* Enable PAE. */
    movl    %cr4,    %eax
    orl     $0x20,   %eax
    movl    %eax,    %cr4

    /* Load CR3 with PML4 physical address. */
    movl    $_pml4,  %eax
    movl    %eax,    %cr3

    /* Set EFER.LME (long-mode enable). */
    movl    $0xC0000080, %ecx
    rdmsr
    orl     $0x100,  %eax
    wrmsr

    /* Enable paging (also activates long mode). */
    movl    %cr0,    %eax
    orl     $0x80000001, %eax   /* PG | PE */
    movl    %eax,    %cr0

    /* Far jump into the 64-bit code segment (selector 0x18). */
    ljmp    $0x18, $_start64

/* ── GDT ────────────────────────────────────────────────────────────── */
.align 16
_gdt32:
    .quad 0x0000000000000000    /* 0x00: null */
    .quad 0x00CF9A000000FFFF    /* 0x08: 32-bit code (base=0, limit=4G) */
    .quad 0x00CF92000000FFFF    /* 0x10: 32-bit data */
    .quad 0x00AF9A000000FFFF    /* 0x18: 64-bit code (L=1) */
    .quad 0x00AF92000000FFFF    /* 0x20: 64-bit data */
_gdt32_end:

_gdt32_ptr:
    .short  _gdt32_end - _gdt32 - 1
    .long   _gdt32

/* ── BSS page-table buffers ──────────────────────────────────────────── */
.section ".bss"
.align 4096
_pml4: .fill 4096, 1, 0
_pdpt: .fill 4096, 1, 0
_pd:   .fill 4096, 1, 0

/* ── 64-bit entry ────────────────────────────────────────────────────── */
.section ".text"
.code64

.global _start64
_start64:
    /* Set up the kernel stack. */
    movabsq $_stack_top, %rsp
    xorq    %rbp, %rbp

    /* Reload 64-bit data segment. */
    movw    $0x20, %ax
    movw    %ax,   %ds
    movw    %ax,   %es
    movw    %ax,   %ss
    xorw    %ax,   %ax
    movw    %ax,   %fs
    movw    %ax,   %gs

    /* Pass boot_params (saved in EBP by _start32) as the first argument.
     * EBP holds a 32-bit physical address; zero-extend to 64 bits via MOVL. */
    movl    %ebp, %edi

    /* Call the Rust kernel entry point; it never returns. */
    callq   kernel_main

1:  hlt
    jmp 1b

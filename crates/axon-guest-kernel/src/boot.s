/* axon-guest-kernel/src/boot.s
 *
 * Linux x86 boot protocol v2.15 header + 32-bit protected-mode entry.
 * Firecracker reads the setup header at offset 0x1F1 from the start of
 * the image, then jumps to code32_start (0x100000) in 32-bit PM with:
 *   - A20 gate enabled
 *   - CR0.PE = 1, paging off
 *   - A flat 32-bit GDT loaded (CS=__KERNEL32_CS, DS/ES/SS=__KERNEL_DS)
 *   - Boot params pointer in ESI (struct boot_params *)
 *   - No FPU, no interrupts
 *
 * We re-establish our own GDT, build minimal identity-map page tables,
 * enter 64-bit long mode, and call kernel_main(boot_params_ptr: u64).
 */

.section ".boot.setup", "ax"

/* ── Linux boot-protocol setup sector (512 bytes) ───────────────────── */

_boot_sector:
    /* Real-mode bootstrap code — unused (LOAD_HIGH), but must fill 512 B. */
    .fill 0x1F0, 1, 0x00        /* pad to offset 0x1F0 */

/* Setup header at offset 0x1F1 (relative to start of image = 0x100000) */
_setup_header:
    .byte  1                    /* 0x1F1: setup_sects = 1 (one 512-byte sector) */
    .byte  0                    /* 0x1F2: root_flags */
    .long  0                    /* 0x1F4: syssize */
    .short 0                    /* 0x1F8: ram_size */
    .short 0                    /* 0x1FA: vid_mode = NORMAL_VGA */
    .short 0xFFFF               /* 0x1FC: root_dev */
    .short 0xAA55               /* 0x1FE: boot_flag (magic) */

    /* Header proper — read by the bootloader / Firecracker */
    .ascii "HdrS"               /* 0x202: header magic */
    .short 0x020f               /* 0x206: version = 2.15 */
    .long  0                    /* 0x208: realmode_swtch */
    .long  0                    /* 0x20C: start_sys_seg (obsolete) */
    .short 0                    /* 0x210: kernel_version (ptr into setup) */
    .byte  0xFF                 /* 0x212: type_of_loader = unknown */
    .byte  0x01                 /* 0x213: loadflags = LOADED_HIGH */
    .short 0                    /* 0x214: setup_move_size */
    .long  _start32             /* 0x216: code32_start = PM entry point */
    .long  0                    /* 0x21A: ramdisk_image */
    .long  0                    /* 0x21E: ramdisk_size */
    .long  0                    /* 0x222: bootsect_kludge (obsolete) */
    .short 0                    /* 0x226: heap_end_ptr */
    .byte  0                    /* 0x228: ext_loader_ver */
    .byte  0                    /* 0x229: ext_loader_type */
    .long  0                    /* 0x22A: cmd_line_ptr */
    .long  0xFFFFFFFF           /* 0x22E: initrd_addr_max */
    .long  0                    /* 0x232: kernel_alignment */
    .byte  0                    /* 0x236: relocatable_kernel */
    .byte  0                    /* 0x237: min_alignment */
    .short 0                    /* 0x238: xloadflags */
    .long  0                    /* 0x23A: cmdline_size */
    .long  0                    /* 0x23E: hardware_subarch */
    .quad  0                    /* 0x242: hardware_subarch_data */
    .long  0                    /* 0x24A: payload_offset */
    .long  0                    /* 0x24E: payload_length */
    .quad  0                    /* 0x252: setup_data */
    .quad  0                    /* 0x25A: pref_address */
    .long  0                    /* 0x262: init_size */
    .long  0                    /* 0x266: handover_offset */
    .long  0                    /* 0x26A: kernel_info_offset */

    /* Pad to exactly 512 bytes. */
    .org   0x200
    .fill  0x200 - (. - _boot_sector), 1, 0

/* ── 32-bit protected-mode entry ─────────────────────────────────────── */

.section ".text32", "ax"
.code32

.global _start32
_start32:
    /* Disable interrupts; Firecracker enters here with IF cleared already. */
    cli

    /* Save boot_params pointer (ESI) — we'll pass it to kernel_main. */
    movl    %esi, %ebp

    /* Load our own flat GDT so we control the descriptor layout. */
    lgdt    _gdt32_ptr - 0x100000

    /* Reload segment registers from our GDT. */
    ljmp    $0x08, $_pm_flush32
_pm_flush32:
    movw    $0x10, %ax
    movw    %ax, %ds
    movw    %ax, %es
    movw    %ax, %fs
    movw    %ax, %gs
    movw    %ax, %ss

    /* Zero BSS. */
    movl    $_bss_start, %edi
    movl    $_bss_end,   %ecx
    subl    %edi, %ecx
    xorl    %eax, %eax
    rep stosl

    /* Build identity-map page tables in a static region.
     * We map the first 4 GiB with 2 MiB huge pages (PS bit) so that
     * the kernel can address all guest memory without a TLB flush.
     * Tables are placed at fixed physical addresses (see linker.ld BSS). */
    movl    $_pml4,   %edi
    movl    $_pdpt,   %esi
    movl    $_pd,     %edx

    /* PML4[0] → PDPT (present, writable) */
    movl    %esi, %eax
    orl     $0x3, %eax
    movl    %eax, (%edi)

    /* PDPT[0] → PD (present, writable) */
    movl    %edx, %eax
    orl     $0x3, %eax
    movl    %eax, (%esi)

    /* PD[0..511] → 512 × 2 MiB huge pages starting at 0x0 */
    movl    $512, %ecx
    movl    $0,   %edi     /* start physical address */
    movl    %edx, %esi     /* PD table pointer */
1:
    movl    %edi, %eax
    orl     $0x83, %eax    /* present | writable | huge-page */
    movl    %eax, (%esi)
    addl    $8, %esi
    addl    $0x200000, %edi
    loopl   1b

    /* Enable PAE. */
    movl    %cr4, %eax
    orl     $0x20, %eax
    movl    %eax, %cr4

    /* Point CR3 at PML4. */
    movl    $_pml4, %eax
    movl    %eax,   %cr3

    /* Set EFER.LME to enable long mode. */
    movl    $0xC0000080, %ecx
    rdmsr
    orl     $0x100, %eax   /* LME */
    wrmsr

    /* Enable paging (also activates long mode since LME is set). */
    movl    %cr0, %eax
    orl     $0x80000001, %eax   /* PG | PE */
    movl    %eax, %cr0

    /* Far jump into 64-bit code segment to flush the instruction pipeline. */
    ljmp    $0x18, $_start64

/* ── GDT for protected + long mode ──────────────────────────────────── */
.align 16
_gdt32:
    .quad 0x0000000000000000    /* 0x00: null descriptor */
    .quad 0x00CF9A000000FFFF    /* 0x08: 32-bit code: base=0, limit=4G, DPL=0 */
    .quad 0x00CF92000000FFFF    /* 0x10: 32-bit data: base=0, limit=4G, DPL=0 */
    .quad 0x00AF9A000000FFFF    /* 0x18: 64-bit code: L=1, DPL=0 */
    .quad 0x00AF92000000FFFF    /* 0x20: 64-bit data: base=0, DPL=0 */
_gdt32_end:

_gdt32_ptr:
    .short  _gdt32_end - _gdt32 - 1
    .long   _gdt32

/* ── Page table buffers (in BSS, zeroed by _start32) ─────────────────── */
.section ".bss"
.align 4096
_pml4: .fill 4096, 1, 0
_pdpt: .fill 4096, 1, 0
_pd:   .fill 4096, 1, 0

/* ── 64-bit kernel entry ──────────────────────────────────────────────── */
.section ".text"
.code64

_start64:
    /* Set up the kernel stack. */
    movabsq $_stack_top, %rsp

    /* Reload 64-bit data segments. */
    movw    $0x20, %ax
    movw    %ax, %ds
    movw    %ax, %es
    movw    %ax, %ss
    xorw    %ax, %ax
    movw    %ax, %fs
    movw    %ax, %gs

    /* Pass boot_params pointer (saved in EBP by _start32) as first arg. */
    movl    %ebp, %edi          /* rdi = boot_params_phys_addr (u32 zero-extended) */

    /* Call the Rust kernel main.  Does not return. */
    callq   kernel_main

    /* Halt loop if kernel_main returns (should never happen). */
1:  hlt
    jmp 1b

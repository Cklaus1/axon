; R17 Slice 1 — multiboot1 boot stub + 32→64 long-mode switch
;
; Assemble:  nasm -f elf64 boot_stub.asm -o boot_stub.o
;
; This object is linked with the Axon kernel object to produce a bootable ELF:
;   axon build --freestanding --emit-obj hello_kernel_slice1.ax --out kernel.o
;   nasm -f elf64 boot_stub.asm -o boot_stub.o
;   ld -T scripts/kernel.ld --entry _start -o kernel.elf boot_stub.o kernel.o
;
; QEMU boot:
;   qemu-system-x86_64 -kernel kernel.elf -debugcon stdio -display none -no-reboot

; ── Multiboot1 header ────────────────────────────────────────────────────────
; Must be 32-bit aligned and within the first 8192 bytes of the loaded image.

MULTIBOOT1_MAGIC equ 0x1BADB002
MULTIBOOT1_FLAGS equ 0x00000000    ; no optional features (no module alignment, no memory map)
MULTIBOOT1_CKSUM equ (-(MULTIBOOT1_MAGIC + MULTIBOOT1_FLAGS))

section .text.entry
align 4
mb1_header:
    dd MULTIBOOT1_MAGIC
    dd MULTIBOOT1_FLAGS
    dd MULTIBOOT1_CKSUM

; ── 64-bit GDT (needed for long mode) ───────────────────────────────────────
section .rodata
align 8
gdt64:
    dq 0                    ; null descriptor
    dq 0x00af9a000000ffff   ; code64, DPL=0, present, execute/read
    dq 0x00af92000000ffff   ; data64, DPL=0, present, read/write
gdt64_ptr:
    dw ($ - gdt64 - 1)     ; limit (2 bytes)
    dd gdt64                ; base  (4 bytes — low 4GB is fine for our kernel)

; ── Page tables — identity-map first 2 MB (one 2MB huge page) ───────────────
; Covers the kernel loaded at 0x100000 by scripts/kernel.ld.
section .bss
align 4096
pml4:  resb 4096
pdpt:  resb 4096
pd:    resb 4096

; 32 KB stack
align 16
resb 32768
stack_top:

; ── Entry point (32-bit protected mode, set by multiboot1 loader) ────────────
section .text.entry
bits 32
global _start
_start:
    mov esp, stack_top

    ; CR4.PAE = 1 (physical address extension required for long mode)
    mov eax, cr4
    or  eax, (1 << 5)
    mov cr4, eax

    ; Build page tables
    ; PML4[0] → PDPT
    mov eax, pdpt
    or  eax, 3               ; present + writable
    mov dword [pml4],   eax
    mov dword [pml4+4], 0

    ; PDPT[0] → PD
    mov eax, pd
    or  eax, 3
    mov dword [pdpt],   eax
    mov dword [pdpt+4], 0

    ; PD[0] = 2 MB huge page at physical 0x0 (present + writable + PS)
    mov dword [pd],   0x83
    mov dword [pd+4], 0

    ; CR3 = PML4
    mov eax, pml4
    mov cr3, eax

    ; EFER.LME = 1 (enable long mode)
    mov ecx, 0xC0000080      ; EFER MSR
    rdmsr
    or  eax, (1 << 8)        ; LME
    wrmsr

    ; CR0.PG | CR0.PE = 1 (activate paging; enables long mode)
    mov eax, cr0
    or  eax, (1 << 31) | 1
    mov cr0, eax

    lgdt [gdt64_ptr]
    ; Far jump into the 64-bit code segment (selector 0x08)
    jmp  0x08:.lm64

bits 64
.lm64:
    ; Reload data segments
    mov ax, 0x10
    mov ds, ax
    mov es, ax
    mov fs, ax
    mov gs, ax
    mov ss, ax
    mov rsp, stack_top

    ; Jump to the Axon kernel entry point
    extern kmain
    call kmain

    ; kmain diverges (returns `never`); halt in case it somehow returns
.halt:
    hlt
    jmp .halt

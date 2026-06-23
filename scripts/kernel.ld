/* Axon default kernel linker script — x86_64 freestanding (R17 Slice 1)
 *
 * Loads the kernel at the conventional 1 MiB address.  The @[entry]-annotated
 * function is passed as --entry to the linker; this script puts its section
 * (.text.entry) first so the ELF entry point lands at the right address.
 *
 * Usage: axon build --freestanding --linker-script scripts/kernel.ld kernel.ax
 * QEMU:  qemu-system-x86_64 -kernel kernel -serial stdio -display none
 */
OUTPUT_FORMAT("elf64-x86-64")
OUTPUT_ARCH(i386:x86-64)

SECTIONS
{
    /* Conventional multiboot / direct-boot load address */
    . = 0x100000;

    .text : {
        KEEP(*(.text.entry))   /* @[entry] fn goes first */
        *(.text .text.*)
    }

    .rodata : ALIGN(4096) {
        *(.rodata .rodata.*)
    }

    .data : ALIGN(4096) {
        *(.data .data.*)
    }

    .bss : ALIGN(4096) {
        __bss_start = .;
        *(.bss .bss.*)
        *(COMMON)
        __bss_end = .;
    }

    /* Discard anything the bare-metal kernel doesn't need */
    /DISCARD/ : {
        *(.eh_frame)
        *(.comment)
    }
}

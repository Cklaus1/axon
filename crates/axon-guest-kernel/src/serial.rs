//! Minimal COM1 serial driver (8250/16550 UART at 0x3F8).
//! Used for boot logging before any other I/O is available.
//! All writes are synchronous (polling the THRE bit).

use core::fmt;

const COM1: u16 = 0x3F8;

/// Read a byte from an I/O port.
#[inline]
unsafe fn inb(port: u16) -> u8 {
    let val: u8;
    core::arch::asm!("in al, dx", out("al") val, in("dx") port, options(nomem, nostack));
    val
}

/// Write a byte to an I/O port.
#[inline]
unsafe fn outb(port: u16, val: u8) {
    core::arch::asm!("out dx, al", in("dx") port, in("al") val, options(nomem, nostack));
}

/// Initialize COM1 at 115200 baud, 8N1.
pub fn init() {
    unsafe {
        outb(COM1 + 1, 0x00); // disable interrupts
        outb(COM1 + 3, 0x80); // DLAB: set baud rate divisor
        outb(COM1 + 0, 0x01); // divisor lo: 1 → 115200 baud
        outb(COM1 + 1, 0x00); // divisor hi
        outb(COM1 + 3, 0x03); // 8 bits, no parity, 1 stop bit; DLAB off
        outb(COM1 + 2, 0xC7); // FIFO: enable, clear, 14-byte threshold
        outb(COM1 + 4, 0x0B); // RTS/DSR, IRQ enable, OUT2
    }
}

/// Write one byte, blocking until the transmit holding register is empty.
pub fn write_byte(b: u8) {
    unsafe {
        // Poll THRE (bit 5 of LSR).
        while inb(COM1 + 5) & 0x20 == 0 {}
        outb(COM1, b);
    }
}

/// Write a byte slice.
pub fn write_bytes(s: &[u8]) {
    for &b in s {
        if b == b'\n' {
            write_byte(b'\r');
        }
        write_byte(b);
    }
}

/// Write a string slice.
pub fn write_str(s: &str) {
    write_bytes(s.as_bytes());
}

// ── fmt::Write impl so we can use write!/writeln! ────────────────────────────

pub struct SerialWriter;

impl fmt::Write for SerialWriter {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        write_str(s);
        Ok(())
    }
}

/// Print without newline.
#[macro_export]
macro_rules! kprint {
    ($($arg:tt)*) => {{
        use core::fmt::Write as _;
        let _ = core::write!($crate::serial::SerialWriter, $($arg)*);
    }};
}

/// Print with newline.
#[macro_export]
macro_rules! kprintln {
    () => { $crate::kprint!("\n") };
    ($($arg:tt)*) => { $crate::kprint!("{}\n", format_args!($($arg)*)) };
}

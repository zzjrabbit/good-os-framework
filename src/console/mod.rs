use core::fmt::{self, Write};
use spin::{Lazy, Mutex};
use x86_64::instructions::interrupts;

use crate::drivers::display::Display;
use os_terminal::Terminal;

mod log;

pub static CONSOLE: Lazy<Mutex<Terminal<Display>>> =
    Lazy::new(|| Mutex::new(Terminal::new(Display::new())));

pub fn init() {
    log::init();
}

#[inline]
pub fn _print(args: fmt::Arguments) {
    interrupts::without_interrupts(|| {
        CONSOLE.lock().write_fmt(args).unwrap();
    });
}

#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => (
        $crate::console::_print(
            format_args!($($arg)*)
        )
    )
}

#[macro_export]
macro_rules! println {
    () => ($crate::print!("\n"));
    ($($arg:tt)*) => ($crate::print!("{}\n", format_args!($($arg)*)))
}

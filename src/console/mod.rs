use core::fmt::{self, Write};
use alloc::boxed::Box;
use spin::{Lazy, Mutex};
use tty::TTYDrawTarget;
use x86_64::instructions::interrupts;

use crate::drivers::display::Display;
use os_terminal::{font::{BitmapFont, TrueTypeFont}, Terminal};

mod log;
pub mod tty;

pub static CONSOLE: Lazy<Mutex<Terminal<TTYDrawTarget>>> =
    Lazy::new(|| Mutex::new(Terminal::new(TTYDrawTarget::new(0))));

pub fn init() {
    tty::init();
    log::init();
    CONSOLE.lock().set_font_manager(Box::new(BitmapFont{}));
}

/// Sets the font of the terminal on TTY0.
pub fn set_font(size: f32,font: &'static [u8]) {
    CONSOLE.lock().set_font_manager(Box::new(TrueTypeFont::new(size, font)));
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

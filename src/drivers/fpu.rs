/*
 * @file    :   fpu.rs
 * @time    :   2023/08/28 13:52:57
 * @author  :   zzjcarrot
 */

// 还是来自rCore的FPU寄存器切换代码

// state saved by fxsave64
// 512 bytes
// https://www.felixcloutier.com/x86/fxsave#tbl-3-47
#[repr(C, align(16))]
#[derive(Debug, Copy, Clone, Default)]
pub struct FpState {
    // 0
    fcw: u16,
    fsw: u16,
    ftw: u16,
    fop: u16,
    word2: u64,
    // 16
    word3: u64,
    mxcsr: u32,
    mxcsr_mask: u32,
    // 32
    mm: [u64; 16],
    // 160
    xmm: [u64; 32],
    // 416
    rest: [u64; 12],
}

impl FpState {
    /// Creates a new FPU context.
    pub fn new() -> Self {
        assert!(core::mem::size_of::<Self>() == 512);
        Self {
            // default values
            // intel manual 11.6.4 Initialization of SSE/SSE2 Extensions
            mxcsr: 0x1f80,
            // intel manual 8.1.5 x87 FPU Control Word
            fcw: 0x037f,
            ..Self::default()
        }
    }

    /// Saves the FPU context to the provided buffer.
    pub fn save(&mut self) {
        unsafe {
            core::arch::x86_64::_fxsave64(self as *mut FpState as *mut u8);
        }
    }

    /// Restores the FPU context from the provided buffer.
    pub fn restore(&self) {
        unsafe {
            core::arch::x86_64::_fxrstor64(self as *const FpState as *const u8);
        }
    }
}

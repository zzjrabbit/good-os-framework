use alloc::{vec::Vec, vec};

use crate::data::bitmap::Bitmap;

use super::process::ProcessId;

/// the signal structure.
#[derive(Debug, Clone, Copy)]
pub struct Signal {
    pub ty: usize,
    pub data: [u64;8],
}

/// The signal manager.
/// You don't need to create one by yourself.
pub struct SignalManager {
    signal_bitmap: Bitmap, 
    signals: Vec<Signal>,
    waiting_for: usize,
    wake_up_process: fn(ProcessId),
    pid: ProcessId,
}

impl SignalManager {
    pub fn new(signal_type_num: usize, wake_up_process_fn: fn(ProcessId), pid: ProcessId) -> Self {
        Self {
            signal_bitmap: Bitmap::new(vec![0;signal_type_num].leak()),
            signals: Vec::new(),
            waiting_for: 0,
            wake_up_process: wake_up_process_fn,
            pid,
        }
    }

    /// Returns whether the signal type is registered.
    pub fn has_signal(&self, signal_type: usize) -> bool {
        self.signal_bitmap.get(signal_type)
    }

    /// Registers a new signal and wakes up the process if it is waiting for the signal.
    pub fn register_signal(&mut self, signal_type: usize, signal: Signal) {
        assert_ne!(signal_type, 0);
        self.signal_bitmap.set(signal_type, true);
        self.signals.push(signal);

        if signal_type == self.waiting_for {
            self.wake_up_process.call((self.pid,));
            self.waiting_for = 0;
        }
    }

    /// Starts to wait for a signal.
    pub fn register_wait_for(&mut self, signal_type: usize) {
        self.waiting_for = signal_type;
    }

    /// Gets a signal of the specified type. Returns None if the signal type is not registered.
    pub fn get_signal(&mut self, signal_type: usize) -> Option<Signal> {
        if self.signal_bitmap.get(signal_type) {
            for idx in 0..self.signals.len() {
                if self.signals[idx].ty == signal_type {
                    let signal = self.signals[idx];
                    return Some(signal);
                }
            }
            return None;
        } else {
            None
        }
    }

    /// Deletes a signal of the specified type. Nothing happens if the signal type is not registered.
    pub fn delete_signal(&mut self, signal_type: usize) {
        if self.signal_bitmap.get(signal_type) {
            self.signal_bitmap.set(signal_type, false);
            for idx in 0..self.signals.len() {
                if self.signals[idx].ty == signal_type {
                    self.signals.remove(idx);
                }
            }
        }
    }
}


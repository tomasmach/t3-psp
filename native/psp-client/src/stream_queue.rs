extern crate alloc;

use alloc::{sync::Arc, vec::Vec};
use core::{
    cell::UnsafeCell,
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
};

pub const BLOCK_BYTES: usize = 8192;
pub const CAPACITY: usize = 32;

struct Ring {
    slots: Vec<UnsafeCell<[u8; BLOCK_BYTES]>>,
    written: AtomicUsize,
    read: AtomicUsize,
    closed: AtomicBool,
    cancelled: AtomicBool,
}
// There is exactly one non-cloneable producer and consumer. Release/acquire
// transfers each slot between them; no reference to a slot leaves this module.
unsafe impl Sync for Ring {}

pub struct Producer {
    ring: Arc<Ring>,
}
pub struct Consumer {
    ring: Arc<Ring>,
}

#[derive(Debug, PartialEq)]
pub enum PushError {
    Full,
    Closed,
}

pub fn channel() -> (Producer, Consumer) {
    // Build on the heap one block at a time, avoiding a 256 KiB stack temporary.
    let mut slots = Vec::with_capacity(CAPACITY);
    for _ in 0..CAPACITY {
        slots.push(UnsafeCell::new([0; BLOCK_BYTES]));
    }
    let ring = Arc::new(Ring {
        slots,
        written: AtomicUsize::new(0),
        read: AtomicUsize::new(0),
        closed: AtomicBool::new(false),
        cancelled: AtomicBool::new(false),
    });
    (Producer { ring: ring.clone() }, Consumer { ring })
}

impl Producer {
    pub fn push(&mut self, block: &[u8; BLOCK_BYTES]) -> Result<(), PushError> {
        if self.ring.closed.load(Ordering::Acquire) || self.ring.cancelled.load(Ordering::Acquire) {
            return Err(PushError::Closed);
        }
        let written = self.ring.written.load(Ordering::Relaxed);
        let read = self.ring.read.load(Ordering::Acquire);
        if written.wrapping_sub(read) >= CAPACITY {
            return Err(PushError::Full);
        }
        unsafe {
            (*self.ring.slots[written % CAPACITY].get()).copy_from_slice(block);
        }
        self.ring
            .written
            .store(written.wrapping_add(1), Ordering::Release);
        Ok(())
    }
    pub fn close(&mut self) {
        self.ring.closed.store(true, Ordering::Release);
    }
    pub fn cancel(&mut self) {
        self.ring.cancelled.store(true, Ordering::Release);
        self.close();
    }
}
impl Drop for Producer {
    fn drop(&mut self) {
        self.close();
    }
}

impl Consumer {
    pub fn pop(&mut self, output: &mut [u8; BLOCK_BYTES]) -> bool {
        if self.ring.cancelled.load(Ordering::Acquire) {
            return false;
        }
        let read = self.ring.read.load(Ordering::Relaxed);
        if read == self.ring.written.load(Ordering::Acquire) {
            return false;
        }
        unsafe {
            output.copy_from_slice(&*self.ring.slots[read % CAPACITY].get());
        }
        self.ring
            .read
            .store(read.wrapping_add(1), Ordering::Release);
        true
    }
    pub fn cancelled(&self) -> bool {
        self.ring.cancelled.load(Ordering::Acquire)
    }
    pub fn finished(&self) -> bool {
        self.cancelled()
            || (self.ring.closed.load(Ordering::Acquire)
                && self.ring.read.load(Ordering::Relaxed)
                    == self.ring.written.load(Ordering::Acquire))
    }
}
impl Drop for Consumer {
    fn drop(&mut self) {
        self.ring.cancelled.store(true, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ordered_wrap_overflow_and_drain() {
        let (mut tx, mut rx) = channel();
        let mut output = [0; BLOCK_BYTES];
        for round in 0..4u8 {
            for n in 0..CAPACITY {
                tx.push(&[round * 32 + n as u8; BLOCK_BYTES]).unwrap();
            }
            assert_eq!(tx.push(&[255; BLOCK_BYTES]), Err(PushError::Full));
            for n in 0..CAPACITY {
                assert!(rx.pop(&mut output));
                assert!(output.iter().all(|b| *b == round * 32 + n as u8));
            }
            assert!(!rx.pop(&mut output));
        }
        tx.push(&[42; BLOCK_BYTES]).unwrap();
        tx.close();
        assert!(!rx.finished());
        assert!(rx.pop(&mut output));
        assert!(rx.finished());
        assert_eq!(tx.push(&output), Err(PushError::Closed));
    }
    #[test]
    fn cancellation_discards_queued_audio() {
        let (mut tx, mut rx) = channel();
        tx.push(&[1; BLOCK_BYTES]).unwrap();
        tx.cancel();
        assert!(rx.finished());
        assert!(!rx.pop(&mut [0; BLOCK_BYTES]));
    }
    #[test]
    fn sequence_counters_can_wrap_without_reusing_unread_slots() {
        let (mut tx, mut rx) = channel();
        tx.ring.written.store(usize::MAX - 15, Ordering::Relaxed);
        tx.ring.read.store(usize::MAX - 15, Ordering::Relaxed);
        for n in 0..CAPACITY {
            tx.push(&[n as u8; BLOCK_BYTES]).unwrap();
        }
        assert_eq!(tx.push(&[255; BLOCK_BYTES]), Err(PushError::Full));
        let mut output = [0; BLOCK_BYTES];
        for n in 0..CAPACITY {
            assert!(rx.pop(&mut output));
            assert!(output.iter().all(|byte| *byte == n as u8));
        }
        tx.close();
        assert!(rx.finished());
    }
    #[test]
    fn concurrent_producer_consumer_keep_order() {
        let (mut tx, mut rx) = channel();
        let worker = std::thread::spawn(move || {
            let mut block = [0; BLOCK_BYTES];
            let mut received = 0u32;
            while !rx.finished() {
                if rx.pop(&mut block) {
                    assert_eq!(u32::from_le_bytes(block[..4].try_into().unwrap()), received);
                    received += 1;
                } else {
                    std::thread::yield_now();
                }
            }
            received
        });
        for n in 0..1000u32 {
            let mut block = [0; BLOCK_BYTES];
            block[..4].copy_from_slice(&n.to_le_bytes());
            while tx.push(&block) == Err(PushError::Full) {
                std::thread::yield_now();
            }
        }
        tx.close();
        assert_eq!(worker.join().unwrap(), 1000);
    }
}

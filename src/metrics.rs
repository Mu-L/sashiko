use std::sync::atomic::{AtomicUsize, Ordering};

pub static PENDING_PATCHES: AtomicUsize = AtomicUsize::new(0);
pub static REVIEWING_PATCHES: AtomicUsize = AtomicUsize::new(0);

pub fn set_pending_patches(count: usize) {
    PENDING_PATCHES.store(count, Ordering::Relaxed);
}

pub fn set_reviewing_patches(count: usize) {
    REVIEWING_PATCHES.store(count, Ordering::Relaxed);
}

pub fn get_pending_patches() -> usize {
    PENDING_PATCHES.load(Ordering::Relaxed)
}

pub fn get_reviewing_patches() -> usize {
    REVIEWING_PATCHES.load(Ordering::Relaxed)
}

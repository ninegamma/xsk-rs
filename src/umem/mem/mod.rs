mod mmap;
use mmap::Mmap;

use std::{
    io,
    num::NonZeroU32,
    ptr::NonNull,
    sync::{Arc, Mutex},
};

use super::{
    FrameLayout,
};

/// A framed, memory mapped region which functions as the working
/// memory for some UMEM.
#[derive(Clone, Debug)]
pub struct UmemRegion {
    // Keep a copy of the pointer to the mmap region to avoid a double
    // deref, through for example an `Arc<Mmap>`. We know this won't
    // dangle since this struct holds an `Arc`d copy of the mmap
    // region.
    addr: NonNull<libc::c_void>,
    len: usize,
    _mmap: Arc<Mutex<Mmap>>,
}

unsafe impl Send for UmemRegion {}

// SAFETY: this impl is only safe in the context of this library and
// assuming the various unsafe requirements are upheld. Mutations to
// the memory region may occur concurrently but always in disjoint
// sections by either the user space process xor the kernel.
unsafe impl Sync for UmemRegion {}

impl UmemRegion {
    pub(super) fn new(
        frame_count: NonZeroU32,
        frame_layout: FrameLayout,
        use_huge_pages: bool,
    ) -> io::Result<Self> {
        let len = (frame_count.get() as usize) * frame_layout.frame_size() as usize;

        let mmap = Mmap::new(len, use_huge_pages)?;

        Ok(Self {
            addr: mmap.addr(),
            len,
            _mmap: Arc::new(Mutex::new(mmap)),
        })
    }

    /// The size of the underlying memory region.
    #[inline]
    pub fn len(&self) -> usize {
        self.len
    }

    /// Get a pointer to the start of the memory region.
    #[inline]
    pub fn as_ptr(&self) -> *mut libc::c_void {
        self.addr.as_ptr()
    }


}

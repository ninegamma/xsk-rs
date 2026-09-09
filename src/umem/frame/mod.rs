



/// This option will be set on all but the last `FrameDesc` if multi-buffer
/// is enabled. Check this to determine if more fragments exist for this packet.
const XDP_PKT_CONTD: u32 = 1 << 0;

/// A [`Umem`](super::Umem) frame descriptor.
///
/// Used to pass frame information between the kernel and
/// userspace. `addr` is an offset in bytes from the start of the
/// [`Umem`](super::Umem) and corresponds to the starting address of
/// the packet data segment of some frame. `length` describes the
/// length (in bytes) of any data stored in the frame's headroom or
/// data segments.
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct FrameDesc {
    pub(crate) addr: u64,
    // Field order must strictly match the kernel xdp_desc struct
    // Length must precede options to align with C memory layout
    pub(crate) length: u32,
    pub(crate) options: u32,
}

impl FrameDesc {
    /// Creates a new frame descriptor.
    ///
    /// `addr` must be the starting address of the packet data segment
    /// of some [`Umem`](super::Umem) frame.
    pub fn new(addr: u64) -> Self {
        Self {
            addr,
            options: 0,
            length: 0,
        }
    }

    /// Replaces the default options with the provided value.
    pub fn with_options(mut self, options: u32) -> Self {
        self.options = options;
        self
    }

    /// Replaces the default segment lengths.
    pub fn with_length(mut self, data: u32) -> Self {
        self.length = data ;
        self
    }

    /// The starting address of the packet data segment of the frame
    /// pointed at by this descriptor.
    #[inline]
    pub fn addr(&self) -> u64 {
        self.addr
    }

    /// Current packet data lengths for the frame pointed
    /// at by this descriptor.
    #[inline]
    pub fn length(&self) -> u32 {
        self.length
    }

    /// Frame options.
    #[inline]
    pub fn options(&self) -> u32 {
        self.options
    }

    /// Set the frame options.
    #[inline]
    pub fn set_options(&mut self, options: u32) {
        self.options = options
    }

    /// True if multi-buffer is enabled and we have more frames for
    /// this packet.
    #[inline]
    pub fn has_more_frames(&self) -> bool {
        (self.options & XDP_PKT_CONTD) == 1
    }

    #[inline]
    pub(crate) fn write_xdp_desc(&self, desc: &mut libxdp_sys::xdp_desc) {
        // SAFETY: FrameDesc is explicitly repr(C) with a field layout, size,
        // and alignment that match libxdp_sys::xdp_desc exactly. Both references
        // point to valid, properly aligned memory of size equal to one xdp_desc,
        // and the mutable reference to desc guarantees exclusive write access
        // without data races.
        unsafe {
            std::ptr::copy_nonoverlapping(
                self as *const FrameDesc as *const libxdp_sys::xdp_desc,
                desc as *mut libxdp_sys::xdp_desc,
                1,
            );
        }
    }

    #[inline]
    pub(crate) fn read_xdp_desc(desc: &libxdp_sys::xdp_desc) -> Self {
        let mut frame_desc = Self::default();
        // SAFETY: FrameDesc is explicitly repr(C) with a field layout, size,
        // and alignment that match libxdp_sys::xdp_desc exactly. Both pointers
        // point to valid, properly aligned memory of size equal to one xdp_desc,
        // and the source reference guarantees valid read access without data races.
        unsafe {
            std::ptr::copy_nonoverlapping(
                desc as *const libxdp_sys::xdp_desc as *const FrameDesc,
                &mut frame_desc as *mut FrameDesc,
                1,
            );
        }
        frame_desc
    }

    /// Copies a contiguous range of kernel descriptors into frame descriptors.
    ///
    /// # Safety
    ///
    /// `src` and `dst` must each point to `count` valid, non-overlapping
    /// descriptors. The descriptor layouts must remain identical.
    #[inline]
    pub(crate) unsafe fn read_xdp_desc_slice(
        src: *const libxdp_sys::xdp_desc,
        dst: *mut Self,
        count: usize,
    ) {
        // SAFETY: The caller guarantees that both ranges are valid and
        // non-overlapping. FrameDesc and xdp_desc have identical repr(C)
        // layouts, so copying the descriptors as bytes preserves all fields.
        unsafe {
            std::ptr::copy_nonoverlapping(src.cast::<Self>(), dst, count);
        }
    }
}

impl Default for FrameDesc {
    /// Creates an empty frame descriptor with an address of zero and
    /// segment lengths also set to zero.
    ///
    /// Since the address of any descriptors created this way is
    /// always zero, before using them to write to the [`Umem`] they
    /// should first be 'initialised' by passing them to either the
    /// [`RxQueue`] or the [`CompQueue`], so they can be populated
    /// with the details of a free frame.
    ///
    /// [`Umem`]: crate::Umem
    /// [`RxQueue`]: crate::RxQueue
    /// [`CompQueue`]: crate::CompQueue
    fn default() -> Self {
        Self {
            addr: 0,
            options: 0,
            length: 0,
        }
    }
}

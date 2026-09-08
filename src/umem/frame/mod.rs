



/// This option will be set on all but the last `FrameDesc` if multi-buffer
/// is enabled. Check this to determine if more fragments exist for this packet.
const XDP_PKT_CONTD: u32 = 1 << 0;

/// A [`Umem`](super::Umem) frame descriptor.
///
/// Used to pass frame information between the kernel and
/// userspace. `addr` is an offset in bytes from the start of the
/// [`Umem`](super::Umem) and corresponds to the starting address of
/// the packet data segment of some frame. `lengths` describes the
/// length (in bytes) of any data stored in the frame's headroom or
/// data segments.
#[derive(Debug, Clone, Copy)]
pub struct FrameDesc {
    pub(crate) addr: u64,
    pub(crate) options: u32,
    pub(crate) length: u32,
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
        desc.addr = self.addr;
        desc.options = self.options;
        desc.len = self.length;
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

use crate::{
    ring::{self, XskRingCons},
    socket::Socket,
};

/// Used to transfer ownership of [`Umem`](super::Umem) frames from
/// kernel-space to user-space.
///
/// Frames received in this queue are those that have been sent via
/// the [`TxQueue`](crate::socket::TxQueue).
///
/// Holding on to this queue keeps the socket it was created with
/// alive, since libxdp unmaps the comp ring only when the last socket
/// created from the same [`Umem`](super::Umem) and bound to the same
/// device and queue id is deleted. Dropping every other handle to
/// that socket therefore will not release the device; this queue and
/// the one returned alongside it have to go too.
///
/// For more information see the
/// [docs](https://www.kernel.org/doc/html/latest/networking/af_xdp.html#umem-completion-ring).
#[derive(Debug)]
pub struct CompQueue {
    ring: XskRingCons,
    _socket: Socket,
}

impl CompQueue {
    pub(crate) fn new(ring: XskRingCons, socket: Socket) -> Self {
        Self {
            ring,
            _socket: socket,
        }
    }

    /// Update `addrs` with addresses of frames whose contents have been
    /// sent (after submission via the [`TxQueue`]) and may now be
    /// used again. Returns the number of elements of `addrs` which
    /// have been updated. The addresses are offsets within this queue's
    /// [`Umem`].
    ///
    /// The number of entries updated will be less than or equal to
    /// the length of `addrs`. Entries will be updated sequentially
    /// from the start of `addrs` until the end.
    ///
    /// Free frames should eventually be added back on to either the
    /// [`FillQueue`] or the [`TxQueue`].
    ///
    /// # Safety
    ///
    /// `addrs` must point to writable memory, and the addresses returned by
    /// this queue must only be reused with the associated [`Umem`].
    ///
    /// [`TxQueue`]: crate::socket::TxQueue
    /// [`FillQueue`]: crate::FillQueue
    /// [`Umem`]: super::Umem
    #[inline]
    pub unsafe fn consume(&mut self, addrs: &mut [u64]) -> usize {
        let nb = addrs.len() as u32;

        if nb == 0 {
            return 0;
        }

        let mut idx = 0;

        let cnt = unsafe { libxdp_sys::xsk_ring_cons__peek(self.ring.as_ptr(), nb, &mut idx) };

        if cnt > 0 {
            // SAFETY: peek returned `cnt` addresses beginning at `idx`, and
            // the ring remains exclusively borrowed through `self` until the
            // addresses are copied and released below.
            unsafe { copy_comp_addrs(self.ring.as_ptr(), idx, &mut addrs[..cnt as usize]) };

            unsafe { libxdp_sys::xsk_ring_cons__release(self.ring.as_ptr(), cnt) };
        }

        cnt as usize
    }

    /// Same as [`consume`] but for a single frame address.
    ///
    /// # Safety
    ///
    /// `addr` must point to writable memory, and the returned address must
    /// only be reused with the associated [`Umem`].
    ///
    /// [`consume`]: Self::consume
    #[inline]
    pub unsafe fn consume_one(&mut self, addr: &mut u64) -> usize {
        let mut idx = 0;

        let cnt = unsafe { libxdp_sys::xsk_ring_cons__peek(self.ring.as_ptr(), 1, &mut idx) };

        if cnt > 0 {
            *addr = unsafe { *libxdp_sys::xsk_ring_cons__comp_addr(self.ring.as_ptr(), idx) };

            unsafe { libxdp_sys::xsk_ring_cons__release(self.ring.as_ptr(), cnt) };
        }

        cnt as usize
    }

    /// The number of entries ready to be consumed, up to `nb`.
    ///
    /// Answers from libxdp's cached producer position unless that
    /// cache is empty, in which case the current position is read.
    /// A cached position only ever trails the real one, so the count
    /// is never an overestimate.
    ///
    /// Note that passing the ring size does not give an exact count,
    /// since a non-empty cache is never refreshed. See
    /// [`nb_avail_exact`] for that.
    ///
    /// [`nb_avail_exact`]: Self::nb_avail_exact
    #[inline]
    pub fn nb_avail(&mut self, nb: u32) -> u32 {
        // SAFETY: the ring is initialised and `&mut self` excludes
        // any other access to it.
        unsafe { libxdp_sys::xsk_cons_nb_avail(self.ring.as_ptr(), nb) }
    }

    /// The number of entries ready to be consumed.
    ///
    /// Reads the current producer position rather than a cached one,
    /// so this can be used to watch a backlog without consuming it.
    /// See [`nb_avail`] for the cached count.
    ///
    /// [`nb_avail`]: Self::nb_avail
    #[inline]
    pub fn nb_avail_exact(&mut self) -> u32 {
        // SAFETY: the ring is initialised and `&mut self` excludes
        // any other access to it.
        unsafe { ring::cons_nb_avail_exact(self.ring.as_ptr()) }
    }
}

/// Copies a batch of completion addresses while handling ring wrap-around
/// once. Completion entries contain only an address, so the descriptor fields
/// are initialized while copying into the strided `FrameDesc` destinations.
#[inline]
unsafe fn copy_comp_addrs(
    ring: *mut libxdp_sys::xsk_ring_cons,
    idx: u32,
    dst: &mut [u64],
) {
    let mask = unsafe { (*ring).mask };
    let size = unsafe { (*ring).size };
    let start = (idx & mask) as usize;
    let first_count = (size as usize - start).min(dst.len());
    let second_count = dst.len() - first_count;
    let ring_addrs = unsafe { (*ring).ring.cast::<u64>() };

    // SAFETY: `peek` guarantees that the ring contains the requested
    // addresses. The two source ranges are contiguous and together cover
    // exactly `dst`.
    unsafe {
        copy_comp_addr_range(ring_addrs.add(start), &mut dst[..first_count]);

        if second_count > 0 {
            copy_comp_addr_range(ring_addrs, &mut dst[first_count..]);
        }
    }
}

#[inline]
unsafe fn copy_comp_addr_range(src: *const u64, dst: &mut [u64]) {
    unsafe { std::ptr::copy_nonoverlapping(src, dst.as_mut_ptr(), dst.len()) };
}

#[cfg(test)]
mod tests {
    use super::copy_comp_addrs;

    #[test]
    fn copy_comp_addrs_copies_across_ring_wrap() {
        let mut ring_addrs: [u64; 4] = [10, 20, 30, 40];
        let ring = libxdp_sys::xsk_ring_cons {
            cached_prod: 0,
            cached_cons: 0,
            mask: 3,
            size: 4,
            producer: std::ptr::null_mut(),
            consumer: std::ptr::null_mut(),
            ring: ring_addrs.as_mut_ptr().cast(),
            flags: std::ptr::null_mut(),
        };
        let mut copied = [0; 3];

        unsafe { copy_comp_addrs(&ring as *const _ as *mut _, 3, &mut copied) };

        assert_eq!(copied, [40, 10, 20]);
    }
}

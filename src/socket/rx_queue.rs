use std::io;

use crate::{
    ring::{self, XskRingCons},
    umem::frame::FrameDesc,
};

use super::{Socket, fd::Fd};

/// The receiving side of an AF_XDP [`Socket`].
///
/// More details can be found in the
/// [docs](https://www.kernel.org/doc/html/latest/networking/af_xdp.html#rx-ring).
#[derive(Debug)]
pub struct RxQueue {
    ring: XskRingCons,
    socket: Socket,
}

impl RxQueue {
    pub(super) fn new(ring: XskRingCons, socket: Socket) -> Self {
        Self { ring, socket }
    }

    /// Update `descs` with information on which [`Umem`] frames have
    /// received packets. Returns the number of elements of `descs`
    /// which have been updated.
    ///
    /// The number of entries updated will be less than or equal to
    /// the length of `descs`. Entries will be updated sequentially
    /// from the start of `descs` until the end.
    ///
    /// Once the contents of the consumed frames have been dealt with
    /// and are no longer required, the frames should eventually be
    /// added back on to either the [`FillQueue`] or the [`TxQueue`].
    ///
    /// # Safety
    ///
    /// The frames passed to this queue must belong to the same
    /// [`Umem`] that this `RxQueue` instance is tied to.
    ///
    /// [`Umem`]: crate::Umem
    /// [`FillQueue`]: crate::FillQueue
    /// [`TxQueue`]: crate::TxQueue
    #[inline]
    pub unsafe fn consume(&mut self, descs: &mut [FrameDesc]) -> usize {
        let nb = descs.len() as u32;

        if nb == 0 {
            return 0;
        }

        let mut idx = 0;

        let cnt = unsafe { libxdp_sys::xsk_ring_cons__peek(self.ring.as_ptr(), nb, &mut idx) };

        if cnt > 0 {
            // SAFETY: peek returned `cnt` descriptors beginning at `idx`, and
            // the ring remains exclusively borrowed through `self` until the
            // descriptors are copied and released below.
            unsafe { copy_rx_descs(self.ring.as_ptr(), idx, &mut descs[..cnt as usize]) };

            unsafe { libxdp_sys::xsk_ring_cons__release(self.ring.as_ptr(), cnt) };
        }

        cnt as usize
    }

    /// Same as [`consume`] but for a single frame descriptor.
    ///
    /// # Safety
    ///
    /// See [`consume`].
    ///
    /// [`consume`]: Self::consume
    #[inline]
    pub unsafe fn consume_one(&mut self, desc: &mut FrameDesc) -> usize {
        let mut idx = 0;

        let cnt = unsafe { libxdp_sys::xsk_ring_cons__peek(self.ring.as_ptr(), 1, &mut idx) };

        if cnt > 0 {
            let recv_pkt_desc =
                unsafe { libxdp_sys::xsk_ring_cons__rx_desc(self.ring.as_ptr(), idx) };

            *desc = FrameDesc::read_xdp_desc(unsafe { &*recv_pkt_desc });

            unsafe { libxdp_sys::xsk_ring_cons__release(self.ring.as_ptr(), cnt) };
        }

        cnt as usize
    }

    /// Same as [`consume`] but poll first to check if there is
    /// anything to read beforehand.
    ///
    /// # Safety
    ///
    /// See [`consume`].
    ///
    /// [`consume`]: RxQueue::consume
    #[inline]
    pub unsafe fn poll_and_consume(
        &mut self,
        descs: &mut [FrameDesc],
        poll_timeout: i32,
    ) -> io::Result<usize> {
        match self.poll(poll_timeout)? {
            true => Ok(unsafe { self.consume(descs) }),
            false => Ok(0),
        }
    }

    /// Same as [`poll_and_consume`] but for a single frame descriptor.
    ///
    /// # Safety
    ///
    /// See [`consume`].
    ///
    /// [`poll_and_consume`]: Self::poll_and_consume
    /// [`consume`]: Self::consume
    #[inline]
    pub unsafe fn poll_and_consume_one(
        &mut self,
        desc: &mut FrameDesc,
        poll_timeout: i32,
    ) -> io::Result<usize> {
        match self.poll(poll_timeout)? {
            true => Ok(unsafe { self.consume_one(desc) }),
            false => Ok(0),
        }
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

    /// Polls the socket, returning `true` if there is data to read.
    #[inline]
    pub fn poll(&mut self, poll_timeout: i32) -> io::Result<bool> {
        self.socket.fd.poll_read(poll_timeout)
    }

    /// A reference to the underlying [`Socket`]'s file descriptor.
    #[inline]
    pub fn fd(&self) -> &Fd {
        &self.socket.fd
    }

    /// A mutable reference to the underlying [`Socket`]'s file descriptor.
    #[inline]
    pub fn fd_mut(&mut self) -> &mut Fd {
        &mut self.socket.fd
    }
}

/// Copies a batch of RX descriptors while handling the ring wrap-around once.
///
/// The ring is power-of-two sized, so `mask` maps the absolute consumer index
/// to the first entry. At most two contiguous ranges are needed for a batch.
#[inline]
unsafe fn copy_rx_descs(ring: *mut libxdp_sys::xsk_ring_cons, idx: u32, dst: &mut [FrameDesc]) {
    let mask = unsafe { (*ring).mask };
    let size = unsafe { (*ring).size };
    let start = idx & mask;
    let first_count = (size - start).min(dst.len() as u32) as usize;
    let second_count = dst.len() - first_count;
    let ring_descs = unsafe { (*ring).ring.cast::<libxdp_sys::xdp_desc>() };

    // SAFETY: `peek` guarantees that the ring contains the requested
    // descriptors. The first and second ranges are contiguous and together
    // cover exactly `dst`; neither source range overlaps the destination.
    unsafe {
        FrameDesc::read_xdp_desc_slice(
            ring_descs.add(start as usize),
            dst.as_mut_ptr(),
            first_count,
        );

        if second_count > 0 {
            FrameDesc::read_xdp_desc_slice(
                ring_descs,
                dst.as_mut_ptr().add(first_count),
                second_count,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::copy_rx_descs;

    #[test]
    fn copy_rx_descs_copies_across_ring_wrap() {
        let mut ring_descs = [
            libxdp_sys::xdp_desc {
                addr: 10,
                len: 11,
                options: 12,
            },
            libxdp_sys::xdp_desc {
                addr: 20,
                len: 21,
                options: 22,
            },
            libxdp_sys::xdp_desc {
                addr: 30,
                len: 31,
                options: 32,
            },
            libxdp_sys::xdp_desc {
                addr: 40,
                len: 41,
                options: 42,
            },
        ];
        let ring = libxdp_sys::xsk_ring_cons {
            cached_prod: 0,
            cached_cons: 0,
            mask: 3,
            size: 4,
            producer: std::ptr::null_mut(),
            consumer: std::ptr::null_mut(),
            ring: ring_descs.as_mut_ptr().cast(),
            flags: std::ptr::null_mut(),
        };
        let mut copied = [crate::umem::frame::FrameDesc::default(); 3];

        unsafe { copy_rx_descs(&ring as *const _ as *mut _, 3, &mut copied) };

        assert_eq!(copied[0].addr(), 40);
        assert_eq!(copied[0].length(), 41);
        assert_eq!(copied[0].options(), 42);
        assert_eq!(copied[1].addr(), 10);
        assert_eq!(copied[1].length(), 11);
        assert_eq!(copied[1].options(), 12);
        assert_eq!(copied[2].addr(), 20);
        assert_eq!(copied[2].length(), 21);
        assert_eq!(copied[2].options(), 22);
    }
}

use std::io;

use crate::{
    ring::{self, XskRingProd},
    socket::{Fd, Socket},
};

/// Used to transfer ownership of [`Umem`](super::Umem) frames from
/// user-space to kernel-space.
///
/// These frames will be used to receive packets, and will eventually
/// be returned via the [`RxQueue`](crate::socket::RxQueue).
///
/// Holding on to this queue keeps the socket it was created with
/// alive, since libxdp unmaps the fill ring only when the last socket
/// created from the same [`Umem`](super::Umem) and bound to the same
/// device and queue id is deleted. Dropping every other handle to
/// that socket therefore will not release the device; this queue and
/// the one returned alongside it have to go too.
///
/// For more information see the
/// [docs](https://www.kernel.org/doc/html/latest/networking/af_xdp.html#umem-fill-ring).
#[derive(Debug)]
pub struct FillQueue {
    ring: XskRingProd,
    _socket: Socket,
}

impl FillQueue {
    pub(crate) fn new(ring: XskRingProd, socket: Socket) -> Self {
        Self {
            ring,
            _socket: socket,
        }
    }

    /// Let the kernel know that the [`Umem`] frame addresses in
    /// `addrs` may be used to receive data. Returns the number of
    /// frames submitted to the kernel.
    ///
    /// Note that if the length of `addrs` is greater than the number
    /// of available spaces on the underlying ring buffer then no
    /// frames at all will be handed over to the kernel.
    ///
    /// Once the frames have been submitted to this queue they should
    /// not be used again until consumed via the [`RxQueue`].
    ///
    /// # Safety
    ///
    /// This function is unsafe as it is possible to cause a data race
    /// if used improperly. For example, by simultaneously submitting
    /// the same frame address to this `FillQueue` and the
    /// [`TxQueue`].
    ///
    /// Furthermore, the addresses passed to this queue must belong to
    /// the same [`Umem`] that this `FillQueue` instance is tied to.
    ///
    /// [`TxQueue`]: crate::TxQueue
    /// [`RxQueue`]: crate::RxQueue
    /// [`Umem`]: super::Umem
    #[inline]
    pub unsafe fn produce(&mut self, addrs: &[u64]) -> usize {
        let nb = addrs.len() as u32;

        if nb == 0 {
            return 0;
        }

        let mut idx = 0;

        let cnt = unsafe { libxdp_sys::xsk_ring_prod__reserve(self.ring.as_ptr(), nb, &mut idx) };

        if cnt > 0 {
            // SAFETY: reserve returned `cnt` slots beginning at `idx`, and
            // the ring remains exclusively borrowed through `self` until
            // the addresses are copied and submitted below.
            unsafe { copy_fill_addrs(self.ring.as_ptr(), idx, &addrs[..cnt as usize]) };

            unsafe { libxdp_sys::xsk_ring_prod__submit(self.ring.as_ptr(), cnt) };
        }

        cnt as usize
    }

    /// Same as [`produce`] but for a single frame address.
    ///
    /// # Safety
    ///
    /// See [`produce`].
    ///
    /// [`produce`]: Self::produce
    #[inline]
    pub unsafe fn produce_one(&mut self, addr: &u64) -> usize {
        let mut idx = 0;

        let cnt = unsafe { libxdp_sys::xsk_ring_prod__reserve(self.ring.as_ptr(), 1, &mut idx) };

        if cnt > 0 {
            unsafe {
                *libxdp_sys::xsk_ring_prod__fill_addr(self.ring.as_ptr(), idx) = *addr
            };

            unsafe { libxdp_sys::xsk_ring_prod__submit(self.ring.as_ptr(), cnt) };
        }

        cnt as usize
    }

    /// Same as [`produce`] but wake up the kernel if required to let
    /// it know there are frames available that may be used to receive
    /// data.
    ///
    /// For more details see the
    /// [docs](https://www.kernel.org/doc/html/latest/networking/af_xdp.html#xdp-use-need-wakeup-bind-flag).
    ///
    /// # Safety
    ///
    /// See [`produce`].
    ///
    /// [`produce`]: Self::produce
    #[inline]
    pub unsafe fn produce_and_wakeup(
        &mut self,
        addrs: &[u64],
        socket_fd: &mut Fd,
        poll_timeout: i32,
    ) -> io::Result<usize> {
        let cnt = unsafe { self.produce(addrs) };

        if cnt > 0 && self.needs_wakeup() {
            self.wakeup(socket_fd, poll_timeout)?;
        }

        Ok(cnt)
    }

    /// Same as [`produce_and_wakeup`] but for a single frame
    /// descriptor.
    ///
    /// # Safety
    ///
    /// See [`produce`].
    ///
    /// [`produce_and_wakeup`]: Self::produce_and_wakeup
    /// [`produce`]: Self::produce
    #[inline]
    pub unsafe fn produce_one_and_wakeup(
        &mut self,
        addr: &u64,
        socket_fd: &mut Fd,
        poll_timeout: i32,
    ) -> io::Result<usize> {
        let cnt = unsafe { self.produce_one(addr) };

        if cnt > 0 && self.needs_wakeup() {
            self.wakeup(socket_fd, poll_timeout)?;
        }

        Ok(cnt)
    }

    /// The number of free slots on the ring, cached where possible.
    ///
    /// Answers from libxdp's cached consumer position while that
    /// cache holds at least `nb` slots, and reads the current
    /// position otherwise. A cached position only ever trails the
    /// real one, so the count is never an overestimate, and it is
    /// not capped at `nb`.
    ///
    /// See [`nb_free_exact`] for a count that is always up to date.
    ///
    /// [`nb_free_exact`]: Self::nb_free_exact
    #[inline]
    pub fn nb_free(&mut self, nb: u32) -> u32 {
        // SAFETY: the ring is initialised and `&mut self` excludes
        // any other access to it.
        unsafe { libxdp_sys::xsk_prod_nb_free(self.ring.as_ptr(), nb) }
    }

    /// The number of free slots on the ring.
    ///
    /// Reads the current consumer position rather than a cached one,
    /// so the count is exact. See [`nb_free`] for the cached count.
    ///
    /// [`nb_free`]: Self::nb_free
    #[inline]
    pub fn nb_free_exact(&mut self) -> u32 {
        // SAFETY: the ring is initialised and `&mut self` excludes
        // any other access to it.
        unsafe { ring::prod_nb_free_exact(self.ring.as_ptr()) }
    }

    /// Wake up the kernel to let it know it can continue using the
    /// fill ring to process received data.
    ///
    /// See [`produce_and_wakeup`] for link to docs with further
    /// explanation.
    ///
    /// [`produce_and_wakeup`]: Self::produce_and_wakeup
    #[inline]
    pub fn wakeup(&self, fd: &mut Fd, poll_timeout: i32) -> io::Result<()> {
        fd.poll_read(poll_timeout)?;
        Ok(())
    }

    /// Check if the [`XDP_USE_NEED_WAKEUP`] flag is set on the fill
    /// ring. If so then this means a call to [`wakeup`] will be
    /// required to continue processing received data.
    ///
    /// See [`produce_and_wakeup`] for a link to docs with further
    /// explanation.
    ///
    /// [`produce_and_wakeup`]: Self::produce_and_wakeup
    /// [`XDP_USE_NEED_WAKEUP`]: libxdp_sys::XDP_USE_NEED_WAKEUP
    /// [`wakeup`]: Self::wakeup
    #[inline]
    pub fn needs_wakeup(&self) -> bool {
        unsafe { libxdp_sys::xsk_ring_prod__needs_wakeup(self.ring.as_ptr()) != 0 }
    }
}

/// Copies a batch of frame addresses into the fill ring while handling
/// wrap-around once. At most two contiguous destination ranges are needed.
#[inline]
unsafe fn copy_fill_addrs(ring: *mut libxdp_sys::xsk_ring_prod, idx: u32, src: &[u64]) {
    let mask = unsafe { (*ring).mask };
    let size = unsafe { (*ring).size };
    let start = (idx & mask) as usize;
    let first_count = (size as usize - start).min(src.len());
    let second_count = src.len() - first_count;
    let ring_addrs = unsafe { (*ring).ring.cast::<u64>() };

    // SAFETY: reserve guarantees that both destination ranges are writable
    // ring entries, and together they cover exactly `src`.
    unsafe {
        std::ptr::copy_nonoverlapping(src.as_ptr(), ring_addrs.add(start), first_count);

        if second_count > 0 {
            std::ptr::copy_nonoverlapping(
                src.as_ptr().add(first_count),
                ring_addrs,
                second_count,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::copy_fill_addrs;

    #[test]
    fn copy_fill_addrs_copies_across_ring_wrap() {
        let mut ring_addrs = [0_u64; 4];
        let ring = libxdp_sys::xsk_ring_prod {
            cached_prod: 0,
            cached_cons: 0,
            mask: 3,
            size: 4,
            producer: std::ptr::null_mut(),
            consumer: std::ptr::null_mut(),
            ring: ring_addrs.as_mut_ptr().cast(),
            flags: std::ptr::null_mut(),
        };
        let src = [40, 50, 60];

        unsafe { copy_fill_addrs(&ring as *const _ as *mut _, 3, &src) };

        assert_eq!(ring_addrs, [50, 60, 0, 40]);
    }
}

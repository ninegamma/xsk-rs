//! # xsk-core
//!
//! `xsk-core` is a hard fork of the original [`xsk-rs`](https://github.com/DouglasGray/xsk-rs) project.
//!
//! The fork is intended for applications that need a small, predictable
//! AF_XDP binding. The UMEM and socket APIs remain largely unchanged from
//! `xsk-rs`, so existing code can continue to use [`Umem`], [`Socket`], fill
//! queues, completion queues, receive queues, and transmit queues in the same
//! general way.
//!
//! The main architectural change is the descriptor model. [`FrameDesc`] is
//! intended to be only a thin Rust layer over libxdp's `libxdp_sys::xsk`
//! descriptor API. It carries the same essential descriptor data that is
//! exchanged with the kernel:
//!
//! - `addr: u64` — the UMEM offset of the packet data;
//! - `length: u32` — the packet data length; and
//! - `options: u32` — descriptor options such as multi-buffer continuation.
//!
//! [`FrameDesc`] does not own packet memory and does not interpret packet
//! contents. Applications that need packet views, protocol parsing, or
//! headroom management should provide those concerns at a higher layer.
//!
//! ## Major changes from `xsk-rs`
//!
//! - Renamed the crate to `xsk-core` and reset its version to `0.1.0` to mark
//!   the fork boundary.
//! - Reworked [`FrameDesc`] to use the native AF_XDP field types: `u64`
//!   addresses, `u32` lengths, and `u32` options.
//! - Frame headroom is shaped by several cooperating layers—the kernel, NIC
//!   driver, eBPF program, and user-space application—and each layer's
//!   requirements must be respected when using `xsk-core`; see
//!   [headroom.md](../headroom.md) for a detailed explanation.
//! - Replaced the former two-part `SegmentLengths` representation with the
//!   single packet length used by `xdp_desc`.
//! - Replaced `with_lengths(headroom, data)` with `with_length(data)` and
//!   exposed direct `addr()`, `length()`, and `options()` accessors.
//! - Updated RX and completion queue handling to copy descriptor address,
//!   length, and options directly from libxdp without narrowing conversions.
//! - Simplified UMEM frame layout and address calculations around the native
//!   descriptor offset model.
//! - Removed the old high-level UMEM frame data and cursor accessors from the
//!   core API. Packet memory access and packet construction belong to the
//!   application or protocol layer using this crate.
//! - Removed integration tests that depended on the removed high-level
//!   frame-memory API.
//!
//! ## Relationship to the original project
//!
//! The original project remains the reference for the inherited AF_XDP socket
//! and UMEM design. This repository deliberately diverges in descriptor and
//! memory access handling, so code that directly uses `SegmentLengths`,
//! `Umem::data`, `Umem::data_mut`, `Umem::headroom`, or the frame cursor API
//! must be adapted to the new layering.
//!
//! For AF_XDP background and kernel-level details, see the [Linux AF_XDP
//! documentation](https://www.kernel.org/doc/html/latest/networking/af_xdp.html).
//!
//! ## Safety requirements
//!
//! The ownership rules of AF_XDP still apply:
//!
//! - Do not access a frame after submitting its descriptor to the fill queue
//!   or transmit ring until the descriptor is returned through the completion
//!   queue or receive ring.
//! - Do not use a descriptor from one UMEM to access another UMEM.
//! - Treat descriptor addresses as offsets within the UMEM associated with the
//!   descriptor.
//!
//! ## Building
//!
//! ```text
//! cargo build
//! cargo test --lib
//! ```
//!
//! Building and using AF_XDP sockets requires Linux, libxdp, and the
//! appropriate kernel and interface configuration. Operations that create veth
//! pairs or bind AF_XDP sockets may require elevated privileges.
//!
//! ## License
//!
//! This fork retains the MIT license of the original project. See [LICENSE](../LICENSE).
#![deny(missing_docs)]
#![deny(missing_debug_implementations)]
#![deny(unsafe_op_in_unsafe_fn)]
#![allow(clippy::doc_lazy_continuation)]

use cfg_if::cfg_if;

cfg_if! {
    if #[cfg(all(target_pointer_width = "64", target_family = "unix"))] {
        pub mod umem;
        pub use umem::{frame::FrameDesc, CompQueue, FillQueue, Umem};

        pub mod socket;
        pub use socket::{RxQueue, Socket, TxQueue};

        pub mod config;

        mod ring;
        mod util;

        #[cfg(test)]
        mod tests {
            use std::mem;

            #[test]
            fn ensure_usize_and_u64_are_same_size() {
                assert_eq!(mem::size_of::<usize>(), mem::size_of::<u64>());
            }
        }
    }
}

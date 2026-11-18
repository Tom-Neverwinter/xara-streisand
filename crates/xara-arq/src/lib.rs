//! Sans-IO ARQ state machines, compression and broadcast codecs for a VARA-compatible HF/FM modem.
//!
//! Nothing here touches sockets or audio: callers feed events in and act on what comes out, which keeps it testable.
pub mod ax25;
pub mod b2f;
pub mod bits;
pub mod compression;
pub mod cq;
pub mod fm;
pub mod hf;
pub mod host;
pub mod kiss;
mod link;
mod num;
pub mod rng;
pub mod session;
pub mod textcomp;
pub mod time;

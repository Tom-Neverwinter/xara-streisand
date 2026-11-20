//! Signal processing for a VARA-compatible HF/FM modem: coding, modulation, receivers and the turbo decoder.
//!
//! `hf` and `fm` hold the per-mode waveforms; `fec`/`turbo` the coding; `decode`/`receiver` turn audio into frames.
pub mod busy;
pub mod call;
pub mod constellation;
pub mod crc;
pub mod cwid;
pub mod dat;
pub mod decode;
pub mod demap;
pub mod dsp;
mod eq;
pub mod fec;
pub mod fm;
pub mod hf;
pub mod keying;
mod ofdm;
mod receiver;
pub mod rnd;
pub mod sat;
pub mod seq;
pub mod turbo;

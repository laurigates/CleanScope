//! Test utilities for `CleanScope`
//!
//! Provides synthetic packet generation and test helpers for validating
//! the frame assembly pipeline without physical USB hardware.

pub mod packet_generator;

pub use packet_generator::*;

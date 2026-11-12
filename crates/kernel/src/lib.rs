//! Pure, allocation-free decisions shared by the UI's asynchronous requests.
//!
//! `proofs/` proves the Charon/Aeneas translation of these exact modules.
//! Nominal pipeline tags, serialization, payload identity and effects live in
//! the application adapter; neither providers nor terminal IO enter this crate.
#![no_std]
#![forbid(unsafe_code)]

pub mod generation;
pub mod request;

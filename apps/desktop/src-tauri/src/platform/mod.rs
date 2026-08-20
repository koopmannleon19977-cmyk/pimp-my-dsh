//! Platform abstractions: Windows Job Object, named pipe, and browser open.
//! Each submodule has a real Windows implementation and a portable
//! (non-Windows) stub so the pure contracts still compile-test.

#[cfg(windows)]
pub(crate) mod winutil;

pub mod browser;
pub mod confinement;
pub mod job;
pub mod pipe;
pub mod web_proxy;

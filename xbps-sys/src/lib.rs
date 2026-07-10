//! Raw, unsafe FFI bindings to `libxbps`, generated at build time by
//! `bindgen` against the *actual* `<xbps.h>` installed on the build
//! machine (see `build.rs`). Nothing in this crate is hand-written or
//! guessed — in particular `struct xbps_handle`'s layout comes straight
//! from the header, since `caerus`'s worker thread allocates one by
//! value (`struct xbps_handle xh;`, same as the original C code did).
//!
//! This crate is intentionally 100% `unsafe`/raw. All safe wrapping
//! (lifetime management, thread-confinement, string conversions) lives
//! in `caerus::backend::package_store`, which is the *only* place in
//! the whole workspace allowed to call into this crate.
#![allow(
    non_camel_case_types,
    non_snake_case,
    non_upper_case_globals,
    dead_code,
    clippy::all
)]

include!(concat!(env!("OUT_DIR"), "/bindings.rs"));

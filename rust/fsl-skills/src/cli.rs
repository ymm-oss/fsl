// SPDX-License-Identifier: Apache-2.0

//! The `fslc skills` command surface.
//!
//! Everything here is about arguments and output. The placement rules live
//! in [`crate::installation::project`] and [`crate::installation::user`].

mod command;
mod report;
mod selection;

// `fslc` is a separate crate, so the one entry point it calls is the only
// item here that has to be `pub`.
pub use command::run;

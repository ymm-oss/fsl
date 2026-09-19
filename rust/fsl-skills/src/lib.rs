// SPDX-License-Identifier: Apache-2.0

//! The Agent Skills the `fslc` binary carries, and putting them on disk.
//!
//! This crate is the whole `fslc skills` surface. It depends on no other
//! `fsl` crate, and nothing in the FSL stack depends on it: the skills are
//! prose for an agent, not part of what the verifier proves. Keeping it out
//! of the `fslc` library is what keeps the payload out of `fsl-lsp` and
//! `fsl-wasm`, which link that library.
//!
//! Two scopes, with deliberately different mechanisms.
//!
//! [`installation::project::Installation`] copies the files into a project's
//! `.claude/skills`.
//! A copy is what a project wants: it is the version that project pinned, and
//! it stays put when another project selects a different `fslc`.
//!
//! [`installation::user::Installation`] writes the payload under the data
//! directory and links `~/.claude/skills/<skill>` to it through `current`.
//! `install.sh`
//! already produces that shape. Linking through `current` is what makes an
//! upgrade one repointed pointer rather than one relinked skill per skill.
//!
//! Both record what they did in a [`Manifest`], so removal takes back exactly
//! what was put down, and a hand-edited file is recognized instead of
//! overwritten.
//!
//! The split is by what each module decides.
//!
//! | Module | Decides |
//! | --- | --- |
//! | [`embedded`] | which skills the binary carries |
//! | [`installation`] | putting them somewhere, and reporting on what is there |
//! | [`cli`] | arguments, subcommands, and the JSON envelope |

pub mod cli;
pub mod embedded;
pub mod installation;

// Re-exported because an integration test names them. Everything else is
// reached through its own module, or is not visible outside this crate.
// `fslc` itself calls one function, `cli::run`.
pub use embedded::{EMBEDDED_SKILL_FILES, embedded_skill_files, embedded_skill_names};
pub use installation::manifest::{Manifest, Scope};
pub use installation::plan::{Action, EntryState};

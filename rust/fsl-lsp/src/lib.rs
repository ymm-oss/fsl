// SPDX-License-Identifier: Apache-2.0

//! Native FSL language-server primitives.

mod ast_projection;
mod index;
mod server;

pub use ast_projection::{AstDeclaration, RoleScopeMismatch, ast_declarations};
pub use fslc_rust::source_diagnostic::{SourceDiagnostic, diagnostics};
pub use index::{DocumentIndex, ImportBinding, IndexError, Reference, Symbol, SymbolRole};
pub use server::run_stdio;

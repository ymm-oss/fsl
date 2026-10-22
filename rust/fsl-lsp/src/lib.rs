// SPDX-License-Identifier: Apache-2.0

//! Native FSL language-server primitives.

mod ast_projection;
mod index;
mod server;

pub use ast_projection::{AstDeclaration, RoleScopeMismatch, ast_declarations};
pub use fslc_rust::source_diagnostic::{SourceDiagnostic, diagnostics};
pub use index::{
    DocumentIndex, GlossaryRow, ImportBinding, IndexError, KeywordUse, Lexicon, Reference, Symbol,
    SymbolRole, span_range,
};
pub use server::run_stdio;

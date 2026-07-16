// SPDX-License-Identifier: Apache-2.0

fn main() -> Result<(), Box<dyn std::error::Error + Sync + Send>> {
    fsl_lsp::run_stdio()
}

// SPDX-License-Identifier: Apache-2.0

//! Fixture facts from the same product extraction used by the real censuses.

use syn::visit::Visit;

#[path = "product_source.rs"]
mod product_source;

#[derive(Default)]
pub struct ProductFacts {
    pub paths: Vec<String>,
    pub public_structs: Vec<String>,
}

impl<'ast> Visit<'ast> for ProductFacts {
    fn visit_item_struct(&mut self, node: &'ast syn::ItemStruct) {
        if matches!(node.vis, syn::Visibility::Public(_)) {
            self.public_structs.push(node.ident.to_string());
            syn::visit::visit_item_struct(self, node);
        }
    }

    fn visit_path(&mut self, node: &'ast syn::Path) {
        self.paths.push(
            node.segments
                .iter()
                .map(|s| s.ident.to_string())
                .collect::<Vec<_>>()
                .join("::"),
        );
        syn::visit::visit_path(self, node);
    }
}

pub fn scan(source: &str) -> ProductFacts {
    let mut facts = ProductFacts::default();
    let product = product_source::product_source(source);
    facts.visit_file(&syn::parse_file(&product).expect("valid scratch Rust"));
    facts
}

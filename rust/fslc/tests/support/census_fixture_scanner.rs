// SPDX-License-Identifier: Apache-2.0

//! Phase-T fixture oracle using the existing inventory's item-level policy.
//! Phase I owns this adapter: route it through the moved shared scanner, rather
//! than retaining a second implementation of product/test classification.

use syn::visit::Visit;

#[derive(Default)]
pub struct ProductFacts {
    pub paths: Vec<String>,
    pub public_structs: Vec<String>,
}

// Copied from parse_entry_inventory::is_test_only at e64d3fb1.
fn is_test_only(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|attribute| {
        let path = attribute.path();
        if path.is_ident("test") {
            return true;
        }
        if !path.is_ident("cfg") {
            return false;
        }
        attribute
            .parse_args::<syn::Ident>()
            .is_ok_and(|ident| ident == "test")
    })
}

impl<'ast> Visit<'ast> for ProductFacts {
    fn visit_item_fn(&mut self, node: &'ast syn::ItemFn) {
        if !is_test_only(&node.attrs) {
            syn::visit::visit_item_fn(self, node);
        }
    }

    fn visit_impl_item_fn(&mut self, node: &'ast syn::ImplItemFn) {
        if !is_test_only(&node.attrs) {
            syn::visit::visit_impl_item_fn(self, node);
        }
    }

    fn visit_item_mod(&mut self, node: &'ast syn::ItemMod) {
        if !is_test_only(&node.attrs) {
            syn::visit::visit_item_mod(self, node);
        }
    }

    fn visit_item_impl(&mut self, node: &'ast syn::ItemImpl) {
        if !is_test_only(&node.attrs) {
            syn::visit::visit_item_impl(self, node);
        }
    }

    fn visit_item_struct(&mut self, node: &'ast syn::ItemStruct) {
        if !is_test_only(&node.attrs) && matches!(node.vis, syn::Visibility::Public(_)) {
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
    facts.visit_file(&syn::parse_file(source).expect("valid scratch Rust"));
    facts
}

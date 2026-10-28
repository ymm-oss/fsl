// SPDX-License-Identifier: Apache-2.0

//! Shared census extraction: the parse-entry inventory's item-level test policy.
//! Retain original token spelling and locations, not comments or test-only items.

use proc_macro2::{Span, TokenStream, TokenTree};
use syn::spanned::Spanned;
use syn::visit::Visit;

pub fn is_test_only(attrs: &[syn::Attribute]) -> bool {
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

struct ProductSource<'a> {
    source: &'a str,
    output: Vec<u8>,
}

impl ProductSource<'_> {
    fn retain_tokens(&mut self, tokens: TokenStream) {
        for token in tokens {
            if let TokenTree::Group(group) = token {
                for span in [group.span_open(), group.span_close()] {
                    let range = span.byte_range();
                    self.output[range.clone()].copy_from_slice(&self.source.as_bytes()[range]);
                }
                self.retain_tokens(group.stream());
            } else {
                let range = token.span().byte_range();
                self.output[range.clone()].copy_from_slice(&self.source.as_bytes()[range]);
            }
        }
    }

    fn blank(&mut self, span: Span) {
        let range = span.byte_range();
        for byte in &mut self.output[range] {
            if !byte.is_ascii_whitespace() {
                *byte = b' ';
            }
        }
    }
}

impl<'ast> Visit<'ast> for ProductSource<'_> {
    fn visit_item(&mut self, node: &'ast syn::Item) {
        let attrs: &[syn::Attribute] = match node {
            syn::Item::Const(n) => &n.attrs,
            syn::Item::Enum(n) => &n.attrs,
            syn::Item::ExternCrate(n) => &n.attrs,
            syn::Item::Fn(n) => &n.attrs,
            syn::Item::ForeignMod(n) => &n.attrs,
            syn::Item::Impl(n) => &n.attrs,
            syn::Item::Macro(n) => &n.attrs,
            syn::Item::Mod(n) => &n.attrs,
            syn::Item::Static(n) => &n.attrs,
            syn::Item::Struct(n) => &n.attrs,
            syn::Item::Trait(n) => &n.attrs,
            syn::Item::TraitAlias(n) => &n.attrs,
            syn::Item::Type(n) => &n.attrs,
            syn::Item::Union(n) => &n.attrs,
            syn::Item::Use(n) => &n.attrs,
            _ => &[],
        };
        if is_test_only(attrs) {
            self.blank(node.span());
        } else {
            syn::visit::visit_item(self, node);
        }
    }

    fn visit_impl_item_fn(&mut self, node: &'ast syn::ImplItemFn) {
        if is_test_only(&node.attrs) {
            self.blank(node.span());
        } else {
            syn::visit::visit_impl_item_fn(self, node);
        }
    }

    fn visit_attribute(&mut self, node: &'ast syn::Attribute) {
        if node.path().is_ident("doc") {
            self.blank(node.span());
        }
    }
}

/// Blank excluded bytes rather than reprinting Rust: line-based census rules
/// and their existing allowlist needles keep their exact spelling and lines.
pub fn product_source(source: &str) -> String {
    let file = syn::parse_file(source).expect("parse census Rust source");
    let mut scanner = ProductSource {
        source,
        output: source
            .bytes()
            .map(|byte| {
                if byte.is_ascii_whitespace() {
                    byte
                } else {
                    b' '
                }
            })
            .collect(),
    };
    scanner.retain_tokens(source.parse().expect("tokenize census Rust source"));
    scanner.visit_file(&file);
    String::from_utf8(scanner.output).expect("retained source is UTF-8")
}

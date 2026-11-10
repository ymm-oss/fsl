// SPDX-License-Identifier: Apache-2.0

//! Namespace perturbations of the same finite placement templates. Rendering
//! existing cells never depends on this module or changes their original input.

use super::{aggregate, matrix};

pub const INAPPLICABLE_COUNT: usize = 608;
pub const INAPPLICABLE_REASON: &str = "compose has no const declaration (parser.rs:410-424); component-owned ambient-const: 38 positions x 16";

pub struct Cell {
    pub id: String,
    pub owner: aggregate::Owner,
    pub bundle: matrix::SourceBundle,
}

fn inject(source: &mut String, constant: bool) {
    if !source.contains("state {") {
        let at = source.find('\n').expect("composition header");
        source.insert_str(
            at,
            if constant {
                "\nconst x = 0\n"
            } else {
                "\nstate { x: Bool } init { x = false }\n"
            },
        );
        return;
    }
    if constant {
        *source = source.replacen("state {", "const x = 0\nstate {", 1);
    } else {
        *source = source.replacen("state {", "state { x: Bool,", 1);
        *source = source.replacen("init {", "init { x = false", 1);
    }
}

pub fn cells() -> Vec<Cell> {
    let mut cells = Vec::new();
    for template in aggregate::namespace_templates() {
        for family in ["ambient-state", "ambient-const", "own-global"] {
            let owner = template.owner();
            let component_owned = matches!(owner, aggregate::Owner::Component(_));
            if family == "ambient-const" && component_owned {
                continue;
            }
            let mut bundle = template.render();
            // Keep a use of the local pattern, as in the independent reproducer.
            if family != "own-global" {
                for (_, source) in &mut bundle.files {
                    *source = source.replace("o is some(x)", "o is some(x) and x == 0");
                }
            }
            let target = if (family == "own-global") == component_owned {
                "b.fsl"
            } else {
                "s.fsl"
            };
            let text = &mut bundle
                .files
                .iter_mut()
                .find(|(f, _)| f == target)
                .unwrap()
                .1;
            inject(text, family == "ambient-const");
            cells.push(Cell {
                id: template.id.replacen("aggregate-pattern", family, 1),
                owner,
                bundle,
            });
        }
    }
    assert_eq!(cells.len(), 2544 - INAPPLICABLE_COUNT);
    cells
}

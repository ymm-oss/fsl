// SPDX-License-Identifier: Apache-2.0

#[path = "support/compose_scratch.rs"]
#[allow(dead_code)]
mod support;
use serde_json::json;
use support::{run, scratch};

#[test]
fn composed_sources_and_type_names_survive_both_aliases() {
    let args = ["refine", "impl.fsl", "abs.fsl", "map.fsl", "--depth", "4"];
    let leaf = |upper, count| {
        format!(
            "spec Leaf {{ entity Item type T = 0..{upper} state {{ x: Int = 0 }} }} verify {{ instances Item = {count} }}"
        )
    };
    for alias in ["p", "q"] {
        let other = if alias == "p" { "q" } else { "p" };
        for (upper, count, name) in [(1, 3, "Item"), (2, 1, "T")] {
            let abs = format!(
                "compose Abs {{ use Leaf as {alias} from \"big.fsl\" use Leaf as {other} from \"small.fsl\" }}"
            );
            let dir = scratch(
                "refine-compose-1599",
                &[
                    ("small.fsl", &leaf(1, 1)),
                    ("big.fsl", &leaf(upper, count)),
                    (
                        "impl.fsl",
                        "compose Impl { use Leaf as p from \"small.fsl\" use Leaf as q from \"small.fsl\" }",
                    ),
                    ("abs.fsl", &abs),
                    ("map.fsl", "refinement R { impl Impl abs Abs maps auto }"),
                ],
            );
            let (packet, exit) = run(&dir, &args);
            assert_eq!(exit, 2, "{packet}");
            assert_eq!(packet["result"], "error");
            assert_eq!(packet["kind"], "type");
            assert_eq!(packet["loc"], json!({"line":1,"column":1}));
            let message = packet["message"].as_str().unwrap();
            assert!(message.contains(&format!("{alias}.{name}")) && !message.contains("__"));
            if name == "Item" {
                assert_eq!(
                    message,
                    format!(
                        "entity '{alias}.Item' has conflicting bounds in impl 'Impl': component '{alias}' from 'small.fsl': verify block instances Item = 1 and abs 'Abs': component '{alias}' from 'big.fsl': verify block instances Item = 3; align instances in the component verify blocks"
                    )
                );
                assert!(!message.contains("--instances"));
            }
            std::fs::write(dir.join("abs.fsl"), abs.replace("big.fsl", "small.fsl")).unwrap();
            let (positive, exit) = run(&dir, &args);
            assert_eq!(exit, 0, "{positive}");
            assert_eq!(positive["result"], "refines");
        }
    }
}

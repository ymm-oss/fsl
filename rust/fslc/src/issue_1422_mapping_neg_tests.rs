// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Ryoichi Izumita

mod issue_1422_mapping_neg {
    use super::*;

    fn negate(value: &Value, binding: bool) -> Result<Value, String> {
        let model = crate::spec_loading::load_model_from_source(
            Path::new("mapping-unit.fsl"),
            "spec K { type Bit = 0..1 state { x: Bit } init { x = 0 } action put(v: Bit) { x = v } }",
        ).unwrap();
        let (raw, bindings) = if binding {
            (json!({"v":17}), json!({"v":value}))
        } else {
            (json!({"v":value}), json!({}))
        };
        let result = mapping_json_expr(
            &KernelExpr::Neg(Box::new(KernelExpr::Var("v".to_owned()))),
            raw.as_object().unwrap(),
            bindings.as_object().unwrap(),
            &model,
        );
        eprintln!(
            "ISSUE_1422_MAPPING_MATH {}",
            json!({"value":value,"binding":binding,"result":result})
        );
        result
    }

    #[test]
    fn minimum_integer_is_rejected_from_raw_and_binding_variables() {
        for binding in [false, true] {
            assert_eq!(
                negate(&json!(i64::MIN), binding),
                Err("invalid mapping arithmetic".to_owned())
            );
        }
    }

    #[test]
    fn representable_negation_and_binding_precedence_are_preserved() {
        for (input, expected) in [
            (i64::MIN + 1, i64::MAX),
            (i64::MAX, -i64::MAX),
            (-1, 1),
            (0, 0),
            (1, -1),
        ] {
            for binding in [false, true] {
                assert_eq!(negate(&json!(input), binding), Ok(json!(expected)));
            }
        }
    }

    #[test]
    fn noninteger_negation_keeps_the_existing_error() {
        for value in [Value::Null, json!(true), json!("0"), json!([]), json!({})] {
            for binding in [false, true] {
                assert_eq!(
                    negate(&value, binding),
                    Err("mapping negation requires integer".to_owned())
                );
            }
        }
    }
}

// SPDX-License-Identifier: Apache-2.0

//! Shared naming transforms for Domain-family projections and generators.

pub(crate) fn snake(value: &str) -> String {
    let mut output = String::new();
    for (index, character) in value.chars().enumerate() {
        if index > 0 && character.is_ascii_uppercase() {
            output.push('_');
        }
        output.push(character.to_ascii_lowercase());
    }
    output
}

#[cfg(test)]
mod tests {
    use super::snake;

    #[test]
    fn preserves_uppercase_digit_and_consecutive_underscore_behavior() {
        assert_eq!(snake("Order2__Item"), "order2___item");
        assert_eq!(snake("HTTP2__API"), "h_t_t_p2___a_p_i");
    }
}

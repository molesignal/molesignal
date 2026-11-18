// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Substitutes arguments into the `SKILL.md` body for explicit `/name arguments` invocations:
//! `$ARGUMENTS` is all arguments, and `$1`...`$9` are the corresponding whitespace-separated
//! arguments. If the body has no placeholders and the arguments are nonempty, appends the arguments
//! to the body.

/// The body with arguments substituted.
pub fn render_invocation(body: &str, arguments: &str) -> String {
    let arguments = arguments.trim();
    let positional: Vec<&str> = arguments.split_whitespace().collect();
    let mut rendered = String::with_capacity(body.len() + arguments.len());
    let mut used = false;
    let mut rest = body;

    while let Some(at) = rest.find('$') {
        rendered.push_str(&rest[..at]);
        let after = &rest[at + 1..];

        if let Some(tail) = after.strip_prefix("ARGUMENTS") {
            rendered.push_str(arguments);
            used = true;
            rest = tail;
            continue;
        }

        match after.as_bytes().first() {
            Some(digit @ b'1'..=b'9') => {
                let index = usize::from(digit - b'1');
                rendered.push_str(positional.get(index).copied().unwrap_or_default());
                used = true;
                rest = &after[1..];
            }
            _ => {
                rendered.push('$');
                rest = after;
            }
        }
    }

    rendered.push_str(rest);

    if !used && !arguments.is_empty() {
        rendered.push_str("\n\nARGUMENTS: ");
        rendered.push_str(arguments);
    }

    rendered
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fills_all_and_positional_arguments() {
        assert_eq!(
            render_invocation("Look at $1 in $2 ($ARGUMENTS)", "checkout prod"),
            "Look at checkout in prod (checkout prod)"
        );
        assert_eq!(
            render_invocation("Only $3 is missing", "a b"),
            "Only  is missing"
        );
    }

    #[test]
    fn appends_arguments_to_a_body_without_placeholders() {
        assert_eq!(
            render_invocation("Triage the incident.", " inc-1 "),
            "Triage the incident.\n\nARGUMENTS: inc-1"
        );
        assert_eq!(render_invocation("Triage.", ""), "Triage.");
    }

    #[test]
    fn leaves_other_dollar_signs_alone() {
        assert_eq!(
            render_invocation("Costs $0 and $HOME stays", "x"),
            "Costs $0 and $HOME stays\n\nARGUMENTS: x"
        );
    }
}

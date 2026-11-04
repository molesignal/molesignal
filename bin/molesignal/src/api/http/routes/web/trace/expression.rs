// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! What a trace read matches, from its parameters or from the field query a
//! client sends as text: the text is read as the Traces page reads it, into
//! the same free text, filters and condition tree.

use query_language::field_query::read_trace_query;

use crate::shared::{Error, Result};

/// The longest field query a read takes, in bytes.
const MAX_EXPRESSION_LENGTH: usize = 8 * 1024;

/// Reads the field query of `expression` into the `q`, `filters` and `where`
/// text of a trace read, as the Traces page would have sent them. The text
/// stands for the three: a request sends one or the other.
pub(super) fn expand(
    expression: &mut Option<String>,
    q: &mut Option<String>,
    filters: &mut Option<String>,
    condition: &mut Option<String>,
) -> Result<()> {
    let Some(text) = expression.take().filter(|text| !text.trim().is_empty()) else {
        return Ok(());
    };

    if [&*q, &*filters, &*condition]
        .into_iter()
        .any(|value| value.as_deref().is_some_and(|text| !text.trim().is_empty()))
    {
        return Err(Error::invalid(
            "`expression` stands for `q`, `filters` and `where`; send one or the other",
        ));
    }

    if text.len() > MAX_EXPRESSION_LENGTH {
        return Err(Error::invalid(format!(
            "the field query is longer than {MAX_EXPRESSION_LENGTH} bytes"
        )));
    }

    let read = read_trace_query(&text)?;
    let encode = |error: serde_json::Error| Error::internal(format!("encode trace query: {error}"));

    *q = read.q;
    *filters = if read.filters.is_empty() {
        None
    } else {
        Some(serde_json::to_string(&read.filters).map_err(encode)?)
    };
    *condition = read
        .condition
        .map(|tree| serde_json::to_string(&tree))
        .transpose()
        .map_err(encode)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn expanded(expression: &str) -> Result<[Option<String>; 3]> {
        let (mut q, mut filters, mut condition) = (None, None, None);

        expand(
            &mut Some(expression.into()),
            &mut q,
            &mut filters,
            &mut condition,
        )?;

        Ok([q, filters, condition])
    }

    #[test]
    fn a_field_query_reads_into_the_parameters_the_page_sends() {
        let [q, filters, condition] =
            expanded("service.name = 'api' AND slow AND (status_code = 'ERROR' OR timeout)")
                .expect("read");

        assert_eq!(q.as_deref(), Some("slow"));
        assert_eq!(
            filters.as_deref(),
            Some(r#"[{"field":"service.name","op":"=","value":"api"}]"#)
        );
        assert_eq!(
            condition.as_deref(),
            Some(r#"{"or":[{"field":"status_code","op":"=","value":"ERROR"},{"text":"timeout"}]}"#)
        );
    }

    #[test]
    fn the_text_and_the_parameters_do_not_come_together() {
        let mut q = Some("slow".to_string());

        let refused = expand(&mut Some("timeout".into()), &mut q, &mut None, &mut None);

        assert!(refused.is_err());
        assert!(expanded("(service.name = 'a'").is_err());
    }
}

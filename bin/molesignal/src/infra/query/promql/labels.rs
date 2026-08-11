// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use super::*;

pub(super) fn apply_label_replace(input: InstantVector, args: &LabelReplaceArgs) -> InstantVector {
    let items = input
        .items
        .into_iter()
        .map(|(mut labels, value)| {
            apply_label_replace_to_labels(&mut labels, args);
            (labels, value)
        })
        .collect();
    InstantVector { items }
}

pub(super) fn apply_label_replace_range(
    mut input: RangeVector,
    args: &LabelReplaceArgs,
) -> RangeVector {
    for point in &mut input.points {
        apply_label_replace_to_labels(&mut point.labels, args);
    }
    input
}

pub(super) fn apply_label_join(input: InstantVector, args: &LabelJoinArgs) -> InstantVector {
    let items = input
        .items
        .into_iter()
        .map(|(mut labels, value)| {
            apply_label_join_to_labels(&mut labels, args);
            (labels, value)
        })
        .collect();
    InstantVector { items }
}

pub(super) fn apply_label_join_range(mut input: RangeVector, args: &LabelJoinArgs) -> RangeVector {
    for point in &mut input.points {
        apply_label_join_to_labels(&mut point.labels, args);
    }
    input
}

pub(super) fn apply_label_replace_to_labels(labels: &mut LabelSet, args: &LabelReplaceArgs) {
    let source = labels
        .get(&args.src_label)
        .map(String::as_str)
        .unwrap_or("");
    if let Some(captures) = args.regex.captures(source) {
        let replacement = expand_label_replacement(&args.replacement, &captures);
        labels.insert(args.dst_label.clone(), replacement);
    }
}

pub(super) fn apply_label_join_to_labels(labels: &mut LabelSet, args: &LabelJoinArgs) {
    let value = args
        .src_labels
        .iter()
        .map(|name| labels.get(name).map(String::as_str).unwrap_or(""))
        .collect::<Vec<_>>()
        .join(&args.separator);
    labels.insert(args.dst_label.clone(), value);
}

pub(super) fn expand_label_replacement(replacement: &str, captures: &Captures<'_>) -> String {
    let mut out = String::new();
    captures.expand(replacement, &mut out);
    out
}

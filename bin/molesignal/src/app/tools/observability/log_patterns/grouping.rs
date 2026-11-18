// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Logs that say the same thing, grouped into patterns.
//!
//! Messages read into the same words are one pattern. Then patterns of one length that differ at
//! a single place, in three ways or more, become one pattern with that place open: "user alice
//! signed in" and the same of bob and carol. Two ways are left as two patterns, since "order
//! accepted" and "order rejected" are different events. Where some logs already have a value at
//! that place, a word found there is one more value. The same rules as the Patterns view of Logs.

use std::collections::{HashMap, HashSet};

use super::tokens::{Token, Wild, pattern_key, pattern_tokens};

/// Words that differ at one place are taken for one pattern from this many apart.
const MERGE_FROM: usize = 3;

/// A pattern shorter than this is an event of its own, not a sentence with a slot in it.
const MERGE_MIN_WORDS: usize = 3;

/// Logs that say the same thing: the words of the pattern and where its logs are among those read,
/// in their order.
#[derive(Debug, Clone)]
pub(super) struct Group {
    pub(super) tokens: Vec<Token>,
    pub(super) rows: Vec<usize>,
}

/// The patterns of `messages`, each message given with its place among the logs read. Messages
/// with nothing to say are left to the caller.
pub(super) fn group_messages<'a>(
    messages: impl IntoIterator<Item = (usize, &'a str)>,
) -> Vec<Group> {
    let mut index = HashMap::<String, usize>::new();
    let mut groups = Vec::<Group>::new();

    for (row, message) in messages {
        let tokens = pattern_tokens(message);
        let key = pattern_key(&tokens);

        match index.get(&key) {
            Some(&at) => groups[at].rows.push(row),
            None => {
                index.insert(key, groups.len());
                groups.push(Group {
                    tokens,
                    rows: vec![row],
                });
            }
        }
    }

    merge_by_single_slot(groups)
}

fn merge_by_single_slot(groups: Vec<Group>) -> Vec<Group> {
    let longest = groups
        .iter()
        .map(|group| group.tokens.len())
        .max()
        .unwrap_or(0);

    let mut current = groups;

    // The first word names the event: it is never opened.
    for slot in 1..longest {
        let mut index = HashMap::<String, usize>::new();
        let mut buckets = Vec::<Vec<Group>>::new();
        let mut next = Vec::new();

        for group in current {
            if group.tokens.len() <= slot || group.tokens.len() < MERGE_MIN_WORDS {
                next.push(group);
                continue;
            }

            let signature = signature(&group.tokens, slot);

            match index.get(&signature) {
                Some(&at) => buckets[at].push(group),
                None => {
                    index.insert(signature, buckets.len());
                    buckets.push(vec![group]);
                }
            }
        }

        for bucket in buckets {
            next.extend(merge_bucket(bucket, slot));
        }

        current = next;
    }

    current
}

/// The pattern with the word at `slot` set aside, keeping only the punctuation around it.
fn signature(tokens: &[Token], slot: usize) -> String {
    tokens
        .iter()
        .enumerate()
        .map(|(index, token)| {
            if index == slot {
                format!("{}\u{1}{}", token.lead, token.trail)
            } else {
                token.key()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Patterns that differ only at `slot`: one pattern with the slot open, or the patterns as they
/// were.
fn merge_bucket(bucket: Vec<Group>, slot: usize) -> Vec<Group> {
    let open = bucket
        .iter()
        .position(|group| group.tokens[slot].wild.is_some());

    // Alone there is nothing to join; words alone are one pattern only from three apart.
    if bucket.len() < 2 || (open.is_none() && bucket.len() < MERGE_FROM) {
        return bucket;
    }

    // The pattern that already has a value there keeps its sample; its kind holds only if all
    // agree.
    let first = open.unwrap_or(0);

    let kinds = bucket
        .iter()
        .map(|group| group.tokens[slot].wild)
        .collect::<HashSet<_>>();

    let wild = if kinds.len() == 1 {
        bucket[first].tokens[slot].wild.unwrap_or(Wild::Any)
    } else {
        Wild::Any
    };

    let mut tokens = bucket[first].tokens.clone();
    tokens[slot].wild = Some(wild);
    tokens[slot].unit.clear();

    let mut rows = bucket
        .into_iter()
        .flat_map(|group| group.rows)
        .collect::<Vec<_>>();

    rows.sort_unstable();

    vec![Group { tokens, rows }]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn patterns(messages: &[&str]) -> Vec<(String, Vec<usize>)> {
        let mut groups = group_messages(messages.iter().copied().enumerate())
            .into_iter()
            .map(|group| (pattern_key(&group.tokens), group.rows))
            .collect::<Vec<_>>();

        groups.sort_by(|left, right| {
            right
                .1
                .len()
                .cmp(&left.1.len())
                .then_with(|| left.0.cmp(&right.0))
        });

        groups
    }

    #[test]
    fn the_logs_that_say_the_same_thing_are_one_pattern() {
        let found = patterns(&[
            "payment authorized for order 8f31c2 in 182ms",
            "order accepted",
            "payment authorized for order 77aa01 in 95ms",
            "upstream timeout after 3 retries",
            "payment authorized for order 1b2c3d in 240ms",
        ]);

        assert_eq!(
            found,
            [
                (
                    "payment authorized for order <any> in <number>ms".to_string(),
                    vec![0, 2, 4]
                ),
                ("order accepted".to_string(), vec![1]),
                (
                    "upstream timeout after <number> retries".to_string(),
                    vec![3]
                ),
            ]
        );
    }

    #[test]
    fn a_place_where_the_logs_differ_by_a_word_in_three_ways_opens() {
        let found = patterns(&[
            "user alice signed in",
            "user bob signed in",
            "user carol signed in",
            "user alice signed out",
        ]);

        assert_eq!(
            found
                .iter()
                .map(|(key, rows)| (key.as_str(), rows.len()))
                .collect::<Vec<_>>(),
            [("user <any> signed in", 3), ("user alice signed out", 1)]
        );
    }

    #[test]
    fn a_word_is_a_value_where_other_logs_have_a_value_in_its_place() {
        let found = patterns(&[
            "payment authorized for order 8f31c2 in 182ms",
            "payment authorized for order ccdffc in 95ms",
            "payment authorized for order 482910 in 240ms",
        ]);

        assert_eq!(
            found,
            [(
                "payment authorized for order <any> in <number>ms".to_string(),
                vec![0, 1, 2]
            )]
        );
    }

    #[test]
    fn a_number_counted_in_two_units_is_a_number() {
        let found = patterns(&["query took 250ms to plan", "query took 2s to plan"]);

        assert_eq!(
            found,
            [("query took <number> to plan".to_string(), vec![0, 1])]
        );
    }

    #[test]
    fn two_events_that_differ_by_a_word_stay_two() {
        let mut keys = patterns(&[
            "order 17 was accepted",
            "order 18 was rejected",
            "order accepted",
            "order rejected",
            "order shipped",
        ])
        .into_iter()
        .map(|(key, _)| key)
        .collect::<Vec<_>>();

        keys.sort();

        assert_eq!(
            keys,
            [
                "order <number> was accepted",
                "order <number> was rejected",
                "order accepted",
                "order rejected",
                "order shipped",
            ]
        );
    }

    #[test]
    fn the_word_a_message_begins_with_never_opens() {
        let found = patterns(&[
            "started worker pool",
            "stopped worker pool",
            "resized worker pool",
        ]);

        assert_eq!(found.len(), 3);
    }

    #[test]
    fn no_messages_make_no_patterns() {
        assert!(group_messages(Vec::<(usize, &str)>::new()).is_empty());
    }
}

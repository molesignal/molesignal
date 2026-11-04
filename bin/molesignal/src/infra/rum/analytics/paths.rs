// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Paths: the pages sessions went through, from a page or to a page. Pages are taken in the order
//! they were opened, a page opened again at once counts once, and each step of the way is a column
//! of the answer: the pages of a column with the sessions that were there, and the links to the
//! next column with the sessions that went that way. A session counts for as many as the share it
//! was kept with says.

use std::collections::{BTreeMap, HashMap};

use serde::Serialize;
use serde_json::Value;

use super::{StepKind, checked_range, events, shared_conditions, text, whole};
use crate::{
    infra::rum::query::{Condition, Filter, WEIGHT, ident, literal},
    shared::{Error, Result},
};

/// The fields a page is told by: the path with the changing parts grouped, the path, a screen's name.
pub const PAGE_FIELDS: [&str; 3] = ["view.url_path_group", "view.url_path", "view.name"];

/// Most pages a column names; the rest are one `OTHER`.
pub const PER_LEVEL: usize = 8;

/// What the pages beyond the most visited of a column are called in an answer.
pub const OTHER: &str = "__other__";

/// Most steps of the way after (or before) the page it starts from.
pub const MAX_DEPTH: usize = 6;

/// Most rows of each kind the statement reads before the answer is cut down to [`PER_LEVEL`].
const NODE_ROWS: usize = 600;
const LINK_ROWS: usize = 1_500;

/// What to count.
#[derive(Debug, Clone, Copy)]
pub struct PathsSpec<'a> {
    pub from: i64,
    pub to: i64,
    pub shared: &'a [Filter],
    /// The page the paths start from, or end at when `backward`; none is the first page of each
    /// session, or the last when `backward`.
    pub anchor: Option<&'a str>,
    pub backward: bool,
    /// How many pages after the anchor (before it when `backward`).
    pub depth: usize,
    /// One of [`PAGE_FIELDS`].
    pub by: &'a str,
}

/// The statement: `node` rows (`depth`, `source` the page, `sessions`) and `link` rows (`depth` of
/// the page the link leaves, `source`, `target`, `sessions`).
pub fn paths_sql(spec: &PathsSpec<'_>) -> Result<String> {
    checked_range(spec.from, spec.to)?;

    if spec.depth == 0 || spec.depth > MAX_DEPTH {
        return Err(Error::invalid(format!(
            "paths go 1 to {MAX_DEPTH} pages from the page they start at"
        )));
    }

    if !PAGE_FIELDS.contains(&spec.by) {
        return Err(Error::invalid(format!(
            "pages are told by {}",
            PAGE_FIELDS.join(", ")
        )));
    }

    let mut found = shared_conditions(StepKind::Views.stream(), spec.shared)?;
    let page = ident(spec.by);

    found.push(Condition::raw(
        &format!("{page} IS NOT NULL"),
        &[spec.by],
        false,
    ));

    let rows = events(
        StepKind::Views.relation(&found, [spec.by]),
        &format!(
            "\"session.id\" AS sid, CAST(_timestamp AS BIGINT) AS ts, CAST({page} AS VARCHAR) AS page, \
             {WEIGHT} AS w"
        ),
        spec.from,
        spec.to,
        &found,
    );

    let anchor = match (
        spec.anchor.map(str::trim).filter(|page| !page.is_empty()),
        spec.backward,
    ) {
        (Some(page), false) => format!(
            "SELECT sid, min(pos) AS p0 FROM n WHERE page = {} GROUP BY sid",
            literal(page)
        ),
        (Some(page), true) => format!(
            "SELECT sid, max(pos) AS p0 FROM n WHERE page = {} GROUP BY sid",
            literal(page)
        ),
        (None, false) => "SELECT sid, min(pos) AS p0 FROM n GROUP BY sid".to_string(),
        (None, true) => "SELECT sid, max(pos) AS p0 FROM n GROUP BY sid".to_string(),
    };

    let (depth, within) = if spec.backward {
        (
            "a.p0 - n.pos",
            format!("n.pos <= a.p0 AND a.p0 - n.pos <= {}", spec.depth),
        )
    } else {
        (
            "n.pos - a.p0",
            format!("n.pos >= a.p0 AND n.pos - a.p0 <= {}", spec.depth),
        )
    };

    Ok(format!(
        "WITH v AS ({rows}), \
         d AS (SELECT sid, ts, page, w, lag(page) OVER (PARTITION BY sid ORDER BY ts) AS prev FROM v), \
         n AS (SELECT sid, page, w, row_number() OVER (PARTITION BY sid ORDER BY ts) AS pos \
               FROM d WHERE prev IS NULL OR prev <> page), \
         a AS ({anchor}), \
         x AS (SELECT n.sid AS sid, n.page AS page, n.w AS w, {depth} AS depth \
               FROM n JOIN a ON n.sid = a.sid WHERE {within}) \
         SELECT * FROM (SELECT 'node' AS kind, depth, page AS source, '' AS target, \
                               CAST(COALESCE(ROUND(SUM(w)), 0) AS BIGINT) AS sessions \
                        FROM x GROUP BY depth, page ORDER BY sessions DESC LIMIT {NODE_ROWS}) AS nodes \
         UNION ALL \
         SELECT * FROM (SELECT 'link' AS kind, l.depth AS depth, l.page AS source, r.page AS target, \
                               CAST(COALESCE(ROUND(SUM(l.w)), 0) AS BIGINT) AS sessions \
                        FROM x l JOIN x r ON l.sid = r.sid AND r.depth = l.depth + 1 \
                        GROUP BY l.depth, l.page, r.page ORDER BY sessions DESC LIMIT {LINK_ROWS}) AS links"
    ))
}

/// A page of a column, with the sessions that were there and those whose way ended there.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PathNode {
    pub column: usize,
    pub page: String,
    pub sessions: u64,
    /// Sessions that went no further (or, going backward, began here).
    pub ends: u64,
}

/// Sessions that went from a page of a column to a page of the next.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PathLink {
    pub column: usize,
    pub source: String,
    pub target: String,
    pub sessions: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Paths {
    /// How many columns there are, the first being where the paths start.
    pub columns: usize,
    /// The sessions that have the page the paths start from.
    pub sessions: u64,
    pub backward: bool,
    pub nodes: Vec<PathNode>,
    pub links: Vec<PathLink>,
}

struct Rows {
    nodes: Vec<(i64, String, u64)>,
    links: Vec<(i64, String, String, u64)>,
}

fn rows_of(columns: &[String], rows: &[Vec<Value>]) -> Rows {
    let at = |name: &str| columns.iter().position(|column| column == name);
    let (kind, depth, source, target, sessions) = (
        at("kind"),
        at("depth"),
        at("source"),
        at("target"),
        at("sessions"),
    );

    let mut found = Rows {
        nodes: Vec::new(),
        links: Vec::new(),
    };

    for row in rows {
        let cell = |index: Option<usize>| index.and_then(|index| row.get(index));
        let count = whole(cell(sessions)).max(0) as u64;

        let (Some(source), depth) = (text(cell(source)), whole(cell(depth))) else {
            continue;
        };

        match text(cell(kind)).as_deref() {
            Some("node") => found.nodes.push((depth, source, count)),
            Some("link") => {
                if let Some(target) = text(cell(target)) {
                    found.links.push((depth, source, target, count));
                }
            }
            _ => {}
        }
    }

    found
}

/// The paths an answer of [`paths_sql`] says: the pages in columns, each column cut to the most
/// visited [`PER_LEVEL`] and the rest one [`OTHER`]; going backward the columns run from the
/// earliest page to the one the paths end at.
pub fn read_paths(depth: usize, backward: bool, columns: &[String], rows: &[Vec<Value>]) -> Paths {
    let found = rows_of(columns, rows);
    let last = depth as i64;
    let column = |at: i64| if backward { last - at } else { at };

    let mut sessions: BTreeMap<(usize, String), u64> = BTreeMap::new();

    for (at, page, count) in &found.nodes {
        if (0..=last).contains(at) {
            *sessions
                .entry((column(*at) as usize, page.clone()))
                .or_default() += count;
        }
    }

    // The most visited pages of each column keep their names.
    let mut kept: HashMap<(usize, String), String> = HashMap::new();

    for level in 0..=depth {
        let mut pages = sessions
            .iter()
            .filter(|((at, _), _)| *at == level)
            .map(|((_, page), count)| (page.clone(), *count))
            .collect::<Vec<_>>();

        pages.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));

        for (rank, (page, _)) in pages.into_iter().enumerate() {
            let name = if rank < PER_LEVEL {
                page.clone()
            } else {
                OTHER.to_string()
            };

            kept.insert((level, page), name);
        }
    }

    let named = |level: usize, page: &str| {
        kept.get(&(level, page.to_string()))
            .cloned()
            .unwrap_or_else(|| OTHER.to_string())
    };

    let mut nodes: BTreeMap<(usize, String), u64> = BTreeMap::new();

    for ((level, page), count) in &sessions {
        *nodes.entry((*level, named(*level, page))).or_default() += count;
    }

    let mut links: BTreeMap<(usize, String, String), u64> = BTreeMap::new();

    for (at, source, target, count) in &found.links {
        if !(0..last).contains(at) {
            continue;
        }

        // Going backward the link leaves the page that came earlier, which is further from the anchor.
        let (from_column, from_page, to_column, to_page) = if backward {
            (
                column(*at + 1) as usize,
                target,
                column(*at) as usize,
                source,
            )
        } else {
            (
                column(*at) as usize,
                source,
                column(*at + 1) as usize,
                target,
            )
        };

        *links
            .entry((
                from_column,
                named(from_column, from_page),
                named(to_column, to_page),
            ))
            .or_default() += count;
    }

    let nodes = nodes
        .into_iter()
        .map(|((level, page), count)| {
            let through: u64 = links
                .iter()
                .filter(|((from, source, target), _)| {
                    if backward {
                        *from + 1 == level && *target == page && !source.is_empty()
                    } else {
                        *from == level && *source == page && !target.is_empty()
                    }
                })
                .map(|(_, count)| *count)
                .sum();

            PathNode {
                column: level,
                page,
                sessions: count,
                ends: count.saturating_sub(through),
            }
        })
        .collect::<Vec<_>>();

    let mut nodes = nodes;

    // The pages of a column by the sessions they hold, and the rest of the pages after them.
    nodes.sort_by(|left, right| {
        left.column
            .cmp(&right.column)
            .then_with(|| (left.page == OTHER).cmp(&(right.page == OTHER)))
            .then_with(|| right.sessions.cmp(&left.sessions))
            .then_with(|| left.page.cmp(&right.page))
    });

    let anchored = if backward { depth } else { 0 };

    Paths {
        columns: depth + 1,
        sessions: nodes
            .iter()
            .filter(|node| node.column == anchored)
            .map(|node| node.sessions)
            .sum(),
        backward,
        nodes,
        links: links
            .into_iter()
            .map(|((level, source, target), sessions)| PathLink {
                column: level,
                source,
                target,
                sessions,
            })
            .collect(),
    }
}

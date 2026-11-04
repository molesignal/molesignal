// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Where people click on a page, and how far down they go.
//!
//! A click is told by the element it landed on and where in that element, so that it lands on the
//! same element whatever the size of the window: the page draws the counts over a recording of the
//! page, by finding the element again. The clicks of an element no recording shows are placed by
//! where on the page they landed. Scrolling is how many views reached each depth. Both count the
//! rows that are stored: a map says where, not how many there would be.

use serde::Serialize;
use serde_json::Value;

use super::sql::{Relation, Scope, ident};
use crate::shared::{Error, Result};

/// An element is cut into this many cells each way.
pub const CELLS: i64 = 10;

/// How deep a band of the scroll map is, in CSS pixels.
const BAND_PX: i64 = 100;
/// The deepest band told apart: a page longer than this is counted in its last band.
const MAX_BANDS: i64 = 1_000;

const ELEMENT: &str = "__element";
const CELL_X: &str = "__cell_x";
const CELL_Y: &str = "__cell_y";
const NAME: &str = "__name";
const SELECTOR: &str = "__selector";
const CLICKS: &str = "__clicks";
const SESSIONS: &str = "__sessions";
const FRUSTRATED: &str = "__frustrated";
const PAGE_X: &str = "__page_x";
const PAGE_Y: &str = "__page_y";
const BAND: &str = "__band";
const VIEWS: &str = "__views";
const HEIGHT: &str = "__height";

/// The element a row is about: where it stands in the page, or the selector of an older row.
const ELEMENT_KEY: &str = "COALESCE(NULLIF(\"action.target.path\", ''), \
                           NULLIF(\"action.target.selector\", ''))";

/// One clicked element and how its clicks fell.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClickedElement {
    /// What finds it in a recording of the page.
    pub element: String,
    pub name: Option<String>,
    pub selector: Option<String>,
    pub clicks: u64,
    pub sessions: u64,
    /// Clicks that came with a sign of frustration.
    pub frustrated: u64,
    /// Where on the page its clicks landed, on average.
    pub page_x: Option<f64>,
    pub page_y: Option<f64>,
    /// The clicks by the cell of the element they landed in: `[x, y, clicks]`, cells counted from
    /// the top left corner. Clicks of rows that do not say are not in them.
    pub cells: Vec<[u64; 3]>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct ClickMap {
    pub clicks: u64,
    pub elements: Vec<ClickedElement>,
}

/// The elements with the most clicks, and the same rows by cell.
pub fn clicks_sql(scope: &Scope<'_>, limit: usize) -> (String, String) {
    let relation = Relation::of(
        scope,
        &[
            "action.target.path",
            "action.target.selector",
            "action.target.name",
            "action.frustration.count",
            "action.page_position.x",
            "action.page_position.y",
            "session.id",
        ],
    );

    let extra = [format!("{ELEMENT_KEY} IS NOT NULL")];

    let elements = format!(
        "{}SELECT {ELEMENT_KEY} AS \"{ELEMENT}\", MAX(\"action.target.name\") AS \"{NAME}\", \
         MAX(\"action.target.selector\") AS \"{SELECTOR}\", COUNT(*) AS \"{CLICKS}\", \
         COUNT(DISTINCT \"session.id\") AS \"{SESSIONS}\", \
         COUNT(CASE WHEN \"action.frustration.count\" > 0 THEN 1 END) AS \"{FRUSTRATED}\", \
         AVG(\"action.page_position.x\") AS \"{PAGE_X}\", \
         AVG(\"action.page_position.y\") AS \"{PAGE_Y}\" \
         FROM {} WHERE {} GROUP BY 1 ORDER BY \"{CLICKS}\" DESC, 1 ASC LIMIT {limit}",
        relation.with,
        relation.from,
        relation.where_clause(&extra),
    );

    let in_cell = |offset: &str, size: &str| {
        format!(
            "CASE WHEN {size} > 0 THEN LEAST({max}, GREATEST(0, CAST(FLOOR(CAST({offset} AS DOUBLE) \
             * {CELLS} / {size}) AS BIGINT))) END",
            offset = ident(offset),
            size = ident(size),
            max = CELLS - 1,
        )
    };

    let relation = Relation::of(
        scope,
        &[
            "action.target.path",
            "action.target.selector",
            "action.target.offset.x",
            "action.target.offset.y",
            "action.target.width",
            "action.target.height",
        ],
    );

    let extra = [
        format!("{ELEMENT_KEY} IS NOT NULL"),
        "\"action.target.offset.x\" IS NOT NULL".to_string(),
        "\"action.target.offset.y\" IS NOT NULL".to_string(),
    ];

    let cells = format!(
        "{}SELECT {ELEMENT_KEY} AS \"{ELEMENT}\", {} AS \"{CELL_X}\", {} AS \"{CELL_Y}\", \
         COUNT(*) AS \"{CLICKS}\" FROM {} WHERE {} GROUP BY 1, 2, 3 \
         ORDER BY \"{CLICKS}\" DESC LIMIT {}",
        relation.with,
        in_cell("action.target.offset.x", "action.target.width"),
        in_cell("action.target.offset.y", "action.target.height"),
        relation.from,
        relation.where_clause(&extra),
        limit.saturating_mul((CELLS * CELLS) as usize),
    );

    (elements, cells)
}

struct Columns<'a>(&'a [String]);

impl Columns<'_> {
    fn at(&self, name: &str) -> Result<usize> {
        self.0
            .iter()
            .position(|column| column == name)
            .ok_or_else(|| Error::internal(format!("a heatmap statement returned no `{name}`")))
    }
}

fn whole(value: Option<&Value>) -> u64 {
    value
        .and_then(|value| {
            value
                .as_u64()
                .or_else(|| value.as_f64().map(|number| number.max(0.0) as u64))
        })
        .unwrap_or_default()
}

fn words(value: Option<&Value>) -> Option<String> {
    value
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_string)
}

/// What the two statements of [`clicks_sql`] came to.
pub fn read_clicks(
    elements: (&[String], &[Vec<Value>]),
    cells: (&[String], &[Vec<Value>]),
) -> Result<ClickMap> {
    if elements.1.is_empty() {
        return Ok(ClickMap::default());
    }

    let columns = Columns(elements.0);
    let element = columns.at(ELEMENT)?;
    let name = columns.at(NAME)?;
    let selector = columns.at(SELECTOR)?;
    let clicks = columns.at(CLICKS)?;
    let sessions = columns.at(SESSIONS)?;
    let frustrated = columns.at(FRUSTRATED)?;
    let page_x = columns.at(PAGE_X)?;
    let page_y = columns.at(PAGE_Y)?;

    let mut found = elements
        .1
        .iter()
        .filter_map(|row| {
            Some(ClickedElement {
                element: words(row.get(element))?,
                name: words(row.get(name)),
                selector: words(row.get(selector)),
                clicks: whole(row.get(clicks)),
                sessions: whole(row.get(sessions)),
                frustrated: whole(row.get(frustrated)),
                page_x: row.get(page_x).and_then(Value::as_f64),
                page_y: row.get(page_y).and_then(Value::as_f64),
                cells: Vec::new(),
            })
        })
        .collect::<Vec<_>>();

    if !cells.1.is_empty() {
        let columns = Columns(cells.0);
        let of_element = columns.at(ELEMENT)?;
        let x = columns.at(CELL_X)?;
        let y = columns.at(CELL_Y)?;
        let clicks = columns.at(CLICKS)?;

        for row in cells.1 {
            let Some(named) = words(row.get(of_element)) else {
                continue;
            };

            // A row with no size has no cell: the element is drawn by its count alone.
            let (Some(x), Some(y)) = (
                row.get(x).filter(|value| !value.is_null()),
                row.get(y).filter(|value| !value.is_null()),
            ) else {
                continue;
            };

            if let Some(element) = found.iter_mut().find(|element| element.element == named) {
                element
                    .cells
                    .push([whole(Some(x)), whole(Some(y)), whole(row.get(clicks))]);
            }
        }
    }

    Ok(ClickMap {
        clicks: found.iter().map(|element| element.clicks).sum(),
        elements: found,
    })
}

/// One depth of a page and how many views reached it.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct ScrollBand {
    /// CSS pixels from the top of the page to the top of the band.
    pub depth: u64,
    /// The share of the views that saw it, from 0 to 1.
    pub reached: f64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScrollMap {
    pub views: u64,
    /// How long the page was: the median of the longest each view saw.
    pub page_height: Option<f64>,
    /// How far half the views got.
    pub median_depth: Option<u64>,
    pub bands: Vec<ScrollBand>,
}

/// How many views stopped in each band of depth, with how long the page was.
pub fn scroll_sql(scope: &Scope<'_>) -> String {
    let relation = Relation::of(
        scope,
        &["view.scroll.max_depth", "view.scroll.max_scroll_height"],
    );

    let extra = ["\"view.scroll.max_depth\" IS NOT NULL".to_string()];

    format!(
        "{}SELECT LEAST({last}, CAST(FLOOR(\"view.scroll.max_depth\" / {BAND_PX}) AS BIGINT)) AS \"{BAND}\", \
         COUNT(*) AS \"{VIEWS}\", \
         approx_percentile_cont(0.5) WITHIN GROUP (ORDER BY \"view.scroll.max_scroll_height\") AS \"{HEIGHT}\" \
         FROM {} WHERE {} GROUP BY 1 ORDER BY 1 ASC",
        relation.with,
        relation.from,
        relation.where_clause(&extra),
        last = MAX_BANDS - 1,
    )
}

/// What [`scroll_sql`] came to: a view that stopped in a band saw every band above it.
pub fn read_scroll(columns: &[String], rows: &[Vec<Value>]) -> Result<ScrollMap> {
    if rows.is_empty() {
        return Ok(ScrollMap::default());
    }

    let columns = Columns(columns);
    let band = columns.at(BAND)?;
    let views = columns.at(VIEWS)?;
    let height = columns.at(HEIGHT)?;

    let stopped = rows
        .iter()
        .map(|row| {
            (
                whole(row.get(band)),
                whole(row.get(views)),
                row.get(height).and_then(Value::as_f64),
            )
        })
        .collect::<Vec<_>>();

    let total = stopped.iter().map(|(_, views, _)| views).sum::<u64>();

    if total == 0 {
        return Ok(ScrollMap::default());
    }

    let last = stopped.iter().map(|(band, ..)| *band).max().unwrap_or(0);
    let mut seen_below = total;
    let mut bands = Vec::with_capacity(usize::try_from(last + 1).unwrap_or_default());
    let mut median_depth = None;

    for index in 0..=last {
        let reached = seen_below as f64 / total as f64;

        bands.push(ScrollBand {
            depth: index * BAND_PX as u64,
            reached,
        });

        if reached >= 0.5 {
            median_depth = Some((index + 1) * BAND_PX as u64);
        }

        seen_below -= stopped
            .iter()
            .filter(|(band, ..)| *band == index)
            .map(|(_, views, _)| views)
            .sum::<u64>();
    }

    // The bands where most views stopped say best how long the page was.
    let page_height = stopped
        .iter()
        .max_by_key(|(_, views, _)| *views)
        .and_then(|(.., height)| *height);

    Ok(ScrollMap {
        views: total,
        page_height,
        median_depth,
        bands,
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::domain::rum::event::RumStream;

    fn names(names: &[&str]) -> Vec<String> {
        names.iter().map(|name| (*name).to_string()).collect()
    }

    #[test]
    fn the_statements_read_the_rows_of_the_scope() {
        let scope = Scope {
            stream: RumStream::Actions,
            from: 10,
            to: 20,
            conditions: &[],
        };

        let (elements, cells) = clicks_sql(&scope, 50);

        assert!(elements.contains("LIMIT 50"), "{elements}");
        assert!(elements.contains("FROM \"rum_actions\""), "{elements}");
        assert!(cells.contains("LIMIT 5000"), "{cells}");
        assert!(cells.contains("GROUP BY 1, 2, 3"), "{cells}");
    }

    #[test]
    fn the_clicks_of_an_element_fall_in_its_cells() {
        let map = read_clicks(
            (
                &names(&[
                    ELEMENT, NAME, SELECTOR, CLICKS, SESSIONS, FRUSTRATED, PAGE_X, PAGE_Y,
                ]),
                &[
                    vec![
                        json!("body>main>button"),
                        json!("Pay"),
                        json!("button.pay"),
                        json!(9),
                        json!(4),
                        json!(2),
                        json!(640.5),
                        json!(1200.0),
                    ],
                    vec![
                        json!("body>a"),
                        Value::Null,
                        Value::Null,
                        json!(1),
                        json!(1),
                        json!(0),
                        Value::Null,
                        Value::Null,
                    ],
                ],
            ),
            (
                &names(&[ELEMENT, CELL_X, CELL_Y, CLICKS]),
                &[
                    vec![json!("body>main>button"), json!(5), json!(4), json!(7)],
                    vec![json!("body>main>button"), json!(9), json!(0), json!(2)],
                    vec![json!("body>a"), Value::Null, Value::Null, json!(1)],
                    vec![json!("gone"), json!(1), json!(1), json!(1)],
                ],
            ),
        )
        .unwrap();

        assert_eq!(map.clicks, 10);
        assert_eq!(map.elements[0].cells, [[5, 4, 7], [9, 0, 2]]);
        assert_eq!(map.elements[0].frustrated, 2);
        assert_eq!(map.elements[0].page_x, Some(640.5));
        assert!(map.elements[1].cells.is_empty());
    }

    #[test]
    fn no_row_is_an_empty_map() {
        assert_eq!(
            read_clicks((&[], &[]), (&[], &[])).unwrap(),
            ClickMap::default()
        );
        assert_eq!(read_scroll(&[], &[]).unwrap(), ScrollMap::default());
    }

    #[test]
    fn a_view_saw_every_band_above_where_it_stopped() {
        let map = read_scroll(
            &names(&[BAND, VIEWS, HEIGHT]),
            &[
                vec![json!(0), json!(2), json!(3200.0)],
                vec![json!(2), json!(6), json!(3000.0)],
                vec![json!(5), json!(2), json!(3400.0)],
            ],
        )
        .unwrap();

        assert_eq!(map.views, 10);
        assert_eq!(map.page_height, Some(3000.0));
        assert_eq!(map.median_depth, Some(300));

        let reached = map
            .bands
            .iter()
            .map(|band| band.reached)
            .collect::<Vec<_>>();

        assert_eq!(reached, [1.0, 0.8, 0.8, 0.2, 0.2, 0.2]);
        assert_eq!(map.bands[3].depth, 300);
    }
}

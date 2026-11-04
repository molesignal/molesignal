// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The panels of the dashboards the product installs: one SQL query on one stream, shown as a
//! number, a trend, a table or bars.

use serde_json::{Map, Value, json};

/// Text that is good, to be watched, and poor, in the inks of the interface.
pub(super) const GOOD: &str = "var(--green-ink)";
pub(super) const WATCH: &str = "var(--yellow-ink)";
pub(super) const POOR: &str = "var(--red-ink)";

/// Where a panel sits in the 24 columns of the grid, in columns and rows.
#[derive(Debug, Clone, Copy)]
pub(super) struct At {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

pub(super) const fn at(x: u32, y: u32, w: u32, h: u32) -> At {
    At { x, y, w, h }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Shape {
    Number,
    Trend,
    Table,
    Bars,
}

/// One step of the colours a value goes through: from `from` up, the value is `color`.
#[derive(Debug, Clone, Copy)]
pub(super) struct Step {
    pub from: Option<f64>,
    pub color: &'static str,
    pub label: Option<&'static str>,
}

pub(super) const fn step(from: Option<f64>, color: &'static str) -> Step {
    Step {
        from,
        color,
        label: None,
    }
}

pub(super) const fn line(from: f64, color: &'static str, label: &'static str) -> Step {
    Step {
        from: Some(from),
        color,
        label: Some(label),
    }
}

pub(super) struct Panel {
    id: &'static str,
    title: &'static str,
    description: &'static str,
    at: At,
    shape: Shape,
    stream: &'static str,
    statement: String,
    field: Map<String, Value>,
    overrides: Vec<Value>,
    options: Map<String, Value>,
    limit: u32,
}

impl Panel {
    fn new(
        shape: Shape,
        id: &'static str,
        title: &'static str,
        at: At,
        stream: &'static str,
        statement: String,
    ) -> Self {
        Self {
            id,
            title,
            description: "",
            at,
            shape,
            stream,
            statement,
            field: Map::new(),
            overrides: Vec::new(),
            options: Map::new(),
            limit: 1000,
        }
    }

    /// A single number, as the title says what it is.
    pub fn number(
        id: &'static str,
        title: &'static str,
        at: At,
        stream: &'static str,
        statement: String,
    ) -> Self {
        let mut panel = Self::new(Shape::Number, id, title, at, stream, statement);

        panel.limit = 1;
        panel.options.insert("calculation".into(), json!("last"));
        panel.options.insert("textMode".into(), json!("value"));
        panel.options.insert("graphMode".into(), json!("none"));
        panel.options.insert("colorMode".into(), json!("none"));

        panel
    }

    /// Values over time: the statement answers a `time` column and a column per line.
    pub fn trend(
        id: &'static str,
        title: &'static str,
        at: At,
        stream: &'static str,
        statement: String,
    ) -> Self {
        let mut panel = Self::new(Shape::Trend, id, title, at, stream, statement);

        panel.limit = 5000;
        panel.field.insert("min".into(), json!(0));
        panel.options.insert("drawStyle".into(), json!("line"));
        panel
            .options
            .insert("lineInterpolation".into(), json!("linear"));
        panel.options.insert("lineWidth".into(), json!(1.5));
        panel.options.insert("fillOpacity".into(), json!(0));
        panel.options.insert("showPoints".into(), json!("never"));
        panel.options.insert("stackMode".into(), json!("none"));
        panel.options.insert("tooltipMode".into(), json!("all"));
        panel.options.insert("legendMode".into(), json!("hidden"));
        panel
            .options
            .insert("legendPlacement".into(), json!("bottom"));

        panel
    }

    pub fn table(
        id: &'static str,
        title: &'static str,
        at: At,
        stream: &'static str,
        statement: String,
    ) -> Self {
        let mut panel = Self::new(Shape::Table, id, title, at, stream, statement);

        panel.limit = 100;
        panel.options.insert("showHeader".into(), json!(true));
        panel.options.insert("striped".into(), json!(false));
        panel.options.insert("pageSize".into(), json!(10));

        panel
    }

    /// A bar for each row, named by the first column and as long as the second.
    pub fn bars(
        id: &'static str,
        title: &'static str,
        at: At,
        stream: &'static str,
        statement: String,
    ) -> Self {
        let mut panel = Self::new(Shape::Bars, id, title, at, stream, statement);

        panel.limit = 100;
        panel
            .options
            .insert("orientation".into(), json!("horizontal"));
        panel.options.insert("groupWidth".into(), json!(0.7));
        panel.options.insert("calculation".into(), json!("last"));
        panel.options.insert("showValues".into(), json!("always"));

        panel
    }

    pub fn about(mut self, description: &'static str) -> Self {
        self.description = description;
        self
    }

    /// The unit the values are written in, and how many decimals they keep.
    pub fn unit(mut self, unit: &str, decimals: u8) -> Self {
        self.field.insert("unit".into(), json!(unit));
        self.field.insert("decimals".into(), json!(decimals));
        self
    }

    /// A trend that shows its lines: one name for each, in a list under the chart.
    pub fn legend(mut self) -> Self {
        self.options.insert("legendMode".into(), json!("list"));
        self.options.insert("legendStats".into(), json!([]));
        self
    }

    /// Where the values stand: a number takes the colour of its step, a trend draws the lines
    /// between them.
    pub fn graded(mut self, steps: &[Step]) -> Self {
        let steps = steps
            .iter()
            .map(|step| {
                let mut value = json!({ "value": step.from, "color": step.color });

                if let (Some(label), Some(object)) = (step.label, value.as_object_mut()) {
                    object.insert("label".into(), json!(label));
                }

                value
            })
            .collect::<Vec<_>>();

        self.field.insert(
            "thresholds".into(),
            json!({ "mode": "absolute", "steps": steps }),
        );

        if self.shape == Shape::Number {
            self.options.insert("colorMode".into(), json!("value"));
        }

        self
    }

    /// How a column of a table is written, by its name.
    pub fn column(mut self, name: &str, unit: &str, decimals: u8) -> Self {
        let id = format!("{}-{}", self.id, self.overrides.len() + 1);

        self.overrides.push(json!({
            "id": id,
            "matcher": { "type": "field_name", "value": name },
            "properties": [
                { "id": "unit", "value": unit },
                { "id": "decimals", "value": decimals },
            ],
        }));

        self
    }

    pub fn build(self) -> Value {
        let visualization = match self.shape {
            Shape::Number => "stat",
            Shape::Trend => "time_series",
            Shape::Table => "table",
            Shape::Bars => "bar_chart",
        };

        let mut query = json!({
            "refId": "A",
            "enabled": true,
            "dataSourceType": "sql",
            "query": {
                "language": "sql",
                "streamName": self.stream,
                "streamType": "logs",
                "statement": self.statement,
                "limit": self.limit,
            },
        });

        if self.shape == Shape::Trend
            && let Some(object) = query.as_object_mut()
        {
            object.insert("format".into(), json!("time_series"));
        }

        let mut field = self.field;

        field.entry("noValue").or_insert_with(|| json!("暂无数据"));

        json!({
            "kind": "panel",
            "id": self.id,
            "title": self.title,
            "description": self.description,
            "gridPos": { "x": self.at.x, "y": self.at.y, "w": self.at.w, "h": self.at.h },
            "queryOptions": {},
            "queries": [query],
            "transformations": [],
            "visualization": {
                "type": visualization,
                "schemaVersion": 1,
                "options": self.options,
            },
            "fieldConfig": field,
            "overrides": self.overrides,
            "links": [],
        })
    }
}

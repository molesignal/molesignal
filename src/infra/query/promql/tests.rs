// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use super::*;

fn ls(pairs: &[(&str, &str)]) -> LabelSet {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

fn scalar_vector(value: f64) -> InstantVector {
    InstantVector {
        items: vec![(LabelSet::new(), value)],
    }
}

fn binary_expr(query: &str) -> BinaryExpr {
    match parser::parse(query).unwrap() {
        Expr::Binary(binary) => binary,
        other => panic!("expected binary expression, got {other:?}"),
    }
}

fn call_expr(query: &str) -> Call {
    match parser::parse(query).unwrap() {
        Expr::Call(call) => call,
        other => panic!("expected call expression, got {other:?}"),
    }
}

#[test]
fn batches_to_series_accepts_int64_value_column() {
    use std::sync::Arc;

    use arrow::{
        array::Int64Array,
        datatypes::{DataType, Field, Schema, TimeUnit},
    };

    // 整数值 metric（JSON ingest `value: 1` 推断成 Int64）也要能出 series。
    let schema = Arc::new(Schema::new(vec![
        Field::new(
            "_timestamp",
            DataType::Timestamp(TimeUnit::Microsecond, None),
            false,
        ),
        Field::new("value", DataType::Int64, false),
        Field::new("host", DataType::Utf8, true),
    ]));
    let ts = TimestampMicrosecondArray::from(vec![10_000_000_i64, 20_000_000]);
    let val = Int64Array::from(vec![5_i64, 7]);
    let host = StringArray::from(vec!["a", "a"]);
    let batch =
        RecordBatch::try_new(schema, vec![Arc::new(ts), Arc::new(val), Arc::new(host)]).unwrap();

    let series = batches_to_series(
        &[batch],
        &Matchers::empty(),
        None,
        0,
        30_000_000,
        usize::MAX,
    )
    .expect("under sample cap");
    assert_eq!(
        series.len(),
        1,
        "Int64 value column must still produce a series"
    );
    assert_eq!(series[0].labels.get("host").map(String::as_str), Some("a"));
    assert_eq!(
        series[0].samples,
        vec![(10_000_000, 5.0), (20_000_000, 7.0)]
    );
}

#[test]
fn batches_to_series_keeps_last_value_for_duplicate_timestamp() {
    use std::sync::Arc;

    use arrow::{
        array::Float64Array,
        datatypes::{DataType, Field, Schema, TimeUnit},
    };

    let schema = Arc::new(Schema::new(vec![
        Field::new(
            "_timestamp",
            DataType::Timestamp(TimeUnit::Microsecond, None),
            false,
        ),
        Field::new("value", DataType::Float64, false),
        Field::new("host", DataType::Utf8, true),
    ]));
    let batch = RecordBatch::try_new(
        schema,
        vec![
            Arc::new(TimestampMicrosecondArray::from(vec![
                10_000_000_i64,
                10_000_000,
                20_000_000,
            ])),
            Arc::new(Float64Array::from(vec![5.0, 7.0, 9.0])),
            Arc::new(StringArray::from(vec!["a", "a", "a"])),
        ],
    )
    .unwrap();

    let series = batches_to_series(
        &[batch],
        &Matchers::empty(),
        None,
        0,
        30_000_000,
        usize::MAX,
    )
    .expect("under sample cap");
    assert_eq!(
        series[0].samples,
        vec![(10_000_000, 7.0), (20_000_000, 9.0)]
    );
}

#[test]
fn batches_to_series_filters_container_metric_and_hides_storage_identity() {
    use std::sync::Arc;

    use arrow::datatypes::{DataType, Field, Schema, TimeUnit};

    let schema = Arc::new(Schema::new(vec![
        Field::new(
            "_timestamp",
            DataType::Timestamp(TimeUnit::Microsecond, None),
            false,
        ),
        Field::new("value", DataType::Float64, false),
        Field::new("metric_name", DataType::Utf8, false),
        Field::new("metric_kind", DataType::Utf8, false),
        Field::new("node.id", DataType::Utf8, false),
    ]));
    let batch = RecordBatch::try_new(
        schema,
        vec![
            Arc::new(TimestampMicrosecondArray::from(vec![
                10_000_000_i64,
                10_000_000,
                20_000_000,
            ])),
            Arc::new(Float64Array::from(vec![1.0, 99.0, 2.0])),
            Arc::new(StringArray::from(vec![
                "requests_total",
                "queue_depth",
                "requests_total",
            ])),
            Arc::new(StringArray::from(vec!["counter", "gauge", "counter"])),
            Arc::new(StringArray::from(vec!["node-1", "node-1", "node-1"])),
        ],
    )
    .unwrap();

    let series = batches_to_series(
        &[batch],
        &Matchers::empty(),
        Some("requests_total"),
        0,
        30_000_000,
        usize::MAX,
    )
    .unwrap();

    assert_eq!(series.len(), 1);
    assert_eq!(
        series[0].samples,
        vec![(10_000_000, 1.0), (20_000_000, 2.0)]
    );
    assert_eq!(series[0].labels, ls(&[("node.id", "node-1")]));
    assert!(!series[0].labels.contains_key("metric_name"));
    assert!(!series[0].labels.contains_key("metric_kind"));
}

#[tokio::test]
async fn missing_system_metric_resolves_to_protected_container_stream() {
    use crate::{
        domain::stream::{
            FieldDef, FieldType, Schema as StreamSchema, StreamDefinition, StreamSettings,
        },
        shared::{ids::Id, time::TimestampMicros},
    };

    struct SystemStreams;

    #[async_trait]
    impl StreamRepository for SystemStreams {
        async fn create(&self, definition: StreamDefinition) -> Result<StreamDefinition> {
            Ok(definition)
        }

        async fn update_schema(&self, _id: &Id, _schema: StreamSchema) -> Result<()> {
            Ok(())
        }

        async fn get(
            &self,
            org_id: &Id,
            name: &str,
            stream_type: StreamType,
        ) -> Result<StreamDefinition> {
            if name != "_molesignal" || stream_type != StreamType::Metrics {
                return Err(Error::not_found(format!("stream {name}")));
            }
            Ok(StreamDefinition {
                id: Id::from_string("system-metrics"),
                org_id: org_id.clone(),
                name: name.into(),
                stream_type,
                schema: StreamSchema {
                    fields: vec![FieldDef {
                        name: "value".into(),
                        data_type: FieldType::Float64,
                        nullable: false,
                        indexed: false,
                        encrypted: false,
                        exact: false,
                    }],
                },
                retention: None,
                created_at: TimestampMicros(1),
                updated_at: TimestampMicros(1),
            })
        }

        async fn list(&self, _org_id: &Id) -> Result<Vec<StreamDefinition>> {
            Ok(Vec::new())
        }

        async fn get_settings(&self, _id: &Id) -> Result<StreamSettings> {
            Ok(StreamSettings::default())
        }

        async fn delete(&self, _id: &Id) -> Result<()> {
            Ok(())
        }
    }

    struct EmptyFiles;

    #[async_trait]
    impl ParquetFileMetaRepository for EmptyFiles {
        async fn insert(&self, _file: crate::domain::storage::ParquetFileMeta) -> Result<()> {
            Ok(())
        }

        async fn find(
            &self,
            _org_id: &Id,
            _stream: &str,
            _stream_type: StreamType,
            _time_range: TimeRange,
        ) -> Result<Vec<crate::domain::storage::ParquetFileMeta>> {
            Ok(Vec::new())
        }

        async fn replace(
            &self,
            _merged_ids: &[Id],
            _new_files: Vec<crate::domain::storage::ParquetFileMeta>,
        ) -> Result<()> {
            Ok(())
        }

        async fn mark_deleted(&self, _ids: &[Id]) -> Result<usize> {
            Ok(0)
        }
    }

    let engine = PromQLEngine::new(
        Arc::new(EmptyFiles),
        Arc::new(object_store::local::LocalFileSystem::new()),
    )
    .with_streams(Arc::new(SystemStreams));

    let source = engine
        .resolve_metric_source(&Id::from_string("_sys"), "requests_total")
        .await
        .unwrap();

    assert_eq!(source.stream, "_molesignal");
    assert_eq!(source.logical_metric.as_deref(), Some("requests_total"));
}

#[test]
fn rate_basic_two_points() {
    let series = vec![Series {
        labels: ls(&[("method", "GET")]),
        samples: vec![(0, 0.0), (60_000_000, 60.0)], // 60 in 60s
    }];
    let r = apply_rate_like("rate", series, Duration::from_secs(60));
    assert_eq!(r.items.len(), 1);
    let v = r.items[0].1;
    assert!((v - 1.0).abs() < 1e-9, "rate must be 1.0/s, got {v}");
}

#[test]
fn rate_and_increase_handle_counter_resets() {
    let samples = vec![
        (0, 100.0),
        (30_000_000, 110.0),
        (60_000_000, 5.0),
        (90_000_000, 10.0),
    ];
    let rate = apply_rate_like(
        "rate",
        vec![Series {
            labels: ls(&[("service", "checkout")]),
            samples: samples.clone(),
        }],
        Duration::from_secs(100),
    );
    let increase = apply_rate_like(
        "increase",
        vec![Series {
            labels: ls(&[("service", "checkout")]),
            samples,
        }],
        Duration::from_secs(100),
    );

    assert!((rate.items[0].1 - 0.2).abs() < 1e-9);
    assert!((increase.items[0].1 - 20.0).abs() < 1e-9);
}

#[test]
fn irate_uses_last_two_points() {
    let series = vec![Series {
        labels: ls(&[("method", "GET")]),
        samples: vec![(0, 0.0), (60_000_000, 30.0), (120_000_000, 90.0)],
    }];
    let r = apply_rate_like("irate", series, Duration::from_secs(120));
    assert_eq!(r.items.len(), 1);
    let v = r.items[0].1;
    assert!((v - 1.0).abs() < 1e-9, "irate must be 1.0/s, got {v}");
}

#[test]
fn irate_uses_post_reset_value_instead_of_negative_delta() {
    let series = vec![Series {
        labels: ls(&[("method", "GET")]),
        samples: vec![(0, 90.0), (60_000_000, 5.0)],
    }];
    let result = apply_rate_like("irate", series, Duration::from_secs(60));
    assert_eq!(result.items.len(), 1);
    assert!((result.items[0].1 - (5.0 / 60.0)).abs() < 1e-9);
}

#[test]
fn over_time_basic_aggregations() {
    let samples = vec![(0, 10.0), (60_000_000, 20.0), (120_000_000, 30.0)];
    assert_eq!(
        apply_over_time_value("sum_over_time", None, &samples),
        Some(60.0)
    );
    assert_eq!(
        apply_over_time_value("avg_over_time", None, &samples),
        Some(20.0)
    );
    assert_eq!(
        apply_over_time_value("min_over_time", None, &samples),
        Some(10.0)
    );
    assert_eq!(
        apply_over_time_value("max_over_time", None, &samples),
        Some(30.0)
    );
    assert_eq!(
        apply_over_time_value("count_over_time", None, &samples),
        Some(3.0)
    );
    assert_eq!(
        apply_over_time_value("last_over_time", None, &samples),
        Some(30.0)
    );
    assert_eq!(
        apply_over_time_value("present_over_time", None, &samples),
        Some(1.0)
    );
}

#[test]
fn quantile_over_time_interpolates() {
    let samples = vec![(0, 10.0), (1, 20.0), (2, 30.0), (3, 40.0)];
    let value = apply_over_time_value("quantile_over_time", Some(0.5), &samples).unwrap();
    assert!((value - 25.0).abs() < 1e-9, "got {value}");
}

#[test]
fn stddev_stdvar_and_mad_over_time() {
    let std_samples = vec![
        (0, 2.0),
        (1, 4.0),
        (2, 4.0),
        (3, 4.0),
        (4, 5.0),
        (5, 5.0),
        (6, 7.0),
        (7, 9.0),
    ];
    let stddev = apply_over_time_value("stddev_over_time", None, &std_samples).unwrap();
    let stdvar = apply_over_time_value("stdvar_over_time", None, &std_samples).unwrap();
    assert!((stddev - 2.0).abs() < 1e-9, "got {stddev}");
    assert!((stdvar - 4.0).abs() < 1e-9, "got {stdvar}");

    let mad_samples = vec![
        (0, 1.0),
        (1, 1.0),
        (2, 2.0),
        (3, 2.0),
        (4, 4.0),
        (5, 6.0),
        (6, 9.0),
    ];
    let mad = apply_over_time_value("mad_over_time", None, &mad_samples).unwrap();
    assert!((mad - 1.0).abs() < 1e-9, "got {mad}");
}

#[test]
fn avg_over_time_range_uses_trailing_window() {
    let series = vec![Series {
        labels: ls(&[("host", "a")]),
        samples: vec![(0, 10.0), (60_000_000, 20.0), (120_000_000, 30.0)],
    }];
    let range = apply_over_time_range(
        "avg_over_time",
        None,
        series,
        Duration::from_secs(120),
        60_000_000,
        120_000_000,
        60_000_000,
    );
    assert_eq!(range.points.len(), 2);
    assert_eq!(range.points[0].ts_us, 60_000_000);
    assert!((range.points[0].value - 15.0).abs() < 1e-9);
    assert_eq!(range.points[1].ts_us, 120_000_000);
    assert!((range.points[1].value - 25.0).abs() < 1e-9);
}

#[test]
fn topk_selects_per_group_and_preserves_source_labels() {
    let input = InstantVector {
        items: vec![
            (ls(&[("service", "api"), ("instance", "a")]), 10.0),
            (ls(&[("service", "api"), ("instance", "b")]), 30.0),
            (ls(&[("service", "worker"), ("instance", "c")]), 20.0),
            (ls(&[("service", "worker"), ("instance", "d")]), 5.0),
        ],
    };
    let modifier = LabelModifier::Include(promql_parser::label::Labels::new(vec!["service"]));
    let out = apply_topk_bottomk("topk", 1.0, input, Some(&modifier)).unwrap();
    assert_eq!(
        out.items,
        vec![
            (ls(&[("service", "api"), ("instance", "b")]), 30.0),
            (ls(&[("service", "worker"), ("instance", "c")]), 20.0),
        ]
    );
}

#[test]
fn regular_aggregate_stddev_stdvar_quantile_and_group() {
    let labels = ls(&[("service", "api")]);
    let values = vec![2.0, 4.0, 4.0, 4.0, 5.0, 5.0, 7.0, 9.0];

    let stddev = apply_regular_aggregate("stddev", labels.clone(), values.clone(), None)
        .unwrap()
        .remove(0);
    assert_eq!(stddev.0, labels);
    assert!((stddev.1 - 2.0).abs() < 1e-9, "got {}", stddev.1);

    let stdvar = apply_regular_aggregate("stdvar", labels.clone(), values.clone(), None)
        .unwrap()
        .remove(0);
    assert!((stdvar.1 - 4.0).abs() < 1e-9, "got {}", stdvar.1);

    let quantile = apply_regular_aggregate(
        "quantile",
        labels.clone(),
        vec![10.0, 20.0, 30.0, 40.0],
        Some(&AggregateParam::Scalar(0.5)),
    )
    .unwrap()
    .remove(0);
    assert!((quantile.1 - 25.0).abs() < 1e-9, "got {}", quantile.1);

    let group = apply_regular_aggregate("group", labels, values, None)
        .unwrap()
        .remove(0);
    assert_eq!(group.1, 1.0);
}

#[test]
fn count_values_adds_value_label_and_counts_samples() {
    let out = apply_regular_aggregate(
        "count_values",
        ls(&[("service", "api")]),
        vec![1.0, 2.0, 1.0, f64::INFINITY],
        Some(&AggregateParam::LabelName("value_label".to_string())),
    )
    .unwrap();

    assert_eq!(
        out,
        vec![
            (ls(&[("service", "api"), ("value_label", "+Inf")]), 1.0),
            (ls(&[("service", "api"), ("value_label", "1")]), 2.0),
            (ls(&[("service", "api"), ("value_label", "2")]), 1.0),
        ]
    );
}

#[test]
fn sample_math_functions_transform_instant_vectors() {
    let input = InstantVector {
        items: vec![
            (ls(&[("instance", "a")]), -3.25),
            (ls(&[("instance", "b")]), 4.75),
        ],
    };

    let abs = apply_sample_math("abs", input.clone(), SampleMathParams::None).unwrap();
    assert_eq!(
        abs.items,
        vec![
            (ls(&[("instance", "a")]), 3.25),
            (ls(&[("instance", "b")]), 4.75),
        ]
    );

    let floor = apply_sample_math("floor", input.clone(), SampleMathParams::None).unwrap();
    assert_eq!(
        floor.items,
        vec![
            (ls(&[("instance", "a")]), -4.0),
            (ls(&[("instance", "b")]), 4.0),
        ]
    );

    let sgn = apply_sample_math("sgn", input, SampleMathParams::None).unwrap();
    assert_eq!(
        sgn.items,
        vec![
            (ls(&[("instance", "a")]), -1.0),
            (ls(&[("instance", "b")]), 1.0),
        ]
    );
}

#[test]
fn round_and_clamp_math_functions_use_scalar_params() {
    let input = InstantVector {
        items: vec![
            (ls(&[("instance", "a")]), -1.0),
            (ls(&[("instance", "b")]), 1.24),
            (ls(&[("instance", "c")]), 12.0),
        ],
    };

    let rounded = apply_sample_math(
        "round",
        input.clone(),
        SampleMathParams::Round { nearest: 0.5 },
    )
    .unwrap();
    assert_eq!(
        rounded.items,
        vec![
            (ls(&[("instance", "a")]), -1.0),
            (ls(&[("instance", "b")]), 1.0),
            (ls(&[("instance", "c")]), 12.0),
        ]
    );

    let clamped = apply_sample_math(
        "clamp",
        input.clone(),
        SampleMathParams::Clamp {
            min: 0.0,
            max: 10.0,
        },
    )
    .unwrap();
    assert_eq!(
        clamped.items,
        vec![
            (ls(&[("instance", "a")]), 0.0),
            (ls(&[("instance", "b")]), 1.24),
            (ls(&[("instance", "c")]), 10.0),
        ]
    );

    let empty = apply_sample_math(
        "clamp",
        input.clone(),
        SampleMathParams::Clamp {
            min: 10.0,
            max: 0.0,
        },
    )
    .unwrap();
    assert!(empty.items.is_empty());

    let min = apply_sample_math(
        "clamp_min",
        input.clone(),
        SampleMathParams::ClampMin { min: 0.0 },
    )
    .unwrap();
    assert_eq!(min.items[0].1, 0.0);
    assert_eq!(min.items[1].1, 1.24);
    assert_eq!(min.items[2].1, 12.0);

    let max =
        apply_sample_math("clamp_max", input, SampleMathParams::ClampMax { max: 10.0 }).unwrap();
    assert_eq!(max.items[0].1, -1.0);
    assert_eq!(max.items[1].1, 1.24);
    assert_eq!(max.items[2].1, 10.0);
}

#[test]
fn trigonometric_and_unit_math_functions_map_samples() {
    let sin = sample_math_value("sin", std::f64::consts::PI / 2.0, SampleMathParams::None)
        .unwrap()
        .unwrap();
    assert!((sin - 1.0).abs() < 1e-9, "got {sin}");

    let deg = sample_math_value("deg", std::f64::consts::PI, SampleMathParams::None)
        .unwrap()
        .unwrap();
    assert!((deg - 180.0).abs() < 1e-9, "got {deg}");

    let rad = sample_math_value("rad", 180.0, SampleMathParams::None)
        .unwrap()
        .unwrap();
    assert!((rad - std::f64::consts::PI).abs() < 1e-9, "got {rad}");

    let range = RangeVector {
        points: vec![
            RangePoint {
                ts_us: 1,
                labels: ls(&[("instance", "a")]),
                value: 4.0,
            },
            RangePoint {
                ts_us: 2,
                labels: ls(&[("instance", "a")]),
                value: 9.0,
            },
        ],
    };
    let sqrt = apply_sample_math_range("sqrt", range, SampleMathParams::None).unwrap();
    assert_eq!(sqrt.points[0].value, 2.0);
    assert_eq!(sqrt.points[1].value, 3.0);
}

#[test]
fn scalar_math_calls_are_classified_as_scalars() {
    assert!(expr_is_scalar(&parser::parse("pi()").unwrap()));
    assert!(expr_is_scalar(&parser::parse("time()").unwrap()));
    assert!(expr_is_scalar(
        &parser::parse("scalar(cpu_usage_percent)").unwrap()
    ));
    let binary = binary_expr("cpu_usage_percent / pi()");
    assert!(expr_is_scalar(binary.rhs.as_ref()));
    assert!(!expr_is_scalar(&Expr::Binary(binary)));
    assert!(!expr_is_scalar(
        &parser::parse("abs(cpu_usage_percent)").unwrap()
    ));
}

#[test]
fn sort_functions_order_instant_vectors_by_value() {
    let input = InstantVector {
        items: vec![
            (ls(&[("instance", "c")]), 2.0),
            (ls(&[("instance", "a")]), 1.0),
            (ls(&[("instance", "b")]), 2.0),
        ],
    };

    let sorted = sort_instant_vector(input.clone(), false);
    assert_eq!(
        sorted.items,
        vec![
            (ls(&[("instance", "a")]), 1.0),
            (ls(&[("instance", "b")]), 2.0),
            (ls(&[("instance", "c")]), 2.0),
        ]
    );

    let sorted_desc = sort_instant_vector(input, true);
    assert_eq!(
        sorted_desc.items,
        vec![
            (ls(&[("instance", "b")]), 2.0),
            (ls(&[("instance", "c")]), 2.0),
            (ls(&[("instance", "a")]), 1.0),
        ]
    );
}

#[test]
fn type_functions_convert_scalar_and_vector_values() {
    assert_eq!(
        time_instant_vector(1_500_000).items,
        vec![(LabelSet::new(), 1.5)]
    );
    assert_eq!(vector_from_scalar(7.0).items, vec![(LabelSet::new(), 7.0)]);

    let scalar = scalar_from_vector(InstantVector {
        items: vec![(ls(&[("instance", "a")]), 9.0)],
    });
    assert_eq!(scalar.items, vec![(LabelSet::new(), 9.0)]);

    let not_single = scalar_from_vector(InstantVector {
        items: vec![
            (ls(&[("instance", "a")]), 9.0),
            (ls(&[("instance", "b")]), 10.0),
        ],
    });
    assert_eq!(not_single.items.len(), 1);
    assert!(not_single.items[0].1.is_nan());
}

#[test]
fn bottomk_range_selects_per_timestamp() {
    let input = RangeVector {
        points: vec![
            RangePoint {
                ts_us: 10,
                labels: ls(&[("host", "a")]),
                value: 3.0,
            },
            RangePoint {
                ts_us: 10,
                labels: ls(&[("host", "b")]),
                value: 1.0,
            },
            RangePoint {
                ts_us: 20,
                labels: ls(&[("host", "a")]),
                value: 2.0,
            },
            RangePoint {
                ts_us: 20,
                labels: ls(&[("host", "b")]),
                value: 4.0,
            },
        ],
    };
    let out = apply_topk_bottomk_range("bottomk", 1.0, input, None).unwrap();
    assert_eq!(out.points.len(), 2);
    assert_eq!(out.points[0].ts_us, 10);
    assert_eq!(out.points[0].labels, ls(&[("host", "b")]));
    assert_eq!(out.points[0].value, 1.0);
    assert_eq!(out.points[1].ts_us, 20);
    assert_eq!(out.points[1].labels, ls(&[("host", "a")]));
    assert_eq!(out.points[1].value, 2.0);
}

#[test]
fn label_replace_sets_destination_label_on_match() {
    let call =
        call_expr(r#"label_replace(http_requests_total, "route", "$1", "path", "/api/(.*)")"#);
    let args = label_replace_args(&call.args).unwrap();
    let input = InstantVector {
        items: vec![
            (ls(&[("path", "/api/users")]), 1.0),
            (ls(&[("path", "/health")]), 2.0),
        ],
    };
    let out = apply_label_replace(input, &args);
    assert_eq!(
        out.items,
        vec![
            (ls(&[("path", "/api/users"), ("route", "users")]), 1.0),
            (ls(&[("path", "/health")]), 2.0),
        ]
    );
}

#[test]
fn label_join_sets_destination_from_source_labels() {
    let call = call_expr(
        r#"label_join(http_requests_total, "target", "/", "service", "instance", "missing")"#,
    );
    let args = label_join_args(&call.args).unwrap();
    let input = InstantVector {
        items: vec![
            (ls(&[("service", "api"), ("instance", "a")]), 1.0),
            (ls(&[("service", "worker")]), 2.0),
        ],
    };

    let out = apply_label_join(input, &args);
    assert_eq!(
        out.items,
        vec![
            (
                ls(&[("service", "api"), ("instance", "a"), ("target", "api/a/")]),
                1.0
            ),
            (ls(&[("service", "worker"), ("target", "worker//")]), 2.0),
        ]
    );
}

#[test]
fn label_join_updates_range_labels() {
    let call = call_expr(r#"label_join(cpu_usage_percent, "target", ":", "service", "host")"#);
    let args = label_join_args(&call.args).unwrap();
    let input = RangeVector {
        points: vec![RangePoint {
            ts_us: 10,
            labels: ls(&[("service", "api"), ("host", "h1")]),
            value: 42.0,
        }],
    };

    let out = apply_label_join_range(input, &args);
    assert_eq!(
        out.points[0].labels.get("target").map(String::as_str),
        Some("api:h1")
    );
}

#[test]
fn binary_vector_scalar_arithmetic_scales_values() {
    let binary = binary_expr("cpu_usage_percent / 100");
    let lhs = InstantVector {
        items: vec![(ls(&[("host", "a")]), 42.0), (ls(&[("host", "b")]), 81.0)],
    };
    let out = apply_binary_instant(&binary, lhs, scalar_vector(100.0), false, true).unwrap();
    assert_eq!(out.items.len(), 2);
    assert_eq!(out.items[0].0, ls(&[("host", "a")]));
    assert!((out.items[0].1 - 0.42).abs() < 1e-9);
    assert_eq!(out.items[1].0, ls(&[("host", "b")]));
    assert!((out.items[1].1 - 0.81).abs() < 1e-9);
}

#[test]
fn binary_comparison_filters_vector_without_bool() {
    let binary = binary_expr("cpu_usage_percent > 80");
    let lhs = InstantVector {
        items: vec![(ls(&[("host", "a")]), 42.0), (ls(&[("host", "b")]), 81.0)],
    };
    let out = apply_binary_instant(&binary, lhs, scalar_vector(80.0), false, true).unwrap();
    assert_eq!(out.items, vec![(ls(&[("host", "b")]), 81.0)]);
}

#[test]
fn binary_comparison_bool_keeps_false_samples() {
    let binary = binary_expr("cpu_usage_percent > bool 80");
    let lhs = InstantVector {
        items: vec![(ls(&[("host", "a")]), 42.0), (ls(&[("host", "b")]), 81.0)],
    };
    let out = apply_binary_instant(&binary, lhs, scalar_vector(80.0), false, true).unwrap();
    assert_eq!(
        out.items,
        vec![(ls(&[("host", "a")]), 0.0), (ls(&[("host", "b")]), 1.0),]
    );
}

#[test]
fn binary_vector_vector_aligns_by_full_label_set() {
    let binary = binary_expr("cpu_usage_percent - cpu_request_percent");
    let lhs = InstantVector {
        items: vec![(ls(&[("host", "a")]), 10.0), (ls(&[("host", "b")]), 20.0)],
    };
    let rhs = InstantVector {
        items: vec![(ls(&[("host", "a")]), 3.0), (ls(&[("host", "c")]), 1.0)],
    };
    let out = apply_binary_instant(&binary, lhs, rhs, false, false).unwrap();
    assert_eq!(out.items, vec![(ls(&[("host", "a")]), 7.0)]);
}

#[test]
fn binary_on_matching_and_group_left_copy_labels() {
    // a{host,code} / on(host) b{host} → 1:1，结果 label 集 = 匹配签名 {host}。
    let lhs = vec![(ls(&[("host", "h1"), ("code", "500")]), 24.0)];
    let rhs = vec![(ls(&[("host", "h1")]), 600.0)];
    let out = match_vectors(&binary_expr("a / on(host) b"), lhs, rhs).unwrap();
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].0, ls(&[("host", "h1")]));
    assert!((out[0].1 - 0.04).abs() < 1e-9, "got {}", out[0].1);

    // group_left(team)：保留 many(lhs) 全 label，并从 one(rhs) 拷 team。
    let lhs = vec![(ls(&[("host", "h1"), ("code", "500")]), 24.0)];
    let rhs = vec![(ls(&[("host", "h1"), ("team", "x")]), 600.0)];
    let out = match_vectors(&binary_expr("a / on(host) group_left(team) b"), lhs, rhs).unwrap();
    assert_eq!(out.len(), 1);
    assert_eq!(
        out[0].0,
        ls(&[("host", "h1"), ("code", "500"), ("team", "x")])
    );
    assert!((out[0].1 - 0.04).abs() < 1e-9);
}

#[test]
fn binary_set_operators_intersect_union_and_complement() {
    let lhs = || vec![(ls(&[("h", "a")]), 1.0), (ls(&[("h", "b")]), 2.0)];
    let rhs = || vec![(ls(&[("h", "b")]), 9.0), (ls(&[("h", "c")]), 9.0)];
    let and = match_vectors(&binary_expr("x and y"), lhs(), rhs()).unwrap();
    assert_eq!(and, vec![(ls(&[("h", "b")]), 2.0)]);
    let unless = match_vectors(&binary_expr("x unless y"), lhs(), rhs()).unwrap();
    assert_eq!(unless, vec![(ls(&[("h", "a")]), 1.0)]);
    let or = match_vectors(&binary_expr("x or y"), lhs(), rhs()).unwrap();
    assert_eq!(
        or,
        vec![
            (ls(&[("h", "a")]), 1.0),
            (ls(&[("h", "b")]), 2.0),
            (ls(&[("h", "c")]), 9.0),
        ]
    );
}

#[test]
fn binary_range_scalar_arithmetic_updates_each_point() {
    let binary = binary_expr("cpu_usage_percent * 100");
    let range = RangeVector {
        points: vec![
            RangePoint {
                ts_us: 10,
                labels: ls(&[("host", "a")]),
                value: 0.42,
            },
            RangePoint {
                ts_us: 20,
                labels: ls(&[("host", "a")]),
                value: 0.81,
            },
        ],
    };
    let out = apply_binary_range_scalar(&binary, 100.0, range, false).unwrap();
    assert_eq!(out.points.len(), 2);
    assert!((out.points[0].value - 42.0).abs() < 1e-9);
    assert!((out.points[1].value - 81.0).abs() < 1e-9);
}

#[test]
fn unary_negates_vectors() {
    let out = negate_instant_vector(InstantVector {
        items: vec![(ls(&[("host", "a")]), 42.0)],
    });
    assert_eq!(out.items, vec![(ls(&[("host", "a")]), -42.0)]);
}

#[test]
fn increase_basic() {
    let series = vec![Series {
        labels: ls(&[]),
        samples: vec![(0, 10.0), (60_000_000, 70.0)],
    }];
    let r = apply_rate_like("increase", series, Duration::from_secs(60));
    assert!((r.items[0].1 - 60.0).abs() < 1e-9);
}

#[test]
fn rate_range_emits_timestamped_points() {
    let series = vec![Series {
        labels: ls(&[("method", "GET")]),
        samples: vec![(0, 0.0), (60_000_000, 60.0), (120_000_000, 120.0)],
    }];
    let range = apply_rate_like_range(
        "rate",
        series,
        Duration::from_secs(120),
        0,
        120_000_000,
        60_000_000,
    );
    assert_eq!(range.points.len(), 2);
    assert_eq!(range.points[0].ts_us, 60_000_000);
    assert!((range.points[0].value - 0.5).abs() < 1e-9);

    let result = range_to_query_result(range, Instant::now(), Some(1000));
    assert_eq!(result.columns, vec!["_timestamp", "value", "method"]);
    assert_eq!(result.rows.len(), 2);
}

#[test]
fn range_result_keeps_the_full_window_for_every_series() {
    let mut points = Vec::new();
    for ts_us in [0, 60_000_000, 120_000_000] {
        points.push(RangePoint {
            labels: ls(&[("service", "checkout")]),
            ts_us,
            value: 10.0,
        });
        points.push(RangePoint {
            labels: ls(&[("service", "gateway")]),
            ts_us,
            value: 20.0,
        });
    }

    let result = range_to_query_result(RangeVector { points }, Instant::now(), Some(100));

    assert_eq!(result.rows.len(), 6);
    let timestamps: Vec<i64> = result
        .rows
        .iter()
        .filter_map(|row| row.first()?.as_i64())
        .collect();
    assert_eq!(timestamps.first(), Some(&0));
    assert_eq!(timestamps.last(), Some(&120_000_000));
}

#[test]
fn range_result_applies_global_limit_without_losing_series_time_coverage() {
    let mut points = Vec::new();
    for ts_us in 0..99 {
        points.push(RangePoint {
            labels: ls(&[("service", "checkout")]),
            ts_us,
            value: 10.0,
        });
        points.push(RangePoint {
            labels: ls(&[("service", "gateway")]),
            ts_us,
            value: 20.0,
        });
    }

    let result = range_to_query_result(RangeVector { points }, Instant::now(), Some(100));

    assert_eq!(result.rows.len(), 100);
    for service in ["checkout", "gateway"] {
        let timestamps = result
            .rows
            .iter()
            .filter(|row| row.get(2).and_then(|value| value.as_str()) == Some(service))
            .filter_map(|row| row.first()?.as_i64())
            .collect::<Vec<_>>();
        assert_eq!(timestamps.len(), 50);
        assert_eq!(timestamps.first(), Some(&0));
        assert_eq!(timestamps.last(), Some(&98));
    }
}

#[test]
fn sum_by_method_groups_correctly() {
    let inner = InstantVector {
        items: vec![
            (ls(&[("method", "GET"), ("code", "200")]), 1.0),
            (ls(&[("method", "GET"), ("code", "500")]), 2.0),
            (ls(&[("method", "POST"), ("code", "200")]), 4.0),
        ],
    };
    let modifier = Some(LabelModifier::Include(promql_parser::label::Labels::new(
        vec!["method"],
    )));
    let grouped = group_by(&inner, modifier.as_ref());
    let mut out: HashMap<LabelSet, f64> = grouped
        .into_iter()
        .map(|(k, v)| (k, v.iter().sum()))
        .collect();
    assert_eq!(out.remove(&ls(&[("method", "GET")])), Some(3.0));
    assert_eq!(out.remove(&ls(&[("method", "POST")])), Some(4.0));
}

#[test]
fn histogram_quantile_p95() {
    // 单一 group，le buckets 累计 [10, 30, 60, 100]
    let inner = InstantVector {
        items: vec![
            (ls(&[("le", "0.1")]), 10.0),
            (ls(&[("le", "0.5")]), 30.0),
            (ls(&[("le", "1.0")]), 60.0),
            (ls(&[("le", "+Inf")]), 100.0),
        ],
    };
    let out = apply_histogram_quantile(0.95, inner);
    assert_eq!(out.items.len(), 1);
    // q=0.95 → 95.0 cumulative，落在 (1.0 → +Inf) 之间 → 返回 +Inf bucket 的 le
    assert!(out.items[0].1.is_infinite());
}

#[test]
fn histogram_quantile_interpolates_inside_bucket() {
    // 单一 group，le buckets 累计 [10, 60]
    let inner = InstantVector {
        items: vec![(ls(&[("le", "1.0")]), 10.0), (ls(&[("le", "2.0")]), 60.0)],
    };
    let out = apply_histogram_quantile(0.5, inner); // target = 30
    // prev=(1.0, 10), cur=(2.0, 60)，span=50，frac=(30-10)/50=0.4 → 1.0 + 0.4*(2.0-1.0)=1.4
    assert!(
        (out.items[0].1 - 1.4).abs() < 1e-9,
        "got {}",
        out.items[0].1
    );
}

#[test]
fn histogram_fraction_basic() {
    // le buckets 累计 [10, 30, 60, 100]，total=100。apply_* 消费 InstantVector，故每次重建。
    let mk = || InstantVector {
        items: vec![
            (ls(&[("le", "0.1")]), 10.0),
            (ls(&[("le", "0.5")]), 30.0),
            (ls(&[("le", "1.0")]), 60.0),
            (ls(&[("le", "+Inf")]), 100.0),
        ],
    };
    // 全区间 [0, +Inf) → 1.0
    let all = apply_histogram_fraction(0.0, f64::INFINITY, mk());
    assert!(
        (all.items[0].1 - 1.0).abs() < 1e-9,
        "got {}",
        all.items[0].1
    );
    // [0.1, 1.0] → (60-10)/100 = 0.5（命中桶边界，无插值）
    let mid = apply_histogram_fraction(0.1, 1.0, mk());
    assert!(
        (mid.items[0].1 - 0.5).abs() < 1e-9,
        "got {}",
        mid.items[0].1
    );
    // [0, 0.5] → 30/100 = 0.3
    let low = apply_histogram_fraction(0.0, 0.5, mk());
    assert!(
        (low.items[0].1 - 0.3).abs() < 1e-9,
        "got {}",
        low.items[0].1
    );
}

#[test]
fn histogram_fraction_interpolates_inside_bucket() {
    // le buckets 累计 [10, 60]，total=60
    let inner = InstantVector {
        items: vec![(ls(&[("le", "1.0")]), 10.0), (ls(&[("le", "2.0")]), 60.0)],
    };
    // upper=1.5 落在 (1.0→2.0)：rank=10+0.5*(60-10)=35；fraction=35/60
    let out = apply_histogram_fraction(0.0, 1.5, inner);
    assert!(
        (out.items[0].1 - 35.0 / 60.0).abs() < 1e-9,
        "got {}",
        out.items[0].1
    );
}

#[tokio::test]
async fn unsupported_function_returns_invalid() {
    use crate::{
        domain::query::{QueryLanguage, QueryRequest},
        shared::{ids::Id, time::TimestampMicros},
    };
    struct EmptyFiles;
    #[async_trait]
    impl ParquetFileMetaRepository for EmptyFiles {
        async fn insert(&self, _f: crate::domain::storage::ParquetFileMeta) -> Result<()> {
            Ok(())
        }
        async fn find(
            &self,
            _o: &Id,
            _s: &str,
            _t: StreamType,
            _r: TimeRange,
        ) -> Result<Vec<crate::domain::storage::ParquetFileMeta>> {
            Ok(Vec::new())
        }
        async fn replace(
            &self,
            _ids: &[Id],
            _new: Vec<crate::domain::storage::ParquetFileMeta>,
        ) -> Result<()> {
            Ok(())
        }
        async fn mark_deleted(&self, _ids: &[Id]) -> Result<usize> {
            Ok(0)
        }
    }
    let engine = PromQLEngine::new(
        Arc::new(EmptyFiles),
        Arc::new(object_store::local::LocalFileSystem::new()),
    );
    let req = QueryRequest {
        org_id: Id::from_string("org"),
        language: QueryLanguage::Promql,
        // native-histogram 函数：classic bucket 模型下不适用，仍返「not yet supported」。
        statement: "histogram_count(metric)".into(),
        time_range: TimeRange::new(TimestampMicros(0), TimestampMicros(1_000_000)),
        stream: None,
        limit: None,
        federation_clusters: Vec::new(),
    };
    let err = engine.execute(req).await.unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("not yet supported"),
        "expected unsupported func msg, got: {msg}"
    );
}

#[test]
fn delta_and_idelta_compute_window_differences() {
    let samples = vec![(0, 10.0), (30_000_000, 20.0), (60_000_000, 50.0)];
    let p = RangeFuncParams::default();
    assert_eq!(
        range_vector_value("delta", &samples, 60_000_000, p),
        Some(40.0)
    );
    assert_eq!(
        range_vector_value("idelta", &samples, 60_000_000, p),
        Some(30.0)
    );
    // 单点不足以求差
    assert_eq!(range_vector_value("delta", &samples[..1], 0, p), None);
}

#[test]
fn resets_and_changes_count_transitions() {
    let resets = vec![(0, 10.0), (1, 5.0), (2, 8.0), (3, 2.0)];
    assert_eq!(
        range_vector_value("resets", &resets, 3, RangeFuncParams::default()),
        Some(2.0)
    );
    let changes = vec![(0, 1.0), (1, 1.0), (2, 2.0), (3, 2.0), (4, 3.0)];
    assert_eq!(
        range_vector_value("changes", &changes, 4, RangeFuncParams::default()),
        Some(2.0)
    );
    // 恒定序列：0 次变化、0 次 reset
    let flat = vec![(0, 7.0), (1, 7.0), (2, 7.0)];
    assert_eq!(
        range_vector_value("changes", &flat, 2, RangeFuncParams::default()),
        Some(0.0)
    );
    assert_eq!(
        range_vector_value("resets", &flat, 2, RangeFuncParams::default()),
        Some(0.0)
    );
}

#[test]
fn deriv_and_predict_linear_fit_a_line() {
    // y = 2*x_sec + c，以 at_us=2e6 为原点：x = -2,-1,0 秒。
    let samples = vec![(0, 5.0), (1_000_000, 7.0), (2_000_000, 9.0)];
    let p = RangeFuncParams {
        predict_t: 5.0,
        ..Default::default()
    };
    let slope = range_vector_value("deriv", &samples, 2_000_000, p).unwrap();
    assert!(
        (slope - 2.0).abs() < 1e-9,
        "deriv slope must be 2/s, got {slope}"
    );
    // 原点（at_us）处值为 9，5 秒后外推 = 9 + 2*5 = 19。
    let predicted = range_vector_value("predict_linear", &samples, 2_000_000, p).unwrap();
    assert!(
        (predicted - 19.0).abs() < 1e-9,
        "predict_linear must be 19, got {predicted}"
    );
}

#[test]
fn holt_winters_smooths_to_trend() {
    let samples = vec![(0, 10.0), (1, 20.0), (2, 30.0)];
    let p = RangeFuncParams {
        hw_sf: 0.5,
        hw_tf: 0.5,
        ..Default::default()
    };
    let v = range_vector_value("holt_winters", &samples, 2, p).unwrap();
    assert!(
        (v - 30.0).abs() < 1e-9,
        "holt_winters must track to 30, got {v}"
    );
    // double_exponential_smoothing 是同义名
    let v2 = range_vector_value("double_exponential_smoothing", &samples, 2, p).unwrap();
    assert!((v2 - 30.0).abs() < 1e-9);
}

#[test]
fn apply_range_vector_func_maps_series_to_instant() {
    let series = vec![Series {
        labels: ls(&[("method", "GET")]),
        samples: vec![(0, 10.0), (60_000_000, 40.0)],
    }];
    let out = apply_range_vector_func("delta", series, 60_000_000, RangeFuncParams::default());
    assert_eq!(out.items.len(), 1);
    assert!((out.items[0].1 - 30.0).abs() < 1e-9);
}

#[test]
fn datetime_functions_extract_utc_fields() {
    // 1_700_000_000s = 2023-11-14 22:13:20 UTC（星期二）。
    let ts = 1_700_000_000.0;
    let field = |f: &str| {
        let v = InstantVector {
            items: vec![(LabelSet::new(), ts)],
        };
        apply_datetime(f, v).items[0].1
    };
    assert_eq!(field("minute"), 13.0);
    assert_eq!(field("hour"), 22.0);
    assert_eq!(field("day_of_month"), 14.0);
    assert_eq!(field("month"), 11.0);
    assert_eq!(field("year"), 2023.0);
    assert_eq!(field("day_of_week"), 2.0); // Tuesday
    assert_eq!(field("day_of_year"), 318.0);
    assert_eq!(field("days_in_month"), 30.0); // November
}

#[test]
fn timestamp_returns_seconds_for_instant_and_range() {
    let iv = InstantVector {
        items: vec![(ls(&[("a", "b")]), 999.0)],
    };
    let out = apply_timestamp(iv, 5_000_000);
    assert_eq!(out.items[0].1, 5.0); // 5_000_000us = 5s, sample value ignored

    let rv = RangeVector {
        points: vec![RangePoint {
            ts_us: 7_000_000,
            labels: ls(&[("a", "b")]),
            value: 42.0,
        }],
    };
    let out = apply_timestamp_range(rv);
    assert_eq!(out.points[0].value, 7.0);
}

#[test]
fn sort_by_label_orders_by_label_values() {
    let input = InstantVector {
        items: vec![
            (ls(&[("service", "web"), ("zone", "b")]), 1.0),
            (ls(&[("service", "api"), ("zone", "a")]), 2.0),
            (ls(&[("service", "api"), ("zone", "b")]), 3.0),
        ],
    };
    let asc = sort_instant_vector_by_label(
        input.clone(),
        &["service".to_string(), "zone".to_string()],
        false,
    );
    let order: Vec<f64> = asc.items.iter().map(|(_, v)| *v).collect();
    assert_eq!(order, vec![2.0, 3.0, 1.0]); // (api,a),(api,b),(web,b)

    let desc = sort_instant_vector_by_label(input, &["service".to_string()], true);
    assert_eq!(
        desc.items[0].0.get("service").map(String::as_str),
        Some("web")
    );
}

#[test]
fn absent_returns_one_with_matcher_labels_when_empty() {
    let expr = parser::parse(r#"up{job="api",env="prod"}"#).unwrap();
    let out = absent_vector(&expr, InstantVector::default());
    assert_eq!(out.items.len(), 1);
    assert_eq!(out.items[0].1, 1.0);
    assert_eq!(out.items[0].0.get("job").map(String::as_str), Some("api"));
    assert_eq!(out.items[0].0.get("env").map(String::as_str), Some("prod"));

    let present = InstantVector {
        items: vec![(ls(&[("x", "y")]), 5.0)],
    };
    assert!(absent_vector(&expr, present).items.is_empty());
}

#[test]
fn absent_over_time_signals_when_window_empty() {
    let ms = match parser::parse(r#"up{job="api"}[5m]"#).unwrap() {
        Expr::MatrixSelector(m) => m,
        other => panic!("expected matrix selector, got {other:?}"),
    };
    assert!(absent_over_time_vector(&ms, true).items.is_empty());
    let out = absent_over_time_vector(&ms, false);
    assert_eq!(out.items.len(), 1);
    assert_eq!(out.items[0].0.get("job").map(String::as_str), Some("api"));
}

#[test]
fn binary_range_or_unions_points_per_timestamp() {
    let lhs = RangeVector {
        points: vec![RangePoint {
            ts_us: 10,
            labels: ls(&[("h", "a")]),
            value: 1.0,
        }],
    };
    let rhs = RangeVector {
        points: vec![
            RangePoint {
                ts_us: 10,
                labels: ls(&[("h", "b")]),
                value: 2.0,
            },
            RangePoint {
                ts_us: 20,
                labels: ls(&[("h", "a")]),
                value: 3.0,
            },
        ],
    };
    let out = apply_binary_range(&binary_expr("x or y"), lhs, rhs).unwrap();
    let mut got: Vec<(i64, String, f64)> = out
        .points
        .iter()
        .map(|p| (p.ts_us, p.labels.get("h").unwrap().clone(), p.value))
        .collect();
    got.sort_by_key(|a| (a.0, a.1.clone()));
    assert_eq!(
        got,
        vec![
            (10, "a".to_string(), 1.0),
            (10, "b".to_string(), 2.0),
            (20, "a".to_string(), 3.0),
        ]
    );
}

#[test]
fn binary_group_right_keeps_many_side_labels() {
    let lhs = vec![(ls(&[("host", "h1")]), 2.0)];
    let rhs = vec![
        (ls(&[("host", "h1"), ("zone", "a")]), 5.0),
        (ls(&[("host", "h1"), ("zone", "b")]), 7.0),
    ];
    let out = match_vectors(&binary_expr("a * on(host) group_right b"), lhs, rhs).unwrap();
    let mut got: Vec<(String, f64)> = out
        .iter()
        .map(|(l, v)| (l.get("zone").unwrap().clone(), *v))
        .collect();
    got.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.total_cmp(&b.1)));
    assert_eq!(got, vec![("a".to_string(), 10.0), ("b".to_string(), 14.0)]);
}

#[test]
fn offset_and_at_modifiers_shift_eval_time() {
    use crate::{
        domain::query::{QueryLanguage, QueryRequest},
        shared::{ids::Id, time::TimestampMicros},
    };

    let req = QueryRequest {
        org_id: Id::from_string("org"),
        language: QueryLanguage::Promql,
        statement: String::new(),
        time_range: TimeRange::new(TimestampMicros(1_000_000), TimestampMicros(10_000_000)),
        stream: None,
        limit: None,
        federation_clusters: Vec::new(),
    };
    let vs = |q: &str| match parser::parse(q).unwrap() {
        Expr::VectorSelector(vs) => vs,
        Expr::MatrixSelector(ms) => ms.vs,
        other => panic!("expected selector, got {other:?}"),
    };
    let at = |q: &str| effective_eval_time(&vs(q), 5_000_000, &req);

    assert_eq!(at("m"), 5_000_000); // 无修饰
    assert_eq!(at("m offset 5m"), 5_000_000 - 300_000_000); // 前移 300s
    assert_eq!(at("m @ end()"), 10_000_000);
    assert_eq!(at("m @ start()"), 1_000_000);
    assert_eq!(at("m @ 100.000"), 100_000_000); // 100s 的 unix 时刻
    assert_eq!(at("m @ 100.000 offset 1m"), 40_000_000); // 100s - 60s
    // matrix selector 上的 offset 同样落在 ms.vs。
    assert_eq!(at("m[5m] offset 1m"), 5_000_000 - 60_000_000);
}

#[test]
fn limitk_takes_k_per_group_deterministically() {
    let input = InstantVector {
        items: vec![
            (ls(&[("svc", "a"), ("i", "1")]), 1.0),
            (ls(&[("svc", "a"), ("i", "2")]), 2.0),
            (ls(&[("svc", "a"), ("i", "3")]), 3.0),
            (ls(&[("svc", "b"), ("i", "9")]), 9.0),
        ],
    };
    let modifier = LabelModifier::Include(promql_parser::label::Labels::new(vec!["svc"]));
    let out = apply_limitk(2.0, input.clone(), Some(&modifier)).unwrap();
    assert_eq!(out.items.len(), 3); // svc=a → 2, svc=b → 1
    let kept_a: Vec<String> = out
        .items
        .iter()
        .filter(|(l, _)| l.get("svc").map(String::as_str) == Some("a"))
        .map(|(l, _)| l.get("i").unwrap().clone())
        .collect();
    assert_eq!(kept_a, vec!["1".to_string(), "2".to_string()]); // 确定性：按 label 集取前 2
    assert_eq!(
        apply_limitk(2.0, input, Some(&modifier)).unwrap().items,
        out.items
    );
}

#[test]
fn limit_ratio_partitions_series_complementarily() {
    let mk = || InstantVector {
        items: (0..20)
            .map(|i| {
                let mut l = LabelSet::new();
                l.insert("id".to_string(), i.to_string());
                (l, i as f64)
            })
            .collect(),
    };
    let keep = apply_limit_ratio(0.3, mk()).unwrap();
    let drop = apply_limit_ratio(-0.7, mk()).unwrap();
    // r 与 -(1-r) 划分出互补、不相交的两半。
    assert_eq!(keep.items.len() + drop.items.len(), 20);
    assert_eq!(apply_limit_ratio(0.3, mk()).unwrap().items, keep.items); // 确定性
    assert_eq!(apply_limit_ratio(1.0, mk()).unwrap().items.len(), 20); // 全留
    assert_eq!(apply_limit_ratio(0.0, mk()).unwrap().items.len(), 0); // 全去
    assert!(apply_limit_ratio(1.5, mk()).is_err()); // 越界
}

#[tokio::test]
async fn subquery_steps_inner_expression_over_window() {
    use crate::{
        domain::query::{QueryLanguage, QueryRequest},
        shared::{ids::Id, time::TimestampMicros},
    };

    struct EmptyFiles;
    #[async_trait]
    impl ParquetFileMetaRepository for EmptyFiles {
        async fn insert(&self, _f: crate::domain::storage::ParquetFileMeta) -> Result<()> {
            Ok(())
        }
        async fn find(
            &self,
            _o: &Id,
            _s: &str,
            _t: StreamType,
            _r: TimeRange,
        ) -> Result<Vec<crate::domain::storage::ParquetFileMeta>> {
            Ok(Vec::new())
        }
        async fn replace(
            &self,
            _ids: &[Id],
            _new: Vec<crate::domain::storage::ParquetFileMeta>,
        ) -> Result<()> {
            Ok(())
        }
        async fn mark_deleted(&self, _ids: &[Id]) -> Result<usize> {
            Ok(0)
        }
    }

    let engine = PromQLEngine::new(
        Arc::new(EmptyFiles),
        Arc::new(object_store::local::LocalFileSystem::new()),
    );
    let req = QueryRequest {
        org_id: Id::from_string("org"),
        language: QueryLanguage::Promql,
        statement: String::new(),
        time_range: TimeRange::new(TimestampMicros(0), TimestampMicros(1_000_000_000)),
        stream: None,
        limit: None,
        federation_clusters: Vec::new(),
    };
    // vector(time()) 是无需文件的 instant 向量；子查询逐步把它在窗口内采样成 series。
    let sq = match parser::parse("vector(time())[200s:100s]").unwrap() {
        Expr::Subquery(sq) => sq,
        other => panic!("expected subquery, got {other:?}"),
    };
    let series = engine
        .eval_subquery(&sq, 1_000_000_000, &req)
        .await
        .unwrap();
    assert_eq!(series.len(), 1);
    // window (800s, 1000s]，步进 100s；time() 返回秒值 = 时间戳本身。
    assert_eq!(
        series[0].samples,
        vec![
            (800_000_000, 800.0),
            (900_000_000, 900.0),
            (1_000_000_000, 1000.0),
        ]
    );
}

// =====================================================================
//  Range step 降采样 + matrix 物化（BENCHMARKS hotspot 修复）
// =====================================================================

#[test]
fn each_step_window_matches_bruteforce() {
    let samples: Vec<(i64, f64)> = (0..500).map(|i| (i * 3_000_000, i as f64)).collect();
    let (start, end, step, range) = (10_000_000_i64, 1_400_000_000, 70_000_000, 120_000_000);
    let mut steps = 0usize;
    each_step_window(&samples, start, end, step, range, |t, win| {
        let expect: Vec<(i64, f64)> = samples
            .iter()
            .copied()
            .filter(|&(ts, _)| ts > t - range && ts <= t)
            .collect();
        assert_eq!(win, &expect[..], "window mismatch at t={t}");
        steps += 1;
    });
    assert_eq!(steps, ((end - start) / step + 1) as usize);
}

#[test]
fn rate_range_output_bounded_by_step_not_sample_density() {
    // 3600 个密集样本（@1s）；step 60s → 输出 60 个点，而不是逐样本 3600 个。
    let samples: Vec<(i64, f64)> = (0..3600).map(|i| (i * 1_000_000, i as f64)).collect();
    let series = vec![Series {
        labels: ls(&[("m", "x")]),
        samples,
    }];
    let out = apply_rate_like_range(
        "rate",
        series,
        Duration::from_secs(300),
        0,
        3_600_000_000,
        60_000_000,
    );
    // 61 个步点（0..=3600s @60s）；t=0 窗口只有 1 个样本被跳过。
    assert_eq!(out.points.len(), 60);
    // t=300s：窗口 (0, 300s] 含 counter 1..=300 → delta=299，rate=299/300。
    let p = out
        .points
        .iter()
        .find(|p| p.ts_us == 300_000_000)
        .expect("step at 300s");
    assert!((p.value - 299.0 / 300.0).abs() < 1e-9, "got {}", p.value);
}

#[test]
fn samples_to_range_vector_steps_with_staleness_lookback() {
    // 样本只覆盖前 60s；step 30s。每个步点取 (t-5min, t] 内最新样本，
    // 超出 staleness lookback 后不再出点。
    let series = vec![Series {
        labels: ls(&[("m", "x")]),
        samples: vec![(0, 1.0), (30_000_000, 2.0), (60_000_000, 3.0)],
    }];
    let out = samples_to_range_vector(series, 0, 600_000_000, 30_000_000);
    let vals: Vec<(i64, f64)> = out.points.iter().map(|p| (p.ts_us, p.value)).collect();
    // t=0 → 1.0；t=30s → 2.0；t=60s..330s → 3.0；t≥360s 窗口 (t-300s, t] 已空。
    assert_eq!(vals.len(), 12);
    assert_eq!(vals[0], (0, 1.0));
    assert_eq!(vals[1], (30_000_000, 2.0));
    assert_eq!(vals[2], (60_000_000, 3.0));
    assert_eq!(*vals.last().unwrap(), (330_000_000, 3.0));
}

#[test]
fn range_step_us_derives_from_span_and_limit() {
    use crate::{
        domain::query::QueryLanguage,
        shared::{ids::Id, time::TimestampMicros},
    };

    let mk = |limit: Option<usize>| QueryRequest {
        org_id: Id::from_string("org"),
        language: QueryLanguage::Promql,
        statement: "m".into(),
        time_range: TimeRange::new(TimestampMicros(0), TimestampMicros(3_600_000_000)),
        stream: None,
        limit,
        federation_clusters: Vec::new(),
    };
    assert_eq!(range_step_us(&mk(Some(100))), 36_000_000); // 1h / 100
    assert_eq!(range_step_us(&mk(None)), 3_600_000); // 缺省 = MAX_RANGE_STEPS
    assert_eq!(range_step_us(&mk(Some(100_000))), 3_600_000); // limit 超大 → 封顶
    assert_eq!(range_step_us(&mk(Some(1))), 3_600_000_000); // 单步 = 整个跨度
}

#[test]
fn batches_to_series_enforces_sample_cap() {
    use std::sync::Arc;

    use arrow::datatypes::{Field, Schema as ArrowSchema, TimeUnit};

    let schema = Arc::new(ArrowSchema::new(vec![
        Field::new(
            "_timestamp",
            DataType::Timestamp(TimeUnit::Microsecond, None),
            false,
        ),
        Field::new("value", DataType::Float64, false),
    ]));
    let ts = TimestampMicrosecondArray::from((1..=10).map(|i| i * 1_000_000).collect::<Vec<i64>>());
    let val = Float64Array::from(vec![1.0_f64; 10]);
    let batch = RecordBatch::try_new(schema, vec![Arc::new(ts), Arc::new(val)]).unwrap();

    let err = batches_to_series(
        std::slice::from_ref(&batch),
        &Matchers::empty(),
        None,
        0,
        i64::MAX,
        5,
    )
    .unwrap_err();
    assert!(
        err.to_string().contains("narrow the time window"),
        "got {err}"
    );
    let ok = batches_to_series(&[batch], &Matchers::empty(), None, 0, i64::MAX, 10).unwrap();
    assert_eq!(ok.len(), 1);
    assert_eq!(ok[0].samples.len(), 10);
}

#[test]
fn batches_to_series_matches_columns_and_merges_across_schemas() {
    use std::sync::Arc;

    use arrow::datatypes::{Field, Schema as ArrowSchema, TimeUnit};

    fn selector_matchers(q: &str) -> Matchers {
        match parser::parse(q).unwrap() {
            Expr::VectorSelector(vs) => vs.matchers,
            other => panic!("expected vector selector, got {other:?}"),
        }
    }

    // batch1：host 列；batch2：schema 演进新增 env 列（全 null）+ 列序不同。
    // host=a 的样本应跨 batch 合并成同一条 series；host=b 被 matcher 过滤。
    let s1 = Arc::new(ArrowSchema::new(vec![
        Field::new(
            "_timestamp",
            DataType::Timestamp(TimeUnit::Microsecond, None),
            false,
        ),
        Field::new("value", DataType::Float64, false),
        Field::new("host", DataType::Utf8, true),
    ]));
    let b1 = RecordBatch::try_new(
        s1,
        vec![
            Arc::new(TimestampMicrosecondArray::from(vec![
                1_000_000_i64,
                2_000_000,
            ])),
            Arc::new(Float64Array::from(vec![1.0, 2.0])),
            Arc::new(StringArray::from(vec!["a", "b"])),
        ],
    )
    .unwrap();
    let s2 = Arc::new(ArrowSchema::new(vec![
        Field::new(
            "_timestamp",
            DataType::Timestamp(TimeUnit::Microsecond, None),
            false,
        ),
        Field::new("env", DataType::Utf8, true),
        Field::new("value", DataType::Float64, false),
        Field::new("host", DataType::Utf8, true),
    ]));
    let b2 = RecordBatch::try_new(
        s2,
        vec![
            Arc::new(TimestampMicrosecondArray::from(vec![3_000_000_i64])),
            Arc::new(StringArray::from(vec![None::<&str>])),
            Arc::new(Float64Array::from(vec![3.0])),
            Arc::new(StringArray::from(vec!["a"])),
        ],
    )
    .unwrap();

    let matchers = selector_matchers(r#"m{host=~"a|c"}"#);
    let series = batches_to_series(&[b1, b2], &matchers, None, 0, i64::MAX, usize::MAX).unwrap();
    assert_eq!(series.len(), 1, "host=b filtered, host=a merged");
    assert_eq!(series[0].labels, ls(&[("host", "a")]));
    assert_eq!(series[0].samples, vec![(1_000_000, 1.0), (3_000_000, 3.0)]);
}

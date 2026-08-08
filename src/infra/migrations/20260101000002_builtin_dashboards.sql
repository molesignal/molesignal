-- SPDX-License-Identifier: Apache-2.0
-- Copyright (c) 2026 MoleSignal Authors

-- Built-in dashboards for the immutable `_sys` organization.
--
-- This file is the single source of truth for bundled Dashboard records. The
-- metric catalog below intentionally lists every production Prometheus metric
-- registered by MoleSignal. Related metrics are split across focused
-- dashboards so each page stays usable while the full catalog remains visible.

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM organizations
        WHERE name = '_sys' AND slug = '_sys' AND system
    ) THEN
        RAISE EXCEPTION 'built-in dashboards require the `_sys` organization';
    END IF;
END
$$;

WITH dashboard_seed (id, uid, title, description, tags, version, is_default) AS (
VALUES
    ('15fcce40-f906-4ef8-bb10-400103d6c6f9', 'molesignal-system-overview', 'MoleSignal System Overview', 'Core health, throughput, saturation, and error signals for the MoleSignal deployment.', $tags$["built-in","molesignal","system","overview"]$tags$::JSONB, 2, TRUE),
    ('5a899a21-8feb-4a23-a86f-2f6959e38a12', 'molesignal-self-telemetry-runtime', 'MoleSignal Self Telemetry & Runtime', 'Complete self-telemetry delivery and metadata database runtime metrics.', $tags$["built-in","molesignal","system","runtime"]$tags$::JSONB, 1, FALSE),
    ('544194a6-12c8-410e-8d8c-99ce409aedc8', 'molesignal-ingestion-storage', 'MoleSignal Ingestion & Storage', 'Complete ingestion, WAL, Parquet, object-store, compaction, and Parquet file-metadata metrics.', $tags$["built-in","molesignal","system","ingestion","storage"]$tags$::JSONB, 1, FALSE),
    ('a978ffac-67c8-46a8-bbf7-638f8d53b61e', 'molesignal-tracing-apm', 'MoleSignal Tracing & APM', 'Complete distributed tracing pipeline and APM projection metrics.', $tags$["built-in","molesignal","system","tracing","apm"]$tags$::JSONB, 1, FALSE),
    ('6739830b-8667-4eba-8506-46a9284ae0fb', 'molesignal-cache-search-federation', 'MoleSignal Cache, Search & Federation', 'Complete cache, Tantivy search, and cross-cluster federation metrics.', $tags$["built-in","molesignal","system","cache","search","federation"]$tags$::JSONB, 1, FALSE)
),
metric_seed (dashboard_uid, position, metric_name, metric_kind, group_labels, unit) AS (
VALUES
    ('molesignal-self-telemetry-runtime', 1, 'self_telemetry_accepted_total', 'counter', 'signal', 'ops/s'),
    ('molesignal-self-telemetry-runtime', 2, 'self_telemetry_batches_total', 'counter', 'signal,outcome', 'ops/s'),
    ('molesignal-self-telemetry-runtime', 3, 'self_telemetry_dropped_total', 'counter', 'signal,reason', 'ops/s'),
    ('molesignal-self-telemetry-runtime', 4, 'self_telemetry_last_success_unixtime', 'timestamp', 'signal', 'seconds'),
    ('molesignal-self-telemetry-runtime', 5, 'self_telemetry_profile_available', 'status', 'kind', 'short'),
    ('molesignal-self-telemetry-runtime', 6, 'self_telemetry_queue_capacity', 'gauge', 'signal', 'short'),
    ('molesignal-self-telemetry-runtime', 7, 'self_telemetry_queue_depth', 'gauge', 'signal', 'short'),
    ('molesignal-self-telemetry-runtime', 8, 'self_telemetry_retries_total', 'counter', 'signal,reason', 'ops/s'),
    ('molesignal-self-telemetry-runtime', 9, 'db_pool_acquire_duration_seconds', 'histogram', 'pool', 'seconds'),
    ('molesignal-self-telemetry-runtime', 10, 'db_pool_connections', 'gauge', 'pool,state', 'short'),
    ('molesignal-ingestion-storage', 1, 'prometheus_active_series', 'gauge', '', 'short'),
    ('molesignal-ingestion-storage', 2, 'prometheus_remote_write_structural_rejections_total', 'counter', 'reason', 'ops/s'),
    ('molesignal-ingestion-storage', 3, 'prometheus_series_admission_rejections_total', 'counter', 'reason', 'ops/s'),
    ('molesignal-ingestion-storage', 4, 'ingester_adaptive_rotation_target_bytes', 'histogram', 'stream_type', 'bytes'),
    ('molesignal-ingestion-storage', 5, 'ingester_buffer_reserved_bytes', 'gauge', '', 'bytes'),
    ('molesignal-ingestion-storage', 6, 'ingester_flush_errors_total', 'counter', 'step', 'ops/s'),
    ('molesignal-ingestion-storage', 7, 'ingester_flush_inflight', 'gauge', 'stream_type', 'short'),
    ('molesignal-ingestion-storage', 8, 'ingester_memory_rejections_total', 'counter', 'stream_type', 'ops/s'),
    ('molesignal-ingestion-storage', 9, 'ingester_parquet_encoded_raw_ratio', 'histogram', 'stream_type', 'short'),
    ('molesignal-ingestion-storage', 10, 'ingester_rotations_total', 'counter', 'stream_type,reason', 'ops/s'),
    ('molesignal-ingestion-storage', 11, 'wal_append_inflight', 'gauge', 'stream_type', 'short'),
    ('molesignal-ingestion-storage', 12, 'wal_append_lock_wait_seconds', 'histogram', 'stream_type', 'seconds'),
    ('molesignal-ingestion-storage', 13, 'wal_fsync_errors_total', 'counter', 'kind', 'ops/s'),
    ('molesignal-ingestion-storage', 14, 'object_store_bytes_total', 'counter', 'backend,op', 'bytes/s'),
    ('molesignal-ingestion-storage', 15, 'object_store_errors_total', 'counter', 'backend,op,reason', 'ops/s'),
    ('molesignal-ingestion-storage', 16, 'object_store_health_check_duration_seconds', 'histogram', 'backend', 'seconds'),
    ('molesignal-ingestion-storage', 17, 'object_store_op_duration_seconds', 'histogram', 'backend,op', 'seconds'),
    ('molesignal-ingestion-storage', 18, 'object_store_operations_total', 'counter', 'backend,op', 'ops/s'),
    ('molesignal-ingestion-storage', 19, 'compactor_downsampled_groups_total', 'counter', '', 'ops/s'),
    ('molesignal-ingestion-storage', 20, 'compactor_failures_total', 'counter', 'reason', 'ops/s'),
    ('molesignal-ingestion-storage', 21, 'compactor_merged_groups_total', 'counter', '', 'ops/s'),
    ('molesignal-ingestion-storage', 22, 'compactor_retention_deleted_total', 'counter', '', 'ops/s'),
    ('molesignal-ingestion-storage', 23, 'parquet_file_meta_dump_delete_partitions_dropped_total', 'counter', '', 'ops/s'),
    ('molesignal-ingestion-storage', 24, 'parquet_file_meta_dump_delete_partitions_rewritten_total', 'counter', '', 'ops/s'),
    ('molesignal-ingestion-storage', 25, 'parquet_file_meta_dump_partitions_skipped_total', 'counter', 'reason', 'ops/s'),
    ('molesignal-ingestion-storage', 26, 'parquet_file_meta_dump_partitions_written_total', 'counter', '', 'ops/s'),
    ('molesignal-ingestion-storage', 27, 'parquet_file_meta_dump_query_hits_total', 'counter', '', 'ops/s'),
    ('molesignal-ingestion-storage', 28, 'parquet_file_meta_dump_query_load_seconds', 'histogram', '', 'seconds'),
    ('molesignal-ingestion-storage', 29, 'parquet_file_meta_dump_query_rows_skipped_total', 'counter', '', 'ops/s'),
    ('molesignal-ingestion-storage', 30, 'parquet_file_meta_dump_rows_written_total', 'counter', '', 'ops/s'),
    ('molesignal-tracing-apm', 1, 'molesignal_trace_decisions_total', 'counter', 'decision,reason', 'ops/s'),
    ('molesignal-tracing-apm', 2, 'molesignal_trace_export_batches_total', 'counter', 'sink,result', 'ops/s'),
    ('molesignal-tracing-apm', 3, 'molesignal_trace_latency_seconds', 'histogram', 'stage,sink', 'seconds'),
    ('molesignal-tracing-apm', 4, 'molesignal_trace_queue_capacity', 'gauge', 'queue', 'short'),
    ('molesignal-tracing-apm', 5, 'molesignal_trace_queue_depth', 'gauge', 'queue', 'short'),
    ('molesignal-tracing-apm', 6, 'molesignal_trace_retries_total', 'counter', 'sink,reason', 'ops/s'),
    ('molesignal-tracing-apm', 7, 'molesignal_trace_spans_total', 'counter', 'stage,result', 'ops/s'),
    ('molesignal-tracing-apm', 8, 'molesignal_trace_system_load_status', 'status', 'component', 'short'),
    ('molesignal-tracing-apm', 9, 'molesignal_trace_tail_cache', 'gauge', 'resource', 'short'),
    ('molesignal-tracing-apm', 10, 'molesignal_apm_api_duration_seconds', 'histogram', 'endpoint', 'seconds'),
    ('molesignal-tracing-apm', 11, 'molesignal_apm_cardinality_total', 'counter', 'reason', 'ops/s'),
    ('molesignal-tracing-apm', 12, 'molesignal_apm_facts_total', 'counter', 'result', 'ops/s'),
    ('molesignal-tracing-apm', 13, 'molesignal_apm_flush_duration_seconds', 'histogram', '', 'seconds'),
    ('molesignal-tracing-apm', 14, 'molesignal_apm_flushes_total', 'counter', 'result', 'ops/s'),
    ('molesignal-tracing-apm', 15, 'molesignal_apm_health', 'status', 'component', 'short'),
    ('molesignal-tracing-apm', 16, 'molesignal_apm_lag_seconds', 'gauge', 'stage', 'seconds'),
    ('molesignal-tracing-apm', 17, 'molesignal_apm_queue', 'gauge', 'resource', 'short'),
    ('molesignal-tracing-apm', 18, 'molesignal_apm_rollup_rows_total', 'counter', 'kind', 'ops/s'),
    ('molesignal-tracing-apm', 19, 'molesignal_apm_rollups_total', 'counter', 'result', 'ops/s'),
    ('molesignal-cache-search-federation', 1, 'cache_evictions_total', 'counter', 'level', 'ops/s'),
    ('molesignal-cache-search-federation', 2, 'cache_hit_ratio', 'ratio', 'level', 'percentunit'),
    ('molesignal-cache-search-federation', 3, 'cache_hits_total', 'counter', 'level', 'ops/s'),
    ('molesignal-cache-search-federation', 4, 'cache_misses_total', 'counter', 'level', 'ops/s'),
    ('molesignal-cache-search-federation', 5, 'cache_parquet_disk_evictions_total', 'counter', '', 'ops/s'),
    ('molesignal-cache-search-federation', 6, 'cache_parquet_disk_hit_ratio', 'ratio', '', 'percentunit'),
    ('molesignal-cache-search-federation', 7, 'cache_parquet_disk_hits_total', 'counter', '', 'ops/s'),
    ('molesignal-cache-search-federation', 8, 'cache_parquet_disk_misses_total', 'counter', '', 'ops/s'),
    ('molesignal-cache-search-federation', 9, 'cache_tantivy_footer_errors_total', 'counter', '', 'ops/s'),
    ('molesignal-cache-search-federation', 10, 'cache_tantivy_footer_evictions_total', 'counter', '', 'ops/s'),
    ('molesignal-cache-search-federation', 11, 'cache_tantivy_footer_hit_ratio', 'ratio', '', 'percentunit'),
    ('molesignal-cache-search-federation', 12, 'cache_tantivy_footer_hits_total', 'counter', '', 'ops/s'),
    ('molesignal-cache-search-federation', 13, 'cache_tantivy_footer_misses_total', 'counter', '', 'ops/s'),
    ('molesignal-cache-search-federation', 14, 'cache_tantivy_result_errors_total', 'counter', '', 'ops/s'),
    ('molesignal-cache-search-federation', 15, 'cache_tantivy_result_evictions_total', 'counter', '', 'ops/s'),
    ('molesignal-cache-search-federation', 16, 'cache_tantivy_result_hit_ratio', 'ratio', '', 'percentunit'),
    ('molesignal-cache-search-federation', 17, 'cache_tantivy_result_hits_total', 'counter', '', 'ops/s'),
    ('molesignal-cache-search-federation', 18, 'cache_tantivy_result_misses_total', 'counter', '', 'ops/s'),
    ('molesignal-cache-search-federation', 19, 'tantivy_pruned_files_total', 'counter', '', 'ops/s'),
    ('molesignal-cache-search-federation', 20, 'tantivy_puffin_blob_range_reads_total', 'counter', '', 'ops/s'),
    ('molesignal-cache-search-federation', 21, 'tantivy_puffin_directory_open_seconds', 'histogram', '', 'seconds'),
    ('molesignal-cache-search-federation', 22, 'tantivy_puffin_directory_open_total', 'counter', '', 'ops/s'),
    ('molesignal-cache-search-federation', 23, 'tantivy_puffin_footer_bytes_read_total', 'counter', '', 'bytes/s'),
    ('molesignal-cache-search-federation', 24, 'federated_search_auth_errors_total', 'counter', 'cluster', 'ops/s'),
    ('molesignal-cache-search-federation', 25, 'federation_events_applied_total', 'counter', 'result', 'ops/s'),
    ('molesignal-cache-search-federation', 26, 'federation_events_pushed_total', 'counter', '', 'ops/s'),
    ('molesignal-cache-search-federation', 27, 'federation_outbox_lag', 'gauge', 'cluster', 'short'),
    ('molesignal-cache-search-federation', 28, 'federation_push_errors_total', 'counter', 'cluster', 'ops/s')
),
overview_panel_seed (
    dashboard_uid, position, id, title, description,
    x, y, width, height, visualization,
    unit, decimals, minimum, maximum, queries
) AS (
VALUES
    ('molesignal-system-overview', 1, 'overview-self-telemetry-throughput', 'Self-telemetry throughput', 'Records accepted for durable self-telemetry ingestion.', 0, 0, 6, 16, 'stat', 'ops/s', 2, 0, NULL, $queries$[{"refId":"A","enabled":true,"dataSourceType":"metrics","legend":"Accepted","query":{"language":"promql","expression":"sum(rate(self_telemetry_accepted_total[5m]))"}}]$queries$::JSONB),
    ('molesignal-system-overview', 2, 'overview-self-telemetry-drops', 'Self-telemetry drops', 'Records dropped before durable ingestion.', 6, 0, 6, 16, 'stat', 'ops/s', 2, 0, NULL, $queries$[{"refId":"A","enabled":true,"dataSourceType":"metrics","legend":"Dropped","query":{"language":"promql","expression":"sum(rate(self_telemetry_dropped_total[5m]))"}}]$queries$::JSONB),
    ('molesignal-system-overview', 3, 'overview-cache-hit-ratio', 'Cache hit ratio', 'Average hit ratio across active cache levels.', 12, 0, 6, 16, 'gauge', 'percentunit', 1, 0, 1, $queries$[{"refId":"A","enabled":true,"dataSourceType":"metrics","legend":"Hit ratio","query":{"language":"promql","expression":"avg(cache_hit_ratio)"}}]$queries$::JSONB),
    ('molesignal-system-overview', 4, 'overview-database-p95', 'Database acquire P95', 'P95 wait time for a metadata database connection.', 18, 0, 6, 16, 'stat', 'seconds', 3, 0, NULL, $queries$[{"refId":"A","enabled":true,"dataSourceType":"metrics","legend":"P95","query":{"language":"promql","expression":"histogram_quantile(0.95, sum by (le) (rate(db_pool_acquire_duration_seconds_bucket[5m])))"}}]$queries$::JSONB),
    ('molesignal-system-overview', 5, 'overview-database-pool-utilization', 'Database pool utilization', 'Checked-out metadata connections divided by configured maximum.', 0, 16, 6, 16, 'gauge', 'percentunit', 1, 0, 1, $queries$[{"refId":"A","enabled":true,"dataSourceType":"metrics","legend":"Utilization","query":{"language":"promql","expression":"sum(db_pool_connections{state=\"checked_out\"}) / clamp_min(sum(db_pool_connections{state=\"max\"}), 1)"}}]$queries$::JSONB),
    ('molesignal-system-overview', 6, 'overview-prometheus-active-series', 'Active Prometheus series', 'Active hashed Prometheus series tracked by ingesters.', 6, 16, 6, 16, 'stat', 'short', 0, 0, NULL, $queries$[{"refId":"A","enabled":true,"dataSourceType":"metrics","legend":"Active series","query":{"language":"promql","expression":"sum(prometheus_active_series)"}}]$queries$::JSONB),
    ('molesignal-system-overview', 7, 'overview-trace-system-health', 'Trace system health', 'Minimum health state across required Trace system components.', 12, 16, 6, 16, 'gauge', 'short', 0, 0, 1, $queries$[{"refId":"A","enabled":true,"dataSourceType":"metrics","legend":"Health","query":{"language":"promql","expression":"min(molesignal_trace_system_load_status)"}}]$queries$::JSONB),
    ('molesignal-system-overview', 8, 'overview-apm-lag', 'APM max lag', 'Maximum projection or rollup lag.', 18, 16, 6, 16, 'stat', 'seconds', 1, 0, NULL, $queries$[{"refId":"A","enabled":true,"dataSourceType":"metrics","legend":"Lag","query":{"language":"promql","expression":"max(molesignal_apm_lag_seconds)"}}]$queries$::JSONB),
    ('molesignal-system-overview', 9, 'overview-ingestion-failures', 'Ingestion failures & rejections', 'Flush failures and bounded ingestion rejection rates.', 0, 32, 12, 20, 'time_series', 'ops/s', 2, 0, NULL, $queries$[{"refId":"A","enabled":true,"dataSourceType":"metrics","legend":"{{step}}","query":{"language":"promql","expression":"sum by (step) (rate(ingester_flush_errors_total[5m]))"}},{"refId":"B","enabled":true,"dataSourceType":"metrics","legend":"{{stream_type}} memory","query":{"language":"promql","expression":"sum by (stream_type) (rate(ingester_memory_rejections_total[5m]))"}},{"refId":"C","enabled":true,"dataSourceType":"metrics","legend":"{{reason}} structural","query":{"language":"promql","expression":"sum by (reason) (rate(prometheus_remote_write_structural_rejections_total[5m]))"}},{"refId":"D","enabled":true,"dataSourceType":"metrics","legend":"{{reason}} admission","query":{"language":"promql","expression":"sum by (reason) (rate(prometheus_series_admission_rejections_total[5m]))"}}]$queries$::JSONB),
    ('molesignal-system-overview', 10, 'overview-object-store-errors', 'Object-store errors', 'Object-store errors grouped by backend, operation, and reason.', 12, 32, 12, 20, 'time_series', 'ops/s', 2, 0, NULL, $queries$[{"refId":"A","enabled":true,"dataSourceType":"metrics","legend":"{{backend}} · {{op}} · {{reason}}","query":{"language":"promql","expression":"sum by (backend, op, reason) (rate(object_store_errors_total[5m]))"}}]$queries$::JSONB),
    ('molesignal-system-overview', 11, 'overview-trace-flow', 'Trace pipeline flow', 'Span handling, export batches, and retry rates.', 0, 52, 12, 20, 'time_series', 'ops/s', 2, 0, NULL, $queries$[{"refId":"A","enabled":true,"dataSourceType":"metrics","legend":"{{stage}} · {{result}}","query":{"language":"promql","expression":"sum by (stage, result) (rate(molesignal_trace_spans_total[5m]))"}},{"refId":"B","enabled":true,"dataSourceType":"metrics","legend":"{{sink}} · {{result}}","query":{"language":"promql","expression":"sum by (sink, result) (rate(molesignal_trace_export_batches_total[5m]))"}},{"refId":"C","enabled":true,"dataSourceType":"metrics","legend":"{{sink}} · {{reason}}","query":{"language":"promql","expression":"sum by (sink, reason) (rate(molesignal_trace_retries_total[5m]))"}}]$queries$::JSONB),
    ('molesignal-system-overview', 12, 'overview-apm-processing', 'APM processing', 'APM fact, flush, and rollup processing rates.', 12, 52, 12, 20, 'time_series', 'ops/s', 2, 0, NULL, $queries$[{"refId":"A","enabled":true,"dataSourceType":"metrics","legend":"facts · {{result}}","query":{"language":"promql","expression":"sum by (result) (rate(molesignal_apm_facts_total[5m]))"}},{"refId":"B","enabled":true,"dataSourceType":"metrics","legend":"flushes · {{result}}","query":{"language":"promql","expression":"sum by (result) (rate(molesignal_apm_flushes_total[5m]))"}},{"refId":"C","enabled":true,"dataSourceType":"metrics","legend":"rollups · {{result}}","query":{"language":"promql","expression":"sum by (result) (rate(molesignal_apm_rollups_total[5m]))"}}]$queries$::JSONB)
),
metric_panels AS (
    SELECT
        metric.dashboard_uid,
        metric.position,
        jsonb_build_object(
            'kind', 'panel',
            'id', metric.dashboard_uid || '-' || replace(metric.metric_name, '_', '-'),
            'title', metric.metric_name,
            'description',
                CASE metric.metric_kind
                    WHEN 'counter' THEN 'Five-minute rate for ' || metric.metric_name || '.'
                    WHEN 'histogram' THEN 'Five-minute P95 for ' || metric.metric_name || '.'
                    WHEN 'timestamp' THEN 'Age derived from ' || metric.metric_name || '.'
                    ELSE 'Current value of ' || metric.metric_name || '.'
                END,
            'gridPos', jsonb_build_object(
                'x', ((metric.position - 1) % 2) * 12,
                'y', ((metric.position - 1) / 2) * 20,
                'w', 12,
                'h', 20
            ),
            'queryOptions', '{}'::JSONB,
            'queries', jsonb_build_array(
                jsonb_build_object(
                    'refId', 'A',
                    'enabled', TRUE,
                    'dataSourceType', 'metrics',
                    'legend',
                        CASE WHEN metric.group_labels = ''
                            THEN metric.metric_name
                            ELSE '{{' || replace(metric.group_labels, ',', '}} · {{') || '}}'
                        END,
                    'query', jsonb_build_object(
                        'language', 'promql',
                        'expression',
                            CASE metric.metric_kind
                                WHEN 'counter' THEN
                                    CASE WHEN metric.group_labels = ''
                                        THEN 'sum(rate(' || metric.metric_name || '[5m]))'
                                        ELSE 'sum by (' || replace(metric.group_labels, ',', ', ') ||
                                             ') (rate(' || metric.metric_name || '[5m]))'
                                    END
                                WHEN 'histogram' THEN
                                    'histogram_quantile(0.95, sum by (le' ||
                                    CASE WHEN metric.group_labels = '' THEN ''
                                        ELSE ', ' || replace(metric.group_labels, ',', ', ')
                                    END ||
                                    ') (rate(' || metric.metric_name || '_bucket[5m])))'
                                WHEN 'timestamp' THEN
                                    CASE WHEN metric.group_labels = ''
                                        THEN 'time() - max(' || metric.metric_name || ')'
                                        ELSE 'time() - max by (' ||
                                             replace(metric.group_labels, ',', ', ') || ') (' ||
                                             metric.metric_name || ')'
                                    END
                                WHEN 'ratio' THEN
                                    CASE WHEN metric.group_labels = ''
                                        THEN 'avg(' || metric.metric_name || ')'
                                        ELSE 'avg by (' || replace(metric.group_labels, ',', ', ') ||
                                             ') (' || metric.metric_name || ')'
                                    END
                                WHEN 'status' THEN
                                    CASE WHEN metric.group_labels = ''
                                        THEN 'min(' || metric.metric_name || ')'
                                        ELSE 'min by (' || replace(metric.group_labels, ',', ', ') ||
                                             ') (' || metric.metric_name || ')'
                                    END
                                ELSE
                                    CASE WHEN metric.group_labels = ''
                                        THEN 'max(' || metric.metric_name || ')'
                                        ELSE 'max by (' || replace(metric.group_labels, ',', ', ') ||
                                             ') (' || metric.metric_name || ')'
                                    END
                            END
                    )
                )
            ),
            'transformations', '[]'::JSONB,
            'visualization', jsonb_build_object(
                'type', 'time_series', 'schemaVersion', 1, 'options', '{}'::JSONB
            ),
            'fieldConfig', jsonb_strip_nulls(jsonb_build_object(
                'unit', metric.unit,
                'decimals', CASE
                    WHEN metric.unit = 'seconds' THEN 3
                    WHEN metric.unit IN ('bytes', 'short') THEN 0
                    ELSE 2
                END,
                'min', CASE
                    WHEN metric.metric_kind IN ('counter', 'histogram', 'ratio', 'status') THEN 0
                    ELSE NULL
                END,
                'max', CASE
                    WHEN metric.metric_kind IN ('ratio', 'status') THEN 1
                    ELSE NULL
                END,
                'noValue', 'No data'
            )),
            'overrides', '[]'::JSONB,
            'links', '[]'::JSONB
        ) AS element
    FROM metric_seed metric
),
overview_panels AS (
    SELECT
        panel.dashboard_uid,
        panel.position,
        jsonb_build_object(
            'kind', 'panel',
            'id', panel.id,
            'title', panel.title,
            'description', panel.description,
            'gridPos', jsonb_build_object(
                'x', panel.x, 'y', panel.y, 'w', panel.width, 'h', panel.height
            ),
            'queryOptions', '{}'::JSONB,
            'queries', panel.queries,
            'transformations', '[]'::JSONB,
            'visualization', jsonb_build_object(
                'type', panel.visualization, 'schemaVersion', 1, 'options', '{}'::JSONB
            ),
            'fieldConfig', jsonb_strip_nulls(jsonb_build_object(
                'unit', panel.unit,
                'decimals', panel.decimals,
                'min', panel.minimum,
                'max', panel.maximum,
                'noValue', 'No data'
            )),
            'overrides', '[]'::JSONB,
            'links', '[]'::JSONB
        ) AS element
    FROM overview_panel_seed panel
),
all_panels AS (
    SELECT dashboard_uid, position, element FROM metric_panels
    UNION ALL
    SELECT dashboard_uid, position, element FROM overview_panels
),
dashboard_models AS (
    SELECT
        dashboard.id,
        dashboard.uid,
        dashboard.title,
        dashboard.description,
        dashboard.tags,
        dashboard.version,
        jsonb_build_object(
            'engine', 'molesignal-dashboard',
            'schemaVersion', 2,
            'id', dashboard.id,
            'uid', dashboard.uid,
            'title', dashboard.title,
            'description', dashboard.description,
            'tags', dashboard.tags,
            'editable', FALSE,
            'defaultDashboard', dashboard.is_default,
            'timeSettings', jsonb_build_object(
                'defaultFrom', 'now-1h', 'defaultTo', 'now', 'timezone', 'browser'
            ),
            'refreshSettings', jsonb_build_object(
                'enabled', TRUE,
                'mode', 'interval',
                'defaultInterval', '1m',
                'allowedIntervals', jsonb_build_array('off', '30s', '1m', '5m')
            ),
            'variables', '[]'::JSONB,
            'annotations', '[]'::JSONB,
            'links', '[]'::JSONB,
            'layout', jsonb_build_object(
                'type', 'grid', 'columns', 24, 'rowHeight', 8, 'gap', 8
            ),
            'elements', COALESCE((
                SELECT jsonb_agg(panel.element ORDER BY panel.position)
                FROM all_panels panel
                WHERE panel.dashboard_uid = dashboard.uid
            ), '[]'::JSONB),
            'version', dashboard.version,
            'createdAt', '2026-08-02T00:00:00Z',
            'updatedAt', '2026-08-02T00:00:00Z',
            'createdBy', 'molesignal-system',
            'updatedBy', 'molesignal-system'
        ) AS model
    FROM dashboard_seed dashboard
)
INSERT INTO dashboards (
    id, org_id, folder_id, uid, title, tags, model, version,
    created_at_micros, updated_at_micros, created_by, updated_by
)
SELECT
    dashboard.id,
    organization.id,
    NULL,
    dashboard.uid,
    dashboard.title,
    dashboard.tags,
    dashboard.model,
    dashboard.version,
    (EXTRACT(EPOCH FROM clock_timestamp()) * 1000000)::BIGINT,
    (EXTRACT(EPOCH FROM clock_timestamp()) * 1000000)::BIGINT,
    'molesignal-system',
    'molesignal-system'
FROM dashboard_models dashboard
JOIN organizations organization
  ON organization.slug = '_sys' AND organization.system
ON CONFLICT (org_id, uid) DO UPDATE
SET folder_id = NULL,
    title = EXCLUDED.title,
    tags = EXCLUDED.tags,
    model = EXCLUDED.model,
    version = EXCLUDED.version,
    updated_at_micros = EXCLUDED.updated_at_micros,
    updated_by = EXCLUDED.updated_by
WHERE dashboards.folder_id IS NOT NULL
   OR dashboards.title IS DISTINCT FROM EXCLUDED.title
   OR dashboards.tags IS DISTINCT FROM EXCLUDED.tags
   OR dashboards.model IS DISTINCT FROM EXCLUDED.model
   OR dashboards.version IS DISTINCT FROM EXCLUDED.version;

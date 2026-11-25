// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The panels of 「主机 · 概览」. The metrics are the ones Node Agent names `molesignal_host_*`;
//! a series of a container carries `container.id`, so the host's own series are the ones without
//! it. Memory is read from `/proc/vmstat`, in pages of 4 KiB.

use serde_json::Value;

use crate::app::{
    dashboard::builtin::panel::{Panel, at},
    node_agent::{EVENTS_STREAM, METRICS_STREAM},
};

/// The labels that pick the chosen host's own series.
const HOST: &str = r#""host.name"="$host", "container.id"="""#;

/// The size of a page of `/proc/vmstat`.
const PAGE_BYTES: u32 = 4096;

/// The start of the interval an event falls in, for the time line.
const BUCKET: &str = "date_bin(INTERVAL '${interval}', _timestamp)";

/// The events of the chosen host.
const ON_HOST: &str = r#""host.name" = ${host:sqlstring}"#;

pub(super) fn panels() -> Vec<Value> {
    resources()
        .into_iter()
        .chain(events())
        .map(Panel::build)
        .collect()
}

/// `metric` of the host, summed over what it is kept by (CPUs, devices).
fn total(metric: &str) -> String {
    format!("sum({metric}{{{HOST}}})")
}

/// The rate per second of the counter `metric` of the host, a line for each `by` label.
fn rate_by(metric: &str, by: &str) -> String {
    format!("sum by ({by}) (rate({metric}{{{HOST}}}[5m]))")
}

/// The bytes of the pages of `/proc/vmstat` keys `keys`, added up.
fn pages(keys: &[&str]) -> String {
    let sum = keys
        .iter()
        .map(|key| total(&format!("molesignal_host_memory_vmstat_{key}")))
        .collect::<Vec<_>>()
        .join(" + ");

    format!("({sum}) * {PAGE_BYTES}")
}

/// CPU, load, memory, disk and network: two panels a row.
fn resources() -> Vec<Panel> {
    vec![
        Panel::metric(
            "cpu",
            "CPU 使用率",
            at(0, 0, 12, 16),
            METRICS_STREAM,
            total("molesignal_host_cpu_util_total"),
            "总计",
        )
        .and(total("molesignal_host_cpu_util_usr"), "用户态")
        .and(total("molesignal_host_cpu_util_sys"), "内核态")
        .about("主机全部 CPU 的使用率，按百分比。")
        .unit("percent", 1),
        Panel::metric(
            "load",
            "负载",
            at(12, 0, 12, 16),
            METRICS_STREAM,
            total("molesignal_host_loadavg_load1"),
            "1 分钟",
        )
        .and(total("molesignal_host_loadavg_load5"), "5 分钟")
        .and(total("molesignal_host_loadavg_load15"), "15 分钟")
        .about("/proc/loadavg 的 1、5、15 分钟平均负载。")
        .unit("short", 2),
        Panel::metric(
            "memory",
            "内存",
            at(0, 16, 12, 16),
            METRICS_STREAM,
            pages(&["nr_free_pages"]),
            "空闲",
        )
        .and(pages(&["nr_active_anon", "nr_inactive_anon"]), "匿名页")
        .and(pages(&["nr_active_file", "nr_inactive_file"]), "文件页")
        .and(
            pages(&["nr_slab_reclaimable", "nr_slab_unreclaimable"]),
            "Slab",
        )
        .about("/proc/vmstat 的页数按 4 KiB 一页换算。匿名页多、空闲少时留意 OOM 事件。")
        .unit("bytes", 1),
        Panel::metric(
            "disk",
            "磁盘吞吐",
            at(12, 16, 12, 16),
            METRICS_STREAM,
            rate_by("molesignal_host_diskio_read_bytes_total", "device"),
            "{{device}} 读",
        )
        .and(
            rate_by("molesignal_host_diskio_written_bytes_total", "device"),
            "{{device}} 写",
        )
        .about("每块磁盘每秒读写的字节数，按 5 分钟的速率。")
        .unit("bytes_per_second", 1),
        Panel::metric(
            "network",
            "网络吞吐",
            at(0, 32, 12, 16),
            METRICS_STREAM,
            rate_by("molesignal_host_netdev_receive_bytes_total", "device"),
            "{{device}} 接收",
        )
        .and(
            rate_by("molesignal_host_netdev_transmit_bytes_total", "device"),
            "{{device}} 发送",
        )
        .about("主机网卡每秒收发的字节数；lo、docker、veth 默认不采集。")
        .unit("bytes_per_second", 1),
        Panel::metric(
            "network-errors",
            "网络错误与丢包",
            at(12, 32, 12, 16),
            METRICS_STREAM,
            rate_by("molesignal_host_netdev_receive_errors_total", "device"),
            "{{device}} 接收错误",
        )
        .and(
            rate_by("molesignal_host_netdev_transmit_errors_total", "device"),
            "{{device}} 发送错误",
        )
        .and(
            rate_by("molesignal_host_netdev_receive_dropped_total", "device"),
            "{{device}} 接收丢弃",
        )
        .and(
            rate_by("molesignal_host_netdev_transmit_dropped_total", "device"),
            "{{device}} 发送丢弃",
        )
        .about("每秒的网卡错误与丢弃包数。持续不为零时对照下方的 dropwatch 与重传事件。")
        .unit("short", 2),
    ]
}

/// The events of the host: how many of each severity over time, which kinds, and the latest.
fn events() -> Vec<Panel> {
    vec![
        Panel::trend(
            "events-trend",
            "事件时间线",
            at(0, 48, 24, 14),
            EVENTS_STREAM,
            format!(
                r#"SELECT {BUCKET} AS "time", COUNT(*) FILTER (WHERE severity_text = 'ERROR') AS "ERROR", COUNT(*) FILTER (WHERE severity_text = 'WARN') AS "WARN", COUNT(*) FILTER (WHERE severity_text = 'INFO') AS "INFO" FROM "{EVENTS_STREAM}" WHERE {ON_HOST} GROUP BY 1 ORDER BY 1"#
            ),
        )
        .about("每个间隔内这台主机上报的事件数，按级别分开。")
        .legend()
        .unit("short", 0),
        Panel::bars(
            "events-by-name",
            "事件类型",
            at(0, 62, 8, 16),
            EVENTS_STREAM,
            format!(
                r#"SELECT "event.name" AS "事件", COUNT(*) AS "次数" FROM "{EVENTS_STREAM}" WHERE {ON_HOST} GROUP BY 1 ORDER BY 2 DESC LIMIT 20"#
            ),
        )
        .about("时间范围内各类事件的次数。")
        .unit("short", 0),
        Panel::table(
            "events-latest",
            "最近事件",
            at(8, 62, 16, 16),
            EVENTS_STREAM,
            format!(
                r#"SELECT _timestamp AS "时间", "event.name" AS "事件", severity_text AS "级别", "event.domain" AS "领域", "event.kind" AS "类型" FROM "{EVENTS_STREAM}" WHERE {ON_HOST} ORDER BY _timestamp DESC LIMIT 50"#
            ),
        )
        .about("最近 50 条事件。容器、进程与内核栈在事件的 body 里，打开「主机事件」保存的查询可以按容器筛选。"),
    ]
}

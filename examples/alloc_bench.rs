// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 分配器对照基准：RSS + 吞吐。
//!
//! 用项目实际最吃分配器的库（Arrow 构 RecordBatch + DataFusion 聚合/过滤/排序查询 +
//! parquet 往返）跑多线程高 churn 负载，量「吞吐(queries/s)」「峰值 RSS」「负载停止 +
//! idle 后 RSS（看分配器是否把内存还给 OS）」。
//!
//! 同一份 workload 用三档配置各跑一遍对比（由 `scripts/alloc_bench.sh` 驱动）：
//!   ① 默认系统分配器（glibc ptmalloc2）
//!   ② glibc 调参：`MALLOC_ARENA_MAX=2 MALLOC_TRIM_THRESHOLD_=131072`（运行期 env，无需重编）
//!   ③ jemalloc：`cargo run --release --example alloc_bench --features jemalloc`
//!
//! 关键：**务必在 glibc Linux（生产目标）上跑**，macOS 的系统分配器与 `MALLOC_ARENA_MAX`
//! 行为不同，数字不代表生产。脚本默认走 docker（debian bookworm）以贴合生产。
//!
//! 可调 env：BENCH_SECONDS(20) BENCH_THREADS(CPU 数) BENCH_ROWS(20000)
//!          BENCH_BATCHES(12) BENCH_IDLE_SECONDS(8)

#[cfg(feature = "jemalloc")]
#[global_allocator]
static GLOBAL: tikv_jemallocator::Jemalloc = tikv_jemallocator::Jemalloc;

use std::{
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use arrow::{
    array::{Int64Array, RecordBatch, StringArray, TimestampMicrosecondArray},
    datatypes::{DataType, Field, Schema, TimeUnit},
};
use datafusion::{datasource::MemTable, prelude::SessionContext};
use parquet::arrow::{ArrowWriter, arrow_reader::ParquetRecordBatchReaderBuilder};

const ALLOC_NAME: &str = if cfg!(feature = "jemalloc") {
    "jemalloc"
} else {
    "system"
};

const SERVICES: [&str; 10] = [
    "api", "db", "cache", "auth", "queue", "web", "worker", "ingest", "query", "alert",
];
const LEVELS: [&str; 5] = ["INFO", "WARN", "ERROR", "DEBUG", "TRACE"];

const QUERIES: [&str; 3] = [
    "SELECT service, count(*) c, avg(latency) a, max(latency) m \
     FROM logs GROUP BY service ORDER BY c DESC",
    "SELECT * FROM logs WHERE level = 'ERROR' ORDER BY ts DESC LIMIT 100",
    "SELECT service, count(*) FROM logs WHERE latency > 500 GROUP BY service",
];

fn env_usize(k: &str, d: usize) -> usize {
    std::env::var(k)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(d)
}

/// 模拟一批日志事件（ts + service + level + msg + latency）：字符串 + 数值混合，制造
/// 真实的分配 churn。`seed` 让每批数据不同。
fn make_batch(rows: usize, seed: u64) -> RecordBatch {
    let mut ts = Vec::with_capacity(rows);
    let mut svc: Vec<&str> = Vec::with_capacity(rows);
    let mut lvl: Vec<&str> = Vec::with_capacity(rows);
    let mut msg: Vec<String> = Vec::with_capacity(rows);
    let mut lat = Vec::with_capacity(rows);
    let mut x = seed.wrapping_mul(2_654_435_761) ^ 0x9e37_79b9_7f4a_7c15;
    for i in 0..rows {
        // xorshift64：确定性伪随机，零依赖。
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        ts.push(1_700_000_000_000_000_i64.wrapping_add(i as i64 * 1_000));
        svc.push(SERVICES[(x as usize) % SERVICES.len()]);
        lvl.push(LEVELS[((x >> 8) as usize) % LEVELS.len()]);
        msg.push(format!("req {} took {}ms path=/v1/x{}", i, x % 500, x % 50));
        lat.push((x % 1_000) as i64);
    }
    let schema = Arc::new(Schema::new(vec![
        Field::new(
            "ts",
            DataType::Timestamp(TimeUnit::Microsecond, None),
            false,
        ),
        Field::new("service", DataType::Utf8, false),
        Field::new("level", DataType::Utf8, false),
        Field::new("msg", DataType::Utf8, false),
        Field::new("latency", DataType::Int64, false),
    ]));
    RecordBatch::try_new(
        schema,
        vec![
            Arc::new(TimestampMicrosecondArray::from(ts)),
            Arc::new(StringArray::from(svc)),
            Arc::new(StringArray::from(lvl)),
            Arc::new(StringArray::from(msg)),
            Arc::new(Int64Array::from(lat)),
        ],
    )
    .expect("build record batch")
}

/// 一轮：建 batch → 注册 MemTable → 跑全部查询；每隔几轮做一次 parquet 往返。
/// 返回本轮完成的查询数。
async fn run_round(rows: usize, batches_n: usize, round: u64, do_parquet: bool) -> u64 {
    let batches: Vec<RecordBatch> = (0..batches_n)
        .map(|b| make_batch(rows, round.wrapping_mul(1_000_003).wrapping_add(b as u64)))
        .collect();
    let schema = batches[0].schema();

    let mut queries = 0u64;
    let mem = MemTable::try_new(schema.clone(), vec![batches.clone()]).expect("memtable");
    let ctx = SessionContext::new();
    ctx.register_table("logs", Arc::new(mem)).expect("register");
    for q in QUERIES {
        let _ = ctx
            .sql(q)
            .await
            .expect("sql")
            .collect()
            .await
            .expect("collect");
        queries += 1;
    }

    if do_parquet {
        // 写到内存 parquet 再读回：制造 parquet 编解码缓冲的分配。
        let mut buf: Vec<u8> = Vec::new();
        {
            let mut w = ArrowWriter::try_new(&mut buf, schema.clone(), None).expect("writer");
            for b in &batches {
                w.write(b).expect("write");
            }
            w.close().expect("close");
        }
        let reader = ParquetRecordBatchReaderBuilder::try_new(bytes::Bytes::from(buf))
            .expect("reader builder")
            .build()
            .expect("reader");
        for rb in reader {
            let _ = rb.expect("rb").num_rows();
        }
        queries += 1;
    }
    queries
}

/// 读 `/proc/self/status` 的 `VmRSS`（KB）；非 Linux 返 None。
fn read_vmrss_kb() -> Option<u64> {
    let s = std::fs::read_to_string("/proc/self/status").ok()?;
    for line in s.lines() {
        if let Some(rest) = line.strip_prefix("VmRSS:") {
            return rest.split_whitespace().next()?.parse().ok();
        }
    }
    None
}

fn main() {
    let seconds = env_usize("BENCH_SECONDS", 20) as u64;
    let threads = env_usize(
        "BENCH_THREADS",
        std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(8),
    )
    .max(1);
    let rows = env_usize("BENCH_ROWS", 20_000);
    let batches = env_usize("BENCH_BATCHES", 12);
    let idle = env_usize("BENCH_IDLE_SECONDS", 8) as u64;

    // 后台采样线程：每 200ms 读 VmRSS，记峰值。
    let peak = Arc::new(AtomicU64::new(0));
    let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let sampler = {
        let peak = peak.clone();
        let stop = stop.clone();
        std::thread::spawn(move || {
            while !stop.load(Ordering::Relaxed) {
                if let Some(kb) = read_vmrss_kb() {
                    peak.fetch_max(kb, Ordering::Relaxed);
                }
                std::thread::sleep(Duration::from_millis(200));
            }
        })
    };

    let counter = Arc::new(AtomicU64::new(0));
    let start = Instant::now();
    let deadline = start + Duration::from_secs(seconds);

    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(threads)
        .enable_all()
        .build()
        .expect("runtime");
    rt.block_on(async {
        let mut handles = Vec::with_capacity(threads);
        for t in 0..threads {
            let counter = counter.clone();
            handles.push(tokio::spawn(async move {
                let mut round = t as u64;
                while Instant::now() < deadline {
                    let q = run_round(rows, batches, round, round.is_multiple_of(4)).await;
                    counter.fetch_add(q, Ordering::Relaxed);
                    round += 1;
                }
            }));
        }
        for h in handles {
            let _ = h.await;
        }
    });
    let elapsed = start.elapsed().as_secs_f64();
    let total = counter.load(Ordering::Relaxed);
    let qps = total as f64 / elapsed.max(1e-9);

    // 负载停止：drop runtime（worker 线程退出），再 idle，让分配器有机会 purge 回 OS。
    drop(rt);
    std::thread::sleep(Duration::from_secs(idle));

    let postidle = read_vmrss_kb();
    stop.store(true, Ordering::Relaxed);
    let _ = sampler.join();
    let peak_kb = peak.load(Ordering::Relaxed);

    // 机器可解析输出（脚本按 KEY=VALUE 抓）。
    println!(
        "ALLOC={ALLOC_NAME} THREADS={threads} SECONDS={seconds} ROWS={rows} BATCHES={batches}"
    );
    println!("THROUGHPUT_QPS={qps:.1}");
    println!(
        "RSS_PEAK_KB={}",
        if peak_kb == 0 { -1 } else { peak_kb as i64 }
    );
    println!(
        "RSS_POSTIDLE_KB={}",
        postidle.map(|v| v as i64).unwrap_or(-1)
    );
}

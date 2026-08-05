// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 节点级 CPU/heap profile capture service。

#[cfg(feature = "profiling-pprof")]
use std::collections::BTreeMap;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use async_trait::async_trait;

#[cfg(feature = "profiling-pprof")]
use crate::{infra::profiles, shared::time::TimestampMicros};
use crate::{infra::profiles::NormalizedProfile, shared::self_telemetry::set_profile_available};

#[derive(Debug, Clone)]
pub struct CapturedProfile {
    /// canonical、未 gzip 的 `perftools.profiles.Profile` protobuf。
    pub raw_pprof: Vec<u8>,
    pub normalized: NormalizedProfile,
}

#[derive(Debug, thiserror::Error)]
pub enum CaptureError {
    #[error("profile duration must be between 1 and 120 seconds")]
    InvalidDuration,
    #[error("a CPU profile capture is already running")]
    Busy,
    #[error("{0} profiling is not available in this build")]
    Unavailable(&'static str),
    #[error("profile capture failed: {0}")]
    Failed(String),
}

#[async_trait]
trait CaptureBackend: Send + Sync {
    fn available(&self) -> bool;
    async fn capture(&self, seconds: u32) -> Result<CapturedProfile, CaptureError>;
}

static CPU_CAPTURE_ACTIVE: AtomicBool = AtomicBool::new(false);

#[cfg(all(feature = "profiling-pprof", unix))]
const CPU_SAMPLE_FREQUENCY_HZ: i32 = 99;

struct CpuCapturePermit;

impl CpuCapturePermit {
    fn try_acquire() -> Result<Self, CaptureError> {
        CPU_CAPTURE_ACTIVE
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map(|_| Self)
            .map_err(|_| CaptureError::Busy)
    }
}

impl Drop for CpuCapturePermit {
    fn drop(&mut self) {
        CPU_CAPTURE_ACTIVE.store(false, Ordering::Release);
    }
}

pub struct ProfilingService {
    cpu: Arc<dyn CaptureBackend>,
    heap: Arc<dyn CaptureBackend>,
}

impl ProfilingService {
    pub fn new() -> Arc<Self> {
        let service = Arc::new(Self {
            cpu: Arc::new(NativeCpuBackend),
            heap: Arc::new(NativeHeapBackend),
        });
        set_profile_available("cpu", service.cpu_available());
        set_profile_available("heap", service.heap_available());
        service
    }

    #[cfg(test)]
    fn with_backends(cpu: Arc<dyn CaptureBackend>, heap: Arc<dyn CaptureBackend>) -> Arc<Self> {
        Arc::new(Self { cpu, heap })
    }

    pub fn cpu_available(&self) -> bool {
        self.cpu.available()
    }

    pub fn heap_available(&self) -> bool {
        self.heap.available()
    }

    pub async fn capture_cpu(&self, seconds: u32) -> Result<CapturedProfile, CaptureError> {
        if !(1..=120).contains(&seconds) {
            return Err(CaptureError::InvalidDuration);
        }
        if !self.cpu.available() {
            return Err(CaptureError::Unavailable("cpu"));
        }
        let _permit = CpuCapturePermit::try_acquire()?;
        self.cpu.capture(seconds).await
    }

    pub async fn capture_heap(&self) -> Result<CapturedProfile, CaptureError> {
        if !self.heap.available() {
            return Err(CaptureError::Unavailable("heap"));
        }
        self.heap.capture(0).await
    }
}

struct NativeCpuBackend;

#[async_trait]
impl CaptureBackend for NativeCpuBackend {
    fn available(&self) -> bool {
        cfg!(all(feature = "profiling-pprof", unix))
    }

    async fn capture(&self, seconds: u32) -> Result<CapturedProfile, CaptureError> {
        #[cfg(all(feature = "profiling-pprof", unix))]
        {
            let started_at = TimestampMicros::now().0;
            tokio::task::spawn_blocking(move || {
                use pprof::protos::Message as _;

                let mut builder =
                    pprof::ProfilerGuardBuilder::default().frequency(CPU_SAMPLE_FREQUENCY_HZ);
                #[cfg(any(
                    target_arch = "x86_64",
                    target_arch = "aarch64",
                    target_arch = "riscv64",
                    target_arch = "loongarch64"
                ))]
                {
                    builder = builder.blocklist(&["libc", "libgcc", "pthread", "vdso"]);
                }
                let guard = builder
                    .build()
                    .map_err(|error| CaptureError::Failed(error.to_string()))?;
                std::thread::sleep(std::time::Duration::from_secs(seconds as u64));
                let report = guard
                    .report()
                    .build()
                    .map_err(|error| CaptureError::Failed(error.to_string()))?;
                let profile = report
                    .pprof()
                    .map_err(|error| CaptureError::Failed(error.to_string()))?;
                let raw_pprof = profile.encode_to_vec();
                let decoded = profiles::decode_pprof(&raw_pprof)
                    .map_err(|error| CaptureError::Failed(error.to_string()))?;
                let mut normalized =
                    profiles::normalize_pprof(&decoded, "molesignal", &BTreeMap::new());
                normalized.start_time_micros = started_at;
                normalized.duration_nanos = i64::from(seconds).saturating_mul(1_000_000_000);
                Ok(CapturedProfile {
                    raw_pprof,
                    normalized,
                })
            })
            .await
            .map_err(|error| CaptureError::Failed(error.to_string()))?
        }
        #[cfg(not(all(feature = "profiling-pprof", unix)))]
        {
            let _ = seconds;
            Err(CaptureError::Unavailable("cpu"))
        }
    }
}

struct NativeHeapBackend;

#[async_trait]
impl CaptureBackend for NativeHeapBackend {
    fn available(&self) -> bool {
        cfg!(all(
            feature = "profiling-pprof",
            feature = "jemalloc",
            target_os = "linux",
            target_env = "gnu"
        ))
    }

    async fn capture(&self, _seconds: u32) -> Result<CapturedProfile, CaptureError> {
        #[cfg(all(
            feature = "profiling-pprof",
            feature = "jemalloc",
            target_os = "linux",
            target_env = "gnu"
        ))]
        {
            tokio::task::spawn_blocking(|| {
                let dump = dump_heap_profile_native().map_err(CaptureError::Failed)?;
                heap_dump_to_capture(&dump)
            })
            .await
            .map_err(|error| CaptureError::Failed(error.to_string()))?
        }
        #[cfg(not(all(
            feature = "profiling-pprof",
            feature = "jemalloc",
            target_os = "linux",
            target_env = "gnu"
        )))]
        {
            Err(CaptureError::Unavailable("heap"))
        }
    }
}

#[cfg(all(
    feature = "profiling-pprof",
    any(
        test,
        all(feature = "jemalloc", target_os = "linux", target_env = "gnu")
    )
))]
fn heap_dump_to_capture(dump: &[u8]) -> Result<CapturedProfile, CaptureError> {
    let parsed = pprof_util::parse_jeheap(std::io::BufReader::new(dump), None)
        .map_err(|error| CaptureError::Failed(format!("parse jemalloc heap: {error}")))?;
    let gzipped = parsed.to_pprof(("inuse_space", "bytes"), ("space", "bytes"), None);
    let raw_pprof = profiles::decompress_pprof_input(&gzipped)
        .map_err(|error| CaptureError::Failed(error.to_string()))?;
    let decoded = profiles::decode_pprof(&raw_pprof)
        .map_err(|error| CaptureError::Failed(error.to_string()))?;
    let mut normalized = profiles::normalize_pprof(&decoded, "molesignal", &BTreeMap::new());
    normalized.start_time_micros = TimestampMicros::now().0;
    Ok(CapturedProfile {
        raw_pprof,
        normalized,
    })
}

/// jemalloc 原生 heap_v2 dump。仅支持默认发布目标 Linux glibc + jemalloc。
#[cfg(all(target_os = "linux", target_env = "gnu", feature = "jemalloc"))]
pub fn dump_heap_profile_native() -> Result<Vec<u8>, String> {
    use std::{
        ffi::{CString, c_char},
        sync::atomic::{AtomicU64, Ordering},
    };

    use tikv_jemalloc_ctl::raw;

    let prof_built = unsafe { raw::read::<bool>(b"opt.prof\0") }
        .map_err(|error| format!("read opt.prof: {error}"))?;
    if !prof_built {
        return Err("jemalloc built/started without prof (opt.prof=false)".into());
    }
    unsafe { raw::write::<bool>(b"prof.active\0", true) }
        .map_err(|error| format!("write prof.active: {error}"))?;

    static SEQUENCE: AtomicU64 = AtomicU64::new(0);
    let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!(
        "molesignal-heap.{}.{}.prof",
        std::process::id(),
        sequence
    ));
    let path_str = path.to_str().ok_or("non-utf8 temp path")?;
    let c_path = CString::new(path_str).map_err(|error| format!("path to CString: {error}"))?;
    let dumped = unsafe { raw::write::<*const c_char>(b"prof.dump\0", c_path.as_ptr()) }
        .map_err(|error| format!("write prof.dump: {error}"))
        .and_then(|()| std::fs::read(&path).map_err(|error| format!("read dump file: {error}")));
    let _ = std::fs::remove_file(&path);
    dumped
}

#[cfg(test)]
mod tests {
    use std::sync::OnceLock;

    use tokio::sync::Notify;

    use super::*;
    use crate::infra::profiles::{self, Frame, ProfileType, Sample, ValueType};

    fn profile() -> CapturedProfile {
        let normalized = NormalizedProfile {
            service: "molesignal".into(),
            profile_type: ProfileType::Cpu,
            sample_types: vec![ValueType::new("samples", "count")],
            default_value_index: 0,
            samples: vec![Sample {
                stack: vec![Frame {
                    function: "work".into(),
                    file: None,
                    line: None,
                    address: None,
                    build_id: None,
                }],
                values: vec![1],
                labels: BTreeMap::new(),
            }],
            period_type: None,
            period: 0,
            start_time_micros: 1,
            duration_nanos: 1,
            labels: BTreeMap::new(),
            trace_id: None,
            span_id: None,
        };
        CapturedProfile {
            raw_pprof: profiles::encode_pprof_raw(&normalized).unwrap(),
            normalized,
        }
    }

    fn test_lock() -> &'static tokio::sync::Mutex<()> {
        static LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
    }

    struct ImmediateBackend;

    #[async_trait]
    impl CaptureBackend for ImmediateBackend {
        fn available(&self) -> bool {
            true
        }

        async fn capture(&self, _seconds: u32) -> Result<CapturedProfile, CaptureError> {
            Ok(profile())
        }
    }

    struct BlockingBackend {
        entered: Arc<Notify>,
        release: Arc<Notify>,
    }

    #[async_trait]
    impl CaptureBackend for BlockingBackend {
        fn available(&self) -> bool {
            true
        }

        async fn capture(&self, _seconds: u32) -> Result<CapturedProfile, CaptureError> {
            self.entered.notify_one();
            self.release.notified().await;
            Ok(profile())
        }
    }

    #[tokio::test]
    async fn duration_is_validated_without_clamping() {
        let _test_guard = test_lock().lock().await;
        let service =
            ProfilingService::with_backends(Arc::new(ImmediateBackend), Arc::new(ImmediateBackend));
        assert!(matches!(
            service.capture_cpu(0).await,
            Err(CaptureError::InvalidDuration)
        ));
        assert!(matches!(
            service.capture_cpu(121).await,
            Err(CaptureError::InvalidDuration)
        ));
        assert!(service.capture_cpu(1).await.is_ok());
    }

    #[tokio::test]
    async fn concurrent_cpu_capture_is_rejected() {
        let _test_guard = test_lock().lock().await;
        let entered = Arc::new(Notify::new());
        let release = Arc::new(Notify::new());
        let service = ProfilingService::with_backends(
            Arc::new(BlockingBackend {
                entered: entered.clone(),
                release: release.clone(),
            }),
            Arc::new(ImmediateBackend),
        );
        let first = {
            let service = service.clone();
            tokio::spawn(async move { service.capture_cpu(1).await })
        };
        entered.notified().await;
        assert!(matches!(
            service.capture_cpu(1).await,
            Err(CaptureError::Busy)
        ));
        release.notify_one();
        assert!(first.await.unwrap().is_ok());
    }

    #[cfg(feature = "profiling-pprof")]
    #[test]
    fn jemalloc_fixture_converts_to_canonical_pprof() {
        let fixture = b"heap_v2/524288\n\
            t*: 1: 64 [0: 0]\n\
            @ 0x1000 0x2000\n\
            t*: 1: 64 [0: 0]\n\
            MAPPED_LIBRARIES:\n";
        let captured = heap_dump_to_capture(fixture).unwrap();
        assert!(profiles::decode_pprof(&captured.raw_pprof).is_ok());
        assert_eq!(captured.normalized.profile_type, ProfileType::InuseSpace);
        assert_eq!(captured.normalized.samples.len(), 1);
    }

    #[cfg(all(feature = "profiling-pprof", unix))]
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn native_cpu_capture_emits_decodable_pprof() {
        let _test_guard = test_lock().lock().await;
        let service = ProfilingService::new();
        let captured = service.capture_cpu(1).await.unwrap();
        let decoded = profiles::decode_pprof(&captured.raw_pprof).unwrap();
        assert_eq!(
            decoded.period,
            1_000_000_000 / i64::from(CPU_SAMPLE_FREQUENCY_HZ)
        );
        assert_eq!(captured.normalized.profile_type, ProfileType::Cpu);
        assert_eq!(captured.normalized.duration_nanos, 1_000_000_000);
    }

    #[cfg(not(all(
        feature = "profiling-pprof",
        feature = "jemalloc",
        target_os = "linux",
        target_env = "gnu"
    )))]
    #[tokio::test]
    async fn unsupported_native_heap_capture_is_explicit() {
        let service = ProfilingService::new();
        assert!(matches!(
            service.capture_heap().await,
            Err(CaptureError::Unavailable("heap"))
        ));
    }
}

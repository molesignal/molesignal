// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Continuous Profiling 的内部规范表示与编解码。
//!
//! 三种摄取来源——pprof / JFR 直传、Pyroscope 兼容 `/ingest`、OTLP Profiles
//! ——都先归一化到 [`NormalizedProfile`]（语义贴近 pprof，便于无损往返），再走
//! 统一的双路落盘：规范 pprof + zstd 归档到 object store，元数据行进
//! `StreamType::Profiles` 流。
//!
//! 火焰图聚合逻辑位于 [`merge`]，其余规范模型与编解码入口保留在本模块。

use std::{
    collections::{BTreeMap, HashMap},
    sync::Arc,
};

use bytes::Bytes;
use object_store::{ObjectStore, ObjectStoreExt, path::Path as ObjPath};
use prost::Message;
use serde_json::{Map, Value};

use crate::{
    domain::ingestion::RawEvent,
    protocol::pprof::profiles as pb,
    shared::{Error, Result, ids::Id, time::TimestampMicros},
};

pub mod merge;

/// 一份 profile 的某一路采样值的类型与单位，对应 pprof `ValueType`。
///
/// 例如 CPU profile 常见 `("cpu", "nanoseconds")` / `("samples", "count")`，
/// 内存 profile 常见 `("alloc_space", "bytes")`。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ValueType {
    /// 值类型名（pprof string_table 解引用后的明文）。
    pub ty: String,
    /// 单位（`nanoseconds` / `bytes` / `count` / ...）。
    pub unit: String,
}

impl ValueType {
    pub fn new(ty: impl Into<String>, unit: impl Into<String>) -> Self {
        Self {
            ty: ty.into(),
            unit: unit.into(),
        }
    }
}

/// 栈中的一帧。已符号化来源带 `function`；未符号化来源（如 eBPF 原生栈）
/// 仅有 `address` + `build_id`，此时 `function` 为空，profile 会被标
/// `unsymbolized = true`。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Frame {
    /// 函数名；未符号化时为空串。
    pub function: String,
    /// 源文件名（可空）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    /// 行号（可空）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<i64>,
    /// 指令地址（未符号化栈用于区分帧）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub address: Option<u64>,
    /// 所属 mapping 的 build_id（未符号化栈关联符号文件用）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub build_id: Option<String>,
}

impl Frame {
    /// 该帧是否已符号化（有函数名）。
    pub fn is_symbolized(&self) -> bool {
        !self.function.is_empty()
    }

    /// 火焰图节点显示名：优先函数名，否则回退到 `0xADDR` 或 `unknown`。
    pub fn display_name(&self) -> String {
        if !self.function.is_empty() {
            self.function.clone()
        } else if let Some(addr) = self.address {
            format!("0x{addr:x}")
        } else {
            "unknown".to_string()
        }
    }
}

/// 一条带权采样：一条调用栈 + 与 `sample_types` 一一对应的值向量 + 样本级标签。
///
/// `stack[0]` 是根（最外层 / 调用链底部）帧，末尾是叶子（on-CPU）。与 folded 文本
/// 同序，便于火焰图自根向下建树；pprof 编解码内部做叶/根翻转保证落盘语义正确。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Sample {
    /// 根在前（`stack[0]` 最外层）的调用栈。
    pub stack: Vec<Frame>,
    /// 与 [`NormalizedProfile::sample_types`] 对齐的值向量。
    pub values: Vec<i64>,
    /// 样本级标签（pprof `Label`）；trace 关联常以 `trace_id` / `span_id` 出现于此。
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub labels: BTreeMap<String, String>,
}

/// profile 的语义类型，用于元数据行的 `profile_type` 列与前端筛选。
///
/// `Other` 兜底任意来源类型名，保证未知类型也能被接纳而非拒收。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProfileType {
    Cpu,
    Wall,
    AllocSpace,
    AllocObjects,
    InuseSpace,
    InuseObjects,
    Lock,
    Contention,
    Goroutines,
    Exceptions,
    Other(String),
}

impl ProfileType {
    /// 稳定字符串表示（落入元数据 `profile_type` 列）。
    pub fn as_str(&self) -> &str {
        match self {
            ProfileType::Cpu => "cpu",
            ProfileType::Wall => "wall",
            ProfileType::AllocSpace => "alloc_space",
            ProfileType::AllocObjects => "alloc_objects",
            ProfileType::InuseSpace => "inuse_space",
            ProfileType::InuseObjects => "inuse_objects",
            ProfileType::Lock => "lock",
            ProfileType::Contention => "contention",
            ProfileType::Goroutines => "goroutines",
            ProfileType::Exceptions => "exceptions",
            ProfileType::Other(s) => s.as_str(),
        }
    }

    /// 从采样类型名 / Pyroscope app 名后缀推断 profile 类型；未知归 `Other`。
    pub fn from_name(name: &str) -> Self {
        match name.trim().to_ascii_lowercase().as_str() {
            "cpu" | "samples" | "process_cpu" | "cpu_samples" => ProfileType::Cpu,
            "wall" | "wall_clock" | "wallclock" => ProfileType::Wall,
            "alloc_space" | "alloc-space" | "allocspace" | "memory_alloc_space" => {
                ProfileType::AllocSpace
            }
            "alloc_objects" | "alloc-objects" | "allocobjects" => ProfileType::AllocObjects,
            "inuse_space" | "inuse-space" | "inusespace" => ProfileType::InuseSpace,
            "inuse_objects" | "inuse-objects" | "inuseobjects" => ProfileType::InuseObjects,
            "lock" | "mutex" | "block" | "delay" => ProfileType::Lock,
            "contention" | "contentions" => ProfileType::Contention,
            "goroutine" | "goroutines" => ProfileType::Goroutines,
            "exception" | "exceptions" => ProfileType::Exceptions,
            other if !other.is_empty() => ProfileType::Other(other.to_string()),
            _ => ProfileType::Other("unknown".to_string()),
        }
    }
}

/// 三协议归一化后的统一 profile 表示。
///
/// 字段贴近 pprof 语义，便于 pprof 来源无损往返；非 pprof 来源
/// （folded / lines / JFR / OTLP）由各自适配器构造此结构，再统一编码为规范 pprof
/// 归档。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct NormalizedProfile {
    /// 服务名（来自 OTLP resource `service.name` / Pyroscope `name` / 上传参数）。
    pub service: String,
    /// profile 语义类型。
    pub profile_type: ProfileType,
    /// 各路采样值的类型与单位（与每个 `Sample.values` 对齐）。
    pub sample_types: Vec<ValueType>,
    /// 用于火焰图聚合的主采样值下标（pprof `default_sample_type`）。
    pub default_value_index: usize,
    /// 带权采样集合。
    pub samples: Vec<Sample>,
    /// 采样事件间隔的类型（pprof `period_type`，可空）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub period_type: Option<ValueType>,
    /// 采样事件间隔（pprof `period`）。
    pub period: i64,
    /// profile 时间窗口起点（micros since epoch）。
    pub start_time_micros: i64,
    /// 采样时长（纳秒）。
    pub duration_nanos: i64,
    /// profile 级标签（如 `pod` / `version` / `region`）。
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub labels: BTreeMap<String, String>,
    /// 关联 trace（可空）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trace_id: Option<String>,
    /// 关联 span（可空）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub span_id: Option<String>,
}

impl NormalizedProfile {
    /// 样本条数。
    pub fn sample_count(&self) -> usize {
        self.samples.len()
    }

    /// 主采样值的合计（用于元数据 `total_value` 列与排序 / 概览）。
    pub fn total_value(&self) -> i64 {
        let idx = self.default_value_index;
        self.samples
            .iter()
            .map(|s| s.values.get(idx).copied().unwrap_or(0))
            .sum()
    }

    /// 是否含未符号化帧（任一样本的任一帧无函数名）。
    pub fn unsymbolized(&self) -> bool {
        self.samples
            .iter()
            .any(|s| s.stack.iter().any(|f| !f.is_symbolized()))
    }

    /// 主采样值的类型名，用作 profile_type 推断回退。
    pub fn default_sample_type_name(&self) -> Option<&str> {
        self.sample_types
            .get(self.default_value_index)
            .map(|vt| vt.ty.as_str())
    }
}

// ===== pprof 编解码（task 2.1）=====
//
// 解码接受 gzip 或裸 protobuf；编码产出 gzip pprof（生态约定）。pprof 来源走"归档
// 原始字节"保证无损往返；非 pprof 来源（folded / lines / JFR / OTLP）由各适配器
// 构造 [`NormalizedProfile`] 后用 [`encode_pprof`] 产出规范 pprof 再归档。

const GZIP_MAGIC: [u8; 2] = [0x1f, 0x8b];

/// 若是 gzip（magic `1f 8b`）则解压，否则原样返回（裸 protobuf）。
fn maybe_gunzip(bytes: &[u8]) -> Result<Vec<u8>> {
    if bytes.len() >= 2 && bytes[0] == GZIP_MAGIC[0] && bytes[1] == GZIP_MAGIC[1] {
        use std::io::Read;
        let mut decoder = flate2::read::GzDecoder::new(bytes);
        let mut out = Vec::new();
        decoder
            .read_to_end(&mut out)
            .map_err(|e| Error::invalid(format!("pprof gunzip: {e}")))?;
        Ok(out)
    } else {
        Ok(bytes.to_vec())
    }
}

/// gzip 压缩裸 pprof protobuf（下载 / 编码产物的生态约定形态）。
pub fn gzip_pprof(bytes: &[u8]) -> Result<Vec<u8>> {
    use std::io::Write;
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder
        .write_all(bytes)
        .map_err(|e| Error::internal(format!("pprof gzip: {e}")))?;
    encoder
        .finish()
        .map_err(|e| Error::internal(format!("pprof gzip finish: {e}")))
}

/// 解码 pprof（gzip 或裸 protobuf）为 prost `Profile`。
pub fn decode_pprof(bytes: &[u8]) -> Result<pb::Profile> {
    let raw = maybe_gunzip(bytes)?;
    pb::Profile::decode(raw.as_slice()).map_err(|e| Error::invalid(format!("pprof decode: {e}")))
}

/// 上传 pprof 归一为裸 protobuf（gunzip if needed），供原样无损归档。
pub fn decompress_pprof_input(bytes: &[u8]) -> Result<Vec<u8>> {
    maybe_gunzip(bytes)
}

/// pprof `Profile` → [`NormalizedProfile`]。`service` / `extra_labels` 由调用方
/// （upload 参数 / Pyroscope `name`）提供，pprof 本身不携带服务名。
pub fn normalize_pprof(
    profile: &pb::Profile,
    service: impl Into<String>,
    extra_labels: &BTreeMap<String, String>,
) -> NormalizedProfile {
    let st = |i: i64| -> &str {
        profile
            .string_table
            .get(i as usize)
            .map(String::as_str)
            .unwrap_or("")
    };
    let funcs: HashMap<u64, &pb::Function> = profile.function.iter().map(|f| (f.id, f)).collect();
    let locs: HashMap<u64, &pb::Location> = profile.location.iter().map(|l| (l.id, l)).collect();
    let mappings: HashMap<u64, &pb::Mapping> = profile.mapping.iter().map(|m| (m.id, m)).collect();

    let sample_types: Vec<ValueType> = profile
        .sample_type
        .iter()
        .map(|vt| ValueType::new(st(vt.r#type), st(vt.unit)))
        .collect();

    let mut samples = Vec::with_capacity(profile.sample.len());
    for s in &profile.sample {
        let mut stack = Vec::new();
        for loc_id in &s.location_id {
            let Some(loc) = locs.get(loc_id) else {
                continue;
            };
            if loc.line.is_empty() {
                // 未符号化帧：仅地址 + build_id。
                let build_id = (loc.mapping_id != 0)
                    .then(|| mappings.get(&loc.mapping_id))
                    .flatten()
                    .map(|m| st(m.build_id).to_string())
                    .filter(|s| !s.is_empty());
                stack.push(Frame {
                    function: String::new(),
                    file: None,
                    line: None,
                    address: (loc.address != 0).then_some(loc.address),
                    build_id,
                });
            } else {
                // 一个 location 多行 = 内联帧，line[0] 最内层。
                for ln in &loc.line {
                    let func = funcs.get(&ln.function_id);
                    let function = func.map(|f| st(f.name).to_string()).unwrap_or_default();
                    let file = func
                        .map(|f| st(f.filename))
                        .filter(|s| !s.is_empty())
                        .map(|s| s.to_string());
                    stack.push(Frame {
                        function,
                        file,
                        line: (ln.line != 0).then_some(ln.line),
                        address: None,
                        build_id: None,
                    });
                }
            }
        }
        // pprof 是叶子在前；翻转为根在前（NormalizedProfile 约定）。
        stack.reverse();
        let mut labels = BTreeMap::new();
        for lab in &s.label {
            let key = st(lab.key);
            if key.is_empty() {
                continue;
            }
            let value = if lab.str != 0 {
                st(lab.str).to_string()
            } else {
                lab.num.to_string()
            };
            labels.insert(key.to_string(), value);
        }
        samples.push(Sample {
            stack,
            values: s.value.clone(),
            labels,
        });
    }

    // 主采样值下标：default_sample_type 命中则用之，否则取最后一路（pprof 约定）。
    let default_value_index = if profile.default_sample_type != 0 {
        let name = st(profile.default_sample_type);
        sample_types
            .iter()
            .position(|vt| vt.ty == name)
            .unwrap_or_else(|| sample_types.len().saturating_sub(1))
    } else {
        sample_types.len().saturating_sub(1)
    };

    let period_type = profile
        .period_type
        .as_ref()
        .map(|vt| ValueType::new(st(vt.r#type), st(vt.unit)));

    let profile_type = ProfileType::from_name(
        sample_types
            .get(default_value_index)
            .map(|vt| vt.ty.as_str())
            .unwrap_or(""),
    );

    let (trace_id, span_id) = extract_trace_ids(&samples, extra_labels);

    NormalizedProfile {
        service: service.into(),
        profile_type,
        sample_types,
        default_value_index,
        samples,
        period_type,
        period: profile.period,
        start_time_micros: profile.time_nanos / 1_000,
        duration_nanos: profile.duration_nanos,
        labels: extra_labels.clone(),
        trace_id,
        span_id,
    }
}

/// 从样本级 / profile 级标签提取 `trace_id` / `span_id`（task 3.3）。
fn extract_trace_ids(
    samples: &[Sample],
    profile_labels: &BTreeMap<String, String>,
) -> (Option<String>, Option<String>) {
    const TRACE_KEYS: [&str; 3] = ["trace_id", "traceID", "traceid"];
    const SPAN_KEYS: [&str; 3] = ["span_id", "spanID", "spanid"];
    let pick = |labels: &BTreeMap<String, String>, keys: &[&str]| -> Option<String> {
        keys.iter()
            .find_map(|k| labels.get(*k))
            .filter(|v| !v.is_empty())
            .cloned()
    };
    let mut trace = pick(profile_labels, &TRACE_KEYS);
    let mut span = pick(profile_labels, &SPAN_KEYS);
    for s in samples {
        if trace.is_none() {
            trace = pick(&s.labels, &TRACE_KEYS);
        }
        if span.is_none() {
            span = pick(&s.labels, &SPAN_KEYS);
        }
        if trace.is_some() && span.is_some() {
            break;
        }
    }
    (trace, span)
}

/// pprof `string_table` 内联器：index 0 恒为 ""。
struct Interner {
    strings: Vec<String>,
    map: HashMap<String, i64>,
}

impl Interner {
    fn new() -> Self {
        let mut map = HashMap::new();
        map.insert(String::new(), 0);
        Self {
            strings: vec![String::new()],
            map,
        }
    }

    fn intern(&mut self, s: &str) -> i64 {
        if let Some(&i) = self.map.get(s) {
            return i;
        }
        let i = self.strings.len() as i64;
        self.strings.push(s.to_string());
        self.map.insert(s.to_string(), i);
        i
    }
}

/// [`NormalizedProfile`] → 规范 pprof（gzip protobuf）。供非 pprof 来源归档 /
/// 下载用。裸 protobuf 版见 [`encode_pprof_raw`]。
pub fn encode_pprof(p: &NormalizedProfile) -> Result<Vec<u8>> {
    gzip_pprof(&encode_pprof_raw(p)?)
}

/// [`NormalizedProfile`] → 裸 pprof protobuf（无 gzip）。归档前的规范形态。
pub fn encode_pprof_raw(p: &NormalizedProfile) -> Result<Vec<u8>> {
    let mut interner = Interner::new();
    let sample_type: Vec<pb::ValueType> = p
        .sample_types
        .iter()
        .map(|vt| pb::ValueType {
            r#type: interner.intern(&vt.ty),
            unit: interner.intern(&vt.unit),
        })
        .collect();

    let mut functions: Vec<pb::Function> = Vec::new();
    let mut fn_ids: HashMap<String, u64> = HashMap::new();
    let mut locations: Vec<pb::Location> = Vec::new();
    let mut loc_ids: HashMap<String, u64> = HashMap::new();
    let mut samples_out: Vec<pb::Sample> = Vec::with_capacity(p.samples.len());

    for s in &p.samples {
        let mut location_id = Vec::with_capacity(s.stack.len());
        // NormalizedProfile 根在前；pprof location_id 叶子在前，翻转写入。
        for fr in s.stack.iter().rev() {
            // 先 intern 字符串，避免与 entry() 闭包争借 interner。
            let name_idx = if fr.function.is_empty() {
                0
            } else {
                interner.intern(&fr.function)
            };
            let file_idx = fr.file.as_deref().map(|f| interner.intern(f)).unwrap_or(0);
            let fn_id = if fr.function.is_empty() {
                0
            } else if let Some(&id) = fn_ids.get(&fr.function) {
                id
            } else {
                let id = functions.len() as u64 + 1;
                functions.push(pb::Function {
                    id,
                    name: name_idx,
                    system_name: 0,
                    filename: file_idx,
                    start_line: 0,
                });
                fn_ids.insert(fr.function.clone(), id);
                id
            };
            let line = fr.line.unwrap_or(0);
            let addr = fr.address.unwrap_or(0);
            let key = format!("{fn_id}|{line}|{addr}");
            let loc_id = if let Some(&id) = loc_ids.get(&key) {
                id
            } else {
                let id = locations.len() as u64 + 1;
                let lines = if fn_id != 0 {
                    vec![pb::Line {
                        function_id: fn_id,
                        line,
                        column: 0,
                    }]
                } else {
                    Vec::new()
                };
                locations.push(pb::Location {
                    id,
                    mapping_id: 0,
                    address: addr,
                    line: lines,
                    is_folded: false,
                });
                loc_ids.insert(key, id);
                id
            };
            location_id.push(loc_id);
        }
        let label: Vec<pb::Label> = s
            .labels
            .iter()
            .map(|(k, v)| pb::Label {
                key: interner.intern(k),
                str: interner.intern(v),
                num: 0,
                num_unit: 0,
            })
            .collect();
        samples_out.push(pb::Sample {
            location_id,
            value: s.values.clone(),
            label,
        });
    }

    let period_type = p.period_type.as_ref().map(|vt| pb::ValueType {
        r#type: interner.intern(&vt.ty),
        unit: interner.intern(&vt.unit),
    });
    let default_sample_type = p
        .sample_types
        .get(p.default_value_index)
        .map(|vt| interner.intern(&vt.ty))
        .unwrap_or(0);

    let profile = pb::Profile {
        sample_type,
        sample: samples_out,
        location: locations,
        function: functions,
        time_nanos: p.start_time_micros.saturating_mul(1_000),
        duration_nanos: p.duration_nanos,
        period_type,
        period: p.period,
        default_sample_type,
        string_table: interner.strings,
        ..Default::default()
    };

    let mut buf = Vec::new();
    profile
        .encode(&mut buf)
        .map_err(|e| Error::internal(format!("pprof encode: {e}")))?;
    Ok(buf)
}

// ===== 归档 + 元数据落盘（task 3.1 / 3.2 / 3.3）=====

/// 归档对象 key：`profiles/<org_id>/<service>/<profile_type>/<yyyymmdd>/<profile_id>.pprof.zst`。
pub fn archive_object_key(
    org_id: &Id,
    service: &str,
    profile_type: &str,
    start_time_micros: i64,
    profile_id: &Id,
) -> String {
    format!(
        "profiles/{}/{}/{}/{}/{}.pprof.zst",
        org_id.0,
        sanitize_key_segment(service),
        sanitize_key_segment(profile_type),
        yyyymmdd(start_time_micros),
        profile_id.0,
    )
}

/// 把非路径安全字符折叠为 `_`，空串回退 `unknown`。
fn sanitize_key_segment(s: &str) -> String {
    let out: String = s
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if out.is_empty() {
        "unknown".to_string()
    } else {
        out
    }
}

/// micros since epoch → `YYYYMMDD`（civil_from_days，Howard Hinnant 算法，无依赖）。
fn yyyymmdd(micros: i64) -> String {
    let days = micros.div_euclid(86_400_000_000);
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097; // [0, 146096]
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = doy - (153 * mp + 2) / 5 + 1; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 }; // [1, 12]
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}{m:02}{d:02}")
}

/// zstd 压缩（level 3，与 RUM replay 归档一致）。
pub fn zstd_compress(bytes: &[u8]) -> Result<Vec<u8>> {
    zstd::stream::encode_all(bytes, 3).map_err(|e| Error::internal(format!("profile zstd: {e}")))
}

/// zstd 解压。
pub fn zstd_decompress(bytes: &[u8]) -> Result<Vec<u8>> {
    zstd::stream::decode_all(bytes).map_err(|e| Error::internal(format!("profile unzstd: {e}")))
}

/// 把裸 pprof protobuf zstd 归档到 object store，返回归档字节数（计入存储配额）。
pub async fn put_archive(
    object_store: &Arc<dyn ObjectStore>,
    key: &str,
    raw_pprof: &[u8],
) -> Result<u64> {
    let compressed = zstd_compress(raw_pprof)?;
    let bytes = compressed.len() as u64;
    let path =
        ObjPath::parse(key).map_err(|e| Error::internal(format!("profile object path: {e}")))?;
    object_store
        .put(&path, Bytes::from(compressed).into())
        .await
        .map_err(|e| Error::internal(format!("profile archive put: {e}")))?;
    Ok(bytes)
}

/// 读回归档对象并 zstd 解压为裸 pprof protobuf（聚合 / 下载用）。
pub async fn get_archive(object_store: &Arc<dyn ObjectStore>, key: &str) -> Result<Vec<u8>> {
    let path =
        ObjPath::parse(key).map_err(|e| Error::internal(format!("profile object path: {e}")))?;
    let got = object_store
        .get(&path)
        .await
        .map_err(|e| Error::internal(format!("profile archive get: {e}")))?;
    let bytes = got
        .bytes()
        .await
        .map_err(|e| Error::internal(format!("profile archive read: {e}")))?;
    zstd_decompress(&bytes)
}

/// 从归档 key 提取 `YYYYMMDD` 日期段；非归档 / 异常 key 返回 `None`。
/// key 形如 `profiles/<org>/<svc>/<type>/<yyyymmdd>/<id>.pprof.zst`，日期在第 5 段。
pub fn archive_key_date(key: &str) -> Option<&str> {
    let seg = key.split('/').nth(4)?;
    (seg.len() == 8 && seg.bytes().all(|b| b.is_ascii_digit())).then_some(seg)
}

/// 删除某 org `profiles/<org>/` 前缀下日期早于 retention cutoff 的归档 blob。
///
/// profiles 元数据行随 stream retention 由 parquet_file_meta sweep 自动清理；归档 blob 是
/// object store 旁路对象、不在 parquet_file_meta 内，故 retention sweep 需调用本函数一并清理
/// （storage spec：到期同时清理 parquet 元数据与归档 blob）。归档按 `yyyymmdd` 分桶，
/// 比较到日级即可。best-effort：单个删除失败仅跳过，下一轮 sweep 兜底；返回成功删除数。
pub async fn sweep_expired_archives(
    object_store: &Arc<dyn ObjectStore>,
    org_id: &Id,
    cutoff_micros: i64,
) -> Result<usize> {
    use futures::TryStreamExt;
    let cutoff = yyyymmdd(cutoff_micros);
    let prefix = ObjPath::parse(format!("profiles/{}", org_id.0))
        .map_err(|e| Error::internal(format!("profile sweep prefix: {e}")))?;
    let mut listing = object_store.list(Some(&prefix));
    let mut deleted = 0usize;
    while let Some(meta) = listing
        .try_next()
        .await
        .map_err(|e| Error::internal(format!("profile sweep list: {e}")))?
    {
        let Some(date) = archive_key_date(meta.location.as_ref()) else {
            continue;
        };
        if date < cutoff.as_str() && object_store.delete(&meta.location).await.is_ok() {
            deleted += 1;
        }
    }
    Ok(deleted)
}

/// 构造 profiles 元数据流的一行 [`RawEvent`]（task 3.2）。
///
/// `timestamp` 由调用方决定（profile 自带时间则用之，否则用接收时间）。
pub fn metadata_event(
    profile: &NormalizedProfile,
    object_key: &str,
    archived_bytes: u64,
    timestamp: TimestampMicros,
) -> RawEvent {
    let mut fields = Map::new();
    fields.insert("service".into(), Value::from(profile.service.clone()));
    fields.insert(
        "profile_type".into(),
        Value::from(profile.profile_type.as_str().to_string()),
    );
    fields.insert("duration_nanos".into(), Value::from(profile.duration_nanos));
    fields.insert(
        "sample_count".into(),
        Value::from(profile.sample_count() as i64),
    );
    fields.insert("total_value".into(), Value::from(profile.total_value()));
    let labels: Map<String, Value> = profile
        .labels
        .iter()
        .map(|(k, v)| (k.clone(), Value::from(v.clone())))
        .collect();
    fields.insert("labels".into(), Value::Object(labels));
    // trace_id / span_id 始终写入（无关联时为空串）：保证 schema-on-write 建出这两列，
    // 否则首条无 trace 关联的 profile 会让流缺列，后续按列名 SELECT 无法 plan。
    // 查询侧已把空串视作"无关联"（`.filter(|s| !s.is_empty())`）。
    fields.insert(
        "trace_id".into(),
        Value::from(profile.trace_id.clone().unwrap_or_default()),
    );
    fields.insert(
        "span_id".into(),
        Value::from(profile.span_id.clone().unwrap_or_default()),
    );
    fields.insert("object_key".into(), Value::from(object_key.to_string()));
    fields.insert("unsymbolized".into(), Value::from(profile.unsymbolized()));
    fields.insert("archived_bytes".into(), Value::from(archived_bytes as i64));
    RawEvent { timestamp, fields }
}

// ===== folded / lines 文本栈解析（task 2.4）=====

/// 解析 folded / collapsed 文本栈（Brendan Gregg 约定）：每行
/// `frame1;frame2;...;frameN <count>`，`frame1` 为根、`frameN` 为叶子，与
/// [`Sample`] 的根在前约定一致。空行 / `#` 注释行跳过。
///
/// Pyroscope 的 `format=lines` 与 `format=folded` 都走本解析器。
pub fn parse_folded(
    text: &str,
    service: impl Into<String>,
    profile_type_name: &str,
    extra_labels: &BTreeMap<String, String>,
) -> Result<NormalizedProfile> {
    let mut samples = Vec::new();
    for (idx, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (stack_str, count_str) = line
            .rsplit_once(char::is_whitespace)
            .ok_or_else(|| Error::invalid(format!("folded line {}: missing count", idx + 1)))?;
        let count: i64 = count_str.trim().parse().map_err(|_| {
            Error::invalid(format!("folded line {}: bad count '{count_str}'", idx + 1))
        })?;
        let stack: Vec<Frame> = stack_str
            .split(';')
            .map(str::trim)
            .filter(|f| !f.is_empty())
            .map(|f| Frame {
                function: f.to_string(),
                file: None,
                line: None,
                address: None,
                build_id: None,
            })
            .collect();
        if stack.is_empty() {
            continue;
        }
        samples.push(Sample {
            stack,
            values: vec![count],
            labels: BTreeMap::new(),
        });
    }
    if samples.is_empty() {
        return Err(Error::invalid("folded profile contains no samples"));
    }
    let type_name = if profile_type_name.is_empty() {
        "samples"
    } else {
        profile_type_name
    };
    let (trace_id, span_id) = extract_trace_ids(&samples, extra_labels);
    Ok(NormalizedProfile {
        service: service.into(),
        profile_type: ProfileType::from_name(type_name),
        sample_types: vec![ValueType::new(type_name, "count")],
        default_value_index: 0,
        samples,
        period_type: None,
        period: 0,
        start_time_micros: 0,
        duration_nanos: 0,
        labels: extra_labels.clone(),
        trace_id,
        span_id,
    })
}

/// 解析 Pyroscope `name` 参数：`app{key=val,...}`。返回
/// `(service, profile_type_hint, labels)`。末段若是已知 profile 类型
/// （如 `myapp.cpu`）则剥离为 hint，否则整体作为 service。
pub fn parse_pyroscope_name(name: &str) -> (String, Option<String>, BTreeMap<String, String>) {
    let (app, labels_str) = match name.split_once('{') {
        Some((a, rest)) => (a.trim(), rest.trim_end_matches('}')),
        None => (name.trim(), ""),
    };
    let mut labels = BTreeMap::new();
    for pair in labels_str.split(',') {
        let pair = pair.trim();
        if pair.is_empty() {
            continue;
        }
        if let Some((k, v)) = pair.split_once('=') {
            let value = v.trim().trim_matches('"');
            labels.insert(k.trim().to_string(), value.to_string());
        }
    }
    if let Some((prefix, suffix)) = app.rsplit_once('.')
        && !prefix.is_empty()
        && !matches!(ProfileType::from_name(suffix), ProfileType::Other(_))
    {
        return (prefix.to_string(), Some(suffix.to_string()), labels);
    }
    (app.to_string(), None, labels)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn archive_key_date_extracts_day_segment() {
        assert_eq!(
            archive_key_date("profiles/org1/api/cpu/20260618/abc.pprof.zst"),
            Some("20260618")
        );
        // non-8-digit date segment → None
        assert_eq!(
            archive_key_date("profiles/o/s/t/notadate/x.pprof.zst"),
            None
        );
        // too few segments (not an archive key) → None
        assert_eq!(archive_key_date("rum/org1/sess/1.replay"), None);
    }

    #[tokio::test]
    async fn sweep_expired_archives_filters_by_prefix_and_date() {
        use object_store::memory::InMemory;
        let store: Arc<dyn ObjectStore> = Arc::new(InMemory::new());
        let org = Id::from_string("org1");
        put_archive(&store, "profiles/org1/api/cpu/20260101/a.pprof.zst", b"x")
            .await
            .unwrap();
        put_archive(&store, "profiles/org1/api/cpu/20260620/b.pprof.zst", b"y")
            .await
            .unwrap();
        // unrelated prefix is never touched.
        put_archive(&store, "rum/org1/sess/1.replay.zst", b"z")
            .await
            .unwrap();

        // cutoff at epoch (1970) → nothing is older → nothing deleted.
        assert_eq!(sweep_expired_archives(&store, &org, 0).await.unwrap(), 0);

        // cutoff far in the future → both profiles archives are older → both gone.
        let far_future_micros = 7_258_118_400_000_000;
        assert_eq!(
            sweep_expired_archives(&store, &org, far_future_micros)
                .await
                .unwrap(),
            2
        );
        assert!(
            get_archive(&store, "profiles/org1/api/cpu/20260101/a.pprof.zst")
                .await
                .is_err()
        );
        // the unrelated object survives.
        assert!(
            get_archive(&store, "rum/org1/sess/1.replay.zst")
                .await
                .is_ok()
        );
    }

    fn frame(fun: &str) -> Frame {
        Frame {
            function: fun.to_string(),
            file: None,
            line: None,
            address: None,
            build_id: None,
        }
    }

    #[test]
    fn total_value_sums_default_index() {
        let p = NormalizedProfile {
            service: "api".into(),
            profile_type: ProfileType::Cpu,
            sample_types: vec![
                ValueType::new("samples", "count"),
                ValueType::new("cpu", "nanoseconds"),
            ],
            default_value_index: 1,
            samples: vec![
                Sample {
                    stack: vec![frame("a")],
                    values: vec![1, 100],
                    labels: BTreeMap::new(),
                },
                Sample {
                    stack: vec![frame("b")],
                    values: vec![1, 250],
                    labels: BTreeMap::new(),
                },
            ],
            period_type: None,
            period: 0,
            start_time_micros: 0,
            duration_nanos: 0,
            labels: BTreeMap::new(),
            trace_id: None,
            span_id: None,
        };
        assert_eq!(p.sample_count(), 2);
        assert_eq!(p.total_value(), 350);
        assert!(!p.unsymbolized());
    }

    #[test]
    fn unsymbolized_detected() {
        let mut f = frame("");
        f.address = Some(0xdead_beef);
        let p = NormalizedProfile {
            service: "api".into(),
            profile_type: ProfileType::Cpu,
            sample_types: vec![ValueType::new("cpu", "nanoseconds")],
            default_value_index: 0,
            samples: vec![Sample {
                stack: vec![f.clone()],
                values: vec![10],
                labels: BTreeMap::new(),
            }],
            period_type: None,
            period: 0,
            start_time_micros: 0,
            duration_nanos: 0,
            labels: BTreeMap::new(),
            trace_id: None,
            span_id: None,
        };
        assert!(p.unsymbolized());
        assert_eq!(f.display_name(), "0xdeadbeef");
    }

    #[test]
    fn profile_type_from_name() {
        assert_eq!(ProfileType::from_name("CPU"), ProfileType::Cpu);
        assert_eq!(
            ProfileType::from_name("alloc_space"),
            ProfileType::AllocSpace
        );
        assert_eq!(
            ProfileType::from_name("weird"),
            ProfileType::Other("weird".into())
        );
    }

    fn cpu_profile() -> NormalizedProfile {
        NormalizedProfile {
            service: "api".into(),
            profile_type: ProfileType::Cpu,
            sample_types: vec![ValueType::new("cpu", "nanoseconds")],
            default_value_index: 0,
            samples: vec![
                Sample {
                    stack: vec![frame("main"), frame("work")],
                    values: vec![100],
                    labels: BTreeMap::new(),
                },
                Sample {
                    stack: vec![frame("main"), frame("idle")],
                    values: vec![50],
                    labels: BTreeMap::new(),
                },
            ],
            period_type: Some(ValueType::new("cpu", "nanoseconds")),
            period: 1_000,
            start_time_micros: 1_700_000_000_000_000,
            duration_nanos: 1_000_000_000,
            labels: BTreeMap::new(),
            trace_id: None,
            span_id: None,
        }
    }

    #[test]
    fn pprof_encode_decode_round_trips_samples() {
        let p = cpu_profile();
        let bytes = encode_pprof(&p).expect("encode");
        // gzip magic present.
        assert_eq!(&bytes[..2], &[0x1f, 0x8b]);
        let decoded = decode_pprof(&bytes).expect("decode");
        let back = normalize_pprof(&decoded, "api", &BTreeMap::new());
        assert_eq!(back.samples, p.samples);
        assert_eq!(back.sample_types, p.sample_types);
        assert_eq!(back.total_value(), p.total_value());
        assert_eq!(back.start_time_micros, p.start_time_micros);
        assert_eq!(back.profile_type, ProfileType::Cpu);
    }

    #[test]
    fn decode_accepts_raw_and_gzip() {
        let p = cpu_profile();
        let gz = encode_pprof(&p).expect("encode");
        // 裸 protobuf（手动去 gzip）也应被接受。
        let raw = maybe_gunzip(&gz).expect("gunzip");
        assert_ne!(&raw[..2], &[0x1f, 0x8b]);
        assert!(decode_pprof(&raw).is_ok());
        assert!(decode_pprof(&gz).is_ok());
    }

    #[test]
    fn trace_ids_extracted_from_sample_labels() {
        let mut labels = BTreeMap::new();
        labels.insert("trace_id".to_string(), "abc".to_string());
        labels.insert("span_id".to_string(), "s1".to_string());
        let mut p = cpu_profile();
        p.samples[0].labels = labels;
        let gz = encode_pprof(&p).expect("encode");
        let back = normalize_pprof(&decode_pprof(&gz).unwrap(), "api", &BTreeMap::new());
        assert_eq!(back.trace_id.as_deref(), Some("abc"));
        assert_eq!(back.span_id.as_deref(), Some("s1"));
    }

    #[test]
    fn archive_key_layout() {
        let key = archive_object_key(
            &Id::from_string("org123"),
            "checkout svc",
            "cpu",
            1_700_000_000_000_000,
            &Id::from_string("p1"),
        );
        assert_eq!(
            key,
            "profiles/org123/checkout_svc/cpu/20231114/p1.pprof.zst"
        );
    }

    #[test]
    fn yyyymmdd_is_civil_date() {
        assert_eq!(yyyymmdd(1_700_000_000_000_000), "20231114");
        assert_eq!(yyyymmdd(0), "19700101");
    }

    #[test]
    fn zstd_round_trips() {
        let data = b"the quick brown fox jumps over the lazy dog".repeat(10);
        let c = zstd_compress(&data).unwrap();
        assert_eq!(zstd_decompress(&c).unwrap(), data);
    }

    #[test]
    fn metadata_event_carries_columns() {
        let p = cpu_profile();
        let ev = metadata_event(
            &p,
            "profiles/o/api/cpu/20231114/x.pprof.zst",
            4096,
            TimestampMicros(p.start_time_micros),
        );
        assert_eq!(ev.fields["service"], Value::from("api"));
        assert_eq!(ev.fields["profile_type"], Value::from("cpu"));
        assert_eq!(ev.fields["total_value"], Value::from(150_i64));
        assert_eq!(ev.fields["sample_count"], Value::from(2_i64));
        assert_eq!(ev.fields["unsymbolized"], Value::from(false));
        assert_eq!(ev.fields["archived_bytes"], Value::from(4096_i64));
        assert_eq!(
            ev.fields["object_key"],
            Value::from("profiles/o/api/cpu/20231114/x.pprof.zst")
        );
        // trace_id / span_id 必须始终成列（无关联时为空串），否则 schema-on-write 缺列。
        assert!(ev.fields.contains_key("trace_id"));
        assert!(ev.fields.contains_key("span_id"));
        assert_eq!(ev.timestamp.0, p.start_time_micros);
    }

    #[test]
    fn folded_parsed_per_spec() {
        let p = parse_folded("a;b;c 10\n", "api", "cpu", &BTreeMap::new()).unwrap();
        assert_eq!(p.samples.len(), 1);
        let names: Vec<&str> = p.samples[0]
            .stack
            .iter()
            .map(|f| f.function.as_str())
            .collect();
        assert_eq!(names, vec!["a", "b", "c"]);
        assert_eq!(p.samples[0].values, vec![10]);
        assert_eq!(p.profile_type, ProfileType::Cpu);
    }

    #[test]
    fn folded_skips_comments_and_blanks() {
        let p = parse_folded(
            "# header\n\nmain;work 5\nmain;idle 3\n",
            "api",
            "",
            &BTreeMap::new(),
        )
        .unwrap();
        assert_eq!(p.samples.len(), 2);
        assert_eq!(p.sample_types[0].ty, "samples");
    }

    #[test]
    fn folded_root_first_survives_pprof_round_trip() {
        let p = parse_folded("a;b;c 10", "api", "cpu", &BTreeMap::new()).unwrap();
        let gz = encode_pprof(&p).unwrap();
        let back = normalize_pprof(&decode_pprof(&gz).unwrap(), "api", &BTreeMap::new());
        let names: Vec<&str> = back.samples[0]
            .stack
            .iter()
            .map(|f| f.function.as_str())
            .collect();
        assert_eq!(names, vec!["a", "b", "c"]);
    }

    #[test]
    fn pyroscope_name_parsed_per_spec() {
        let (svc, hint, labels) = parse_pyroscope_name("checkout{env=prod}");
        assert_eq!(svc, "checkout");
        assert_eq!(hint, None);
        assert_eq!(labels.get("env").map(String::as_str), Some("prod"));
    }

    #[test]
    fn pyroscope_name_strips_known_type_suffix() {
        let (svc, hint, _labels) = parse_pyroscope_name("myapp.cpu{}");
        assert_eq!(svc, "myapp");
        assert_eq!(hint.as_deref(), Some("cpu"));
    }
}

// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! DataFusion ScalarUDFs。
//!
//! 当前提供：
//! - `extract_pattern(message TEXT) -> TEXT`：按 org 的 `log_patterns` 表逐条 regex 匹配，
//!   priority DESC，首个命中返回 `category`；无匹配返 NULL。
//! - `decrypt(col TEXT) -> TEXT`：字段级静态加密的查询端还原。对 `enc:1:` 前缀的密文用
//!   `CipherRootKey` 还原明文；非该格式的值（明文 / 其它）原样透传；解密失败返 NULL。
//! - `mask(col TEXT) -> TEXT`：按 org 的 `regex_patterns` 规则即时脱敏。命中片段替换成各
//!   规则的 `replacement`（支持 `$1` 捕获组）；底层原值不变，仅查询结果脱敏。复用 domain
//!   [`Masker`]，与写入端脱敏同一套逻辑。
//!
//! 编译时预热：UDF 注册时把 `(regex, category)` 闭包捕获在 ScalarUDF 内；regex 编译失败
//! 整批 panic 不合适，逐条 try_match 时 lazy 编译，错误规则跳过并 warn。

use std::{collections::HashMap, sync::Arc};

use arrow::array::{Array, ArrayRef, StringArray};
use datafusion::{
    arrow::datatypes::DataType,
    logical_expr::{
        ColumnarValue, ScalarFunctionArgs, ScalarUDF, ScalarUDFImpl, Signature, Volatility,
    },
};

use crate::{domain::masking::Masker, infra::cipher::decrypt_field, shared::Result};

/// 输入：DB 行里的 `(regex_source, category, priority)`。
/// 真正的 `Regex::new` 在 UDF 实例化时一次完成；Hash/Eq 走源串避免 Regex 不可哈希。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CompiledPattern {
    pub regex_source: String,
    pub category: String,
    pub priority: i32,
}

/// 把一组 pattern 注册成 `extract_pattern` ScalarUDF。
pub fn build_extract_pattern_udf(patterns: Vec<CompiledPattern>) -> ScalarUDF {
    let inner = ExtractPatternUdf::new(patterns);
    ScalarUDF::from(inner)
}

#[derive(Debug, PartialEq, Eq, Hash)]
struct ExtractPatternUdf {
    signature: Signature,
    /// 源串列表（Hash/Eq 走它）。
    patterns: Vec<CompiledPattern>,
}

impl ExtractPatternUdf {
    fn new(mut patterns: Vec<CompiledPattern>) -> Self {
        patterns.sort_by_key(|p| std::cmp::Reverse(p.priority));
        Self {
            signature: Signature::exact(vec![DataType::Utf8], Volatility::Immutable),
            patterns,
        }
    }
}

impl ScalarUDFImpl for ExtractPatternUdf {
    fn name(&self) -> &str {
        "extract_pattern"
    }

    fn signature(&self) -> &Signature {
        &self.signature
    }

    fn return_type(&self, _arg_types: &[DataType]) -> datafusion::error::Result<DataType> {
        Ok(DataType::Utf8)
    }

    fn invoke_with_args(
        &self,
        args: ScalarFunctionArgs,
    ) -> datafusion::error::Result<ColumnarValue> {
        // 仅支持 1 个 Utf8 列 / 标量
        let value = match args.args.into_iter().next() {
            Some(v) => v,
            None => {
                return Err(datafusion::error::DataFusionError::Execution(
                    "extract_pattern requires 1 argument".into(),
                ));
            }
        };
        let array = match value {
            ColumnarValue::Array(a) => a,
            ColumnarValue::Scalar(s) => s.to_array_of_size(1).map_err(|e| {
                datafusion::error::DataFusionError::Execution(format!("scalar→array: {e}"))
            })?,
        };
        let str_arr = array
            .as_any()
            .downcast_ref::<StringArray>()
            .ok_or_else(|| {
                datafusion::error::DataFusionError::Execution(
                    "extract_pattern expects Utf8 input".into(),
                )
            })?;

        let mut out: Vec<Option<String>> = Vec::with_capacity(str_arr.len());
        for i in 0..str_arr.len() {
            if str_arr.is_null(i) {
                out.push(None);
                continue;
            }
            let s = str_arr.value(i);
            // 现场 try-compile：常见用法每条 UDF 只用几条规则，开销很小
            let hit = self.patterns.iter().find_map(|p| {
                regex::Regex::new(&p.regex_source).ok().and_then(|re| {
                    if re.is_match(s) {
                        Some(p.category.clone())
                    } else {
                        None
                    }
                })
            });
            out.push(hit);
        }
        let arr: ArrayRef = Arc::new(StringArray::from(out));
        Ok(ColumnarValue::Array(arr))
    }
}

/// 把字段级加密的 `decrypt(col)` 注册成 ScalarUDF（持有该 org 预载的 DEK 映射 id→raw）。
pub fn build_decrypt_udf(keys: Arc<HashMap<String, Vec<u8>>>) -> ScalarUDF {
    ScalarUDF::from(DecryptUdf::new(keys))
}

/// `decrypt(col TEXT) -> TEXT`：见模块头。持有 org 的 DEK 映射，逐值还原。
struct DecryptUdf {
    signature: Signature,
    keys: Arc<HashMap<String, Vec<u8>>>,
}

impl std::fmt::Debug for DecryptUdf {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DecryptUdf").finish_non_exhaustive()
    }
}

// `ScalarUDFImpl` 要求 DynEq + DynHash；key 映射不是 Eq/Hash，且单个 ctx 内只有一个 decrypt
// UDF，故按 signature 比较 / 哈希（不纳入 keys）即可。
impl PartialEq for DecryptUdf {
    fn eq(&self, other: &Self) -> bool {
        self.signature == other.signature
    }
}
impl Eq for DecryptUdf {}
impl std::hash::Hash for DecryptUdf {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.signature.hash(state);
    }
}

impl DecryptUdf {
    fn new(keys: Arc<HashMap<String, Vec<u8>>>) -> Self {
        Self {
            signature: Signature::exact(vec![DataType::Utf8], Volatility::Immutable),
            keys,
        }
    }
}

impl ScalarUDFImpl for DecryptUdf {
    fn name(&self) -> &str {
        "decrypt"
    }

    fn signature(&self) -> &Signature {
        &self.signature
    }

    fn return_type(&self, _arg_types: &[DataType]) -> datafusion::error::Result<DataType> {
        Ok(DataType::Utf8)
    }

    fn invoke_with_args(
        &self,
        args: ScalarFunctionArgs,
    ) -> datafusion::error::Result<ColumnarValue> {
        let value = args.args.into_iter().next().ok_or_else(|| {
            datafusion::error::DataFusionError::Execution("decrypt requires 1 argument".into())
        })?;
        let array = match value {
            ColumnarValue::Array(a) => a,
            ColumnarValue::Scalar(s) => s.to_array_of_size(1).map_err(|e| {
                datafusion::error::DataFusionError::Execution(format!("scalar→array: {e}"))
            })?,
        };
        let str_arr = array
            .as_any()
            .downcast_ref::<StringArray>()
            .ok_or_else(|| {
                datafusion::error::DataFusionError::Execution("decrypt expects Utf8 input".into())
            })?;

        let mut out: Vec<Option<String>> = Vec::with_capacity(str_arr.len());
        for i in 0..str_arr.len() {
            if str_arr.is_null(i) {
                out.push(None);
                continue;
            }
            // 密文 → 明文；非密文原样透传；解密失败 → NULL（见 decrypt_field 语义）。
            out.push(decrypt_field(&self.keys, str_arr.value(i)));
        }
        let arr: ArrayRef = Arc::new(StringArray::from(out));
        Ok(ColumnarValue::Array(arr))
    }
}

/// 把一个编译好的 [`Masker`] 注册成 `mask(col)` ScalarUDF（持有该 org 全部脱敏规则）。
pub fn build_mask_udf(masker: Arc<Masker>) -> ScalarUDF {
    ScalarUDF::from(MaskUdf::new(masker))
}

/// `mask(col TEXT) -> TEXT`：见模块头。持有 org 的脱敏规则，逐值即时脱敏。
struct MaskUdf {
    signature: Signature,
    masker: Arc<Masker>,
}

impl std::fmt::Debug for MaskUdf {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MaskUdf").finish_non_exhaustive()
    }
}

// 同 DecryptUdf：Masker 非 Eq/Hash，且单 ctx 内只有一个 mask UDF，按 signature 比较 / 哈希。
impl PartialEq for MaskUdf {
    fn eq(&self, other: &Self) -> bool {
        self.signature == other.signature
    }
}
impl Eq for MaskUdf {}
impl std::hash::Hash for MaskUdf {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.signature.hash(state);
    }
}

impl MaskUdf {
    fn new(masker: Arc<Masker>) -> Self {
        Self {
            signature: Signature::exact(vec![DataType::Utf8], Volatility::Immutable),
            masker,
        }
    }
}

impl ScalarUDFImpl for MaskUdf {
    fn name(&self) -> &str {
        "mask"
    }

    fn signature(&self) -> &Signature {
        &self.signature
    }

    fn return_type(&self, _arg_types: &[DataType]) -> datafusion::error::Result<DataType> {
        Ok(DataType::Utf8)
    }

    fn invoke_with_args(
        &self,
        args: ScalarFunctionArgs,
    ) -> datafusion::error::Result<ColumnarValue> {
        let value = args.args.into_iter().next().ok_or_else(|| {
            datafusion::error::DataFusionError::Execution("mask requires 1 argument".into())
        })?;
        let array = match value {
            ColumnarValue::Array(a) => a,
            ColumnarValue::Scalar(s) => s.to_array_of_size(1).map_err(|e| {
                datafusion::error::DataFusionError::Execution(format!("scalar→array: {e}"))
            })?,
        };
        let str_arr = array
            .as_any()
            .downcast_ref::<StringArray>()
            .ok_or_else(|| {
                datafusion::error::DataFusionError::Execution("mask expects Utf8 input".into())
            })?;

        let mut out: Vec<Option<String>> = Vec::with_capacity(str_arr.len());
        for i in 0..str_arr.len() {
            if str_arr.is_null(i) {
                out.push(None);
                continue;
            }
            out.push(Some(self.masker.mask_str(str_arr.value(i)).into_owned()));
        }
        let arr: ArrayRef = Arc::new(StringArray::from(out));
        Ok(ColumnarValue::Array(arr))
    }
}

/// 把 DB 行整理成 UDF 可用形式；不可解析的 regex 仅 warn 跳过。
pub fn compile_patterns(rows: Vec<(String, String, i32)>) -> Vec<CompiledPattern> {
    let mut out = Vec::with_capacity(rows.len());
    for (re_src, category, priority) in rows {
        match regex::Regex::new(&re_src) {
            Ok(_) => out.push(CompiledPattern {
                regex_source: re_src,
                category,
                priority,
            }),
            Err(e) => tracing::warn!(
                regex = %re_src,
                error = %e,
                "log_pattern regex compile failed; skipped"
            ),
        }
    }
    out
}

#[allow(dead_code)]
fn _phantom_result_marker() -> Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use arrow::array::StringArray;
    use datafusion::logical_expr::ColumnarValue;

    use super::*;

    fn mk_args(values: &[Option<&str>]) -> ScalarFunctionArgs {
        use arrow::datatypes::Field;
        let arr: ArrayRef = Arc::new(StringArray::from_iter(values.iter().copied()));
        let return_field = Arc::new(Field::new("out", DataType::Utf8, true));
        let cfg = Arc::new(datafusion::config::ConfigOptions::default());
        ScalarFunctionArgs {
            args: vec![ColumnarValue::Array(arr)],
            number_rows: values.len(),
            return_field,
            arg_fields: vec![],
            config_options: cfg,
        }
    }

    #[test]
    fn returns_first_match_by_priority() {
        let pats = compile_patterns(vec![
            (r"^panic".into(), "panic".into(), 100),
            (r".+".into(), "other".into(), 1),
        ]);
        let udf = ExtractPatternUdf::new(pats);
        let out = udf
            .invoke_with_args(mk_args(&[
                Some("panic at the disco"),
                Some("just text"),
                None,
            ]))
            .unwrap();
        let arr = match out {
            ColumnarValue::Array(a) => a,
            _ => panic!("expected array"),
        };
        let sa = arr.as_any().downcast_ref::<StringArray>().unwrap();
        assert_eq!(sa.value(0), "panic");
        assert_eq!(sa.value(1), "other");
        assert!(sa.is_null(2));
    }

    #[test]
    fn empty_patterns_returns_null_for_all() {
        let udf = ExtractPatternUdf::new(vec![]);
        let out = udf.invoke_with_args(mk_args(&[Some("anything")])).unwrap();
        let arr = match out {
            ColumnarValue::Array(a) => a,
            _ => panic!("expected array"),
        };
        let sa = arr.as_any().downcast_ref::<StringArray>().unwrap();
        assert!(sa.is_null(0));
    }

    #[test]
    fn bad_regex_is_silently_skipped() {
        let pats = compile_patterns(vec![
            (r"[invalid(".into(), "bad".into(), 100),
            (r"good".into(), "good".into(), 50),
        ]);
        assert_eq!(pats.len(), 1, "bad regex must be skipped");
    }

    #[test]
    fn decrypt_udf_restores_ciphertext_and_passes_through_plain() {
        use crate::infra::cipher::{OrgFieldKey, encrypt_field};

        let dek = OrgFieldKey {
            key_id: "key-1".into(),
            version: 1,
            raw_key: vec![6u8; 32],
        };
        let ct = encrypt_field(&dek, "alice@example.com").unwrap();
        let keys: HashMap<String, Vec<u8>> =
            [("key-1".to_string(), vec![6u8; 32])].into_iter().collect();
        let udf = DecryptUdf::new(Arc::new(keys));

        let out = udf
            .invoke_with_args(mk_args(&[Some(ct.as_str()), Some("plain text"), None]))
            .unwrap();
        let arr = match out {
            ColumnarValue::Array(a) => a,
            _ => panic!("expected array"),
        };
        let sa = arr.as_any().downcast_ref::<StringArray>().unwrap();
        assert_eq!(sa.value(0), "alice@example.com", "ciphertext decrypted");
        assert_eq!(sa.value(1), "plain text", "non-ciphertext passed through");
        assert!(sa.is_null(2), "null stays null");
    }

    #[test]
    fn mask_udf_redacts_matches_and_keeps_nulls() {
        let masker = Masker::compile([(r"\d{3}-\d{2}-\d{4}".to_string(), "[SSN]".to_string())]);
        let udf = MaskUdf::new(Arc::new(masker));
        let out = udf
            .invoke_with_args(mk_args(&[Some("ssn 123-45-6789"), Some("clean"), None]))
            .unwrap();
        let arr = match out {
            ColumnarValue::Array(a) => a,
            _ => panic!("expected array"),
        };
        let sa = arr.as_any().downcast_ref::<StringArray>().unwrap();
        assert_eq!(sa.value(0), "ssn [SSN]", "match redacted");
        assert_eq!(sa.value(1), "clean", "non-match passed through");
        assert!(sa.is_null(2), "null stays null");
    }
}

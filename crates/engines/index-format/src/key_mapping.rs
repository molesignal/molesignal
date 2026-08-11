// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Canonical Parquet → Tantivy/Puffin sidecar key mapping.
//!
//! ```text
//! {org}/{stream_type}/{dataset_kind}/{stream}/{YYYY}/{MM}/{DD}/{HH}/{id}.parquet
//! files/{org}/index/{stream_type}/{dataset_kind}/{stream}/{YYYY}/{MM}/{DD}/{HH}/{id}.ttv
//! ```

pub fn convert_parquet_file_name_to_tantivy_file(parquet_key: &str) -> Option<String> {
    let parts: Vec<&str> = parquet_key.split('/').collect();
    let [
        org,
        stream_type,
        dataset_kind,
        stream,
        year,
        month,
        day,
        hour,
        file,
    ] = parts.as_slice()
    else {
        return None;
    };
    if !matches!(
        *stream_type,
        "logs" | "metrics" | "traces" | "profiles" | "extend"
    ) {
        return None;
    }
    if !valid_path_segment(dataset_kind) || !valid_path_segment(stream) {
        return None;
    }
    validate_partition(year, month, day, hour)?;
    let stem = file.strip_suffix(".parquet")?;
    if stem.is_empty() || stem.contains('/') {
        return None;
    }
    Some(format!(
        "files/{org}/index/{stream_type}/{dataset_kind}/{stream}/{year}/{month}/{day}/{hour}/{stem}.ttv"
    ))
}

fn valid_path_segment(value: &str) -> bool {
    !value.is_empty()
        && !matches!(value, "." | "..")
        && value.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '_' | '-' | '.')
        })
}

fn validate_partition(year: &str, month: &str, day: &str, hour: &str) -> Option<()> {
    if year.len() != 4 || month.len() != 2 || day.len() != 2 || hour.len() != 2 {
        return None;
    }
    let year: i32 = year.parse().ok()?;
    let month: u32 = month.parse().ok()?;
    let day: u32 = day.parse().ok()?;
    let hour: u32 = hour.parse().ok()?;
    chrono::NaiveDate::from_ymd_opt(year, month, day)?;
    (hour < 24).then_some(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_hourly_raw_and_summary_datasets() {
        assert_eq!(
            convert_parquet_file_name_to_tantivy_file(
                "orgA/logs/raw/log_app/2026/01/15/09/abc123.parquet"
            )
            .unwrap(),
            "files/orgA/index/logs/raw/log_app/2026/01/15/09/abc123.ttv"
        );
        assert_eq!(
            convert_parquet_file_name_to_tantivy_file(
                "orgA/traces/trace_summary/svc/2026/03/04/23/xyz.parquet"
            )
            .unwrap(),
            "files/orgA/index/traces/trace_summary/svc/2026/03/04/23/xyz.ttv"
        );
    }

    #[test]
    fn rejects_old_layout_and_invalid_partition() {
        assert!(
            convert_parquet_file_name_to_tantivy_file(
                "orgA/logs/log_app/2026-01-15/abc123.parquet"
            )
            .is_none()
        );
        assert!(
            convert_parquet_file_name_to_tantivy_file(
                "orgA/logs/raw/app/2026/02/30/09/abc.parquet"
            )
            .is_none()
        );
        assert!(
            convert_parquet_file_name_to_tantivy_file(
                "orgA/logs/raw/app/2026/02/20/24/abc.parquet"
            )
            .is_none()
        );
        assert!(
            convert_parquet_file_name_to_tantivy_file("orgA/logs/raw/../2026/02/20/09/abc.parquet")
                .is_none()
        );
    }
}

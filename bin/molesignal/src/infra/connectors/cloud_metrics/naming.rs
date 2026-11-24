// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! How cloud metrics are named, the way yet-another-cloudwatch-exporter names CloudWatch's:
//! `<cloud>_<namespace>_<metric>_<statistic>`, lower snake case, one sample per statistic. So
//! `AWS/EC2` `CPUUtilization` `Average` is `aws_ec2_cpuutilization_average`, and the same rule
//! names every other cloud's metrics (`aliyun_ecs_dashboard_cpuutilization_average`).

/// `<prefix>_<namespace>_<metric>[_<statistic>]`: the namespace lower-cased first, every part in
/// snake case, empty parts left out.
pub fn metric_name(prefix: &str, namespace: &str, metric: &str, statistic: Option<&str>) -> String {
    [
        snake(prefix),
        snake(&namespace.to_ascii_lowercase()),
        snake(metric),
        statistic.map(snake).unwrap_or_default(),
    ]
    .into_iter()
    .filter(|part| !part.is_empty())
    .collect::<Vec<_>>()
    .join("_")
}

/// Lower snake case of `text`: words split where a lower-case letter meets a capital
/// (`NetworkIn` → `network_in`, but `CPUUtilization` → `cpuutilization` and `5XXError` →
/// `5xxerror`), anything but ASCII letters and digits as `_`, no `_` repeated or at either end.
pub fn snake(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 4);
    let mut previous: Option<char> = None;

    for ch in text.chars() {
        if ch.is_ascii_alphanumeric() {
            if ch.is_ascii_uppercase() && previous.is_some_and(|p| p.is_ascii_lowercase()) {
                out.push('_');
            }

            out.push(ch.to_ascii_lowercase());
        } else if !out.ends_with('_') {
            out.push('_');
        }

        previous = Some(ch);
    }

    out.trim_matches('_').to_string()
}

/// A name as a label name: anything but ASCII letters, digits and `_` as `_`, case kept
/// (`InstanceId` stays `InstanceId`, as in `dimension_InstanceId`).
pub fn label_safe(name: &str) -> String {
    name.chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '_' })
        .collect()
}

/// The label of a dimension: `dimension_<Name>`.
pub fn dimension_label(name: &str) -> String {
    format!("dimension_{}", label_safe(name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_cloud_is_named_by_one_rule() {
        assert_eq!(
            metric_name("aws", "ec2", "CPUUtilization", Some("average")),
            "aws_ec2_cpuutilization_average"
        );
        assert_eq!(
            metric_name("aliyun", "ecs_dashboard", "CPUUtilization", Some("Average")),
            "aliyun_ecs_dashboard_cpuutilization_average"
        );
        assert_eq!(
            metric_name("tencentcloud", "CVM", "CpuUsage", Some("max")),
            "tencentcloud_cvm_cpu_usage_max"
        );
        assert_eq!(
            metric_name("gcp", "compute", "instance/cpu/utilization", None),
            "gcp_compute_instance_cpu_utilization"
        );
    }

    #[test]
    fn words_split_only_after_a_lower_case_letter() {
        assert_eq!(snake("NetworkIn"), "network_in");
        assert_eq!(
            snake("HTTPCode_Target_5XX_Count"),
            "httpcode_target_5xx_count"
        );
        assert_eq!(snake("TM(25%:75%)"), "tm_25_75");
        assert_eq!(snake("Percentage CPU"), "percentage_cpu");
    }

    #[test]
    fn labels_keep_their_case() {
        assert_eq!(dimension_label("InstanceId"), "dimension_InstanceId");
        assert_eq!(dimension_label("k8s-pod/app"), "dimension_k8s_pod_app");
    }
}

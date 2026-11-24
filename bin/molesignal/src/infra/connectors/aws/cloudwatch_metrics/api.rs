// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The two CloudWatch actions a metric pull calls, over the Query protocol: a SigV4-signed form
//! POST answered in XML, as STS is called.
//!
//! - `ListMetrics`: the series of a namespace that had data in the last three hours, 500 a page.
//! - `GetMetricData`: up to 500 queries (one series, one statistic each) over a time range.
//!
//! See <https://docs.aws.amazon.com/AmazonCloudWatch/latest/APIReference/API_ListMetrics.html> and
//! <https://docs.aws.amazon.com/AmazonCloudWatch/latest/APIReference/API_GetMetricData.html>.

use chrono::{DateTime, Utc};
use kernel::{Error, Result};
use object_store::aws::AwsCredentialProvider;
use signals::http_trace::{self, HttpTarget};

use crate::infra::connectors::aws::{
    self,
    sigv4::{self, SigningKey},
};

const FORM: &str = "application/x-www-form-urlencoded; charset=utf-8";
const VERSION: &str = "2010-08-01";

/// How many queries one `GetMetricData` takes.
pub const MAX_QUERIES: usize = 500;

/// One series: a metric of the namespace with one set of dimension values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Series {
    pub metric_name: String,
    pub dimensions: Vec<(String, String)>,
}

/// One query of a `GetMetricData`: a statistic of a series, by its index in the series list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Query {
    pub series: usize,
    pub statistic: String,
}

/// The values a query returned.
#[derive(Debug, Clone, PartialEq)]
pub struct DataResult {
    /// `m<n>`: the query's position in the request.
    pub id: String,
    /// Unix seconds, each the start of its period.
    pub timestamps: Vec<i64>,
    pub values: Vec<f64>,
}

/// Signs and sends Query-protocol requests to one region.
pub struct Api<'a> {
    pub region: &'a str,
    pub credentials: &'a AwsCredentialProvider,
    pub client: reqwest::Client,
}

impl Api<'_> {
    /// One page of the series of `namespace`, and the token of the next.
    pub async fn list_metrics(
        &self,
        namespace: &str,
        metric_name: Option<&str>,
        next_token: Option<&str>,
    ) -> Result<(Vec<Series>, Option<String>)> {
        let xml = self
            .call(list_metrics_params(namespace, metric_name, next_token))
            .await?;

        parse_list_metrics(&xml)
    }

    /// One page of the values of `queries` over `[start, end)`, and the token of the next.
    pub async fn get_metric_data(
        &self,
        request: &DataRequest<'_>,
        next_token: Option<&str>,
    ) -> Result<(Vec<DataResult>, Option<String>)> {
        let xml = self
            .call(get_metric_data_params(request, next_token))
            .await?;

        parse_get_metric_data(&xml)
    }

    async fn call(&self, params: Vec<(String, String)>) -> Result<String> {
        let credential = self
            .credentials
            .get_credential()
            .await
            .map_err(|e| Error::unauthorized(format!("aws credentials: {e}")))?;

        let body = url::form_urlencoded::Serializer::new(String::new())
            .extend_pairs(params)
            .finish();
        let host = aws::monitoring_host(self.region);

        let signed = sigv4::sign_post(
            &SigningKey {
                access_key: &credential.key_id,
                secret_key: &credential.secret_key,
                session_token: credential.token.as_deref(),
            },
            self.region,
            "monitoring",
            &host,
            &[("content-type", FORM)],
            body.as_bytes(),
            &sigv4::amz_date_now(),
        );

        let mut request = self
            .client
            .post(format!("https://{host}/"))
            .header("content-type", FORM)
            .header("x-amz-date", &signed.amz_date)
            .header("authorization", &signed.authorization);

        if let Some(token) = credential
            .token
            .as_deref()
            .filter(|token| !token.is_empty())
        {
            request = request.header("x-amz-security-token", token);
        }

        let response = http_trace::send(&self.client, request.body(body), HttpTarget::ThirdParty)
            .await
            .map_err(|e| Error::unavailable(format!("cloudwatch request: {e}")))?;

        let status = response.status();

        let text = response
            .text()
            .await
            .map_err(|e| Error::unavailable(format!("cloudwatch response: {e}")))?;

        if !status.is_success() {
            return Err(refusal(status.as_u16(), &text));
        }

        Ok(text)
    }
}

/// What a `GetMetricData` asks for.
pub struct DataRequest<'a> {
    pub namespace: &'a str,
    pub period_secs: i64,
    pub series: &'a [Series],
    pub queries: &'a [Query],
    pub start: i64,
    pub end: i64,
}

fn base(action: &str) -> Vec<(String, String)> {
    vec![
        ("Action".into(), action.into()),
        ("Version".into(), VERSION.into()),
    ]
}

pub fn list_metrics_params(
    namespace: &str,
    metric_name: Option<&str>,
    next_token: Option<&str>,
) -> Vec<(String, String)> {
    let mut params = base("ListMetrics");
    params.push(("Namespace".into(), namespace.into()));
    params.push(("RecentlyActive".into(), "PT3H".into()));

    if let Some(metric_name) = metric_name {
        params.push(("MetricName".into(), metric_name.into()));
    }

    if let Some(token) = next_token {
        params.push(("NextToken".into(), token.into()));
    }

    params
}

pub fn get_metric_data_params(
    request: &DataRequest<'_>,
    next_token: Option<&str>,
) -> Vec<(String, String)> {
    let mut params = base("GetMetricData");
    params.push(("StartTime".into(), iso8601(request.start)));
    params.push(("EndTime".into(), iso8601(request.end)));
    params.push(("ScanBy".into(), "TimestampAscending".into()));

    for (index, query) in request.queries.iter().enumerate() {
        let series = &request.series[query.series];
        let prefix = format!("MetricDataQueries.member.{}", index + 1);
        let metric = format!("{prefix}.MetricStat.Metric");

        params.push((format!("{prefix}.Id"), format!("m{index}")));
        params.push((format!("{metric}.Namespace"), request.namespace.into()));
        params.push((format!("{metric}.MetricName"), series.metric_name.clone()));

        for (position, (name, value)) in series.dimensions.iter().enumerate() {
            let dimension = format!("{metric}.Dimensions.member.{}", position + 1);
            params.push((format!("{dimension}.Name"), name.clone()));
            params.push((format!("{dimension}.Value"), value.clone()));
        }

        params.push((
            format!("{prefix}.MetricStat.Period"),
            request.period_secs.to_string(),
        ));
        params.push((format!("{prefix}.MetricStat.Stat"), query.statistic.clone()));
        params.push((format!("{prefix}.ReturnData"), "true".into()));
    }

    if let Some(token) = next_token {
        params.push(("NextToken".into(), token.into()));
    }

    params
}

fn iso8601(seconds: i64) -> String {
    DateTime::<Utc>::from_timestamp(seconds, 0)
        .unwrap_or_default()
        .format("%Y-%m-%dT%H:%M:%SZ")
        .to_string()
}

fn document(xml: &str) -> Result<roxmltree::Document<'_>> {
    roxmltree::Document::parse(xml)
        .map_err(|e| Error::unavailable(format!("cloudwatch response: {e}")))
}

/// The direct child of `node` named `name`.
fn child<'a, 'input>(
    node: roxmltree::Node<'a, 'input>,
    name: &str,
) -> Option<roxmltree::Node<'a, 'input>> {
    node.children()
        .find(|child| child.tag_name().name() == name)
}

/// The text of the direct child of `node` named `name`.
fn text<'a>(node: roxmltree::Node<'a, '_>, name: &str) -> Option<&'a str> {
    child(node, name)
        .and_then(|child| child.text())
        .map(str::trim)
}

/// The `member` elements under the direct child of `node` named `list`.
fn members<'a, 'input>(
    node: roxmltree::Node<'a, 'input>,
    list: &str,
) -> Vec<roxmltree::Node<'a, 'input>> {
    child(node, list)
        .map(|list| {
            list.children()
                .filter(|member| member.tag_name().name() == "member")
                .collect()
        })
        .unwrap_or_default()
}

/// The token of the next page: absent or empty means there is none.
fn next_token(result: roxmltree::Node<'_, '_>) -> Option<String> {
    text(result, "NextToken")
        .filter(|token| !token.is_empty())
        .map(str::to_string)
}

fn result_node<'a, 'input>(
    doc: &'a roxmltree::Document<'input>,
    name: &str,
) -> Result<roxmltree::Node<'a, 'input>> {
    doc.descendants()
        .find(|node| node.tag_name().name() == name)
        .ok_or_else(|| Error::unavailable(format!("cloudwatch response holds no {name}")))
}

pub fn parse_list_metrics(xml: &str) -> Result<(Vec<Series>, Option<String>)> {
    let doc = document(xml)?;
    let result = result_node(&doc, "ListMetricsResult")?;

    let series = members(result, "Metrics")
        .into_iter()
        .filter_map(|metric| {
            let dimensions = members(metric, "Dimensions")
                .into_iter()
                .filter_map(|dimension| {
                    Some((
                        text(dimension, "Name")?.to_string(),
                        text(dimension, "Value").unwrap_or_default().to_string(),
                    ))
                })
                .collect();

            Some(Series {
                metric_name: text(metric, "MetricName")?.to_string(),
                dimensions,
            })
        })
        .collect();

    Ok((series, next_token(result)))
}

pub fn parse_get_metric_data(xml: &str) -> Result<(Vec<DataResult>, Option<String>)> {
    let doc = document(xml)?;
    let result = result_node(&doc, "GetMetricDataResult")?;

    let results = members(result, "MetricDataResults")
        .into_iter()
        .filter_map(|data| {
            let timestamps = members(data, "Timestamps")
                .into_iter()
                .filter_map(|member| member.text())
                .filter_map(|at| DateTime::parse_from_rfc3339(at.trim()).ok())
                .map(|at| at.timestamp())
                .collect();

            let values = members(data, "Values")
                .into_iter()
                .filter_map(|member| member.text())
                .map(|value| value.trim().parse::<f64>().unwrap_or(f64::NAN))
                .collect();

            Some(DataResult {
                id: text(data, "Id")?.to_string(),
                timestamps,
                values,
            })
        })
        .collect();

    Ok((results, next_token(result)))
}

/// A refused request as an error, by the code CloudWatch gave.
fn refusal(status: u16, xml: &str) -> Error {
    let doc = roxmltree::Document::parse(xml).ok();

    let field = |name: &str| {
        doc.as_ref().and_then(|doc| {
            doc.descendants()
                .find(|node| node.tag_name().name() == name)
                .and_then(|node| node.text())
                .map(str::to_string)
        })
    };

    let code = field("Code").unwrap_or_default();
    let message = format!(
        "cloudwatch {status} {code}: {}",
        field("Message").unwrap_or_default()
    );

    match code.as_str() {
        "Throttling" | "ThrottlingException" | "RequestLimitExceeded" => {
            Error::resource_exhausted(message)
        }
        "AccessDenied" | "AccessDeniedException" | "UnauthorizedOperation" => {
            Error::forbidden(message)
        }
        "InvalidClientTokenId" | "SignatureDoesNotMatch" | "ExpiredToken" => {
            Error::unauthorized(message)
        }
        "InvalidParameterValue" | "InvalidParameterCombination" | "MissingParameter" => {
            Error::invalid(message)
        }
        _ => Error::unavailable(message),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIST: &str = r#"<ListMetricsResponse xmlns="http://monitoring.amazonaws.com/doc/2010-08-01/">
      <ListMetricsResult>
        <Metrics>
          <member>
            <Dimensions>
              <member><Name>InstanceId</Name><Value>i-0abc</Value></member>
            </Dimensions>
            <MetricName>CPUUtilization</MetricName>
            <Namespace>AWS/EC2</Namespace>
          </member>
          <member>
            <Dimensions/>
            <MetricName>CPUUtilization</MetricName>
            <Namespace>AWS/EC2</Namespace>
          </member>
        </Metrics>
        <NextToken>page-2</NextToken>
      </ListMetricsResult>
      <ResponseMetadata><RequestId>r</RequestId></ResponseMetadata>
    </ListMetricsResponse>"#;

    const DATA: &str = r#"<GetMetricDataResponse xmlns="http://monitoring.amazonaws.com/doc/2010-08-01/">
      <GetMetricDataResult>
        <MetricDataResults>
          <member>
            <Id>m0</Id>
            <Label>CPUUtilization</Label>
            <Timestamps>
              <member>2026-11-24T10:00:00Z</member>
              <member>2026-11-24T10:05:00Z</member>
            </Timestamps>
            <Values>
              <member>12.5</member>
              <member>7.0</member>
            </Values>
            <StatusCode>Complete</StatusCode>
          </member>
        </MetricDataResults>
        <NextToken></NextToken>
        <Messages/>
      </GetMetricDataResult>
    </GetMetricDataResponse>"#;

    #[test]
    fn list_metrics_gives_the_series_and_the_next_page() {
        let (series, next) = parse_list_metrics(LIST).unwrap();

        assert_eq!(
            series,
            [
                Series {
                    metric_name: "CPUUtilization".into(),
                    dimensions: vec![("InstanceId".into(), "i-0abc".into())],
                },
                Series {
                    metric_name: "CPUUtilization".into(),
                    dimensions: Vec::new(),
                },
            ]
        );
        assert_eq!(next.as_deref(), Some("page-2"));
    }

    #[test]
    fn get_metric_data_gives_each_querys_values_at_their_period_start() {
        let (results, next) = parse_get_metric_data(DATA).unwrap();
        let start = DateTime::parse_from_rfc3339("2026-11-24T10:00:00Z")
            .unwrap()
            .timestamp();

        assert_eq!(next, None);
        assert_eq!(results[0].id, "m0");
        assert_eq!(results[0].timestamps, [start, start + 300]);
        assert_eq!(results[0].values, [12.5, 7.0]);
    }

    #[test]
    fn a_query_names_its_series_statistic_and_period() {
        let series = [Series {
            metric_name: "CPUUtilization".into(),
            dimensions: vec![("InstanceId".into(), "i-0abc".into())],
        }];
        let queries = [Query {
            series: 0,
            statistic: "Average".into(),
        }];

        let params = get_metric_data_params(
            &DataRequest {
                namespace: "AWS/EC2",
                period_secs: 300,
                series: &series,
                queries: &queries,
                start: 1_700_000_100,
                end: 1_700_000_400,
            },
            None,
        );

        let value = |key: &str| {
            params
                .iter()
                .find(|(name, _)| name == key)
                .map(|(_, value)| value.as_str())
        };

        assert_eq!(value("Action"), Some("GetMetricData"));
        assert_eq!(value("MetricDataQueries.member.1.Id"), Some("m0"));
        assert_eq!(
            value("MetricDataQueries.member.1.MetricStat.Metric.Dimensions.member.1.Name"),
            Some("InstanceId")
        );
        assert_eq!(
            value("MetricDataQueries.member.1.MetricStat.Stat"),
            Some("Average")
        );
        assert_eq!(
            value("MetricDataQueries.member.1.MetricStat.Period"),
            Some("300")
        );
        assert_eq!(value("StartTime"), Some("2023-11-14T22:15:00Z"));
    }

    #[test]
    fn a_refusal_says_what_kind_of_failure_it_is() {
        let throttled = r#"<ErrorResponse><Error><Type>Sender</Type><Code>Throttling</Code>
            <Message>Rate exceeded</Message></Error></ErrorResponse>"#;

        assert!(matches!(
            refusal(400, throttled),
            Error::ResourceExhausted(_)
        ));
        assert!(matches!(
            refusal(
                403,
                "<ErrorResponse><Error><Code>AccessDenied</Code></Error></ErrorResponse>"
            ),
            Error::Forbidden(_)
        ));
    }
}

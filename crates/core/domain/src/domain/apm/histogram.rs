// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use serde::{Deserialize, Serialize};

use crate::shared::{Error, Result};

pub const APM_HISTOGRAM_SCHEMA_VERSION: u16 = 1;
pub const APM_HISTOGRAM_UPPER_BOUNDS_MICROS: [u64; 18] = [
    1_000,
    2_000,
    4_000,
    8_000,
    16_000,
    32_000,
    64_000,
    128_000,
    256_000,
    512_000,
    1_000_000,
    2_000_000,
    4_000_000,
    8_000_000,
    16_000_000,
    30_000_000,
    60_000_000,
    u64::MAX,
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistogramSchema {
    pub version: u16,
    pub upper_bounds_micros: Vec<u64>,
}

impl HistogramSchema {
    pub fn v1() -> Self {
        Self {
            version: APM_HISTOGRAM_SCHEMA_VERSION,
            upper_bounds_micros: APM_HISTOGRAM_UPPER_BOUNDS_MICROS.to_vec(),
        }
    }

    pub fn validate(&self) -> Result<()> {
        if self.version == 0 || self.upper_bounds_micros.len() < 2 {
            return Err(Error::invalid(
                "APM histogram version and bucket count must be non-zero",
            ));
        }
        if self.upper_bounds_micros.last() != Some(&u64::MAX) {
            return Err(Error::invalid(
                "APM histogram must end with the +Inf bucket",
            ));
        }
        if self
            .upper_bounds_micros
            .windows(2)
            .any(|pair| pair[0] >= pair[1])
        {
            return Err(Error::invalid(
                "APM histogram upper bounds must be strictly increasing",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LatencyHistogram {
    pub schema_version: u16,
    pub counts: Vec<u64>,
    pub sum_micros: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_micros: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_micros: Option<u64>,
}

impl LatencyHistogram {
    pub fn empty(schema: &HistogramSchema) -> Self {
        Self {
            schema_version: schema.version,
            counts: vec![0; schema.upper_bounds_micros.len()],
            sum_micros: 0,
            min_micros: None,
            max_micros: None,
        }
    }

    pub fn count(&self) -> u64 {
        self.counts.iter().copied().sum()
    }

    pub fn observe(&mut self, schema: &HistogramSchema, duration_micros: u64) -> Result<()> {
        self.ensure_schema(schema)?;
        let bucket = schema
            .upper_bounds_micros
            .iter()
            .position(|upper| duration_micros <= *upper)
            .ok_or_else(|| Error::internal("APM histogram is missing +Inf"))?;
        self.counts[bucket] = self.counts[bucket].saturating_add(1);
        self.sum_micros = self.sum_micros.saturating_add(duration_micros);
        self.min_micros = Some(
            self.min_micros
                .map_or(duration_micros, |current| current.min(duration_micros)),
        );
        self.max_micros = Some(
            self.max_micros
                .map_or(duration_micros, |current| current.max(duration_micros)),
        );
        Ok(())
    }

    pub fn merge(&mut self, other: &Self) -> Result<()> {
        if self.schema_version != other.schema_version || self.counts.len() != other.counts.len() {
            return Err(Error::invalid(
                "cannot merge incompatible APM histogram schemas",
            ));
        }
        for (target, value) in self.counts.iter_mut().zip(&other.counts) {
            *target = target.saturating_add(*value);
        }
        self.sum_micros = self.sum_micros.saturating_add(other.sum_micros);
        self.min_micros = match (self.min_micros, other.min_micros) {
            (Some(left), Some(right)) => Some(left.min(right)),
            (left, right) => left.or(right),
        };
        self.max_micros = match (self.max_micros, other.max_micros) {
            (Some(left), Some(right)) => Some(left.max(right)),
            (left, right) => left.or(right),
        };
        Ok(())
    }

    pub fn quantile(&self, schema: &HistogramSchema, quantile: f64) -> Result<Option<u64>> {
        self.ensure_schema(schema)?;
        if !quantile.is_finite() || !(0.0..=1.0).contains(&quantile) {
            return Err(Error::invalid("APM histogram quantile must be in [0, 1]"));
        }
        let count = self.count();
        if count == 0 {
            return Ok(None);
        }
        let rank = ((count as f64) * quantile).ceil().max(1.0) as u64;
        let mut cumulative = 0_u64;
        for (index, value) in self.counts.iter().enumerate() {
            cumulative = cumulative.saturating_add(*value);
            if cumulative >= rank {
                let upper = schema.upper_bounds_micros[index];
                return Ok(Some(if upper == u64::MAX {
                    self.max_micros.unwrap_or_default()
                } else {
                    upper
                }));
            }
        }
        Ok(self.max_micros)
    }

    fn ensure_schema(&self, schema: &HistogramSchema) -> Result<()> {
        if self.schema_version != schema.version
            || self.counts.len() != schema.upper_bounds_micros.len()
        {
            return Err(Error::invalid(
                "APM histogram does not match the requested schema",
            ));
        }
        Ok(())
    }
}

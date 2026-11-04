// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// 微秒级时间戳。默认值是纪元起点。
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
#[serde(transparent)]
pub struct TimestampMicros(pub i64);

impl TimestampMicros {
    pub fn now() -> Self {
        Self(Utc::now().timestamp_micros())
    }

    pub fn from_datetime(dt: DateTime<Utc>) -> Self {
        Self(dt.timestamp_micros())
    }

    pub fn from_secs(secs: i64) -> Self {
        Self(secs.saturating_mul(1_000_000))
    }

    pub fn from_millis(millis: i64) -> Self {
        Self(millis.saturating_mul(1_000))
    }

    pub fn to_datetime(self) -> DateTime<Utc> {
        DateTime::from_timestamp_micros(self.0).unwrap_or_default()
    }
}

/// 时间区间，闭区间 `[start, end]`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimeRange {
    pub start: TimestampMicros,
    pub end: TimestampMicros,
}

impl TimeRange {
    pub fn new(start: TimestampMicros, end: TimestampMicros) -> Self {
        Self { start, end }
    }

    /// 单点构造：把单个时间戳包成 [t, t]。
    pub fn at(t: TimestampMicros) -> Self {
        Self { start: t, end: t }
    }

    pub fn duration_micros(&self) -> i64 {
        self.end.0.saturating_sub(self.start.0)
    }

    /// 闭区间包含。
    pub fn contains(&self, t: TimestampMicros) -> bool {
        self.start.0 <= t.0 && t.0 <= self.end.0
    }

    /// 闭区间相交（共享至少一个点即为 true）。
    pub fn overlaps(&self, other: TimeRange) -> bool {
        self.start.0 <= other.end.0 && other.start.0 <= self.end.0
    }
}

/// 时间点按 `TIMESTAMPTZ` 存取，精度同为微秒。`i64::MAX` 与 `i64::MIN` 表示不设上限、下限，
/// 存成 `infinity` 与 `-infinity`：它们落在 `DateTime<Utc>` 的范围之外，却是查询区间常用的端点。
#[cfg(feature = "postgres")]
mod postgres {
    use chrono::{DateTime, Utc};
    use sqlx::{
        Decode, Encode, Postgres, Type,
        encode::IsNull,
        error::BoxDynError,
        postgres::{PgArgumentBuffer, PgHasArrayType, PgTypeInfo, PgValueFormat, PgValueRef},
    };

    use super::TimestampMicros;

    /// Postgres 的纪元 2000-01-01T00:00:00Z，自 Unix 纪元起的微秒数。二进制格式存的是距它的微秒数，
    /// `i64::MAX` 与 `i64::MIN` 是 `infinity` 与 `-infinity`。
    const POSTGRES_EPOCH: i64 = 946_684_800_000_000;

    impl Type<Postgres> for TimestampMicros {
        fn type_info() -> PgTypeInfo {
            <DateTime<Utc> as Type<Postgres>>::type_info()
        }
    }

    impl PgHasArrayType for TimestampMicros {
        fn array_type_info() -> PgTypeInfo {
            <DateTime<Utc> as PgHasArrayType>::array_type_info()
        }
    }

    impl Encode<'_, Postgres> for TimestampMicros {
        fn encode_by_ref(&self, buf: &mut PgArgumentBuffer) -> Result<IsNull, BoxDynError> {
            Encode::<Postgres>::encode(to_postgres(self.0), buf)
        }

        fn size_hint(&self) -> usize {
            std::mem::size_of::<i64>()
        }
    }

    impl<'r> Decode<'r, Postgres> for TimestampMicros {
        fn decode(value: PgValueRef<'r>) -> Result<Self, BoxDynError> {
            if value.format() == PgValueFormat::Binary {
                let micros = <i64 as Decode<'r, Postgres>>::decode(value)?;
                return Ok(Self(from_postgres(micros)));
            }

            match value.as_str()? {
                "infinity" => Ok(Self(i64::MAX)),
                "-infinity" => Ok(Self(i64::MIN)),
                _ => {
                    let value = <DateTime<Utc> as Decode<'r, Postgres>>::decode(value)?;
                    Ok(Self(value.timestamp_micros()))
                }
            }
        }
    }

    /// 紧挨 `i64::MIN` 的值减去纪元会溢出，饱和成 `-infinity`。
    fn to_postgres(micros: i64) -> i64 {
        match micros {
            i64::MAX => i64::MAX,
            micros => micros.saturating_sub(POSTGRES_EPOCH),
        }
    }

    /// `infinity` 和 Postgres 范围最末约 30 年加上纪元会超出 `i64`，饱和成 `i64::MAX`。
    fn from_postgres(micros: i64) -> i64 {
        match micros {
            i64::MIN => i64::MIN,
            micros => micros.saturating_add(POSTGRES_EPOCH),
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        fn encoded(micros: i64) -> i64 {
            let mut buf = PgArgumentBuffer::default();
            let _ = TimestampMicros(micros).encode_by_ref(&mut buf).unwrap();
            i64::from_be_bytes(buf.as_slice().try_into().unwrap())
        }

        #[test]
        fn open_ends_are_infinity() {
            assert_eq!(encoded(i64::MAX), i64::MAX);
            assert_eq!(encoded(i64::MIN), i64::MIN);
            assert_eq!(from_postgres(i64::MAX), i64::MAX);
            assert_eq!(from_postgres(i64::MIN), i64::MIN);
        }

        #[test]
        fn instants_count_from_the_postgres_epoch() {
            assert_eq!(encoded(POSTGRES_EPOCH), 0);
            assert_eq!(encoded(0), -POSTGRES_EPOCH);
            assert_eq!(
                from_postgres(encoded(1_793_719_205_807_318)),
                1_793_719_205_807_318
            );
            assert_eq!(from_postgres(encoded(-1)), -1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(us: i64) -> TimestampMicros {
        TimestampMicros(us)
    }

    #[test]
    fn now_is_monotonic_and_nonzero() {
        let a = TimestampMicros::now();
        let b = TimestampMicros::now();
        assert!(a.0 > 0);
        assert!(b.0 >= a.0);
    }

    #[test]
    fn from_secs_and_millis() {
        assert_eq!(TimestampMicros::from_secs(1).0, 1_000_000);
        assert_eq!(TimestampMicros::from_millis(1).0, 1_000);
    }

    #[test]
    fn contains_inclusive_bounds() {
        let r = TimeRange::new(t(10), t(20));
        assert!(r.contains(t(10)));
        assert!(r.contains(t(15)));
        assert!(r.contains(t(20)));
        assert!(!r.contains(t(9)));
        assert!(!r.contains(t(21)));
    }

    #[test]
    fn overlaps_inclusive_and_disjoint() {
        let a = TimeRange::new(t(10), t(20));
        // identical → overlap
        assert!(a.overlaps(TimeRange::new(t(10), t(20))));
        // nested → overlap
        assert!(a.overlaps(TimeRange::new(t(12), t(15))));
        // touching at single point → overlap (closed interval)
        assert!(a.overlaps(TimeRange::new(t(20), t(30))));
        assert!(a.overlaps(TimeRange::new(t(0), t(10))));
        // strictly before / after → no
        assert!(!a.overlaps(TimeRange::new(t(0), t(9))));
        assert!(!a.overlaps(TimeRange::new(t(21), t(30))));
    }

    #[test]
    fn datetime_roundtrip() {
        let now = TimestampMicros::now();
        let dt = now.to_datetime();
        assert_eq!(TimestampMicros::from_datetime(dt).0, now.0);
    }
}

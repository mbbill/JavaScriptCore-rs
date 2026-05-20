use crate::runtime::state::{ObjectId, StringId, StructureId};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct DateInstance {
    /// ECMAScript Date stores the clipped millisecond number and caches local
    /// or UTC calendar projections. It does not own time-zone policy.
    pub object: Option<ObjectId>,
    pub structure: Option<StructureId>,
    pub internal_number: f64,
    pub cache: DateCacheState,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct DateCacheState {
    pub local_cached_for_ms: Option<f64>,
    pub utc_cached_for_ms: Option<f64>,
    pub local_fields: Option<GregorianDateTimeFields>,
    pub utc_fields: Option<GregorianDateTimeFields>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub struct GregorianDateTimeFields {
    pub year: i32,
    pub month: u8,
    pub day: u8,
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
    pub millisecond: u16,
    pub utc_offset_minutes: i32,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DateFormattingRequest {
    pub date: ObjectId,
    pub mode: DateFormattingMode,
    pub locale: Option<StringId>,
    pub time_zone: Option<StringId>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum DateFormattingMode {
    #[default]
    ToString,
    ToDateString,
    ToTimeString,
    ToISOString,
    ToUTCString,
    ToLocaleString,
    ToLocaleDateString,
    ToLocaleTimeString,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum TimeClipResult {
    #[default]
    Valid,
    NaN,
    OutOfRange,
}

pub const MAX_TIME_VALUE_MS: f64 = 8_640_000_000_000_000.0;

pub fn time_clip_result(value: f64) -> TimeClipResult {
    if value.is_nan() || !value.is_finite() {
        TimeClipResult::NaN
    } else if value.abs() > MAX_TIME_VALUE_MS {
        TimeClipResult::OutOfRange
    } else {
        TimeClipResult::Valid
    }
}

impl DateInstance {
    pub fn cache_hit(&self, utc: bool) -> bool {
        if utc {
            self.cache.utc_cached_for_ms == Some(self.internal_number)
                && self.cache.utc_fields.is_some()
        } else {
            self.cache.local_cached_for_ms == Some(self.internal_number)
                && self.cache.local_fields.is_some()
        }
    }
}

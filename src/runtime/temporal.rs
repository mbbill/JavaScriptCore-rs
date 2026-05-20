use crate::runtime::exception::JsResult;
use crate::runtime::state::{HostHookId, ObjectId, RuntimeValue, StringId};

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TemporalObject {
    pub object: Option<ObjectId>,
    pub hooks: TemporalHostHooks,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TemporalHostHooks {
    /// Temporal delegates calendar and time-zone lookup to Intl/ICU bridges.
    pub calendar_from_identifier: Option<HostHookId>,
    pub time_zone_from_identifier: Option<HostHookId>,
    pub system_time_zone: Option<HostHookId>,
    pub exact_time: Option<HostHookId>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TemporalValueObject {
    pub object: Option<ObjectId>,
    pub kind: TemporalObjectKind,
    pub calendar: Option<StringId>,
    pub time_zone: Option<StringId>,
    pub slots: TemporalSlots,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum TemporalObjectKind {
    #[default]
    PlainDate,
    PlainTime,
    PlainDateTime,
    PlainMonthDay,
    PlainYearMonth,
    Instant,
    Duration,
    ZonedDateTime,
    Calendar,
    TimeZone,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TemporalSlots {
    pub iso_year: i32,
    pub iso_month: u8,
    pub iso_day: u8,
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
    pub millisecond: u16,
    pub microsecond: u16,
    pub nanosecond: u16,
    pub epoch_nanoseconds: Option<i128>,
    pub duration: Option<TemporalDurationSlots>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub struct TemporalDurationSlots {
    pub years: i64,
    pub months: i64,
    pub weeks: i64,
    pub days: i64,
    pub hours: i64,
    pub minutes: i64,
    pub seconds: i64,
    pub milliseconds: i64,
    pub microseconds: i64,
    pub nanoseconds: i64,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum TemporalUnit {
    Year,
    Month,
    Week,
    Day,
    Hour,
    Minute,
    #[default]
    Second,
    Millisecond,
    Microsecond,
    Nanosecond,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum TemporalOverflow {
    #[default]
    Constrain,
    Reject,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum TemporalRoundingMode {
    Ceil,
    Floor,
    Expand,
    Trunc,
    HalfCeil,
    HalfFloor,
    HalfExpand,
    HalfTrunc,
    #[default]
    HalfEven,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TemporalDifferenceOptions {
    pub largest_unit: TemporalUnit,
    pub smallest_unit: TemporalUnit,
    pub rounding_mode: TemporalRoundingMode,
    pub rounding_increment: u32,
}

pub trait TemporalOperations {
    fn temporal_now(&mut self, kind: TemporalObjectKind) -> JsResult<TemporalValueObject>;
    fn temporal_from(
        &mut self,
        kind: TemporalObjectKind,
        item: RuntimeValue,
        options: RuntimeValue,
    ) -> JsResult<TemporalValueObject>;
    fn temporal_add_duration(
        &mut self,
        value: TemporalValueObject,
        duration: TemporalDurationSlots,
        options: RuntimeValue,
    ) -> JsResult<TemporalValueObject>;
    fn temporal_difference(
        &mut self,
        left: TemporalValueObject,
        right: TemporalValueObject,
        options: TemporalDifferenceOptions,
    ) -> JsResult<TemporalDurationSlots>;
    fn reject_object_with_calendar_or_time_zone(&mut self, object: ObjectId) -> JsResult<()>;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum TemporalSlotValidationError {
    InvalidDate,
    InvalidTime,
    MissingInstantEpoch,
    MissingDurationSlots,
    CalendarNotAllowed,
    TimeZoneNotAllowed,
}

impl TemporalValueObject {
    pub fn validate_slots(&self) -> Result<(), TemporalSlotValidationError> {
        match self.kind {
            TemporalObjectKind::Instant => {
                if self.slots.epoch_nanoseconds.is_none() {
                    return Err(TemporalSlotValidationError::MissingInstantEpoch);
                }
            }
            TemporalObjectKind::Duration => {
                if self.slots.duration.is_none() {
                    return Err(TemporalSlotValidationError::MissingDurationSlots);
                }
            }
            TemporalObjectKind::PlainTime => validate_time_slots(&self.slots)?,
            TemporalObjectKind::PlainDate
            | TemporalObjectKind::PlainDateTime
            | TemporalObjectKind::PlainMonthDay
            | TemporalObjectKind::PlainYearMonth
            | TemporalObjectKind::ZonedDateTime => {
                validate_date_slots(&self.slots)?;
                validate_time_slots(&self.slots)?;
            }
            TemporalObjectKind::Calendar | TemporalObjectKind::TimeZone => {}
        }
        Ok(())
    }
}

fn validate_date_slots(slots: &TemporalSlots) -> Result<(), TemporalSlotValidationError> {
    if slots.iso_month == 0 || slots.iso_month > 12 || slots.iso_day == 0 || slots.iso_day > 31 {
        Err(TemporalSlotValidationError::InvalidDate)
    } else {
        Ok(())
    }
}

fn validate_time_slots(slots: &TemporalSlots) -> Result<(), TemporalSlotValidationError> {
    if slots.hour > 23
        || slots.minute > 59
        || slots.second > 59
        || slots.millisecond > 999
        || slots.microsecond > 999
        || slots.nanosecond > 999
    {
        Err(TemporalSlotValidationError::InvalidTime)
    } else {
        Ok(())
    }
}

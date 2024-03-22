// Copyright 2024 RisingWave Labs
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

#![allow(clippy::redundant_closure_call)]
#![allow(unused_variables)]
use std::sync::Arc;

use anyhow::{Context, Ok};
use arrow_array::{array::*, builder::*};
use arrow_buffer::{Buffer, MutableBuffer, OffsetBuffer};
use arrow_data::ArrayData;
use arrow_schema::DataType;
use libc::c_void;

use crate::v8::V8;

macro_rules! get_typed_array {
    ($array_type: ty, $typed_array: ty, $scope:expr, $array:expr, $mul:expr) => {{
        let array = $array.as_any().downcast_ref::<$array_type>().unwrap();
        let values = array.values().as_ref().to_vec().into_boxed_slice();
        let length = values.len();
        let byte_length = length * $mul;
        let data_ptr = Box::leak(values) as *mut _ as *mut c_void;
        let store = unsafe {
            v8::ArrayBuffer::new_backing_store_from_ptr(
                data_ptr,
                byte_length,
                backing_store_deleter_callback,
                std::ptr::null_mut(),
            )
        }
        .make_shared();

        let buffer = v8::ArrayBuffer::with_backing_store($scope, &store);
        let ty_array = <$typed_array>::new($scope, buffer, 0, length)
            .context("Couldn't create a TypedArray")?;

        Ok(ty_array.into())
    }};
}

macro_rules! build_timestamp_dayjs_array {
    ($array_type: ty, $scope:expr, $values:expr, $function_name:expr, $tz:expr) => {{
        let mut builder = <$array_type>::with_capacity($values.len());

        #[cfg(feature = "with-dayjs")]
        let dayjs = {
            let context = $scope.get_current_context();
            let global = context.global($scope);
            let key = v8::String::new($scope, "dayjs").context("Couldn't create a string")?;
            global.get($scope, key.into())
        };
        #[cfg(not(feature = "with-dayjs"))]
        let dayjs: Option<v8::Local<v8::Value>> = None;

        for val in $values {
            let val = v8::Local::new($scope, val);
            if val.is_null() || val.is_undefined() {
                builder.append_null();
            } else if val.is_date() {
                let date = v8::Local::<v8::Date>::try_from(val)?;
                builder.append_value(date.value_of() as i64);
            } else if let Some(dayjs) = dayjs {
                #[cfg(feature = "with-dayjs")]
                {
                    let dayjs = v8::Local::<v8::Function>::try_from(dayjs)?;
                    let recv = v8::undefined($scope);
                    let dayjs_value = dayjs
                        .call($scope, recv.into(), &[val])
                        .context("Couldn't call the function")?;
                    let dayjs_object = v8::Local::<v8::Object>::try_from(dayjs_value)?;

                    let dayjs_object = if let Some(tz) = $tz {
                        let key =
                            v8::String::new($scope, "tz").context("Couldn't create a string")?;
                        let tz_func = dayjs_object
                            .get($scope, key.into())
                            .context("Couldn't get the tz function")?;
                        let tz_func = v8::Local::<v8::Function>::try_from(tz_func)?;
                        let tz = v8::String::new($scope, tz).context("Couldn't create a string")?;
                        let obj = tz_func
                            .call($scope, dayjs_object.into(), &[tz.into()])
                            .context("Couldn't call the tz function")?;
                        v8::Local::<v8::Object>::try_from(obj)?
                    } else {
                        dayjs_object
                    };

                    let key = v8::String::new($scope, $function_name)
                        .context("Couldn't create a string")?;
                    let val = dayjs_object
                        .get($scope, key.into())
                        .context("Couldn't get the valueOf")?;

                    let value_of_func = v8::Local::<v8::Function>::try_from(val)?;
                    let ms = value_of_func
                        .call($scope, dayjs_object.into(), &[])
                        .context("Couldn't call the valueOf")?;

                    if ms.is_number() {
                        let ms = ms.to_number($scope).context("Couldn't convert to number")?;
                        builder.append_value(ms.value() as i64);
                    } else {
                        return Err(anyhow::anyhow!("Invalid type"));
                    }
                }
                #[cfg(not(feature = "with-dayjs"))]
                return Err(anyhow::anyhow!("Dayjs is not enabled"));
            } else {
                return Err(anyhow::anyhow!("Invalid Date type"));
            }
        }

        Ok(Arc::new(builder.finish()))
    }};
}

macro_rules! build_duration_array {
    ($array_type: ty, $scope:expr, $values:expr, $function_name: expr, $ms: ident, $closure:expr) => {{
        let mut builder = <$array_type>::with_capacity($values.len());

        #[cfg(feature = "with-dayjs")]
        let duration = {
            let context = $scope.get_current_context();
            let global = context.global($scope);
            crate::v8::V8::grab::<v8::Function>($scope, global.into(), "dayjs.duration")
        };
        #[cfg(not(feature = "with-dayjs"))]
        let duration: Option<v8::Local<v8::Function>> = None;

        for val in $values {
            let val = v8::Local::new($scope, val);
            if val.is_null() || val.is_undefined() {
                builder.append_null();
            } else if let Some(dayjs) = duration {
                #[cfg(feature = "with-dayjs")]
                {
                    let key = v8::String::new($scope, "$ms").context("Couldn't create a string")?;
                    let duration_object = v8::Local::<v8::Object>::try_from(val)?;
                    let duration_object = if !duration_object
                        .has_own_property($scope, key.into())
                        .unwrap_or_default()
                    {
                        let recv = v8::undefined($scope);
                        let duration_value = dayjs
                            .call($scope, recv.into(), &[val])
                            .context("Couldn't call the function")?;
                        v8::Local::<v8::Object>::try_from(duration_value)?
                    } else {
                        duration_object
                    };

                    let key = v8::String::new($scope, $function_name)
                        .context("Couldn't create a string")?;
                    let val = duration_object
                        .get($scope, key.into())
                        .context("Couldn't get the conversion function")?;

                    let value_of_func = v8::Local::<v8::Function>::try_from(val)?;
                    let ms = value_of_func
                        .call($scope, duration_object.into(), &[])
                        .context("Couldn't call the conversion function")?;

                    if ms.is_number() {
                        let $ms = ms.to_number($scope).context("Couldn't convert to number")?;
                        let val = $closure($ms);
                        builder.append_value(val);
                    } else {
                        return Err(anyhow::anyhow!("Invalid type"));
                    }
                }
                #[cfg(not(feature = "with-dayjs"))]
                return Err(anyhow::anyhow!("Dayjs is not enabled"));
            } else {
                return Err(anyhow::anyhow!("Invalid Date type"));
            }
        }
        Ok(Arc::new(builder.finish()))
    }};
}

unsafe extern "C" fn backing_store_deleter_callback(
    data: *mut c_void,
    byte_length: usize,
    _deleter_data: *mut c_void,
) {
    let _ = Box::from_raw(std::slice::from_raw_parts_mut(data as *mut u8, byte_length));
}

/// Build arrow array from JS objects.
pub fn build_array(
    data_type: &DataType,
    scope: &mut v8::HandleScope<'_>,
    values: Vec<v8::Global<v8::Value>>,
) -> anyhow::Result<ArrayRef> {
    match data_type {
        DataType::Null => {
            let mut builder = NullBuilder::with_capacity(values.len());
            for val in values {
                let val = v8::Local::new(scope, val);
                if val.is_null() || val.is_undefined() {
                    builder.append_null();
                } else {
                    builder.append_empty_value();
                }
            }
            Ok(Arc::new(builder.finish()))
        }
        DataType::Boolean => {
            let mut builder = BooleanBuilder::with_capacity(values.len());
            for val in values {
                let val = v8::Local::new(scope, val);
                if val.is_null() || val.is_undefined() {
                    builder.append_null();
                } else if val.is_boolean() || val.is_boolean_object() {
                    let val = val.boolean_value(scope);
                    builder.append_value(val);
                } else {
                    return Err(anyhow::anyhow!("Invalid type"));
                }
            }
            Ok(Arc::new(builder.finish()))
        }
        DataType::Int8 => {
            let mut builder = Int8Builder::with_capacity(values.len());
            for val in values {
                let val = v8::Local::new(scope, val);
                if val.is_null() || val.is_undefined() {
                    builder.append_null();
                } else if val.is_number() || val.is_number_object() {
                    let number = v8::Local::<v8::Number>::try_from(val)?;
                    let val = number.value() as i8;
                    builder.append_value(val);
                } else {
                    return Err(anyhow::anyhow!("Invalid type"));
                }
            }
            Ok(Arc::new(builder.finish()))
        }
        DataType::Int16 => {
            let mut builder = Int16Builder::with_capacity(values.len());
            for val in values {
                let val = v8::Local::new(scope, val);
                if val.is_null() || val.is_undefined() {
                    builder.append_null();
                } else if val.is_number() || val.is_number_object() {
                    let number = v8::Local::<v8::Number>::try_from(val)?;
                    let val = number.value() as i16;
                    builder.append_value(val);
                } else {
                    return Err(anyhow::anyhow!("Invalid type"));
                }
            }
            Ok(Arc::new(builder.finish()))
        }
        DataType::Int32 => {
            let mut builder = Int32Builder::with_capacity(values.len());
            for val in values {
                let val = v8::Local::new(scope, val);
                if val.is_null() || val.is_undefined() {
                    builder.append_null();
                } else if val.is_number() || val.is_number_object() {
                    let number = v8::Local::<v8::Number>::try_from(val)?;
                    let val = number.value() as i32;
                    builder.append_value(val);
                } else {
                    return Err(anyhow::anyhow!("Invalid type"));
                }
            }
            Ok(Arc::new(builder.finish()))
        }
        DataType::Int64 => {
            let mut builder = Int64Builder::with_capacity(values.len());
            for val in values {
                let val = v8::Local::new(scope, val);
                if val.is_null() || val.is_undefined() {
                    builder.append_null();
                } else if val.is_big_int() {
                    let val = val
                        .to_big_int(scope)
                        .context("Couldn't convert to BigInt")?;

                    let (val, _) = val.i64_value();

                    builder.append_value(val);
                } else if val.is_number_object() || val.is_number() {
                    let number = v8::Local::<v8::Number>::try_from(val)?;
                    let val = number.value() as i64;
                    builder.append_value(val);
                } else {
                    return Err(anyhow::anyhow!("Invalid type"));
                }
            }
            Ok(Arc::new(builder.finish()))
        }
        DataType::UInt8 => {
            let mut builder = UInt8Builder::with_capacity(values.len());
            for val in values {
                let val = v8::Local::new(scope, val);
                if val.is_null() || val.is_undefined() {
                    builder.append_null();
                } else if val.is_number() || val.is_number_object() {
                    let val = val.uint32_value(scope).context("Couldn't convert to i32")?;
                    builder.append_value(val as u8);
                } else {
                    return Err(anyhow::anyhow!("Invalid type"));
                }
            }
            Ok(Arc::new(builder.finish()))
        }
        DataType::UInt16 => {
            let mut builder = UInt16Builder::with_capacity(values.len());
            for val in values {
                let val = v8::Local::new(scope, val);
                if val.is_null() || val.is_undefined() {
                    builder.append_null();
                } else if val.is_number() || val.is_number_object() {
                    let val = val.uint32_value(scope).context("Couldn't convert to i32")?;
                    builder.append_value(val as u16);
                } else {
                    return Err(anyhow::anyhow!("Invalid type"));
                }
            }
            Ok(Arc::new(builder.finish()))
        }

        DataType::UInt32 => {
            let mut builder = UInt32Builder::with_capacity(values.len());
            for val in values {
                let val = v8::Local::new(scope, val);
                if val.is_null() || val.is_undefined() {
                    builder.append_null();
                } else if val.is_number() || val.is_number_object() {
                    let val = val.uint32_value(scope).context("Couldn't convert to i32")?;
                    builder.append_value(val);
                } else {
                    return Err(anyhow::anyhow!("Invalid type"));
                }
            }
            Ok(Arc::new(builder.finish()))
        }
        DataType::UInt64 => {
            let mut builder = UInt64Builder::with_capacity(values.len());
            for val in values {
                let val = v8::Local::new(scope, val);
                if val.is_null() || val.is_undefined() {
                    builder.append_null();
                } else if val.is_big_int() {
                    let val = val
                        .to_big_int(scope)
                        .context("Couldn't convert to BigInt")?;

                    let (val, _) = val.u64_value();

                    builder.append_value(val);
                } else if val.is_number() || val.is_number_object() {
                    let val = val.number_value(scope).context("Couldn't convert to i32")?;
                    builder.append_value(val as u64);
                } else {
                    return Err(anyhow::anyhow!("Invalid type"));
                }
            }
            Ok(Arc::new(builder.finish()))
        }
        DataType::Float32 => {
            let mut builder = Float32Builder::with_capacity(values.len());
            for val in values {
                let val = v8::Local::new(scope, val);
                if val.is_null() || val.is_undefined() {
                    builder.append_null();
                } else if val.is_number() || val.is_number_object() {
                    let val = val.number_value(scope).context("Couldn't convert to i32")?;
                    builder.append_value(val as f32);
                } else {
                    return Err(anyhow::anyhow!("Invalid type"));
                }
            }
            Ok(Arc::new(builder.finish()))
        }
        DataType::Float64 => {
            let mut builder = Float64Builder::with_capacity(values.len());
            for val in values {
                let val = v8::Local::new(scope, val);
                if val.is_null() || val.is_undefined() {
                    builder.append_null();
                } else if val.is_number() || val.is_number_object() {
                    let val = val.number_value(scope).context("Couldn't convert to i32")?;
                    builder.append_value(val);
                } else {
                    return Err(anyhow::anyhow!("Invalid type"));
                }
            }
            Ok(Arc::new(builder.finish()))
        }
        DataType::Timestamp(u, tz) => match u {
            arrow_schema::TimeUnit::Second => {
                build_timestamp_dayjs_array!(TimestampSecondBuilder, scope, values, "unix", tz)
            }
            arrow_schema::TimeUnit::Millisecond => build_timestamp_dayjs_array!(
                TimestampMillisecondBuilder,
                scope,
                values,
                "valueOf",
                tz
            ),
            arrow_schema::TimeUnit::Microsecond => {
                Err(anyhow::anyhow!("Timestamp Microseconds is not supported "))
            }
            arrow_schema::TimeUnit::Nanosecond => {
                Err(anyhow::anyhow!("Timestamp Nanoseconds is not supported "))
            }
        },
        DataType::Date32 => {
            let mut builder = Date32Builder::with_capacity(values.len());
            let context = scope.get_current_context();
            let global = context.global(scope);
            let key = v8::String::new(scope, "dayjs").context("Couldn't create a string")?;
            let dayjs = global.get(scope, key.into());

            for val in values {
                let val = v8::Local::new(scope, val);
                if val.is_null() || val.is_undefined() {
                    builder.append_null();
                } else if val.is_date() {
                    let date = v8::Local::<v8::Date>::try_from(val)?;
                    builder.append_value((date.value_of() as i64 / 86400000_i64) as i32);
                } else if let Some(dayjs) = dayjs {
                    let dayjs = v8::Local::<v8::Function>::try_from(dayjs)?;
                    let recv = v8::undefined(scope);
                    let dayjs_value = dayjs
                        .call(scope, recv.into(), &[val])
                        .context("Couldn't call the function")?;
                    let dayjs_object = v8::Local::<v8::Object>::try_from(dayjs_value)?;
                    let utc = v8::String::new(scope, "utc").context("Couldn't create a string")?;
                    let utc_func = dayjs_object
                        .get(scope, utc.into())
                        .context("Couldn't get the utc")?;
                    let utc_func = v8::Local::<v8::Function>::try_from(utc_func)?;
                    let obj = utc_func
                        .call(scope, dayjs_object.into(), &[])
                        .context("Couldn't call the utc")?;
                    let obj = v8::Local::<v8::Object>::try_from(obj)?;
                    let key =
                        v8::String::new(scope, "startOf").context("Couldn't create a string")?;
                    let start_of_func = obj
                        .get(scope, key.into())
                        .context("Couldn't get the startOf")?;
                    let start_of_func = v8::Local::<v8::Function>::try_from(start_of_func)?;
                    let day_param =
                        v8::String::new(scope, "day").context("Couldn't create a string")?;
                    let obj = start_of_func
                        .call(scope, obj.into(), &[day_param.into()])
                        .context("Couldn't call the startOf")?;

                    let obj = v8::Local::<v8::Object>::try_from(obj)?;

                    let key =
                        v8::String::new(scope, "valueOf").context("Couldn't create a string")?;
                    let val = obj
                        .get(scope, key.into())
                        .context("Couldn't get the valueOf")?;

                    let value_of_func = v8::Local::<v8::Function>::try_from(val)?;
                    let ms = value_of_func
                        .call(scope, dayjs_object.into(), &[])
                        .context("Couldn't call the valueOf")?;

                    if ms.is_number() {
                        let ms = ms.to_number(scope).context("Couldn't convert to number")?;
                        builder.append_value((ms.value() as i64 / 86400000_i64) as i32);
                    } else {
                        return Err(anyhow::anyhow!("Invalid type"));
                    }
                } else {
                    return Err(anyhow::anyhow!("Invalid Date type"));
                }
            }
            Ok(Arc::new(builder.finish()))
        }
        DataType::Date64 => {
            let mut builder = Date64Builder::with_capacity(values.len());
            let context = scope.get_current_context();
            let global = context.global(scope);
            let key = v8::String::new(scope, "dayjs").context("Couldn't create a string")?;
            let dayjs = global.get(scope, key.into());

            for val in values {
                let val = v8::Local::new(scope, val);
                if val.is_null() || val.is_undefined() {
                    builder.append_null();
                } else if val.is_date() {
                    let date = v8::Local::<v8::Date>::try_from(val)?;
                    builder.append_value(date.value_of() as i64);
                } else if let Some(dayjs) = dayjs {
                    let dayjs = v8::Local::<v8::Function>::try_from(dayjs)?;
                    let recv = v8::undefined(scope);
                    let dayjs_value = dayjs
                        .call(scope, recv.into(), &[val])
                        .context("Couldn't call the function")?;
                    let dayjs_object = v8::Local::<v8::Object>::try_from(dayjs_value)?;
                    let utc = v8::String::new(scope, "utc").context("Couldn't create a string")?;
                    let utc_func = dayjs_object
                        .get(scope, utc.into())
                        .context("Couldn't get the utc")?;
                    let utc_func = v8::Local::<v8::Function>::try_from(utc_func)?;
                    let obj = utc_func
                        .call(scope, dayjs_object.into(), &[])
                        .context("Couldn't call the utc")?;
                    let obj = v8::Local::<v8::Object>::try_from(obj)?;
                    let key =
                        v8::String::new(scope, "startOf").context("Couldn't create a string")?;
                    let start_of_func = obj
                        .get(scope, key.into())
                        .context("Couldn't get the startOf")?;
                    let start_of_func = v8::Local::<v8::Function>::try_from(start_of_func)?;
                    let day_param =
                        v8::String::new(scope, "day").context("Couldn't create a string")?;
                    let obj = start_of_func
                        .call(scope, obj.into(), &[day_param.into()])
                        .context("Couldn't call the startOf")?;

                    let obj = v8::Local::<v8::Object>::try_from(obj)?;

                    let key =
                        v8::String::new(scope, "valueOf").context("Couldn't create a string")?;
                    let val = obj
                        .get(scope, key.into())
                        .context("Couldn't get the valueOf")?;

                    let value_of_func = v8::Local::<v8::Function>::try_from(val)?;
                    let ms = value_of_func
                        .call(scope, dayjs_object.into(), &[])
                        .context("Couldn't call the valueOf")?;

                    if ms.is_number() {
                        let ms = ms.to_number(scope).context("Couldn't convert to number")?;
                        builder.append_value(ms.value() as i64);
                    } else {
                        return Err(anyhow::anyhow!("Invalid type"));
                    }
                } else {
                    return Err(anyhow::anyhow!("Invalid Date type"));
                }
            }
            Ok(Arc::new(builder.finish()))
        }
        DataType::Time32(u) => match u {
            arrow_schema::TimeUnit::Second => {
                build_duration_array!(
                    Time32SecondBuilder,
                    scope,
                    values,
                    "asSeconds",
                    ms,
                    |ms: v8::Local<v8::Number>| ms.value() as i32
                )
            }
            arrow_schema::TimeUnit::Millisecond => {
                build_duration_array!(
                    Time32MillisecondBuilder,
                    scope,
                    values,
                    "asMilliseconds",
                    ms,
                    |ms: v8::Local<v8::Number>| ms.value() as i32
                )
            }
            arrow_schema::TimeUnit::Microsecond => {
                Err(anyhow::anyhow!("Time32 Microseconds is not supported"))
            }
            arrow_schema::TimeUnit::Nanosecond => {
                Err(anyhow::anyhow!("Time32 Nanoseconds is not supported"))
            }
        },
        DataType::Time64(_) => Err(anyhow::anyhow!("Time64 is not supported")),
        DataType::Duration(u) => match u {
            arrow_schema::TimeUnit::Second => {
                build_duration_array!(
                    DurationSecondBuilder,
                    scope,
                    values,
                    "asSeconds",
                    ms,
                    |ms: v8::Local<v8::Number>| ms.value() as i64
                )
            }
            arrow_schema::TimeUnit::Millisecond => {
                build_duration_array!(
                    DurationMillisecondBuilder,
                    scope,
                    values,
                    "asMilliseconds",
                    ms,
                    |ms: v8::Local<v8::Number>| ms.value() as i64
                )
            }
            arrow_schema::TimeUnit::Microsecond => {
                Err(anyhow::anyhow!("Duration Microseconds is not supported"))
            }
            arrow_schema::TimeUnit::Nanosecond => {
                Err(anyhow::anyhow!("Duration Nanoseconds is not supported"))
            }
        },
        DataType::Interval(u) => match u {
            arrow_schema::IntervalUnit::YearMonth => {
                build_duration_array!(
                    IntervalYearMonthBuilder,
                    scope,
                    values,
                    "asMonths",
                    ms,
                    |ms: v8::Local<v8::Number>| ms.value() as i32
                )
            }
            arrow_schema::IntervalUnit::DayTime => {
                build_duration_array!(
                    IntervalDayTimeBuilder,
                    scope,
                    values,
                    "asMilliseconds",
                    ms,
                    |ms: v8::Local<v8::Number>| {
                        let ms = ms.value() as i64;
                        let days = (ms / 86400000_i64) as i32;
                        let ms = (ms % 86400000_i64) as i32;
                        let m = ms as u64 & u32::MAX as u64;
                        let d = (days as u64 & u32::MAX as u64) << 32;
                        (m | d) as i64
                    }
                )
            }
            arrow_schema::IntervalUnit::MonthDayNano => {
                build_duration_array!(
                    IntervalMonthDayNanoBuilder,
                    scope,
                    values,
                    "asMilliseconds",
                    ms,
                    |ms: v8::Local<v8::Number>| {
                        let ms = ms.value() as i64;
                        let months = (ms / 2592000000_i64) as i32;
                        let days = ((ms - (months as i64 * 2592000000_i64)) / 86400000_i64) as i32;
                        let nanos = (ms % 86400000_i64) * 100000_i64;

                        let m = (months as u128 & u32::MAX as u128) << 96;
                        let d = (days as u128 & u32::MAX as u128) << 64;
                        let n = nanos as u128 & u64::MAX as u128;
                        (m | d | n) as i128
                    }
                )
            }
        },
        DataType::Binary => {
            let mut builder = BinaryBuilder::with_capacity(values.len(), 1024);
            for val in values {
                let val = v8::Local::new(scope, val);
                if val.is_null() || val.is_undefined() {
                    builder.append_null();
                } else if val.is_typed_array() {
                    let array = v8::Local::<v8::TypedArray>::try_from(val)?;
                    let store = array
                        .buffer(scope)
                        .context("Couldn't get the buffer")?
                        .get_backing_store();
                    let data = store.data().unwrap().as_ptr() as *mut u8;
                    let len = store.byte_length();
                    let data = unsafe { std::slice::from_raw_parts(data, len) };
                    builder.append_value(data);
                } else if val.is_array() {
                    let array = v8::Local::<v8::Array>::try_from(val)?;
                    let len = array.length();
                    let mut data = vec![];
                    for i in 0..len {
                        let val = array
                            .get_index(scope, i)
                            .context("Couldn't get the index")?;
                        if val.is_number() {
                            let val = val.uint32_value(scope).context("Couldn't convert to u32")?;
                            if val >> 8 == 0 {
                                data.push(val as u8);
                            } else {
                                data.extend(&val.to_le_bytes());
                            }
                        } else {
                            let val = val.to_string(scope).context("Couldn't convert to string")?;
                            data.extend(val.to_rust_string_lossy(scope).as_bytes());
                        }
                    }
                    builder.append_value(data);
                } else if val.is_array_buffer() {
                    let array = v8::Local::<v8::ArrayBuffer>::try_from(val)?;
                    let store = array.get_backing_store();
                    let data = store.data().unwrap().as_ptr() as *mut u8;
                    let len = store.byte_length();
                    let data = unsafe { std::slice::from_raw_parts(data, len) };

                    builder.append_value(data);
                } else {
                    let s = val.to_string(scope).context("Couldn't convert to string")?;
                    builder.append_value(s.to_rust_string_lossy(scope));
                }
            }
            Ok(Arc::new(builder.finish()))
        }
        DataType::FixedSizeBinary(_) => todo!(),
        // decimal type
        DataType::LargeBinary => {
            let mut builder = LargeBinaryBuilder::with_capacity(values.len(), 1024);

            for val in values {
                let val = v8::Local::new(scope, val);
                if val.is_null() || val.is_undefined() {
                    builder.append_null();
                } else {
                    if let std::result::Result::Ok(obj) = v8::Local::<v8::Object>::try_from(val) {
                        let key = v8::String::new(scope, "toString")
                            .context("Can not allocate a new string")?;
                        if let Some(func) = obj.get(scope, key.into()) {
                            if let std::result::Result::Ok(func) =
                                v8::Local::<v8::Function>::try_from(func)
                            {
                                if let Some(val) = func.call(scope, obj.into(), &[]) {
                                    let val = val
                                        .to_string(scope)
                                        .context("Couldn't convert to string")?;
                                    builder.append_value(val.to_rust_string_lossy(scope));
                                    continue;
                                }
                            }
                        }
                    }

                    let number = val.to_number(scope).context("Couldn't convert to number")?;
                    let value = number.value();
                    builder.append_value(value.to_string());
                }
            }
            Ok(Arc::new(builder.finish()))
        }
        DataType::Utf8 => {
            let mut builder = StringBuilder::with_capacity(values.len(), 1024);
            for val in values {
                let val = v8::Local::new(scope, val);
                if val.is_null() || val.is_undefined() {
                    builder.append_null();
                } else {
                    let s = val.to_string(scope).context("Couldn't convert to string")?;
                    builder.append_value(s.to_rust_string_lossy(scope));
                }
            }
            Ok(Arc::new(builder.finish()))
        }
        DataType::LargeUtf8 => {
            let mut builder = LargeStringBuilder::with_capacity(values.len(), 1024);
            for val in values {
                let val = v8::Local::new(scope, val);
                if val.is_null() || val.is_undefined() {
                    builder.append_null();
                } else if let Some(s) = v8::json::stringify(scope, val) {
                    builder.append_value(s.to_rust_string_lossy(scope));
                } else {
                    builder.append_null();
                }
            }
            Ok(Arc::new(builder.finish()))
        }
        // list
        DataType::List(inner) => {
            // flatten lists
            let mut flatten_values = vec![];
            let mut offsets = Vec::<i32>::with_capacity(values.len() + 1);

            let mut buffer = MutableBuffer::new(values.len() * 4);
            let mut buffer_offsets = Vec::<i32>::with_capacity(values.len() + 1);
            let mut buffer_start_index = 0;

            offsets.push(0);
            buffer_offsets.push(0);

            for val in &values {
                let val = v8::Local::new(scope, val.clone());
                if !val.is_null() && !val.is_undefined() {
                    if val.is_array() {
                        let array = v8::Local::<v8::Array>::try_from(val)?;
                        flatten_values.reserve(array.length() as usize);
                        for i in 0..array.length() {
                            let elem = array
                                .get_index(scope, i)
                                .context("Couldn't get the index")?;
                            flatten_values.push(v8::Global::new(scope, elem));
                        }
                    } else if val.is_typed_array() {
                        let array = v8::Local::<v8::TypedArray>::try_from(val)?;
                        let store = array
                            .buffer(scope)
                            .context("Couldn't get the buffer")?
                            .get_backing_store();
                        let data = store.data().unwrap().as_ptr() as *mut u8;
                        let len = store.byte_length();
                        let data = unsafe { std::slice::from_raw_parts(data, len) };
                        buffer.extend_from_slice(data);
                        buffer_start_index += len;
                        buffer_offsets.push(buffer_start_index as i32);
                    } else if val.is_array_buffer() {
                        let array = v8::Local::<v8::ArrayBuffer>::try_from(val)?;
                        let store = array.get_backing_store();
                        let data = store.data().unwrap().as_ptr() as *mut u8;
                        let len = store.byte_length();
                        let data = unsafe { std::slice::from_raw_parts(data, len) };

                        buffer.extend_from_slice(data);
                        buffer_start_index += len;
                        buffer_offsets.push(buffer_start_index as i32);
                    } else {
                        return Err(anyhow::anyhow!("Invalid type"));
                    }
                }
                offsets.push(flatten_values.len() as i32);
            }

            if !buffer.is_empty() {
                let value_data = ArrayData::builder(inner.data_type().clone())
                    .len(buffer.len())
                    .add_buffer(buffer.into())
                    .build()?;

                let value_offsets = Buffer::from_vec(buffer_offsets);

                let list_data_type = DataType::List(inner.clone());
                let list_data = ArrayData::builder(list_data_type)
                    .len(1)
                    .add_buffer(value_offsets)
                    .add_child_data(value_data)
                    .build()
                    .unwrap();
                let list_array = ListArray::from(list_data);

                return Ok(Arc::new(list_array));
            }

            let values_array = build_array(inner.data_type(), scope, flatten_values)?;
            let nulls = values
                .iter()
                .map(|v| {
                    let v = v8::Local::new(scope, v);
                    !v.is_null() && !v.is_undefined()
                })
                .collect();
            Ok(Arc::new(ListArray::new(
                inner.clone(),
                OffsetBuffer::new(offsets.into()),
                values_array,
                Some(nulls),
            )))
        }
        DataType::FixedSizeList(_, _) => todo!(),
        DataType::LargeList(_) => todo!(),
        DataType::Struct(fields) => {
            let mut arrays = Vec::with_capacity(fields.len());
            for field in fields {
                let mut field_values = Vec::with_capacity(values.len());
                for val in &values {
                    let val = v8::Local::new(scope, val.clone());
                    let v = if val.is_null() || val.is_undefined() {
                        v8::null(scope).into()
                    } else {
                        let object = v8::Local::<v8::Object>::try_from(val)?;
                        let name = v8::String::new(scope, field.name())
                            .context("Couldn't create a string")?;
                        object.get(scope, name.into()).context("Field not found")?
                    };
                    field_values.push(v8::Global::new(scope, v));
                }
                arrays.push(build_array(field.data_type(), scope, field_values)?);
            }
            let nulls = values
                .iter()
                .map(|v| {
                    let v = v8::Local::new(scope, v);
                    !v.is_null() && !v.is_undefined()
                })
                .collect();
            Ok(Arc::new(StructArray::new(
                fields.clone(),
                arrays,
                Some(nulls),
            )))
        }
        DataType::Union(_, _) => todo!(),
        DataType::Dictionary(_, _) => todo!(),
        DataType::Decimal128(_, _) => todo!(),
        DataType::Decimal256(_, _) => todo!(),
        DataType::Map(_, _) => todo!(),
        DataType::RunEndEncoded(_, _) => todo!(),
        _ => todo!(),
    }
}

pub fn get_jsvalue<'s>(
    scope: &mut v8::HandleScope<'s>,
    array: &dyn Array,
    big_decimal: &v8::Global<v8::Function>,
    i: usize,
) -> anyhow::Result<v8::Local<'s, v8::Value>> {
    if array.is_null(i) {
        return Ok(v8::null(scope).into());
    }
    match array.data_type() {
        DataType::Null => Ok(v8::null(scope).into()),
        DataType::Boolean => {
            let array = array.as_any().downcast_ref::<BooleanArray>().unwrap();
            Ok(v8::Boolean::new(scope, array.value(i)).into())
        }
        DataType::Int8 => {
            let array = array.as_any().downcast_ref::<Int8Array>().unwrap();
            Ok(v8::Integer::new(scope, array.value(i) as i32).into())
        }
        DataType::Int16 => {
            let array = array.as_any().downcast_ref::<Int16Array>().unwrap();
            Ok(v8::Integer::new(scope, array.value(i) as i32).into())
        }
        DataType::Int32 => {
            let array = array.as_any().downcast_ref::<Int32Array>().unwrap();
            Ok(v8::Integer::new(scope, array.value(i)).into())
        }
        DataType::Int64 => {
            let array = array.as_any().downcast_ref::<Int64Array>().unwrap();
            Ok(v8::BigInt::new_from_i64(scope, array.value(i)).into())
        }
        DataType::UInt8 => {
            let array = array.as_any().downcast_ref::<UInt8Array>().unwrap();
            Ok(v8::Integer::new(scope, array.value(i) as i32).into())
        }
        DataType::UInt16 => {
            let array = array.as_any().downcast_ref::<UInt16Array>().unwrap();
            Ok(v8::Integer::new(scope, array.value(i) as i32).into())
        }
        DataType::UInt32 => {
            let array = array.as_any().downcast_ref::<UInt32Array>().unwrap();
            Ok(v8::Integer::new(scope, array.value(i) as i32).into())
        }
        DataType::UInt64 => {
            let array = array.as_any().downcast_ref::<UInt64Array>().unwrap();
            Ok(v8::BigInt::new_from_u64(scope, array.value(i)).into())
        }
        DataType::Float32 => {
            let array = array.as_any().downcast_ref::<Float32Array>().unwrap();
            Ok(v8::Number::new(scope, array.value(i) as f64).into())
        }
        DataType::Float64 => {
            let array = array.as_any().downcast_ref::<Float64Array>().unwrap();
            Ok(v8::Number::new(scope, array.value(i)).into())
        }
        DataType::Date32 => {
            let array = array.as_any().downcast_ref::<Date32Array>().unwrap();
            #[cfg(not(feature = "with-dayjs"))]
            {
                let date = v8::Date::new(scope, (array.value(i) as i64 * 86400000_i64) as f64)
                    .context("Couldn't create a date")?;
                Ok(date.into())
            }

            #[cfg(feature = "with-dayjs")]
            {
                let script = format!(
                    "dayjs(BigInt('{}'))",
                    (array.value(i) as i64 * 86400000_i64)
                );
                let script = V8::compile_script(scope, "timeValue", &script)?
                    .expect("should compile script");

                Ok(script.run(scope).context("should run script")?)
            }
        }
        DataType::Date64 => {
            let array = array.as_any().downcast_ref::<Date64Array>().unwrap();
            #[cfg(not(feature = "with-dayjs"))]
            {
                let date = v8::Date::new(scope, array.value(i) as f64)
                    .context("Couldn't create a date")?;
                Ok(date.into())
            }
            #[cfg(feature = "with-dayjs")]
            {
                let script = format!("dayjs(BigInt('{}'))", (array.value(i)));
                let script = V8::compile_script(scope, "timeValue", &script)?
                    .expect("should compile script");

                Ok(script.run(scope).context("should run script")?)
            }
        }
        DataType::Time32(u) => {
            #[cfg(feature = "with-dayjs")]
            {
                let script = match u {
                    arrow_schema::TimeUnit::Second => {
                        let array = array.as_any().downcast_ref::<Time32SecondArray>().unwrap();
                        format!("dayjs.duration({}, 'seconds')", array.value(i))
                    }
                    arrow_schema::TimeUnit::Millisecond => {
                        let array = array
                            .as_any()
                            .downcast_ref::<Time32MillisecondArray>()
                            .unwrap();
                        format!("dayjs.duration({}, 'milliseconds')", array.value(i))
                    }
                    _ => Err(anyhow::anyhow!("Invalid time unit"))?,
                };

                let script = V8::compile_script(scope, "timeValue", &script)?
                    .expect("should compile script");
                Ok(script.run(scope).context("should run script")?)
            }
            #[cfg(not(feature = "with-dayjs"))]
            {
                Err(anyhow::anyhow!("Time32 is not supported"))?
            }
        }
        DataType::Time64(_) => Err(anyhow::anyhow!("Time64 is not supported"))?,
        DataType::Duration(u) => {
            #[cfg(feature = "with-dayjs")]
            {
                let script = match u {
                    arrow_schema::TimeUnit::Second => {
                        let array = array
                            .as_any()
                            .downcast_ref::<DurationSecondArray>()
                            .unwrap();
                        format!("dayjs.duration({}, 'seconds')", array.value(i))
                    }
                    arrow_schema::TimeUnit::Millisecond => {
                        let array = array
                            .as_any()
                            .downcast_ref::<DurationMillisecondArray>()
                            .unwrap();
                        format!("dayjs.duration({}, 'milliseconds')", array.value(i))
                    }
                    _ => Err(anyhow::anyhow!("Invalid time unit"))?,
                };

                let script = V8::compile_script(scope, "timeValue", &script)?
                    .expect("should compile script");
                Ok(script.run(scope).context("should run script")?)
            }
            #[cfg(not(feature = "with-dayjs"))]
            {
                Err(anyhow::anyhow!("Duration is not supported"))?
            }
        }
        DataType::Interval(u) => {
            #[cfg(feature = "with-dayjs")]
            {
                let script = match u {
                    arrow_schema::IntervalUnit::YearMonth => {
                        let array = array
                            .as_any()
                            .downcast_ref::<IntervalYearMonthArray>()
                            .unwrap();
                        format!("dayjs.duration({}, 'months')", array.value(i))
                    }
                    arrow_schema::IntervalUnit::DayTime => {
                        let array = array
                            .as_any()
                            .downcast_ref::<IntervalDayTimeArray>()
                            .unwrap();

                        let value = array.value(i);
                        let days = (value >> 32) as i32;
                        let ms = value as i32;

                        let total_millis = days as i64 * 86400000_i64 + ms as i64;

                        format!("dayjs.duration({})", total_millis)
                    }
                    arrow_schema::IntervalUnit::MonthDayNano => {
                        let array = array
                            .as_any()
                            .downcast_ref::<IntervalMonthDayNanoArray>()
                            .unwrap();

                        let val = array.value(i);

                        let months = (val >> 96) as i32;
                        let days = (val >> 64) as i32;
                        let nanos = val as i64;
                        let millis = nanos / 1000000_i64;

                        let total_millis =
                            months as i64 * 2592000000_i64 + days as i64 * 86400000_i64 + millis;

                        format!("dayjs.duration({})", total_millis)
                    }
                };

                let script = V8::compile_script(scope, "intervalValue", &script)?
                    .expect("should compile script");
                Ok(script.run(scope).context("should run script")?)
            }
            #[cfg(not(feature = "with-dayjs"))]
            {
                Err(anyhow::anyhow!("Interval is not supported"))?
            }
        }
        DataType::Binary => {
            let array = array.as_any().downcast_ref::<BinaryArray>().unwrap();
            let bytes = array.value(i).to_vec();
            let len = bytes.len();
            let backing_store = v8::ArrayBuffer::new_backing_store_from_vec(bytes).make_shared();
            let buffer = v8::ArrayBuffer::with_backing_store(scope, &backing_store);
            let u8array = v8::Uint8Array::new(scope, buffer, 0, len)
                .context("Couldn't create a Uint8Array")?;

            Ok(u8array.into())
        }
        DataType::FixedSizeBinary(_) => todo!(),
        // decimal type
        DataType::LargeBinary => {
            let array = array.as_any().downcast_ref::<LargeBinaryArray>().unwrap();
            let str = std::str::from_utf8(array.value(i))?;
            let val = v8::String::new(scope, str).context("Couldn't create a string")?;
            let bigdecimal = v8::Local::new(scope, big_decimal);
            let recv = v8::undefined(scope);
            let try_catch = &mut v8::TryCatch::new(scope);
            let result = bigdecimal.new_instance(try_catch, &[val.into()]);

            match result {
                Some(r) => Ok(r.into()),
                None => {
                    assert!(try_catch.has_caught());
                    //Avoids killing the isolate even if it was requested
                    if try_catch.is_execution_terminating() {
                        try_catch.cancel_terminate_execution();
                        anyhow::bail!("Execution was terminated");
                    }
                    let exception = try_catch.exception().unwrap();

                    crate::v8::V8::exception_to_err_result(try_catch, exception, false)
                }
            }
        }
        DataType::Utf8 => {
            let array = array.as_any().downcast_ref::<StringArray>().unwrap();
            let string =
                v8::String::new(scope, array.value(i)).context("Couldn't create a string")?;
            Ok(string.into())
        }
        DataType::LargeUtf8 => {
            let array = array.as_any().downcast_ref::<LargeStringArray>().unwrap();
            let json_string =
                v8::String::new(scope, array.value(i)).context("Couldn't create a string")?;
            Ok(v8::json::parse(scope, json_string).context("Couldn't parse the json string")?)
        }
        DataType::List(inner) => {
            let array = array.as_any().downcast_ref::<ListArray>().unwrap();
            let list = array.value(i);

            match inner.data_type() {
                DataType::Int8 => get_typed_array!(Int8Array, v8::Int8Array, scope, list, 1),
                DataType::Int16 => get_typed_array!(Int16Array, v8::Int16Array, scope, list, 2),
                DataType::Int32 => get_typed_array!(Int32Array, v8::Int32Array, scope, list, 4),
                DataType::Int64 => get_typed_array!(Int64Array, v8::BigInt64Array, scope, list, 8),
                DataType::UInt8 => get_typed_array!(UInt8Array, v8::Uint8Array, scope, list, 1),
                DataType::UInt16 => get_typed_array!(UInt16Array, v8::Uint16Array, scope, list, 2),
                DataType::UInt32 => get_typed_array!(UInt32Array, v8::Uint32Array, scope, list, 4),
                DataType::UInt64 => {
                    get_typed_array!(UInt64Array, v8::BigUint64Array, scope, list, 8)
                }
                DataType::Float32 => {
                    get_typed_array!(Float32Array, v8::Float32Array, scope, list, 4)
                }
                DataType::Float64 => {
                    get_typed_array!(Float64Array, v8::Float64Array, scope, list, 8)
                }
                _ => {
                    let array = v8::Array::new(scope, list.len() as i32);
                    for j in 0..list.len() {
                        let val = get_jsvalue(scope, list.as_ref(), big_decimal, j)?;
                        array.set_index(scope, j as u32, val);
                    }
                    Ok(array.into())
                }
            }
        }
        DataType::FixedSizeList(_, _) => todo!(),
        DataType::LargeList(_) => todo!(),
        DataType::Struct(fields) => {
            let array = array.as_any().downcast_ref::<StructArray>().unwrap();
            let object = v8::Object::new(scope);
            for (j, field) in fields.iter().enumerate() {
                let value = get_jsvalue(scope, array.column(j).as_ref(), big_decimal, i)?;
                let name =
                    v8::String::new(scope, field.name()).context("Couldn't create a string")?;
                if !object.set(scope, name.into(), value).unwrap_or_default() {
                    return Err(anyhow::anyhow!("Couldn't set the field"));
                }
            }
            Ok(object.into())
        }
        DataType::Union(_, _) => todo!(),
        DataType::Dictionary(_, _) => todo!(),
        DataType::Decimal128(_, _) => todo!(),
        DataType::Decimal256(_, _) => todo!(),
        DataType::Map(_, _) => todo!(),
        DataType::RunEndEncoded(_, _) => todo!(),
        _ => todo!(),
    }
}

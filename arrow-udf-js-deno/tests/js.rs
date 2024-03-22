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

use std::sync::Arc;

#[cfg(feature = "with-dayjs")]
use arrow_array::{temporal_conversions::time_to_time32ms, Date32Array, Time32MillisecondArray};

use arrow_array::{
    types::*, ArrayRef, BinaryArray, Int32Array, LargeBinaryArray, LargeStringArray, ListArray,
    PrimitiveArray, RecordBatch, StringArray, StructArray,
};
use arrow_cast::{display::ArrayFormatter, pretty::pretty_format_batches};

#[cfg(feature = "with-dayjs")]
use arrow_schema::TimeUnit;

use arrow_schema::{DataType, Field, Schema};
use arrow_udf_js_deno::{CallMode, Runtime};

#[cfg(feature = "with-dayjs")]
use chrono::{NaiveDate, NaiveTime};

use expect_test::{expect, Expect};

#[tokio::test(flavor = "current_thread")]
async fn test_gcd() {
    let runtime = Runtime::new();

    let js_code = r#"
        export function gcd(a, b) {
            while (b != 0) {
                let t = b;
                b = a % b;
                a = t;
            }
            return a;
        }
    "#;
    runtime
        .add_function(
            "gcd",
            DataType::Int32,
            CallMode::ReturnNullOnNullInput,
            js_code,
        )
        .await
        .unwrap();

    let schema = Schema::new(vec![
        Field::new("x", DataType::Int32, true),
        Field::new("y", DataType::Int32, true),
    ]);
    let arg0 = Int32Array::from(vec![Some(25), None]);
    let arg1 = Int32Array::from(vec![Some(15), None]);
    let input =
        RecordBatch::try_new(Arc::new(schema), vec![Arc::new(arg0), Arc::new(arg1)]).unwrap();

    let output = runtime.call("gcd", input).await.unwrap();
    check(
        &[output],
        expect![[r#"
        +-----+
        | gcd |
        +-----+
        | 5   |
        |     |
        +-----+"#]],
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn test_gcd_multithread() {
    let runtime = Runtime::new();

    let js_code = r#"
        export function gcd(a, b) {
            while (b != 0) {
                let t = b;
                b = a % b;
                a = t;
            }
            return a;
        }
    "#;
    runtime
        .add_function(
            "gcd",
            DataType::Int32,
            CallMode::ReturnNullOnNullInput,
            js_code,
        )
        .await
        .unwrap();

    let schema = Schema::new(vec![
        Field::new("x", DataType::Int32, true),
        Field::new("y", DataType::Int32, true),
    ]);
    let arg0 = Int32Array::from(vec![Some(25), None]);
    let arg1 = Int32Array::from(vec![Some(15), None]);
    let input =
        RecordBatch::try_new(Arc::new(schema), vec![Arc::new(arg0), Arc::new(arg1)]).unwrap();

    let output = runtime.call("gcd", input).await.unwrap();
    check(
        &[output],
        expect![[r#"
        +-----+
        | gcd |
        +-----+
        | 5   |
        |     |
        +-----+"#]],
    );
}

#[tokio::test(flavor = "current_thread")]
async fn test_to_string() {
    let runtime = Runtime::new();

    let js_code = r#"
        export function to_string(a) {
            if (a == null) {
                return "null";
            }
            return a.toString();
        }
    "#;
    runtime
        .add_function(
            "to_string",
            DataType::Utf8,
            CallMode::CalledOnNullInput,
            js_code,
        )
        .await
        .unwrap();

    let schema = Schema::new(vec![Field::new("x", DataType::Int32, true)]);
    let arg0 = Int32Array::from(vec![Some(5), None]);
    let input = RecordBatch::try_new(Arc::new(schema), vec![Arc::new(arg0)]).unwrap();

    let output = runtime.call("to_string", input).await.unwrap();
    check(
        &[output],
        expect![[r#"
        +-----------+
        | to_string |
        +-----------+
        | 5         |
        | null      |
        +-----------+"#]],
    );
}

#[tokio::test(flavor = "current_thread")]
async fn test_concat() {
    let runtime = Runtime::new();

    runtime
        .add_function(
            "concat",
            DataType::Binary,
            CallMode::ReturnNullOnNullInput,
            r#"
            export function concat(a, b) {
                return [ ...a, ...b];

            }
            "#,
        )
        .await
        .unwrap();

    let schema = Schema::new(vec![
        Field::new("a", DataType::Binary, true),
        Field::new("b", DataType::Binary, true),
    ]);
    let arg0 = BinaryArray::from(vec![&b"hello"[..]]);
    let arg1 = BinaryArray::from(vec![&b"world"[..]]);
    let input =
        RecordBatch::try_new(Arc::new(schema), vec![Arc::new(arg0), Arc::new(arg1)]).unwrap();

    let output = runtime.call("concat", input).await.unwrap();
    check(
        &[output],
        expect![[r#"
        +----------------------+
        | concat               |
        +----------------------+
        | 68656c6c6f776f726c64 |
        +----------------------+"#]],
    );
}

#[tokio::test(flavor = "current_thread")]
async fn test_json_array_access() {
    let runtime = Runtime::new();

    runtime
        .add_function(
            "json_array_access",
            DataType::LargeUtf8,
            CallMode::ReturnNullOnNullInput,
            r#"
            export function json_array_access(array, i) {
                return array[i];
            }
            "#,
        )
        .await
        .unwrap();

    let schema = Schema::new(vec![
        Field::new("array", DataType::LargeUtf8, true),
        Field::new("i", DataType::Int32, true),
    ]);
    let arg0 = LargeStringArray::from(vec![r#"[1, null, ""]"#]);
    let arg1 = Int32Array::from(vec![0]);
    let input =
        RecordBatch::try_new(Arc::new(schema), vec![Arc::new(arg0), Arc::new(arg1)]).unwrap();

    let output = runtime.call("json_array_access", input).await.unwrap();
    check(
        &[output],
        expect![[r#"
        +-------------------+
        | json_array_access |
        +-------------------+
        | 1                 |
        +-------------------+"#]],
    );
}

#[tokio::test(flavor = "current_thread")]
async fn test_json_stringify() {
    let runtime = Runtime::new();

    runtime
        .add_function(
            "json_stringify",
            DataType::Utf8,
            CallMode::ReturnNullOnNullInput,
            r#"
            export function json_stringify(object) {
                return JSON.stringify(object);
            }
            "#,
        )
        .await
        .unwrap();

    let schema = Schema::new(vec![Field::new("json", DataType::LargeUtf8, true)]);
    let arg0 = LargeStringArray::from(vec![r#"[1, null, ""]"#]);
    let input = RecordBatch::try_new(Arc::new(schema), vec![Arc::new(arg0)]).unwrap();

    let output = runtime.call("json_stringify", input).await.unwrap();
    check(
        &[output],
        expect![[r#"
        +----------------+
        | json_stringify |
        +----------------+
        | [1,null,""]    |
        +----------------+"#]],
    );
}

#[tokio::test(flavor = "current_thread")]
async fn test_decimal_add() {
    let runtime = Runtime::new();

    runtime
        .add_function(
            "decimal_add",
            DataType::LargeBinary,
            CallMode::ReturnNullOnNullInput,
            r#"
            export function decimal_add(a, b) {
                return a.add(b);
            }
            "#,
        )
        .await
        .unwrap();

    let schema = Schema::new(vec![
        Field::new("a", DataType::LargeBinary, true),
        Field::new("b", DataType::LargeBinary, true),
    ]);
    let arg0 = LargeBinaryArray::from(vec![b"0.0001".as_ref()]);
    let arg1 = LargeBinaryArray::from(vec![b"0.0002".as_ref()]);
    let input =
        RecordBatch::try_new(Arc::new(schema), vec![Arc::new(arg0), Arc::new(arg1)]).unwrap();

    let output = runtime.call("decimal_add", input).await.unwrap();
    check(
        &[output],
        expect![[r#"
        +--------------+
        | decimal_add  |
        +--------------+
        | 302e30303033 |
        +--------------+"#]],
    );
}

#[tokio::test(flavor = "current_thread")]
async fn test_primitive_types() {
    let runtime = Runtime::new();

    async fn create_function<T>(
        runtime: &Runtime,
        data_type: DataType,
        value: T::Native,
    ) -> (String, RecordBatch)
    where
        T: ArrowPrimitiveType,
        PrimitiveArray<T>: From<Vec<T::Native>>,
    {
        let name = format!("{}_type", data_type.to_string().to_lowercase());
        let js_code = format!(
            r#"
            export function {}(a) {{
                return a;
            }}
            "#,
            name
        );

        runtime
            .add_function(
                &name,
                data_type.clone(),
                CallMode::ReturnNullOnNullInput,
                &js_code,
            )
            .await
            .unwrap();

        let schema = Schema::new(vec![Field::new("a", data_type, true)]);
        //let arg0 = T::from(vec![9007199254740991_i64]);

        let arg0 = PrimitiveArray::<T>::from(vec![value]);
        let input = RecordBatch::try_new(Arc::new(schema), vec![Arc::new(arg0)]).unwrap();

        (name, input)
    }

    let cases = [
        (
            create_function::<Int8Type>(&runtime, DataType::Int8, 1).await,
            "1",
            DataType::Int8,
        ),
        (
            create_function::<Int16Type>(&runtime, DataType::Int16, 1).await,
            "1",
            DataType::Int16,
        ),
        (
            create_function::<Int32Type>(&runtime, DataType::Int32, 1).await,
            "1",
            DataType::Int32,
        ),
        (
            create_function::<Int64Type>(&runtime, DataType::Int64, 9007199254740991_i64).await,
            "9007199254740991",
            DataType::Int64,
        ),
        (
            create_function::<UInt8Type>(&runtime, DataType::UInt8, 1).await,
            "1",
            DataType::UInt8,
        ),
        (
            create_function::<UInt16Type>(&runtime, DataType::UInt16, 1).await,
            "1",
            DataType::UInt16,
        ),
        (
            create_function::<UInt32Type>(&runtime, DataType::UInt32, 1).await,
            "1",
            DataType::UInt32,
        ),
        (
            create_function::<UInt64Type>(&runtime, DataType::UInt64, 9007199254740991_u64).await,
            "9007199254740991",
            DataType::UInt64,
        ),
        (
            create_function::<Float32Type>(&runtime, DataType::Float32, 0.1_f32).await,
            "0.1",
            DataType::Float32,
        ),
        (
            create_function::<Float64Type>(&runtime, DataType::Float64, 0.1_f64).await,
            "0.1",
            DataType::Float64,
        ),
    ];

    for ((name, input), expected, expected_type) in cases.into_iter() {
        let output = runtime.call(&name, input).await.unwrap();
        let col = output.column(0);

        assert_eq!(col.data_type(), &expected_type);

        let formatter = ArrayFormatter::try_new(col.as_ref(), &Default::default()).unwrap();
        let result = formatter.value(0).to_string();
        assert_eq!(result, expected);
    }
}

#[tokio::test(flavor = "current_thread")]
async fn test_decimal_gc() {
    let runtime = Runtime::new();

    runtime
        .add_function(
            "decimal_gc",
            DataType::LargeBinary,
            CallMode::ReturnNullOnNullInput,
            r#"
            export function decimal_gc(a) {
                let result = new BigDecimal(1);
                for ( let i = 2; i <= a; i++) {
                    result = result.mul(new BigDecimal(i));
                }
                return result;
            }
            "#,
        )
        .await
        .unwrap();

    let schema = Schema::new(vec![Field::new("a", DataType::Int32, true)]);
    let arg0 = Int32Array::from(vec![100]);
    let input = RecordBatch::try_new(Arc::new(schema), vec![Arc::new(arg0)]).unwrap();

    let output = runtime.call("decimal_gc", input).await.unwrap();
    check(
        &[output],
        expect![[r#"
        +------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------+
        | decimal_gc                                                                                                                                                                                                                                                                                                                   |
        +------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------+
        | 3933333236323135343433393434313532363831363939323338383536323636373030343930373135393638323634333831363231343638353932393633383935323137353939393933323239393135363038393431343633393736313536353138323836323533363937393230383237323233373538323531313835323130393136383634303030303030303030303030303030303030303030303030 |
        +------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------+"#]],
    );
}

#[tokio::test(flavor = "current_thread")]
async fn test_typed_array() {
    let runtime = Runtime::new();

    runtime
        .add_function(
            "object_type",
            DataType::Utf8,
            CallMode::ReturnNullOnNullInput,
            r#"
            export function object_type(a) {
                return Object.prototype.toString.call(a);
            }
            "#,
        )
        .await
        .unwrap();

    /// Generate a record batch with a single column of type `List<T>`.
    fn array_input<T: ArrowPrimitiveType>() -> RecordBatch {
        let schema = Schema::new(vec![Field::new(
            "x",
            DataType::new_list(T::DATA_TYPE, true),
            true,
        )]);
        let arg0 =
            ListArray::from_iter_primitive::<T, _, _>(vec![Some(vec![Some(Default::default())])]);
        RecordBatch::try_new(Arc::new(schema), vec![Arc::new(arg0)]).unwrap()
    }

    let cases = [
        // (input, JS object type)
        (array_input::<Int8Type>(), "Int8Array"),
        (array_input::<Int16Type>(), "Int16Array"),
        (array_input::<Int32Type>(), "Int32Array"),
        (array_input::<Int64Type>(), "BigInt64Array"),
        (array_input::<UInt8Type>(), "Uint8Array"),
        (array_input::<UInt16Type>(), "Uint16Array"),
        (array_input::<UInt32Type>(), "Uint32Array"),
        (array_input::<UInt64Type>(), "BigUint64Array"),
        (array_input::<Float32Type>(), "Float32Array"),
        (array_input::<Float64Type>(), "Float64Array"),
    ];

    for (input, expected) in cases.into_iter() {
        let output = runtime.call("object_type", input).await.unwrap();
        let object_type = output
            .column(0)
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap()
            .value(0);
        assert_eq!(object_type, format!("[object {}]", expected));
    }
}

#[tokio::test(flavor = "current_thread")]
async fn test_return_array() {
    let runtime = Runtime::new();

    runtime
        .add_function(
            "to_array",
            DataType::new_list(DataType::Int32, true),
            CallMode::CalledOnNullInput,
            r#"
            export function to_array(x) {
                if(x == null) {
                    return null;
                }
                return [x];
            }
            "#,
        )
        .await
        .unwrap();

    let schema = Schema::new(vec![Field::new("x", DataType::Int32, true)]);
    let arg0 = Int32Array::from(vec![Some(1), None, Some(3)]);
    let input = RecordBatch::try_new(Arc::new(schema), vec![Arc::new(arg0)]).unwrap();

    let output = runtime.call("to_array", input).await.unwrap();
    check(
        &[output],
        expect![[r#"
        +----------+
        | to_array |
        +----------+
        | [1]      |
        |          |
        | [3]      |
        +----------+"#]],
    );
}

#[tokio::test(flavor = "current_thread")]
async fn test_key_value() {
    let runtime = Runtime::new();

    runtime
        .add_function(
            "key_value",
            DataType::Struct(
                vec![
                    Field::new("key", DataType::Utf8, true),
                    Field::new("value", DataType::Utf8, true),
                ]
                .into(),
            ),
            CallMode::ReturnNullOnNullInput,
            r#"
            export function key_value(s) {
                const [key, value] = s.split("=", 2);
                return {key, value};
            }
            "#,
        )
        .await
        .unwrap();

    let schema = Schema::new(vec![Field::new("x", DataType::Utf8, true)]);
    let arg0 = StringArray::from(vec!["a=b"]);
    let input = RecordBatch::try_new(Arc::new(schema), vec![Arc::new(arg0)]).unwrap();

    let output = runtime.call("key_value", input).await.unwrap();
    check(
        &[output],
        expect![[r#"
        +--------------------+
        | key_value          |
        +--------------------+
        | {key: a, value: b} |
        +--------------------+"#]],
    );
}

#[tokio::test(flavor = "current_thread")]
async fn test_struct_to_json() {
    let runtime = Runtime::new();

    runtime
        .add_function(
            "to_json",
            DataType::LargeUtf8,
            CallMode::ReturnNullOnNullInput,
            r#"
            export function to_json(object) {
                return object;
            }
            "#,
        )
        .await
        .unwrap();

    let schema = Schema::new(vec![Field::new(
        "struct",
        DataType::Struct(
            vec![
                Field::new("key", DataType::Utf8, true),
                Field::new("value", DataType::Utf8, true),
            ]
            .into(),
        ),
        true,
    )]);
    let arg0 = StructArray::from(vec![
        (
            Arc::new(Field::new("key", DataType::Utf8, true)),
            Arc::new(StringArray::from(vec![Some("a"), None])) as ArrayRef,
        ),
        (
            Arc::new(Field::new("value", DataType::Utf8, true)),
            Arc::new(StringArray::from(vec![Some("b"), None])),
        ),
    ]);
    let input = RecordBatch::try_new(Arc::new(schema), vec![Arc::new(arg0)]).unwrap();

    let output = runtime.call("to_json", input).await.unwrap();
    check(
        &[output],
        expect![[r#"
        +---------------------------+
        | to_json                   |
        +---------------------------+
        | {"key":"a","value":"b"}   |
        | {"key":null,"value":null} |
        +---------------------------+"#]],
    );
}

#[tokio::test(flavor = "current_thread")]
#[cfg(feature = "with-dayjs")]
async fn test_temporal() {
    let runtime = Runtime::new();

    let schema = Schema::new(vec![
        Field::new("date", DataType::Date32, true),
        Field::new("time", DataType::Time32(TimeUnit::Millisecond), true),
    ]);

    runtime
        .add_function(
            "to_timestamp",
            DataType::Timestamp(TimeUnit::Millisecond, None),
            CallMode::ReturnNullOnNullInput,
            r#"
            export function to_timestamp(date, time) {
               return date.add(time);
            }
            "#,
        )
        .await
        .unwrap();

    let arg0 = Date32Array::from(vec![Date32Type::from_naive_date(
        NaiveDate::from_ymd_opt(2022, 4, 8).unwrap(),
    )]);
    let arg1 = Time32MillisecondArray::from(vec![time_to_time32ms(
        NaiveTime::from_hms_micro_opt(12, 34, 56, 789_012).unwrap(),
    )]);
    let input =
        RecordBatch::try_new(Arc::new(schema), vec![Arc::new(arg0), Arc::new(arg1)]).unwrap();

    let output = runtime.call("to_timestamp", input).await.unwrap();
    check(
        &[output],
        expect![[r#"
        +-------------------------+
        | to_timestamp            |
        +-------------------------+
        | 2022-04-08T12:34:56.789 |
        +-------------------------+"#]],
    );
}

#[tokio::test(flavor = "current_thread")]
#[cfg(feature = "with-dayjs")]
async fn test_interval() {
    use arrow_array::{IntervalDayTimeArray, IntervalMonthDayNanoArray, IntervalYearMonthArray};

    let runtime = Runtime::new();

    runtime
        .add_function(
            "interval_type",
            DataType::Int64,
            CallMode::ReturnNullOnNullInput,
            r#"
            export function interval_type(a) {
                return a.asMilliseconds();
            }
            "#,
        )
        .await
        .unwrap();

    /// Generate a record batch with a single column of type `List<T>`.
    fn interval_input(unit: arrow_schema::IntervalUnit) -> RecordBatch {
        let schema = Schema::new(vec![Field::new(
            "x",
            DataType::Interval(unit.clone()),
            true,
        )]);

        match unit {
            arrow_schema::IntervalUnit::YearMonth => {
                let arr = IntervalYearMonthArray::from(vec![
                    arrow_array::types::IntervalYearMonthType::make_value(1, 2),
                ]);
                RecordBatch::try_new(Arc::new(schema), vec![Arc::new(arr)]).unwrap()
            }
            arrow_schema::IntervalUnit::DayTime => {
                let arr = IntervalDayTimeArray::from(vec![
                    arrow_array::types::IntervalDayTimeType::make_value(15, 33600000),
                ]);
                RecordBatch::try_new(Arc::new(schema), vec![Arc::new(arr)]).unwrap()
            }
            arrow_schema::IntervalUnit::MonthDayNano => {
                let arr = IntervalMonthDayNanoArray::from(vec![
                    arrow_array::types::IntervalMonthDayNanoType::make_value(7, 8, 10),
                ]);
                RecordBatch::try_new(Arc::new(schema), vec![Arc::new(arr)]).unwrap()
            }
        }
    }

    let cases = [
        // (input, JS object type)
        (
            interval_input(arrow_schema::IntervalUnit::YearMonth),
            36792000000_i64,
        ),
        (
            interval_input(arrow_schema::IntervalUnit::DayTime),
            1329600000_i64,
        ),
        (
            interval_input(arrow_schema::IntervalUnit::MonthDayNano),
            18835200000_i64,
        ),
    ];

    for (input, expected) in cases.into_iter() {
        let output = runtime.call("interval_type", input).await.unwrap();
        let result = output
            .column(0)
            .as_any()
            .downcast_ref::<arrow_array::Int64Array>()
            .unwrap()
            .value(0);
        assert_eq!(result, expected);
    }
}

#[tokio::test(flavor = "current_thread")]
#[cfg(feature = "with-dayjs")]
async fn test_interval_identity() {
    use arrow_array::{IntervalDayTimeArray, IntervalMonthDayNanoArray, IntervalYearMonthArray};

    /// Generate a record batch with a single column of type `List<T>`.
    fn interval_input(unit: arrow_schema::IntervalUnit) -> RecordBatch {
        let schema = Schema::new(vec![Field::new(
            "x",
            DataType::Interval(unit.clone()),
            true,
        )]);

        match unit {
            arrow_schema::IntervalUnit::YearMonth => {
                let arr = IntervalYearMonthArray::from(vec![
                    arrow_array::types::IntervalYearMonthType::make_value(1, 2),
                ]);
                RecordBatch::try_new(Arc::new(schema), vec![Arc::new(arr)]).unwrap()
            }
            arrow_schema::IntervalUnit::DayTime => {
                let arr = IntervalDayTimeArray::from(vec![
                    arrow_array::types::IntervalDayTimeType::make_value(15, 33600000),
                ]);
                RecordBatch::try_new(Arc::new(schema), vec![Arc::new(arr)]).unwrap()
            }
            arrow_schema::IntervalUnit::MonthDayNano => {
                let arr = IntervalMonthDayNanoArray::from(vec![
                    arrow_array::types::IntervalMonthDayNanoType::make_value(7, 8, 10),
                ]);
                RecordBatch::try_new(Arc::new(schema), vec![Arc::new(arr)]).unwrap()
            }
        }
    }

    let cases = [
        // (input, JS object type)
        (
            interval_input(arrow_schema::IntervalUnit::YearMonth),
            arrow_schema::IntervalUnit::YearMonth,
        ),
        (
            interval_input(arrow_schema::IntervalUnit::DayTime),
            arrow_schema::IntervalUnit::DayTime,
        ),
        (
            interval_input(arrow_schema::IntervalUnit::MonthDayNano),
            arrow_schema::IntervalUnit::MonthDayNano,
        ),
    ];

    for (input, unit) in cases.into_iter() {
        let runtime = Runtime::new();

        runtime
            .add_function(
                "interval_type",
                DataType::Interval(unit.clone()),
                CallMode::ReturnNullOnNullInput,
                r#"
                export function interval_type(a) {
                    return a;
                }
                "#,
            )
            .await
            .unwrap();

        let output = runtime.call("interval_type", input).await.unwrap();

        match unit {
            arrow_schema::IntervalUnit::YearMonth => {
                let result = output
                    .column(0)
                    .as_any()
                    .downcast_ref::<arrow_array::IntervalYearMonthArray>()
                    .unwrap()
                    .value(0);
                assert_eq!(result, 14);
            }
            arrow_schema::IntervalUnit::DayTime => {
                let result = output
                    .column(0)
                    .as_any()
                    .downcast_ref::<arrow_array::IntervalDayTimeArray>()
                    .unwrap()
                    .value(0);
                let (day, millis) = IntervalDayTimeType::to_parts(result);
                assert_eq!(day, 15);
                assert_eq!(millis, 33600000);
            }
            arrow_schema::IntervalUnit::MonthDayNano => {
                let result = output
                    .column(0)
                    .as_any()
                    .downcast_ref::<arrow_array::IntervalMonthDayNanoArray>()
                    .unwrap()
                    .value(0);
                let (months, days, nanos) = IntervalMonthDayNanoType::to_parts(result);
                assert_eq!(months, 7);
                assert_eq!(days, 8);
                assert_eq!(nanos, 0);
            }
        }
    }
}

#[tokio::test(flavor = "current_thread")]
async fn test_range() {
    let runtime = Runtime::new();

    runtime
        .add_function(
            "range",
            DataType::Int32,
            CallMode::ReturnNullOnNullInput,
            r#"
            export function* range(n) {
                for (let i = 0; i < n; i++) {
                    yield i;
                }
            }
            "#,
        )
        .await
        .unwrap();

    let schema = Schema::new(vec![Field::new("x", DataType::Int32, true)]);
    let arg0 = Int32Array::from(vec![Some(1), None, Some(3)]);
    let input = RecordBatch::try_new(Arc::new(schema), vec![Arc::new(arg0)]).unwrap();

    let mut outputs = runtime
        .call_table_function("range", input, 2)
        .await
        .unwrap();

    assert_eq!(outputs.schema().field(0).name(), "row");
    assert_eq!(outputs.schema().field(1).name(), "range");
    assert_eq!(outputs.schema().field(1).data_type(), &DataType::Int32);

    let o1 = outputs.try_next().await.unwrap().unwrap();
    let o2 = outputs.try_next().await.unwrap().unwrap();
    assert_eq!(o1.num_rows(), 2);
    assert_eq!(o2.num_rows(), 2);
    assert!(outputs.try_next().await.unwrap().is_none());

    check(
        &[o1, o2],
        expect![[r#"
        +-----+-------+
        | row | range |
        +-----+-------+
        | 0   | 0     |
        | 2   | 0     |
        | 2   | 1     |
        | 2   | 2     |
        +-----+-------+"#]],
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn test_range_multi() {
    let runtime = Runtime::new();

    runtime
        .add_function(
            "range",
            DataType::Int32,
            CallMode::ReturnNullOnNullInput,
            r#"
            export function* range(n) {
                for (let i = 0; i < n; i++) {
                    yield i;
                }
            }
            "#,
        )
        .await
        .unwrap();

    let schema = Schema::new(vec![Field::new("x", DataType::Int32, true)]);
    let arg0 = Int32Array::from(vec![Some(1), None, Some(3)]);
    let input = RecordBatch::try_new(Arc::new(schema), vec![Arc::new(arg0)]).unwrap();

    let mut outputs = runtime
        .call_table_function("range", input, 2)
        .await
        .unwrap();

    assert_eq!(outputs.schema().field(0).name(), "row");
    assert_eq!(outputs.schema().field(1).name(), "range");
    assert_eq!(outputs.schema().field(1).data_type(), &DataType::Int32);

    let o1 = outputs.try_next().await.unwrap().unwrap();
    let o2 = outputs.try_next().await.unwrap().unwrap();
    assert_eq!(o1.num_rows(), 2);
    assert_eq!(o2.num_rows(), 2);
    assert!(outputs.try_next().await.unwrap().is_none());

    check(
        &[o1, o2],
        expect![[r#"
        +-----+-------+
        | row | range |
        +-----+-------+
        | 0   | 0     |
        | 2   | 0     |
        | 2   | 1     |
        | 2   | 2     |
        +-----+-------+"#]],
    );
}

/// assert Runtime is Send and Sync
#[test]
fn test_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<Runtime>();
}

/// Compare the actual output with the expected output.
#[track_caller]
fn check(actual: &[RecordBatch], expect: Expect) {
    expect.assert_eq(&pretty_format_batches(actual).unwrap().to_string());
}

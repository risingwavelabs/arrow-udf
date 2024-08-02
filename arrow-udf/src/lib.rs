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

#![doc = include_str!("../README.md")]

use arrow_array::RecordBatch;
pub use arrow_schema::ArrowError as Error;
pub use arrow_udf_macros::function;

/// A specialized `Result` type for Arrow UDF operations.
pub type Result<T> = std::result::Result<T, Error>;

pub mod ffi;
#[cfg(feature = "global_registry")]
pub mod sig;
pub mod types;

/// A scalar function that operates on a record batch.
pub type ScalarFunction = fn(input: &RecordBatch) -> Result<RecordBatch>;

/// A table function that operates on a record batch and returns an iterator of record batches.
pub type TableFunction =
    for<'a> fn(input: &'a RecordBatch) -> Result<Box<dyn Iterator<Item = RecordBatch> + 'a>>;

/// Internal APIs used by macros.
#[doc(hidden)]
pub mod codegen {
    pub use arrow_arith;
    pub use arrow_array;
    pub use arrow_schema;
    pub use chrono;
    pub use genawaiter;
    #[cfg(feature = "global_registry")]
    pub use linkme;
    pub use rust_decimal;
    pub use serde_json;
}

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

//! Function signature and registry.
//!
//! # Example
//!
//! ```
//! use arrow_udf::{function, sig::REGISTRY};
//! use arrow_schema::{DataType::Int32, Field};
//!
//! // define a function
//! #[function("add(int, int) -> int")]
//! fn add(lhs: i32, rhs: i32) -> i32 {
//!    lhs + rhs
//! }
//!
//! // lookup the function by name and types
//! let int32 = Field::new("", Int32, true);
//! let sig = REGISTRY.get("add", &[int32.clone(), int32.clone()], &int32).unwrap();
//! ```

use super::{ScalarFunction, TableFunction};
use arrow_schema::{Field, Fields};
use std::collections::HashMap;

/// A function signature.
pub struct FunctionSignature {
    /// The name of the function.
    pub name: String,

    /// The argument types.
    pub arg_types: Fields,

    /// Whether the function is variadic.
    pub variadic: bool,

    /// The return type.
    pub return_type: Field,

    /// The function
    pub function: FunctionKind,
}

/// Function pointer.
pub enum FunctionKind {
    Scalar(ScalarFunction),
    Table(TableFunction),
}

impl FunctionKind {
    /// Check if the function is a scalar function.
    pub fn is_scalar(&self) -> bool {
        matches!(self, Self::Scalar(_))
    }

    /// Check if the function is a table function.
    pub fn is_table(&self) -> bool {
        matches!(self, Self::Table(_))
    }

    /// Convert to a scalar function.
    pub fn as_scalar(&self) -> Option<ScalarFunction> {
        match self {
            Self::Scalar(f) => Some(*f),
            _ => None,
        }
    }

    /// Convert to a table function.
    pub fn as_table(&self) -> Option<TableFunction> {
        match self {
            Self::Table(f) => Some(*f),
            _ => None,
        }
    }
}

impl FunctionSignature {
    /// Check if the function signature matches the given argument types and return type.
    fn matches(&self, arg_types: &[Field], return_type: &Field) -> bool {
        if !(self.return_type.data_type() == return_type.data_type()
            && self.return_type.metadata() == return_type.metadata())
        {
            return false;
        }
        if arg_types.len() < self.arg_types.len() {
            return false;
        }
        for (target, ty) in self.arg_types.iter().zip(arg_types) {
            if !(target.data_type() == ty.data_type() && target.metadata() == ty.metadata()) {
                return false;
            }
        }
        if self.variadic {
            true
        } else {
            arg_types.len() == self.arg_types.len()
        }
    }
}

/// A collection of distributed `#[function]` signatures.
#[doc(hidden)]
#[linkme::distributed_slice]
pub static SIGNATURES: [fn() -> FunctionSignature];

/// Global function registry.
pub static REGISTRY: std::sync::LazyLock<FunctionRegistry> = std::sync::LazyLock::new(|| {
    let mut signatures = HashMap::<String, Vec<FunctionSignature>>::new();
    for sig in SIGNATURES {
        let sig = sig();
        signatures.entry(sig.name.clone()).or_default().push(sig);
    }
    FunctionRegistry { signatures }
});

/// Function registry.
#[derive(Default)]
pub struct FunctionRegistry {
    signatures: HashMap<String, Vec<FunctionSignature>>,
}

impl FunctionRegistry {
    /// Get the function signature by name and types.
    pub fn get(
        &self,
        name: &str,
        arg_types: &[Field],
        return_type: &Field,
    ) -> Option<&FunctionSignature> {
        let sigs = self.signatures.get(name)?;
        sigs.iter().find(|sig| sig.matches(arg_types, return_type))
    }

    /// Iterate over all function signatures.
    pub fn iter(&self) -> impl Iterator<Item = &FunctionSignature> {
        self.signatures.values().flatten()
    }
}

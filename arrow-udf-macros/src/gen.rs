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

//! Generate code for the functions.

use itertools::Itertools;
use proc_macro2::{Ident, Span};
use quote::{format_ident, quote};

use super::*;

impl FunctionAttr {
    /// Expands the wildcard in function arguments or return type.
    pub fn expand(&self) -> Vec<Self> {
        let args = self.args.iter().map(|ty| types::expand_type_wildcard(ty));
        let ret = types::expand_type_wildcard(&self.ret);
        // multi_cartesian_product should emit an empty set if the input is empty.
        let args_cartesian_product =
            args.multi_cartesian_product()
                .chain(match self.args.is_empty() {
                    true => vec![vec![]],
                    false => vec![],
                });
        let mut attrs = Vec::new();
        for (args, ret) in args_cartesian_product.cartesian_product(ret) {
            let attr = FunctionAttr {
                args: args.iter().map(|s| s.to_string()).collect(),
                ret: ret.to_string(),
                ..self.clone()
            };
            attrs.push(attr);
        }
        attrs
    }

    /// Generate a descriptor of the scalar or table function.
    ///
    /// The types of arguments and return value should not contain wildcard.
    pub fn generate_function_descriptor(&self, user_fn: &UserFunctionAttr) -> Result<TokenStream2> {
        let name = self.name.clone();
        let variadic = matches!(self.args.last(), Some(t) if t == "...");
        let args = match variadic {
            true => &self.args[..self.args.len() - 1],
            false => &self.args[..],
        }
        .iter()
        .map(|ty| field("", ty))
        .collect_vec();
        let ret = field(&self.name, &self.ret);

        let eval_name = match &self.output {
            Some(output) => format_ident!("{}", output),
            None => format_ident!("{}_eval", self.ident_name()),
        };
        let sig_name = format_ident!("{}_sig", self.ident_name());
        let ffi_name = format_ident!("{}_ffi", self.ident_name());
        let export_name = format!("arrowudf_{}", base64_encode(&self.normalize_signature()));
        let eval_function = self.generate_function(user_fn, &eval_name)?;
        let kind = match self.is_table_function {
            true => quote! { Table },
            false => quote! { Scalar },
        };
        let ffi_wrapper = match self.is_table_function {
            true => quote! { table_wrapper },
            false => quote! { scalar_wrapper },
        };

        Ok(quote! {
            #eval_function

            #[export_name = #export_name]
            unsafe extern "C" fn #ffi_name(ptr: *const u8, len: usize, out: *mut arrow_udf::ffi::CSlice) -> i32 {
                arrow_udf::ffi::#ffi_wrapper(#eval_name, ptr, len, out)
            }

            #[cfg(feature = "global_registry")]
            #[::arrow_udf::codegen::linkme::distributed_slice(::arrow_udf::sig::SIGNATURES)]
            fn #sig_name() -> ::arrow_udf::sig::FunctionSignature {
                use ::arrow_udf::sig::{FunctionSignature, FunctionKind};
                use ::arrow_udf::codegen::arrow_schema::{self, TimeUnit, IntervalUnit, Field};

                let args: Vec<Field> = vec![#(#args),*];
                FunctionSignature {
                    name: #name.into(),
                    arg_types: args.into(),
                    variadic: #variadic,
                    return_type: #ret,
                    function: FunctionKind::#kind(#eval_name),
                }
            }
        })
    }

    /// Generate a scalar or table function.
    fn generate_function(
        &self,
        user_fn: &UserFunctionAttr,
        eval_fn_name: &Ident,
    ) -> Result<TokenStream2> {
        let fn_with_visibility = if let Some(visiblity) = &self.visibility {
            // handle the scope of the visibility by parsing the visibility string
            match syn::parse_str::<syn::Visibility>(visiblity)? {
                syn::Visibility::Public(token) => quote! { #token fn },
                syn::Visibility::Restricted(vis_restricted) => quote! { #vis_restricted fn },
                syn::Visibility::Inherited => quote! { fn },
            }
        } else {
            quote! { fn }
        };

        let variadic = matches!(self.args.last(), Some(t) if t == "...");
        let num_args = self.args.len() - if variadic { 1 } else { 0 };
        let user_fn_name = format_ident!("{}", user_fn.name);

        let children_indices = (0..num_args).collect_vec();

        /// Return a list of identifiers with the given prefix and indices.
        fn idents(prefix: &str, indices: &[usize]) -> Vec<Ident> {
            indices
                .iter()
                .map(|i| format_ident!("{prefix}{i}"))
                .collect()
        }
        let inputs = idents("i", &children_indices);
        let arrays = idents("a", &children_indices);
        let arg_arrays = children_indices
            .iter()
            .map(|i| format_ident!("{}", types::array_type(&self.args[*i])));
        let ret_array_type = format_ident!("{}", types::array_type(&self.ret));
        let ret_data_type = field(&self.name, &self.ret);

        let variadic_args = variadic.then(|| quote! { variadic_row, });
        let context = user_fn.context.then(|| quote! { &self.context, });
        let writer = user_fn.write.then(|| quote! { builder, });
        let await_ = user_fn.async_.then(|| quote! { .await });
        // transform inputs for array arguments
        // e.g. for `int[]`, transform `ArrayRef` -> `&[T]`
        let transformed_inputs = inputs
            .iter()
            .zip(&self.args)
            .map(|(input, ty)| transform_input(input, ty));
        // call the user defined function
        let mut output = quote! { #user_fn_name(
            #(#transformed_inputs,)*
            #variadic_args
            #context
            #writer
        ) #await_ };
        // handle error if the function returns `Result`
        // wrap a `Some` if the function doesn't return `Option`
        output = if self.is_table_function {
            match user_fn.return_type_kind {
                ReturnTypeKind::T => quote! { Some(#output) },
                ReturnTypeKind::Option => output,
                ReturnTypeKind::Result => {
                    quote! { match #output {
                        Ok(x) => Some(x),
                        Err(e) => {
                            index_builder.append_value(i as i32);
                            builder.append_null();
                            error_builder.append_value(e.to_string());
                            None
                        }
                    } }
                }
                ReturnTypeKind::ResultOption => {
                    quote! { match #output {
                        Ok(x) => x,
                        Err(e) => {
                            index_builder.append_value(i as i32);
                            builder.append_null();
                            error_builder.append_value(e.to_string());
                            None
                        }
                    } }
                }
            }
        } else {
            match user_fn.return_type_kind {
                ReturnTypeKind::T => quote! { Some(#output) },
                ReturnTypeKind::Option => output,
                ReturnTypeKind::Result => {
                    quote! { match #output {
                        Ok(x)  => { error_builder.append_null(); Some(x) },
                        Err(e) => { error_builder.append_value(e.to_string()); None }
                    } }
                }
                ReturnTypeKind::ResultOption => {
                    quote! { match #output {
                        Ok(x)  => { error_builder.append_null(); x },
                        Err(e) => { error_builder.append_value(e.to_string()); None }
                    } }
                }
            }
        };
        // if user function accepts non-option arguments, we assume the function
        // returns null on null input, so we need to unwrap the inputs before calling.
        let some_inputs = inputs
            .iter()
            .zip(user_fn.args_option.iter())
            .map(|(input, opt)| {
                if *opt {
                    quote! { #input }
                } else {
                    quote! { Some(#input) }
                }
            });
        if !self.is_table_function && user_fn.has_error() {
            output = quote! {
                match (#(#inputs,)*) {
                    (#(#some_inputs,)*) => #output,
                    _ => { error_builder.append_null(); None },
                }
            };
        } else {
            output = quote! {
                match (#(#inputs,)*) {
                    (#(#some_inputs,)*) => #output,
                    _ => None,
                }
            };
        }

        let eval = if self.is_table_function {
            let builder = builder(&self.ret);
            let append_output = gen_append(&self.ret);
            let error_append_null = user_fn
                .has_error()
                .then(|| quote! { error_builder.append_null(); });
            let element = match user_fn.iterator_item_kind.clone().unwrap() {
                ReturnTypeKind::T => quote! {{ #error_append_null; Some(v) }},
                ReturnTypeKind::Option => quote! {{ #error_append_null; v }},
                ReturnTypeKind::Result => {
                    quote! { match v {
                        Ok(x) => { error_builder.append_null(); Some(x) },
                        Err(e) => { error_builder.append_value(e.to_string()); None }
                    } }
                }
                ReturnTypeKind::ResultOption => {
                    quote! { match v {
                        Ok(x) => { error_builder.append_null(); x },
                        Err(e) => { error_builder.append_value(e.to_string()); None }
                    } }
                }
            };

            let error_field = user_fn.has_error().then(|| {
                quote! { Field::new("error", DataType::Utf8, true), }
            });
            let let_error_builder = user_fn.has_error().then(|| {
                quote! { let mut error_builder = StringBuilder::with_capacity(input.num_rows(), input.num_rows() * 16); }
            });
            let error_array = user_fn.has_error().then(|| {
                quote! { Arc::new(error_builder.finish()) }
            });
            let yield_batch = quote! {
                let index_array = Arc::new(index_builder.finish());
                let value_array = Arc::new(builder.finish());
                yield_!(RecordBatch::try_new(SCHEMA.clone(), vec![index_array, value_array, #error_array]).unwrap());
            };
            quote! {{
                static SCHEMA: std::sync::LazyLock<SchemaRef> = std::sync::LazyLock::new(|| {
                    Arc::new(Schema::new(vec![
                        Field::new("row", DataType::Int32, true),
                        #ret_data_type,
                        #error_field
                    ]))
                });
                let mut index_builder = Int32Builder::with_capacity(input.num_rows());
                let mut builder = #builder;
                let builder = &mut builder;
                #let_error_builder
                for i in 0..input.num_rows() {
                    #(let #inputs = unsafe { (!#arrays.is_null(i)).then(|| #arrays.value_unchecked(i)) };)*
                    let Some(iter) = (#output) else {
                        continue;
                    };
                    for v in iter {
                        index_builder.append_value(i as i32);
                        let v = #element;
                        #append_output
                        if index_builder.len() == BATCH_SIZE {
                            #yield_batch
                        }
                    }
                }
                if index_builder.len() > 0 {
                    #yield_batch
                }
            }}
        } else if let Some(batch_fn) = &self.batch_fn {
            if variadic {
                return Err(Error::new(
                    Span::call_site(),
                    "customized batch function is not supported for variadic functions",
                ));
            }
            // user defined batch function
            let fn_name = format_ident!("{}", batch_fn);
            quote! {
                let c = #fn_name(#(#arrays),*);
                let array = Arc::new(c);
            }
        } else if types::is_primitive(&self.ret)
            && self.args.iter().all(|ty| types::is_primitive(ty))
            && self.args.len() <= 2
            && user_fn.is_pure()
            && !variadic
        {
            // SIMD optimization for primitive types
            match self.args.len() {
                0 => quote! {
                    let c = #ret_array_type::from_iter_values(
                        std::iter::repeat_with(|| #user_fn_name()).take(input.num_rows())
                    );
                    let array = Arc::new(c);
                },
                1 => quote! {
                    let c: #ret_array_type = arrow_arith::arity::unary(a0, #user_fn_name);
                    let array = Arc::new(c);
                },
                2 => quote! {
                    let c: #ret_array_type = arrow_arith::arity::binary(a0, a1, #user_fn_name)?;
                    let array = Arc::new(c);
                },
                n => todo!("SIMD optimization for {n} arguments"),
            }
        } else {
            // no optimization
            let builder = builder(&self.ret);
            // append the `output` to the `builder`
            let append_output = if user_fn.write {
                if !matches!(
                    self.ret.as_str(),
                    "string" | "binary" | "largestring" | "largebinary"
                ) {
                    return Err(Error::new(
                        Span::call_site(),
                        "`&mut Write` can only be used for functions that return `string`, `binary`, `largestring`, or `largebinary`",
                    ));
                }
                quote! {{
                    if #output.is_some() {
                        builder.append_value("");
                    } else {
                        builder.append_null();
                    }
                }}
            } else {
                let append = gen_append(&self.ret);
                quote! {{
                    let v = #output;
                    #append
                }}
            };
            quote! {
                let mut builder = #builder;
                let builder = &mut builder;
                for i in 0..input.num_rows() {
                    #(let #inputs = unsafe { (!#arrays.is_null(i)).then(|| #arrays.value_unchecked(i)) };)*
                    #append_output
                }
                let array = Arc::new(builder.finish());
            }
        };

        let eval_and_return = if self.is_table_function {
            quote! {
                #eval
            }
        } else {
            let error_field = user_fn.has_error().then(|| {
                quote! { Field::new("error", DataType::Utf8, true), }
            });
            let let_error_builder = user_fn.has_error().then(|| {
                quote! { let mut error_builder = StringBuilder::with_capacity(input.num_rows(), input.num_rows() * 16); }
            });
            let error_array = user_fn.has_error().then(|| {
                quote! { Arc::new(error_builder.finish()) }
            });
            quote! {
                #let_error_builder
                #eval

                static SCHEMA: std::sync::LazyLock<SchemaRef> = std::sync::LazyLock::new(|| {
                    Arc::new(Schema::new(vec![#ret_data_type, #error_field]))
                });
                Ok(RecordBatch::try_new(SCHEMA.clone(), vec![array, #error_array]).unwrap())
            }
        };

        // downcast input arrays
        let downcast_arrays = quote! {
            #(
                let #arrays: &#arg_arrays = input.column(#children_indices).as_any().downcast_ref()
                    .ok_or_else(|| ::arrow_udf::codegen::arrow_schema::ArrowError::CastError(
                        format!("expect {} for the {}-th argument", stringify!(#arg_arrays), #children_indices)
                    ))?;
            )*
        };

        // the function body
        let body = quote! {
            use ::std::sync::Arc;
            use ::arrow_udf::{Result, Error};
            use ::arrow_udf::codegen::arrow_array;
            use ::arrow_udf::codegen::arrow_array::RecordBatch;
            use ::arrow_udf::codegen::arrow_array::array::*;
            use ::arrow_udf::codegen::arrow_array::builder::*;
            use ::arrow_udf::codegen::arrow_schema::{Schema, SchemaRef, Field, DataType, IntervalUnit, TimeUnit};
            use ::arrow_udf::codegen::arrow_arith;
            use ::arrow_udf::codegen::arrow_schema;
            use ::arrow_udf::codegen::chrono;
            use ::arrow_udf::codegen::rust_decimal;
            use ::arrow_udf::codegen::serde_json;

            #eval_and_return
        };

        Ok(if self.is_table_function {
            quote! {
                #fn_with_visibility #eval_fn_name<'a>(input: &'a ::arrow_udf::codegen::arrow_array::RecordBatch)
                    -> ::arrow_udf::Result<Box<dyn Iterator<Item = ::arrow_udf::codegen::arrow_array::RecordBatch> + 'a>>
                {
                    const BATCH_SIZE: usize = 1024;
                    use ::arrow_udf::codegen::genawaiter::{rc::gen, yield_};
                    use ::arrow_udf::codegen::arrow_array::array::*;
                    #downcast_arrays
                    Ok(Box::new(gen!({ #body }).into_iter()))
                }
            }
        } else {
            quote! {
                #fn_with_visibility #eval_fn_name(input: &::arrow_udf::codegen::arrow_array::RecordBatch)
                    -> ::arrow_udf::Result<::arrow_udf::codegen::arrow_array::RecordBatch>
                {
                    #downcast_arrays
                    #body
                }
            }
        })
    }
}

/// Returns a `Field` from type name.
pub fn field(name: &str, ty: &str) -> TokenStream2 {
    let data_type = if let Some(ty) = ty.strip_suffix("[]") {
        let inner = field("item", ty);
        quote! { arrow_schema::DataType::List(Arc::new(#inner)) }
    } else if let Some(s) = ty.strip_prefix("struct ") {
        let struct_type = format_ident!("{}", s);
        quote! { arrow_schema::DataType::Struct(#struct_type::fields()) }
    } else {
        let variant: TokenStream2 = types::data_type(ty).parse().unwrap();
        quote! { arrow_schema::DataType::#variant }
    };
    let with_metadata = match ty {
        "json" => {
            quote! { .with_metadata([("ARROW:extension:name".into(), "arrowudf.json".into())].into()) }
        }
        "decimal" => {
            quote! { .with_metadata([("ARROW:extension:name".into(), "arrowudf.decimal".into())].into()) }
        }
        _ => quote! {},
    };
    quote! {
        arrow_schema::Field::new(#name, #data_type, true) #with_metadata
    }
}

/// Generate a builder for the given type.
fn builder(ty: &str) -> TokenStream2 {
    match ty {
        // `NullBuilder::with_capacity` is deprecated since v52.0, use `NullBuilder::new` instead.
        "null" => quote! { NullBuilder::new() },
        "string" => quote! { StringBuilder::with_capacity(input.num_rows(), 1024) },
        "binary" => quote! { BinaryBuilder::with_capacity(input.num_rows(), 1024) },
        "largestring" => quote! { LargeStringBuilder::with_capacity(input.num_rows(), 1024) },
        "largebinary" => quote! { LargeBinaryBuilder::with_capacity(input.num_rows(), 1024) },
        "decimal" => {
            quote! { StringBuilder::with_capacity(input.num_rows(), input.num_rows() * 8) }
        }
        "json" => quote! { StringBuilder::with_capacity(input.num_rows(), input.num_rows() * 8) },
        s if s.ends_with("[]") => {
            let values_builder = builder(ty.strip_suffix("[]").unwrap());
            quote! { ListBuilder::<Box<dyn ArrayBuilder>>::with_capacity(Box::new(#values_builder), input.num_rows()) }
        }
        s if s.starts_with("struct ") => {
            let struct_ident = format_ident!("{}", &s[7..]);
            quote! { StructBuilder::from_fields(#struct_ident::fields(), input.num_rows()) }
        }
        _ => {
            let builder_type = format_ident!("{}", types::array_builder_type(ty));
            quote! { #builder_type::with_capacity(input.num_rows()) }
        }
    }
}

/// Return the builder type for the given type.
///
/// This should be consistent with `StructBuilder::from_fields`.
pub fn builder_type(ty: &str) -> TokenStream2 {
    if ty.ends_with("[]") {
        quote! { ListBuilder::<Box<dyn ArrayBuilder>> }
    } else {
        types::array_builder_type(ty).parse().unwrap()
    }
}

/// Generate code to append the `v: Option<T>` to the `builder`.
fn gen_append(ty: &str) -> TokenStream2 {
    let append_value = gen_append_value(ty);
    let append_null = gen_append_null(ty);
    quote! {
        match v {
            Some(v) => #append_value,
            None => #append_null,
        }
    }
}

/// Generate code to append the `v: T` to the `builder: &mut Builder`.
pub fn gen_append_value(ty: &str) -> TokenStream2 {
    if let Some(inner_ty) = ty.strip_suffix("[]") {
        let value_builder_type = builder_type(inner_ty);
        quote! {{
            // builder.values() is Box<dyn ArrayBuilder>
            let value_builder = builder.values().as_any_mut().downcast_mut::<#value_builder_type>().expect("downcast list value builder");
            value_builder.extend(v.into_iter().map(Some));
            builder.append(true);
        }}
    } else if ty.starts_with("struct ") {
        quote! {{
            v.append_to(builder);
        }}
    } else if ty == "json" {
        quote! {{
            // builder: StringBuilder
            use std::fmt::Write;
            write!(builder, "{}", v).expect("write json");
            builder.append_value("");
        }}
    } else if ty == "decimal" {
        quote! { builder.append_value(v.to_string()) }
    } else if ty == "date32" {
        quote! { builder.append_value(arrow_array::types::Date32Type::from_naive_date(v)) }
    } else if ty == "time64" {
        quote! { builder.append_value(arrow_array::temporal_conversions::time_to_time64us(v)) }
    } else if ty == "timestamp" {
        quote! { builder.append_value(v.and_utc().timestamp_micros()) }
    } else if ty == "interval" {
        quote! { builder.append_value({
            let v: arrow_udf::types::Interval = v.into();
            arrow_array::types::IntervalMonthDayNanoType::make_value(v.months, v.days, v.nanos)
        }) }
    } else if ty == "null" {
        quote! { builder.append_empty_value() }
    } else {
        quote! { builder.append_value(v) }
    }
}

/// Generate code to append null to the `builder: &mut Builder`.
pub fn gen_append_null(ty: &str) -> TokenStream2 {
    if let Some(s) = ty.strip_prefix("struct ") {
        let struct_type = format_ident!("{}", s);
        quote! { #struct_type::append_null(builder) }
    } else {
        quote! { builder.append_null() }
    }
}

/// Generate code to transform the input from the type got from arrow array to the type in the user function.
///
/// | Data Type       | Arrow Value Type | User Function Type               |
/// | --------------- | ---------------- | -------------------------------- |
/// | `date32`        | `i32`            | `chrono::NaiveDate`              |
/// | `time64`        | `i64`            | `chrono::NaiveTime`              |
/// | `timestamp`     | `i64`            | `chrono::NaiveDateTime`          |
/// | `interval`      | `i128`           | `arrow_udf::types::Interval`     |
/// | `decimal`       | `&str`           | `rust_decimal::Decimal`          |
/// | `json`          | `&str`           | `serde_json::Value`              |
/// | `int8[]`        | `ArrayRef`       | `&[i8]`                          |
/// | `int16[]`       | `ArrayRef`       | `&[i16]`                         |
/// | `int32[]`       | `ArrayRef`       | `&[i32]`                         |
/// | `int64[]`       | `ArrayRef`       | `&[i64]`                         |
/// | `uint8[]`       | `ArrayRef`       | `&[u8]`                          |
/// | `uint16[]`      | `ArrayRef`       | `&[u16]`                         |
/// | `uint32[]`      | `ArrayRef`       | `&[u32]`                         |
/// | `uint64[]`      | `ArrayRef`       | `&[u64]`                         |
/// | `float32[]`     | `ArrayRef`       | `&[f32]`                         |
/// | `float64[]`     | `ArrayRef`       | `&[f64]`                         |
/// | `string[]`      | `ArrayRef`       | `arrow::array::StringArray`      |
/// | `binary[]`      | `ArrayRef`       | `arrow::array::BinaryArray`      |
/// | `largestring[]` | `ArrayRef`       | `arrow::array::LargeStringArray` |
/// | `largebinary[]` | `ArrayRef`       | `arrow::array::LargeBinaryArray` |
fn transform_input(input: &Ident, ty: &str) -> TokenStream2 {
    if ty == "decimal" {
        return quote! { #input.parse::<rust_decimal::Decimal>().expect("invalid decimal") };
    } else if ty == "date32" {
        return quote! { arrow_array::types::Date32Type::to_naive_date(#input) };
    } else if ty == "time64" {
        return quote! { arrow_array::temporal_conversions::as_time::<arrow_array::types::Time64MicrosecondType>(#input).expect("invalid time") };
    } else if ty == "timestamp" {
        return quote! { arrow_array::temporal_conversions::as_datetime::<arrow_array::types::TimestampMicrosecondType>(#input).expect("invalid timestamp") };
    } else if ty == "interval" {
        return quote! {{
            let (months, days, nanos) = arrow_array::types::IntervalMonthDayNanoType::to_parts(#input);
            arrow_udf::types::Interval { months, days, nanos }
        }};
    } else if ty == "json" {
        return quote! { #input.parse::<serde_json::Value>().expect("invalid json") };
    } else if let Some(elem_type) = ty.strip_suffix("[]") {
        if types::is_primitive(elem_type) {
            let array_type = format_ident!("{}", types::array_type(elem_type));
            return quote! {{
                let primitive_array: &#array_type = #input.as_primitive();
                primitive_array.values().as_ref()
            }};
        } else if elem_type == "string" {
            return quote! {
                #input.as_any().downcast_ref::<arrow_array::StringArray>().expect("string array")
            };
        } else if elem_type == "binary" {
            return quote! {
                #input.as_any().downcast_ref::<arrow_array::BinaryArray>().expect("binary array")
            };
        } else if elem_type == "largestring" {
            return quote! {
                #input.as_any().downcast_ref::<arrow_array::LargeStringArray>().expect("large string array")
            };
        } else if elem_type == "largebinary" {
            return quote! {
                #input.as_any().downcast_ref::<arrow_array::LargeBinaryArray>().expect("large binary array")
            };
        } else {
            return quote! { #input };
        }
    }
    quote! { #input }
}

/// Encode a string to a symbol name using customized base64.
pub fn base64_encode(input: &str) -> String {
    use base64::{
        alphabet::Alphabet,
        engine::{general_purpose::NO_PAD, GeneralPurpose},
        Engine,
    };
    // standard base64 uses '+' and '/', which is not a valid symbol name.
    // we use '$' and '_' instead.
    let alphabet =
        Alphabet::new("ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789$_").unwrap();
    let engine = GeneralPurpose::new(&alphabet, NO_PAD);
    engine.encode(input)
}

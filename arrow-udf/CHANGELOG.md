# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.10.0] - 2026-09-03

### Added

- Support list types such as `varchar[]` and `int32[]` as arguments of DuckDB scalar functions, backed by the new `duckdb` module.

## [0.9.0] - 2026-04-22

### Changed

- Update `arrow` version from `56` to `58`.
- Update `duckdb` version from `=1.4` to `=1.10502`.

### Breaking Changes

- The `duckdb` feature no longer enables `loadable-extension` on `duckdb-rs`. Users building a DuckDB loadable extension should enable it explicitly on their own `duckdb` dependency.

## [0.8.0] - 2026-01-14

### Changed

- Update `duckdb` version to `1.4`.
- Update `arrow` version to `56`.
- Update to edition 2024.

## [0.7.0] - 2025-08-26

### Changed

- Update `arrow` version from `54` to `55`.
- `TableFunction` is `Send`.

## [0.6.2] - 2025-03-21

### Fixed

- Bump `arrow-udf-macros` to fix compiling error regarding to `once_cell`.

## [0.6.1] - 2025-03-19

### Changed

- Change `arrow` version from `>=53,<55` to `54`.

## [0.6.0] - 2025-02-24

### Changed

- `TableFunction` now returns a `RecordBatchReader`.

## [0.5.3] - 2025-02-13

### Changed

- Support `arrow` version 54.

### Fixed

- Fix `global_registry` feature.

## [0.5.2] - 2024-12-24

### Fixed

- Bump `arrow-udf-macros` to 0.5.0 to fix the compilation error. 0.5.1 is yanked.

## [0.5.1] - 2024-12-23

### Changed

- Now users of the `arrow-udf::function` macro don't need to add `genawaiter` as a dependency.

## [0.5.0] - 2024-10-10

### Fixed

- Upgrade `arrow-udf-macros` for the compilation fix.

### Changed

- Upgrade `arrow` version requirement to `=53`.

## [0.4.0] - 2024-09-19

### Fixed

- Fix deprecated warnings with `arrow` v52.
- Allow writer-style return value for `largestring`.
- deps: Pin all arrow version to 52 instead.

## [0.3.0] - 2024-04-25

### Added

- Add support for `int8`, `uint8`, `uint16`, `uint32`, `uint64`, `largestring`, and `largebinary` type.

### Breaking Changes

- The following types are renamed to align with Arrow:
    - `void` -> `null`
    - `int2` -> `int16`
    - `int4` -> `int32`
    - `int8` -> `int64`
    - `float4` -> `float32`
    - `float8` -> `float64`
    - `date` -> `date32`
    - `time` -> `time64`
    - `varchar` -> `string`
    - `bytea` -> `binary`

- `json` and `decimal` type are no longer mapped to `LargeString` and `LargeBinary` respectively. They are now mapped to [extension types](https://arrow.apache.org/docs/format/Columnar.html#format-metadata-extension-types) with `String` as the storage type.
    - `json`: `ARROW:extension:name` = `arrorudf.json`
    - `decimal`: `ARROW:extension:name` = `arrowudf.decimal`

## [0.2.2] - 2024-04-02

### Changed

- Update `arrow` version to >=50.

### Fixed

- Fix compile error when returning struct type in table functions.

## [0.2.1] - 2024-02-29

### Added

- Re-export `chrono`, `rust_decimal` and `serde_json` in `types` module.

## [0.2.0] - 2024-02-29

### Added

- Add `StructType` trait and derive macro to define struct types.
- Support `->>` as an alias for `-> setof` in `#[function]`.

### Changed

- **Breaking**: The inline syntax `struct<..>` in function signatures is deprecated. Use `struct StructType` instead.

## [0.1.3] - 2024-02-18

### Fixed

- Fix the bug that the `#[function]` macro does not support more than 12 arguments.

## [0.1.2] - 2024-01-31

### Added

- Support type alias `numeric` and `jsonb` in `#[function]`.
- Support `varchar[]` and `bytea[]` as arguments.

## [0.1.1] - 2024-01-17

### Fixed

- Fixed returning a list in a struct. e.g. `-> struct<a:int[]>` is now supported.
- However, the trade-off is that the performance of returning a list might decrease.

## [0.1.0] - 2024-01-13

### Added

- Initial release. Support basic scalar functions and table functions.

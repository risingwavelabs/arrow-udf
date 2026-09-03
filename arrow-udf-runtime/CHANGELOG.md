# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Breaking Changes

- JavaScript UDF: decimal values are now [big.js](https://github.com/MikeMcl/big.js) objects instead of the QuickJS `BigDecimal`. Operators no longer work on them, use methods like `add`/`sub`/`mul`/`div`/`cmp` instead.
- JavaScript UDF: `Decimal128`/`Decimal256` results are rounded half-up to the column scale instead of truncated, and rejected if they exceed the column precision.

### Changed

- Update `rquickjs` version from `0.6` to `0.12`. The JavaScript engine is now quickjs-ng.

## [0.9.0] - 2026-04-22

### Changed

- Update `arrow` version from `54` to `58`.
- Update `wasmtime` and `wasi-common` version from `27` to `36`.
- Update `tonic` version from `0.12` to `0.14`.

## [0.8.0] - 2025-04-10

### Changed

- Remove `SubInterpreter` and all related code from Python UDF runtime.

## [0.7.0] - 2025-03-22

### Changed

- Merge all runtime packages into `arrow-udf-runtime`. You can use features like `javascript` to only enable JavaScript UDF runtime specifically. By default all runtimes are included.

# Copyright 2024 RisingWave Labs
#
# Licensed under the Apache License, Version 2.0 (the "License");
# you may not use this file except in compliance with the License.
# You may obtain a copy of the License at
#
#     http://www.apache.org/licenses/LICENSE-2.0
#
# Unless required by applicable law or agreed to in writing, software
# distributed under the License is distributed on an "AS IS" BASIS,
# WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
# See the License for the specific language governing permissions and
# limitations under the License.

from typing import *
import pyarrow as pa
import pyarrow.flight
import pyarrow.parquet
import inspect
import traceback
import json
from concurrent.futures import ThreadPoolExecutor
import concurrent
from decimal import Decimal
import signal


class UserDefinedFunction:
    """
    Base interface for user-defined function.
    """

    _name: str
    _input_schema: pa.Schema
    _result_schema: pa.Schema
    _io_threads: Optional[int]
    _executor: Optional[ThreadPoolExecutor]

    def eval_batch(self, batch: pa.RecordBatch) -> Iterator[pa.RecordBatch]:
        """
        Apply the function on a batch of inputs.
        """
        return iter([])


class ScalarFunction(UserDefinedFunction):
    """
    Base interface for user-defined scalar function. A user-defined scalar functions maps zero, one,
    or multiple scalar values to a new scalar value.
    """

    def __init__(self, *args, **kwargs):
        self._io_threads = kwargs.pop("io_threads")
        self._executor = (
            ThreadPoolExecutor(max_workers=self._io_threads)
            if self._io_threads is not None
            else None
        )
        super().__init__(*args, **kwargs)

    def eval(self, *args) -> Any:
        """
        Method which defines the logic of the scalar function.
        """
        pass

    def eval_batch(self, batch: pa.RecordBatch) -> Iterator[pa.RecordBatch]:
        # parse value from json string for jsonb columns
        inputs = [[v.as_py() for v in array] for array in batch]

        if self._executor is not None:
            # evaluate the function for each row
            tasks = [
                self._executor.submit(self._func, *[col[i] for col in inputs])
                for i in range(batch.num_rows)
            ]
            column = [
                future.result() for future in concurrent.futures.as_completed(tasks)
            ]
        else:
            # evaluate the function for each row
            column = [
                self.eval(*[col[i] for col in inputs]) for i in range(batch.num_rows)
            ]

        array = _to_arrow_array(column, self._result_schema.types[0])

        yield pa.RecordBatch.from_arrays([array], schema=self._result_schema)


def _to_arrow_array(column: List, type: pa.DataType) -> pa.Array:
    """Return a function to convert a list of python objects to an arrow array."""
    if pa.types.is_list(type):
        # FIXME
        func = _to_arrow_array(type.value_type)
        return lambda array: [(func(v) if v is not None else None) for v in array]

    if pa.types.is_struct(type):
        fields = [
            _to_arrow_array([v.get(field.name) for v in column], field.type)
            for field in type
        ]
        return pa.StructArray.from_arrays(fields, fields=type)

    if type.equals(JsonType()):
        s = pa.array([json.dumps(v) for v in column], type=pa.string())
        return pa.ExtensionArray.from_storage(JsonType(), s)

    if type.equals(DecimalType()):
        s = pa.array(
            [_decimal_to_str(v) if v is not None else None for v in column],
            type=pa.string(),
        )
        return pa.ExtensionArray.from_storage(DecimalType(), s)

    return pa.array(column, type=type)


def _decimal_to_str(v: Decimal) -> str:
    if not isinstance(v, Decimal):
        raise ValueError(f"Expected Decimal, got {v}")
    # use `f` format to avoid scientific notation, e.g. `1e10`
    return format(v, "f")


class TableFunction(UserDefinedFunction):
    """
    Base interface for user-defined table function. A user-defined table functions maps zero, one,
    or multiple scalar values to a new table value.
    """

    BATCH_SIZE = 1024

    def eval(self, *args) -> Iterator:
        """
        Method which defines the logic of the table function.
        """
        yield

    def eval_batch(self, batch: pa.RecordBatch) -> Iterator[pa.RecordBatch]:
        class RecordBatchBuilder:
            """A utility class for constructing Arrow RecordBatch by row."""

            schema: pa.Schema
            columns: List[List]

            def __init__(self, schema: pa.Schema):
                self.schema = schema
                self.columns = [[] for _ in self.schema.types]

            def len(self) -> int:
                """Returns the number of rows in the RecordBatch being built."""
                return len(self.columns[0])

            def append(self, index: int, value: Any):
                """Appends a new row to the RecordBatch being built."""
                self.columns[0].append(index)
                self.columns[1].append(value)

            def build(self) -> pa.RecordBatch:
                """Builds the RecordBatch from the accumulated data and clears the state."""
                # Convert the columns to arrow arrays
                arrays = [
                    pa.array(col, type)
                    for col, type in zip(self.columns, self.schema.types)
                ]
                # Reset columns
                self.columns = [[] for _ in self.schema.types]
                return pa.RecordBatch.from_arrays(arrays, schema=self.schema)

        builder = RecordBatchBuilder(self._result_schema)

        # Iterate through rows in the input RecordBatch
        for row_index in range(batch.num_rows):
            row = tuple(column[row_index].as_py() for column in batch)
            for result in self.eval(*row):
                builder.append(row_index, result)
                if builder.len() == self.BATCH_SIZE:
                    yield builder.build()
        if builder.len() != 0:
            yield builder.build()


class UserDefinedScalarFunctionWrapper(ScalarFunction):
    """
    Base Wrapper for Python user-defined scalar function.
    """

    _func: Callable

    def __init__(self, func, input_types, result_type, name=None, io_threads=None):
        self._func = func
        self._input_schema = pa.schema(
            zip(
                inspect.getfullargspec(func)[0],
                [_to_data_type(t) for t in _to_list(input_types)],
            )
        )
        self._result_schema = pa.schema([("output", _to_data_type(result_type))])
        self._name = name or (
            func.__name__ if hasattr(func, "__name__") else func.__class__.__name__
        )
        super().__init__(io_threads=io_threads)

    def __call__(self, *args):
        return self._func(*args)

    def eval(self, *args):
        return self._func(*args)


class UserDefinedTableFunctionWrapper(TableFunction):
    """
    Base Wrapper for Python user-defined table function.
    """

    _func: Callable

    def __init__(self, func, input_types, result_types, name=None):
        self._func = func
        self._name = name or (
            func.__name__ if hasattr(func, "__name__") else func.__class__.__name__
        )
        self._input_schema = pa.schema(
            zip(
                inspect.getfullargspec(func)[0],
                [_to_data_type(t) for t in _to_list(input_types)],
            )
        )
        self._result_schema = pa.schema(
            [
                ("row_index", pa.int32()),
                (
                    self._name,
                    (
                        pa.struct([("", _to_data_type(t)) for t in result_types])
                        if isinstance(result_types, list)
                        else _to_data_type(result_types)
                    ),
                ),
            ]
        )

    def __call__(self, *args):
        return self._func(*args)

    def eval(self, *args):
        return self._func(*args)


def _to_list(x):
    if isinstance(x, list):
        return x
    else:
        return [x]


def udf(
    input_types: Union[List[Union[str, pa.DataType]], Union[str, pa.DataType]],
    result_type: Union[str, pa.DataType],
    name: Optional[str] = None,
    io_threads: Optional[int] = None,
) -> Callable:
    """
    Annotation for creating a user-defined scalar function.

    Parameters:
    - input_types: A list of strings or Arrow data types that specifies the input data types.
    - result_type: A string or an Arrow data type that specifies the return value type.
    - name: An optional string specifying the function name. If not provided, the original name will be used.
    - io_threads: Number of I/O threads used per data chunk for I/O bound functions.

    Example:
    ```
    @udf(input_types=['INT', 'INT'], result_type='INT')
    def gcd(x, y):
        while y != 0:
            (x, y) = (y, x % y)
        return x
    ```

    I/O bound Example:
    ```
    @udf(input_types=['INT'], result_type='INT', io_threads=64)
    def external_api(x):
        response = requests.get(my_endpoint + '?param=' + x)
        return response["data"]
    ```
    """

    if io_threads is not None and io_threads > 1:
        return lambda f: UserDefinedScalarFunctionWrapper(
            f, input_types, result_type, name, io_threads=io_threads
        )
    else:
        return lambda f: UserDefinedScalarFunctionWrapper(
            f, input_types, result_type, name
        )


def udtf(
    input_types: Union[List[Union[str, pa.DataType]], Union[str, pa.DataType]],
    result_types: Union[List[Union[str, pa.DataType]], Union[str, pa.DataType]],
    name: Optional[str] = None,
) -> Callable:
    """
    Annotation for creating a user-defined table function.

    Parameters:
    - input_types: A list of strings or Arrow data types that specifies the input data types.
    - result_types A list of strings or Arrow data types that specifies the return value types.
    - name: An optional string specifying the function name. If not provided, the original name will be used.

    Example:
    ```
    @udtf(input_types='INT', result_types='INT')
    def series(n):
        for i in range(n):
            yield i
    ```
    """

    return lambda f: UserDefinedTableFunctionWrapper(f, input_types, result_types, name)


class UdfServer(pa.flight.FlightServerBase):
    """
    A server that provides user-defined functions to clients.

    Example:
    ```
    server = UdfServer(location="0.0.0.0:8815")
    server.add_function(my_udf)
    server.serve()
    ```
    """

    # UDF server based on Apache Arrow Flight protocol.
    # Reference: https://arrow.apache.org/cookbook/py/flight.html#simple-parquet-storage-service-with-arrow-flight

    _location: str
    _functions: Dict[str, UserDefinedFunction]

    def __init__(self, location="0.0.0.0:8815", **kwargs):
        super(UdfServer, self).__init__("grpc://" + location, **kwargs)
        self._location = location
        self._functions = {}

    def get_flight_info(self, context, descriptor):
        """Return the result schema of a function."""
        udf = self._functions[descriptor.path[0].decode("utf-8")]
        # return the concatenation of input and output schema
        full_schema = pa.schema(list(udf._input_schema) + list(udf._result_schema))
        # we use `total_records` to indicate the number of input arguments
        return pa.flight.FlightInfo(
            schema=full_schema,
            descriptor=descriptor,
            endpoints=[],
            total_records=len(udf._input_schema),
            total_bytes=0,
        )

    def add_function(self, udf: UserDefinedFunction):
        """Add a function to the server."""
        name = udf._name
        if name in self._functions:
            raise ValueError("Function already exists: " + name)

        input_types = ",".join(
            [_data_type_to_string(t) for t in udf._input_schema.types]
        )
        if isinstance(udf, TableFunction):
            output_type = udf._result_schema.types[-1]
            if isinstance(output_type, pa.StructType):
                output_type = ",".join(
                    f"{field.name} {_data_type_to_string(field.type)}"
                    for i, field in enumerate(output_type)
                )
                output_type = f"TABLE({output_type})"
            else:
                output_type = _data_type_to_string(output_type)
                output_type = f"TABLE(output {output_type})"
        else:
            output_type = _data_type_to_string(udf._result_schema.types[-1])

        sql = f"CREATE FUNCTION {name}({input_types}) RETURNS {output_type} AS '{name}' USING LINK 'http://{self._location}';"
        print(f"added function: {name}, corresponding SQL:\n{sql}\n")
        self._functions[name] = udf

    def do_exchange(self, context, descriptor, reader, writer):
        """Call a function from the client."""
        udf = self._functions[descriptor.path[0].decode("utf-8")]
        writer.begin(udf._result_schema)
        try:
            for batch in reader:
                # print(pa.Table.from_batches([batch.data]))
                for output_batch in udf.eval_batch(batch.data):
                    writer.write_batch(output_batch)
        except Exception as e:
            print(traceback.print_exc())
            raise e

    def serve(self):
        """
        Block until the server shuts down.

        This method only returns if shutdown() is called or a signal (SIGINT, SIGTERM) received.
        """
        print(
            "Note: You can use arbitrary function names and struct field names in CREATE FUNCTION statements."
            f"\n\nlistening on {self._location}"
        )
        signal.signal(signal.SIGTERM, lambda s, f: self.shutdown())
        super(UdfServer, self).serve()


class JsonScalar(pa.ExtensionScalar):
    def as_py(self):
        return json.loads(self.value.as_py())


class JsonType(pa.ExtensionType):
    def __init__(self):
        super().__init__(pa.string(), "arrowudf.json")

    def __arrow_ext_serialize__(self):
        # since we don't have a parameterized type, we don't need extra
        # metadata to be deserialized
        return b""

    @classmethod
    def __arrow_ext_deserialize__(self, storage_type, serialized):
        # return an instance of this subclass given the serialized
        # metadata.
        return JsonType()

    def __arrow_ext_scalar_class__(self):
        return JsonScalar


class DecimalScalar(pa.ExtensionScalar):
    def as_py(self):
        return Decimal(self.value.as_py())


class DecimalType(pa.ExtensionType):
    def __init__(self):
        super().__init__(pa.string(), "arrowudf.decimal")

    def __arrow_ext_serialize__(self):
        # since we don't have a parameterized type, we don't need extra
        # metadata to be deserialized
        return b""

    @classmethod
    def __arrow_ext_deserialize__(self, storage_type, serialized):
        # return an instance of this subclass given the serialized
        # metadata.
        return DecimalType()

    def __arrow_ext_scalar_class__(self):
        return DecimalScalar


pa.register_extension_type(JsonType())
pa.register_extension_type(DecimalType())


def _to_data_type(t: Union[str, pa.DataType]) -> pa.DataType:
    """
    Convert a SQL data type string or `pyarrow.DataType` to `pyarrow.DataType`.
    """
    if isinstance(t, str):
        return _string_to_data_type(t)
    else:
        return t


def _string_to_data_type(type: str):
    """
    Convert a SQL data type string to `pyarrow.DataType`.
    """
    t = type.upper()
    if t.endswith("[]"):
        return pa.list_(_string_to_data_type(type[:-2]))
    elif t.startswith("STRUCT"):
        # extract 'STRUCT<a:INT, b:VARCHAR, c:STRUCT<INT>, ...>'
        type_list = type[7:-1]  # strip "STRUCT<>"
        fields = []
        start = 0
        depth = 0
        for i, c in enumerate(type_list):
            if c == "<":
                depth += 1
            elif c == ">":
                depth -= 1
            elif c == "," and depth == 0:
                name, t = type_list[start:i].split(":")
                name = name.strip()
                t = t.strip()
                fields.append(pa.field(name, _string_to_data_type(t)))
                start = i + 1
        if ":" in type_list[start:].strip():
            name, t = type_list[start:].split(":")
            name = name.strip()
            t = t.strip()
            fields.append(pa.field(name, _string_to_data_type(t)))
        return pa.struct(fields)
    elif t in ("BOOLEAN", "BOOL"):
        return pa.bool_()
    elif t in ("TINYINT", "INT8"):
        return pa.int8()
    elif t in ("SMALLINT", "INT16"):
        return pa.int16()
    elif t in ("INT", "INTEGER", "INT32"):
        return pa.int32()
    elif t in ("BIGINT", "INT64"):
        return pa.int64()
    elif t in ("UINT8"):
        return pa.uint8()
    elif t in ("UINT16"):
        return pa.uint16()
    elif t in ("UINT32"):
        return pa.uint32()
    elif t in ("UINT64"):
        return pa.uint64()
    elif t in ("FLOAT32", "REAL"):
        return pa.float32()
    elif t in ("FLOAT64", "DOUBLE PRECISION"):
        return pa.float64()
    elif t.startswith("DECIMAL") or t.startswith("NUMERIC"):
        if t == "DECIMAL" or t == "NUMERIC":
            return DecimalType()
        rest = t[8:-1]  # remove "DECIMAL(" and ")"
        if "," in rest:
            precision, scale = rest.split(",")
            return pa.decimal128(int(precision), int(scale))
        else:
            return pa.decimal128(int(rest), 0)
    elif t in ("DATE32", "DATE"):
        return pa.date32()
    elif t in ("TIME64", "TIME", "TIME WITHOUT TIME ZONE"):
        return pa.time64("us")
    elif t in ("TIMESTAMP", "TIMESTAMP WITHOUT TIME ZONE"):
        return pa.timestamp("us")
    elif t.startswith("INTERVAL"):
        return pa.month_day_nano_interval()
    elif t in ("STRING", "VARCHAR"):
        return pa.string()
    elif t in ("LARGE_STRING"):
        return pa.large_string()
    elif t in ("JSON", "JSONB"):
        return JsonType()
    elif t in ("BINARY", "BYTEA"):
        return pa.binary()
    elif t in ("LARGE_BINARY"):
        return pa.large_binary()

    raise ValueError(f"Unsupported type: {t}")


def _data_type_to_string(t: pa.DataType) -> str:
    """
    Convert a `pyarrow.DataType` to a SQL data type string.
    """
    if isinstance(t, pa.ListType):
        return _data_type_to_string(t.value_type) + "[]"
    elif t.equals(pa.bool_()):
        return "BOOLEAN"
    elif t.equals(pa.int8()):
        return "TINYINT"
    elif t.equals(pa.int16()):
        return "SMALLINT"
    elif t.equals(pa.int32()):
        return "INT"
    elif t.equals(pa.int64()):
        return "BIGINT"
    elif t.equals(pa.uint8()):
        return "uint8"
    elif t.equals(pa.uint16()):
        return "uint16"
    elif t.equals(pa.uint32()):
        return "uint32"
    elif t.equals(pa.uint64()):
        return "uint64"
    elif t.equals(pa.float32()):
        return "FLOAT4"
    elif t.equals(pa.float64()):
        return "FLOAT8"
    elif t.equals(DecimalType()):
        return "DECIMAL"
    elif pa.types.is_decimal(t):
        return f"DECIMAL({t.precision},{t.scale})"
    elif t.equals(pa.date32()):
        return "DATE"
    elif t.equals(pa.time64("us")):
        return "TIME"
    elif t.equals(pa.timestamp("us")):
        return "TIMESTAMP"
    elif t.equals(pa.month_day_nano_interval()):
        return "INTERVAL"
    elif t.equals(pa.string()):
        return "VARCHAR"
    elif t.equals(pa.binary()):
        return "BYTEA"
    elif t.equals(pa.large_string()):
        return "large_string"
    elif t.equals(pa.large_binary()):
        return "large_binary"
    elif t.equals(JsonType()):
        return "JSON"
    elif isinstance(t, pa.StructType):
        return (
            "STRUCT<"
            + ",".join(
                f"{field.name}: {_data_type_to_string(field.type)}" for field in t
            )
            + ">"
        )
    else:
        raise ValueError(f"Unsupported type: {t}")
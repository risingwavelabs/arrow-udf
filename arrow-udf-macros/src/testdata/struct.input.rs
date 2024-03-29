#[derive(StructType)]
struct Data {
    a: (),
    b: bool,
    c: i16,
    d: i32,
    e: i64,
    f: f32,
    g: f64,
    h: Decimal,
    i: NaiveDate,
    j: NaiveTime,
    k: NaiveDateTime,
    l: Interval,
    m: serde_json::Value,
    n: String,
    o: Vec<u8>,
    p: Vec<String>,
    q: KeyValue<'static>,
}

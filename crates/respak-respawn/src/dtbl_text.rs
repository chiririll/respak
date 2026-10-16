//! `dtbl -> dtbl.csv` and `dtbl -> dtbl.json`: a datatable as text.
//!
//! The CSV is rsx's (`datatable.cpp`, the repak input format), byte for byte: a line of quoted
//! column names, one line per row, then a line of quoted type names with no newline after it.
//! Bools are `true` / `false`, floats are printed like C++ `ostream` (6 significant digits, as
//! `%g`), vectors are `"<x,y,z>"`, strings and asset paths are quoted without escaping.
//!
//! The JSON is respak's own: `{"columns": [{"name", "type"}], "rows": [[cell, ...]]}`, one row
//! per line; floats round-trip exactly, a vector is `[x, y, z]`, a non-finite float is `null`.

use std::fmt::Write as _;
use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use respak_core::capability::{Cell, Table};
use respak_core::value::BlobData;
use respak_core::{
    ConvertHandler, Cx, Error, ErrorKind, FormatId, Result, Target, TypedTarget, Value, ValueKey,
};
use serde::ser::SerializeTuple;
use serde::{Deserialize, Serialize, Serializer};

/// The CSV and JSON targets take no options.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct NoOptions {}

fn table_of<'a>(source: &'a Value, target: &str) -> Result<&'a dyn Table> {
    source.as_table().ok_or_else(|| {
        Error::new(ErrorKind::Custom {
            source: None,
            message: format!("{target} must be made from a table"),
        })
    })
}

fn blob(source: &Value, target: &Target, format: &'static str, bytes: Vec<u8>) -> Result<Value> {
    let options = TypedTarget::<NoOptions>::parse(target)?;
    let key = ValueKey::converted(
        Arc::new(source.key().clone()),
        FormatId::new(format),
        options.normalized(),
    );
    Ok(Arc::new(BlobData::with_format(
        FormatId::new(format),
        Bytes::from(bytes),
        key,
    )))
}

/// A float as C++ `ostream << float` prints it by default (`%g`, precision 6).
pub fn format_g(v: f32) -> String {
    let v = f64::from(v);
    if v.is_nan() {
        return if v.is_sign_negative() {
            "-nan".into()
        } else {
            "nan".into()
        };
    }
    if v.is_infinite() {
        return if v > 0.0 { "inf".into() } else { "-inf".into() };
    }
    if v == 0.0 {
        return if v.is_sign_negative() {
            "-0".into()
        } else {
            "0".into()
        };
    }
    fn trim(s: &str) -> &str {
        if s.contains('.') {
            s.trim_end_matches('0').trim_end_matches('.')
        } else {
            s
        }
    }
    // The exponent after rounding to 6 significant digits decides the style, as in C.
    let sci = format!("{v:.5e}");
    let (mantissa, exp) = sci.split_once('e').expect("exponent");
    let exp: i32 = exp.parse().expect("integer exponent");
    if !(-4..6).contains(&exp) {
        format!(
            "{}e{}{:02}",
            trim(mantissa),
            if exp < 0 { '-' } else { '+' },
            exp.abs()
        )
    } else {
        trim(&format!("{v:.*}", (5 - exp) as usize)).to_owned()
    }
}

/// The table as rsx's CSV.
pub fn to_csv(table: &dyn Table) -> String {
    let mut out = String::new();
    let quoted = |items: &mut dyn Iterator<Item = &str>| {
        items
            .map(|s| format!("\"{s}\""))
            .collect::<Vec<_>>()
            .join(",")
    };
    out += &quoted(&mut table.columns().iter().map(|c| c.name.as_str()));
    out.push('\n');
    for row in table.rows() {
        for (i, cell) in row.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            match cell {
                Cell::Bool(b) => out += if *b { "true" } else { "false" },
                Cell::Int(n) => write!(out, "{n}").expect("write to String"),
                Cell::Float(f) => out += &format_g(*f),
                Cell::Vector([x, y, z]) => write!(
                    out,
                    "\"<{},{},{}>\"",
                    format_g(*x),
                    format_g(*y),
                    format_g(*z)
                )
                .expect("write to String"),
                Cell::String(s) => write!(out, "\"{s}\"").expect("write to String"),
            }
        }
        out.push('\n');
    }
    out += &quoted(&mut table.columns().iter().map(|c| c.ty.name()));
    out
}

/// A cell as JSON; floats keep their `f32` precision.
struct JsonCell<'a>(&'a Cell);

impl Serialize for JsonCell<'_> {
    fn serialize<S: Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        match self.0 {
            Cell::Bool(b) => s.serialize_bool(*b),
            Cell::Int(n) => s.serialize_i32(*n),
            Cell::Float(f) => s.serialize_f32(*f),
            Cell::Vector(v) => {
                let mut t = s.serialize_tuple(3)?;
                for f in v {
                    t.serialize_element(f)?;
                }
                t.end()
            }
            Cell::String(text) => s.serialize_str(text),
        }
    }
}

/// The table as respak's JSON.
pub fn to_json(table: &dyn Table) -> Result<String> {
    let mut out = String::from("{\n  \"columns\": [");
    for (i, c) in table.columns().iter().enumerate() {
        out += if i > 0 { ",\n    " } else { "\n    " };
        write!(
            out,
            "{{\"name\": {}, \"type\": \"{}\"}}",
            compact(&c.name)?,
            c.ty.name()
        )
        .expect("write to String");
    }
    out += if table.columns().is_empty() {
        "],\n  \"rows\": ["
    } else {
        "\n  ],\n  \"rows\": ["
    };
    for (i, row) in table.rows().iter().enumerate() {
        out += if i > 0 { ",\n    " } else { "\n    " };
        out.push('[');
        for (j, cell) in row.iter().enumerate() {
            if j > 0 {
                out += ", ";
            }
            out += &compact(&JsonCell(cell))?;
        }
        out.push(']');
    }
    out += if table.rows().is_empty() {
        "]\n}\n"
    } else {
        "\n  ]\n}\n"
    };
    Ok(out)
}

/// One value as compact JSON.
fn compact<T: Serialize + ?Sized>(v: &T) -> Result<String> {
    serde_json::to_string(v).map_err(|e| Error::custom("dtbl.json", e))
}

/// Convert edge `dtbl -> dtbl.csv`.
pub struct DtblCsvEncode;

#[async_trait]
impl ConvertHandler for DtblCsvEncode {
    async fn convert(&self, source: Value, target: &Target, _cx: &Cx) -> Result<Value> {
        let csv = to_csv(table_of(&source, "dtbl.csv")?);
        blob(&source, target, "dtbl.csv", csv.into_bytes())
    }
}

/// Convert edge `dtbl -> dtbl.json`.
pub struct DtblJsonEncode;

#[async_trait]
impl ConvertHandler for DtblJsonEncode {
    async fn convert(&self, source: Value, target: &Target, _cx: &Cx) -> Result<Value> {
        let json = to_json(table_of(&source, "dtbl.json")?)?;
        blob(&source, target, "dtbl.json", json.into_bytes())
    }
}

#[cfg(test)]
mod tests {
    use respak_core::capability::{Column, ColumnType};

    use super::*;

    struct T(Vec<Column>, Vec<Vec<Cell>>);

    impl Table for T {
        fn columns(&self) -> &[Column] {
            &self.0
        }
        fn rows(&self) -> &[Vec<Cell>] {
            &self.1
        }
    }

    fn table() -> T {
        let col = |name: &str, ty| Column {
            name: name.into(),
            ty,
        };
        T(
            vec![
                col("b", ColumnType::Bool),
                col("i", ColumnType::Int),
                col("f", ColumnType::Float),
                col("v", ColumnType::Vector),
                col("s", ColumnType::String),
                col("a", ColumnType::Asset),
                col("n", ColumnType::AssetNoPrecache),
            ],
            vec![
                vec![
                    Cell::Bool(true),
                    Cell::Int(-7),
                    Cell::Float(0.1),
                    Cell::Vector([1.0, -2.5, 1e7]),
                    Cell::String("a \"q\"".into()),
                    Cell::String("mdl/x.rmdl".into()),
                    Cell::String(String::new()),
                ],
                vec![
                    Cell::Bool(false),
                    Cell::Int(0),
                    Cell::Float(123456.7),
                    Cell::Vector([0.0, 0.0001, 0.00001]),
                    Cell::String("!!DATA EXCLUDED!!".into()),
                    Cell::String("x".into()),
                    Cell::String("y".into()),
                ],
            ],
        )
    }

    #[test]
    fn floats_print_like_cpp_ostream() {
        let cases: [(f32, &str); 14] = [
            (0.0, "0"),
            (-0.0, "-0"),
            (1.0, "1"),
            (0.1, "0.1"),
            (-2.5, "-2.5"),
            (100.0, "100"),
            (123456.0, "123456"),
            (123456.7, "123457"),
            (1234567.0, "1.23457e+06"),
            (999999.5, "1e+06"),
            (0.0001, "0.0001"),
            (0.00001, "1e-05"),
            (2.7182817, "2.71828"),
            (1e-30, "1e-30"),
        ];
        for (v, want) in cases {
            assert_eq!(format_g(v), want, "{v:e}");
        }
        assert_eq!(format_g(f32::INFINITY), "inf");
    }

    #[test]
    fn csv_is_rsx_text() {
        let want = "\"b\",\"i\",\"f\",\"v\",\"s\",\"a\",\"n\"\n\
            true,-7,0.1,\"<1,-2.5,1e+07>\",\"a \"q\"\",\"mdl/x.rmdl\",\"\"\n\
            false,0,123457,\"<0,0.0001,1e-05>\",\"!!DATA EXCLUDED!!\",\"x\",\"y\"\n\
            \"bool\",\"int\",\"float\",\"vector\",\"string\",\"asset\",\"asset_noprecache\"";
        assert_eq!(to_csv(&table()), want);
    }

    #[test]
    fn json_keeps_types_and_exact_floats() {
        let json = to_json(&table()).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(
            v["columns"][3],
            serde_json::json!({"name": "v", "type": "vector"})
        );
        assert_eq!(v["rows"][0][0], true);
        assert_eq!(v["rows"][0][4], "a \"q\"");
        assert!(
            json.contains("[true, -7, 0.1, [1.0,-2.5,10000000.0]"),
            "{json}"
        );
        assert_eq!(v["rows"][1][2].as_f64().unwrap() as f32, 123456.7);
    }

    #[test]
    fn an_empty_table_is_valid_json() {
        let json = to_json(&T(Vec::new(), Vec::new())).unwrap();
        assert_eq!(json, "{\n  \"columns\": [],\n  \"rows\": []\n}\n");
    }
}

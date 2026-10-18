//! `table.csv` and `table.json` from any [`Table`]: the universal text targets.
//!
//! The CSV is RFC 4180: a header of column names, a row per line, fields quoted when they hold a
//! comma, a quote or a line break (quotes doubled). A bool is `true` / `false`, a float is
//! printed with its shortest round-trip form, a vector is `x;y;z`.
//!
//! The JSON is `{"columns": [{"name", "type"}], "rows": [[cell, ...]]}`, one row per line;
//! floats round-trip exactly, a vector is `[x, y, z]` and a non-finite float is `null`. (This is
//! what `dtbl.json` always wrote; `dtbl` uses this code.)

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

/// The targets take no options.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct NoOptions {}

/// Convert edge `<table> -> table.csv`, from any value with the [`Table`] capability.
pub struct TableCsvEncode;

/// Convert edge `<table> -> table.json`, from any value with the [`Table`] capability.
pub struct TableJsonEncode;

fn field(out: &mut String, s: &str) {
    if s.contains([',', '"', '\n', '\r']) {
        out.push('"');
        out.push_str(&s.replace('"', "\"\""));
        out.push('"');
    } else {
        out.push_str(s);
    }
}

/// The table as RFC 4180 CSV.
pub fn to_csv(table: &dyn Table) -> String {
    let mut out = String::new();
    for (i, c) in table.columns().iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        field(&mut out, &c.name);
    }
    out.push('\n');
    for row in table.rows() {
        for (i, cell) in row.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            match cell {
                Cell::Bool(b) => out += if *b { "true" } else { "false" },
                Cell::Int(n) => write!(out, "{n}").expect("write to String"),
                Cell::Float(f) => write!(out, "{f}").expect("write to String"),
                Cell::Vector([x, y, z]) => write!(out, "{x};{y};{z}").expect("write to String"),
                Cell::String(s) => field(&mut out, s),
            }
        }
        out.push('\n');
    }
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
    serde_json::to_string(v).map_err(|e| Error::custom("table.json", e))
}

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

#[async_trait]
impl ConvertHandler for TableCsvEncode {
    async fn convert(&self, source: Value, target: &Target, _cx: &Cx) -> Result<Value> {
        let csv = to_csv(table_of(&source, "table.csv")?);
        blob(&source, target, "table.csv", csv.into_bytes())
    }
}

#[async_trait]
impl ConvertHandler for TableJsonEncode {
    async fn convert(&self, source: Value, target: &Target, _cx: &Cx) -> Result<Value> {
        let json = to_json(table_of(&source, "table.json")?)?;
        blob(&source, target, "table.json", json.into_bytes())
    }
}

#[cfg(test)]
mod tests {
    use respak_core::capability::{Column, ColumnType};
    use serde_json::{json, Value as Json};

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
                col("v,x", ColumnType::Vector),
                col("s", ColumnType::String),
            ],
            vec![
                vec![
                    Cell::Bool(true),
                    Cell::Int(-7),
                    Cell::Float(0.1),
                    Cell::Vector([1.0, -2.5, 1e7]),
                    Cell::String("a \"q\", b\r\nc".into()),
                ],
                vec![
                    Cell::Bool(false),
                    Cell::Int(0),
                    Cell::Float(f32::NAN),
                    Cell::Vector([0.0; 3]),
                    Cell::String(String::new()),
                ],
            ],
        )
    }

    #[test]
    fn csv_quotes_only_what_needs_it() {
        assert_eq!(
            to_csv(&table()),
            "b,i,f,\"v,x\",s\n\
             true,-7,0.1,1;-2.5;10000000,\"a \"\"q\"\", b\r\nc\"\n\
             false,0,NaN,0;0;0,\n"
        );
    }

    #[test]
    fn json_has_a_row_per_line_and_null_for_non_finite_floats() {
        let text = to_json(&table()).unwrap();
        let parsed: Json = serde_json::from_str(&text).unwrap();
        assert_eq!(
            parsed["columns"][3],
            json!({"name": "v,x", "type": "vector"})
        );
        assert_eq!(parsed["rows"][0][3], json!([1.0, -2.5, 1e7]));
        assert_eq!(parsed["rows"][1][2], Json::Null);
        // `{`, `columns` and its five lines, `rows`, two rows and the closers.
        assert_eq!(text.lines().count(), 13);
    }

    #[test]
    fn an_empty_table_is_valid_json() {
        let text = to_json(&T(Vec::new(), Vec::new())).unwrap();
        let parsed: Json = serde_json::from_str(&text).unwrap();
        assert_eq!(parsed, json!({"columns": [], "rows": []}));
        assert_eq!(to_csv(&T(Vec::new(), Vec::new())), "\n");
    }
}

use super::*;
use crate::stlt::Field;

fn field(name: &str, ty: FieldType, offset: u32, sub: u8) -> Field {
    Field {
        name: name.into(),
        help: String::new(),
        ty,
        offset,
        sub,
    }
}

fn layout(fields: Vec<Field>, total: u32, alignment: u32, subs: Vec<Layout>) -> Layout {
    Layout {
        name: None,
        fields,
        element_count: 1,
        total_size: total,
        extra_data_size_index: 0,
        alignment,
        subs,
    }
}

#[test]
fn collects_strings_at_any_depth() {
    let v = json!({"a": "x/y.rpak", "n": 3, "e": "", "l": [{"s": "z"}, null]});
    let mut out = Vec::new();
    collect_strings(&v, &mut out);
    out.sort();
    assert_eq!(out, ["x/y.rpak", "z"]);
}

#[test]
fn decodes_scalars_vectors_and_arrays() {
    let mut sub = layout(
        vec![
            field("n", FieldType::Int, 0, 0),
            field("b", FieldType::Bool, 4, 0),
        ],
        5,
        4,
        Vec::new(),
    );
    sub.element_count = 2;
    let root = layout(
        vec![
            field("f", FieldType::Float, 0, 0),
            field("v", FieldType::Float2, 4, 0),
            field("s", FieldType::String, 16, 0),
            field("list", FieldType::Array, 24, 0),
        ],
        40,
        8,
        vec![sub],
    );
    let mut buf = vec![0u8; 40];
    buf[0..4].copy_from_slice(&0.5f32.to_le_bytes());
    buf[4..8].copy_from_slice(&1.0f32.to_le_bytes());
    buf[8..12].copy_from_slice(&2.25f32.to_le_bytes());
    buf[16..24].copy_from_slice(&[1, 0, 0, 0, 8, 0, 0, 0]);
    // Elements are 8 bytes apart (5 aligned to 4 is 8) from offset 24.
    buf[24..28].copy_from_slice(&7i32.to_le_bytes());
    buf[28] = 1;
    buf[32..36].copy_from_slice(&(-3i32).to_le_bytes());
    let json = decode(&root, &reader(&buf), 0, &mut |_| Some("txt".into()), 0).unwrap();
    assert_eq!(
        json,
        json!({
            "f": 0.5,
            "v": [1.0, 2.25],
            "s": "txt",
            "list": [{"n": 7, "b": true}, {"n": -3, "b": false}],
        })
    );
}

#[test]
fn dynamic_array_out_of_range_is_corrupt() {
    let element = layout(vec![field("n", FieldType::Int, 0, 0)], 4, 4, Vec::new());
    let root = layout(
        vec![field("list", FieldType::DynamicArray, 0, 0)],
        8,
        4,
        vec![element],
    );
    let mut buf = vec![0u8; 8];
    buf[0..4].copy_from_slice(&1000i32.to_le_bytes());
    buf[4..8].copy_from_slice(&8u32.to_le_bytes());
    assert!(decode(&root, &reader(&buf), 0, &mut |_| None, 0).is_err());
}

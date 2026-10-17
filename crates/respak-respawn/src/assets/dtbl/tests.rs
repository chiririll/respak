use super::*;

fn header(v1: bool, cols: i32, rows: i32, stride: i32) -> Vec<u8> {
    let mut h = vec![0u8; if v1 { 0x28 } else { 0x20 }];
    h[0..4].copy_from_slice(&cols.to_le_bytes());
    h[4..8].copy_from_slice(&rows.to_le_bytes());
    h[8..16].copy_from_slice(&0x100u64.to_le_bytes());
    h[16..24].copy_from_slice(&0x200u64.to_le_bytes());
    let at = if v1 { 0x20 } else { 0x18 };
    h[at..at + 4].copy_from_slice(&stride.to_le_bytes());
    h
}

#[test]
fn the_variant_comes_from_the_asset_version_and_pak_date() {
    assert_eq!(variant(0, 0).unwrap(), Version::new(0));
    assert_eq!(variant(1, CREATED_V1_1_FROM).unwrap(), Version::new(1));
    assert_eq!(
        variant(1, CREATED_V1_1_FROM + 1).unwrap(),
        Version::with_minor(1, 1)
    );
    assert_eq!(
        variant(1, CREATED_V1_1_TO - 1).unwrap(),
        Version::with_minor(1, 1)
    );
    assert_eq!(variant(1, CREATED_V1_1_TO).unwrap(), Version::new(1));
    assert!(matches!(
        variant(2, 0).unwrap_err().kind(),
        ErrorKind::UnsupportedVersion { .. }
    ));
}

#[test]
fn both_header_layouts_parse() {
    let h = DtblHeader::parse(Version::new(0), &header(false, 3, 5, 24)).unwrap();
    assert_eq!(
        (h.num_columns, h.num_rows, h.columns, h.rows, h.row_stride),
        (3, 5, Some(0x100), Some(0x200), 24)
    );
    assert_eq!(h.column_size(), 0x10);
    let h = DtblHeader::parse(Version::with_minor(1, 1), &header(true, 3, 5, 24)).unwrap();
    assert_eq!((h.row_stride, h.column_size()), (24, 0x18));
}

#[test]
fn a_wrong_header_length_or_negative_count_is_corrupt() {
    let short = DtblHeader::parse(Version::new(1), &header(false, 1, 1, 4)).unwrap_err();
    assert!(matches!(short.kind(), ErrorKind::Corrupt { .. }));
    let negative = DtblHeader::parse(Version::new(0), &header(false, -1, 1, 4)).unwrap_err();
    assert!(matches!(negative.kind(), ErrorKind::Corrupt { .. }));
}

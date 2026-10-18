use super::*;

#[test]
fn a_colour_tag_is_split_off() {
    assert_eq!(
        parse_color_tag("<clr:255,0,10>Hi"),
        (Some([255, 0, 10]), "Hi")
    );
    assert_eq!(parse_color_tag("<clr: 1, 2 ,3>x"), (Some([1, 2, 3]), "x"));
    assert_eq!(parse_color_tag("<clr:0,0,0>"), (Some([0, 0, 0]), ""));
    assert_eq!(parse_color_tag("plain"), (None, "plain"));
}

#[test]
fn malformed_tags_give_the_whole_string() {
    for s in [
        "<clr:1,2>x",
        "<clr:1,2,3 x",
        "<clr:1,2,3,4>x",
        "<clr:256,0,0>x",
        "<clr:-1,0,0>x",
        "<clr:a,b,c>x",
        "x<clr:1,2,3>",
    ] {
        assert_eq!(parse_color_tag(s), (None, s), "{s}");
    }
}

#[test]
fn the_header_is_checked() {
    let mut h = [0u8; HEADER_LEN];
    h[0..4].copy_from_slice(&2i32.to_le_bytes());
    h[4..8].copy_from_slice(&4i32.to_le_bytes());
    h[8..16].copy_from_slice(&0x40u64.to_le_bytes());
    let ok = SubtHeader::parse(&h).unwrap();
    assert_eq!((ok.string_count, ok.entry_count), (2, 4));
    assert_eq!((ok.entries, ok.strings), (Some(0x40), None));

    assert!(SubtHeader::parse(&h[..20]).is_err());
    let mut negative = h;
    negative[4..8].copy_from_slice(&(-1i32).to_le_bytes());
    assert!(SubtHeader::parse(&negative).is_err());
    let mut null = h;
    null[8..16].fill(0);
    assert!(SubtHeader::parse(&null).is_err());
    null[4..8].fill(0);
    assert!(SubtHeader::parse(&null).is_ok());
}

#[test]
fn the_russian_pak_name_hashes_to_its_real_guid() {
    // Seen with `discover` on the retail `subtitles_russian.rpak` (2026-10-18).
    assert_eq!(
        crate::guid::string_to_guid("subtitles/subtitles_russian.rpak"),
        0x0770_D3AB_D7E3_E286
    );
}

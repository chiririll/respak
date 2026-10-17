use super::read::*;
use super::*;

#[test]
fn stride_rounds_up_to_the_alignment() {
    let layout = |total_size, alignment| Layout {
        name: None,
        fields: Vec::new(),
        element_count: 1,
        total_size,
        extra_data_size_index: 0,
        alignment,
        subs: Vec::new(),
    };
    assert_eq!(layout(12, 8).stride(), 16);
    assert_eq!(layout(16, 8).stride(), 16);
    assert_eq!(layout(5, 1).stride(), 5);
    assert_eq!(layout(0, 0).stride(), 0);
}

#[test]
fn names_stop_at_nul_and_survive_a_bad_offset() {
    let names = b"\0abc\0de";
    assert_eq!(text(names, 1), "abc");
    assert_eq!(text(names, 5), "de");
    assert_eq!(text(names, 99), "");
}

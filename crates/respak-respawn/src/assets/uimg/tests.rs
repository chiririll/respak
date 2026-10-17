use super::read::*;
use super::*;

fn sprite(uv: [f32; 4]) -> Sprite {
    Sprite {
        path: "a".into(),
        named: true,
        hash: 0,
        uv,
        layout: [0.0; 8],
        render: [0, 0],
    }
}

#[test]
fn position_truncates_and_size_rounds() {
    // 0.5/64 of 64 = 0.5 -> 0; 1.5/64 of 64 -> 1; size 2.5/64 of 64 -> 3.
    let r = sprite([0.5 / 64.0, 1.5 / 64.0, 2.5 / 64.0, 0.49 / 64.0 + 1.0 / 64.0])
        .rect(64, 64)
        .unwrap();
    assert_eq!((r.x, r.y, r.width, r.height), (0, 1, 3, 1));
}

#[test]
fn empty_sprite_has_no_rect() {
    assert_eq!(sprite([0.0, 0.0, 0.0, 0.5]).rect(64, 64), None);
    assert_eq!(sprite([0.0, 0.0, 0.5, 0.2 / 64.0]).rect(64, 64), None);
}

#[test]
fn names_are_cut_at_the_nul() {
    let blob = b"ui/a\0ui/b\0";
    assert_eq!(cstr(blob, 0).as_deref(), Some("ui/a"));
    assert_eq!(cstr(blob, 5).as_deref(), Some("ui/b"));
    assert_eq!(cstr(blob, 4), None);
    assert_eq!(cstr(blob, 99), None);
}

#[test]
fn resize_picks_the_nearest_pixel() {
    let src: Vec<u8> = [1u8, 2].iter().flat_map(|&v| [v; 4]).collect();
    assert_eq!(
        resize(&src, 2, 1, 4, 1),
        [1, 1, 1, 1, 1, 1, 1, 1, 2, 2, 2, 2, 2, 2, 2, 2]
    );
}

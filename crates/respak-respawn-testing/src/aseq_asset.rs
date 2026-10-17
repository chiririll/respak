use crate::builder::PakBuilder;
use crate::bytes::{cstr, put, put16};

/// An IEEE half float (normal numbers and zero only, truncating).
pub fn half(v: f32) -> u16 {
    if v == 0.0 {
        return if v.is_sign_negative() { 0x8000 } else { 0 };
    }
    let bits = v.to_bits();
    let exponent = ((bits >> 23) & 0xFF) as i32 - 127 + 15;
    assert!(
        (1..31).contains(&exponent),
        "{v} is not a normal half float"
    );
    (((bits >> 16) & 0x8000) as u16) | ((exponent as u16) << 10) | ((bits >> 13) & 0x3FF) as u16
}

/// A `Vector48`.
pub fn vec48(v: [f32; 3]) -> [u8; 6] {
    let mut out = [0u8; 6];
    for (i, x) in v.iter().enumerate() {
        out[2 * i..2 * i + 2].copy_from_slice(&half(*x).to_le_bytes());
    }
    out
}

/// A `Quaternion64` from `[x, y, z, w]`.
pub fn quat64(q: [f32; 4]) -> [u8; 8] {
    let axis = |v: f32| ((v * 1_048_576.5 + 1_048_576.0).round() as i64).clamp(0, 0x1F_FFFF) as u64;
    let packed =
        axis(q[0]) | (axis(q[1]) << 21) | (axis(q[2]) << 42) | (u64::from(q[3] < 0.0) << 63);
    packed.to_le_bytes()
}

/// A track block of `valid = 0`: one `i16` per frame.
pub fn raw_track(values: &[i16]) -> Vec<u8> {
    let mut out = vec![0, values.len() as u8];
    out.extend(values.iter().flat_map(|v| v.to_le_bytes()));
    out
}

/// A `mstudioanim_valueptr_t`: `offset` bytes to the first track, `axes` (bit 2 X, 1 Y, 0 Z),
/// then the track distances in 2-byte units.
pub fn value_ptr(offset: u16, axes: u8, axis1: u8, axis2: u8) -> [u8; 4] {
    let head = (offset | (u16::from(axes) << 13)).to_le_bytes();
    [head[0], head[1], axis1, axis2]
}

/// A bone's header (`size`, `tracked`: 0x1 scale, 0x2 rotation, 0x4 position) and its `data`.
pub fn bone(tracked: u8, data: &[u8]) -> Vec<u8> {
    let mut out = ((data.len() as u16 + 2) | (u16::from(tracked) << 13))
        .to_le_bytes()
        .to_vec();
    out.extend_from_slice(data);
    out
}

/// The bone flag array: 4-bit entries, or 6-bit when `six`.
pub fn flag_array(flags: &[u8], six: bool) -> Vec<u8> {
    let bits = if six { 6 } else { 4 };
    let mut out = vec![0u8; (bits * flags.len()).div_ceil(8).next_multiple_of(2)];
    for (i, f) in flags.iter().enumerate() {
        let at = bits * i;
        let word = u32::from(*f) << (at & 7);
        out[at >> 3] |= word as u8;
        if let Some(next) = out.get_mut((at >> 3) + 1) {
            *next |= (word >> 8) as u8;
        }
    }
    out
}

/// One section: the flag array of `bones` (flags, bone block) and then every non-empty block.
pub fn section(six: bool, bones: &[(u8, Vec<u8>)]) -> Vec<u8> {
    let flags: Vec<u8> = bones.iter().map(|b| b.0).collect();
    let mut out = flag_array(&flags, six);
    for (_, block) in bones {
        out.extend_from_slice(block);
    }
    out
}

/// One animation of a sequence (`mstudioanimdesc_v19_1_t`).
#[derive(Clone)]
pub struct Anim {
    pub name: String,
    pub fps: f32,
    /// `0x1` looping, `0x4` delta, `0x20` all zeros, `0x20000` has data, `0x200000` datapoint,
    /// `0x40000` frame movement.
    pub flags: i32,
    pub frames: i32,
    pub asqd: u64,
    pub stall: u16,
    pub section_frames: u16,
    /// The section table: an offset, or `!offset` for a section in `dataExtra`.
    pub sections: Vec<i32>,
    /// A `mstudioframemovement_t` (see [`movement`]).
    pub movement: Option<Vec<u8>>,
}

impl Anim {
    /// 30 fps, has data, no sections.
    pub fn new(name: &str, frames: i32, asqd: u64) -> Self {
        Self {
            name: name.into(),
            fps: 30.0,
            flags: 0x20000,
            frames,
            asqd,
            stall: 0,
            section_frames: 0,
            sections: Vec::new(),
            movement: None,
        }
    }
}

/// The bone weight list of a sequence.
#[derive(Clone)]
pub enum Weights {
    /// `weightlistindex` 1.
    Zero,
    /// `weightlistindex` 3.
    One,
    /// A float per bone.
    List(Vec<f32>),
}

/// A sequence (`mstudioseqdesc_v18_t`) with its animations.
#[derive(Clone)]
pub struct Seq {
    pub label: String,
    /// `0x20000`: the sequence has scale data.
    pub flags: i32,
    /// `groupsize`; the number of animations when `None`.
    pub group: Option<[u8; 2]>,
    pub weights: Weights,
    pub autolayers: Vec<u64>,
    pub anims: Vec<Anim>,
}

impl Seq {
    /// No scale, all weights one, no autolayers.
    pub fn new(label: &str, anims: Vec<Anim>) -> Self {
        Self {
            label: label.into(),
            flags: 0,
            group: None,
            weights: Weights::One,
            autolayers: Vec::new(),
            anims,
        }
    }
}

/// The block `aseq` points at: the seqdesc at 0, the animation table at 0x80, the label at
/// 0xC0, weights at 0x100, autolayers at 0x200 and animation `i` at `0x400 + 0x400 * i`
/// (animdesc, section table at +0x40, name at +0x80, frame movement at +0x100). Offsets are
/// even, so `FIX_OFFSET` leaves them alone.
pub fn seq_block(seq: &Seq) -> Vec<u8> {
    let mut b = vec![0u8; 0x400 + 0x400 * seq.anims.len()];
    put16(&mut b, 0x00, 0xC0);
    cstr(&mut b, 0xC0, &seq.label);
    put(&mut b, 0x04, &seq.flags.to_le_bytes());
    put16(&mut b, 0x2A, 0x80);
    let group = seq.group.unwrap_or([seq.anims.len() as u8, 1]);
    b[0x54] = group[0];
    b[0x55] = group[1];
    match &seq.weights {
        Weights::Zero => put16(&mut b, 0x52, 1),
        Weights::One => put16(&mut b, 0x52, 3),
        Weights::List(list) => {
            put16(&mut b, 0x52, 0x100);
            for (i, w) in list.iter().enumerate() {
                put(&mut b, 0x100 + 4 * i, &w.to_le_bytes());
            }
        }
    }
    put16(&mut b, 0x4E, seq.autolayers.len() as u16);
    put16(&mut b, 0x50, 0x200);
    for (i, guid) in seq.autolayers.iter().enumerate() {
        put(&mut b, 0x200 + 0x20 * i, &guid.to_le_bytes());
    }
    for (i, a) in seq.anims.iter().enumerate() {
        let at = 0x400 + 0x400 * i;
        put16(&mut b, 0x80 + 2 * i, at as u16);
        put(&mut b, at, &a.fps.to_le_bytes());
        put(&mut b, at + 4, &a.flags.to_le_bytes());
        put(&mut b, at + 8, &a.frames.to_le_bytes());
        put16(&mut b, at + 0xC, 0x80);
        cstr(&mut b, at + 0x80, &a.name);
        if let Some(m) = &a.movement {
            put16(&mut b, at + 0xE, 0x100);
            put(&mut b, at + 0x100, m);
        }
        put(&mut b, at + 0x18, &a.asqd.to_le_bytes());
        put16(&mut b, at + 0x2A, 0x40);
        for (k, s) in a.sections.iter().enumerate() {
            put(&mut b, at + 0x40 + 4 * k, &s.to_le_bytes());
        }
        put16(&mut b, at + 0x2C, a.stall);
        put16(&mut b, at + 0x2E, a.section_frames);
    }
    b
}

/// A `mstudioframemovement_t`: `scale` (x, y, z, yaw), the frames per section, and per section
/// the four tracks (x, y, z, yaw; `None` = no track).
pub fn movement(
    scale: [f32; 4],
    section_frames: i32,
    sections: &[[Option<Vec<u8>>; 4]],
) -> Vec<u8> {
    let mut out = vec![0u8; 0x14 + 2 * sections.len()];
    for (i, s) in scale.iter().enumerate() {
        put(&mut out, 4 * i, &s.to_le_bytes());
    }
    put(&mut out, 0x10, &section_frames.to_le_bytes());
    for (i, tracks) in sections.iter().enumerate() {
        while !out.len().is_multiple_of(2) {
            out.push(0);
        }
        let section_at = out.len();
        put16(&mut out, 0x14 + 2 * i, section_at as u16);
        out.extend([0u8; 8]);
        for (k, track) in tracks.iter().enumerate() {
            if let Some(t) = track {
                while !out.len().is_multiple_of(2) {
                    out.push(0);
                }
                let track_at = out.len() - section_at;
                put16(&mut out, section_at + 2 * k, track_at as u16);
                out.extend_from_slice(t);
            }
        }
    }
    out
}

impl PakBuilder {
    /// A sequence of asset `version` (12 to 14; 64-byte header) whose data block is `block` (see
    /// [`aseq::seq_block`]), listing `models`, with `extra` as its `dataExtra`.
    pub fn aseq(
        &mut self,
        guid: u64,
        name: &str,
        version: u32,
        block: &[u8],
        models: &[u64],
        extra: Option<&[u8]>,
    ) {
        let block_at = self.data(block);
        let name_at = self.string(name);
        let mut h = vec![0u8; 64];
        let mut pointers = vec![(0x00, block_at), (0x08, name_at)];
        if !models.is_empty() {
            let list: Vec<u8> = models.iter().flat_map(|g| g.to_le_bytes()).collect();
            pointers.push((0x20, self.data(&list)));
            put(&mut h, 0x18, &(models.len() as u16).to_le_bytes());
        }
        if let Some(extra) = extra {
            pointers.push((0x38, self.data(extra)));
            put(&mut h, 0x1C, &(extra.len() as u32).to_le_bytes());
        }
        self.asset(guid, b"aseq", version, h, &pointers, None);
    }

    /// An `asqd` (asset version 1) holding `blob`.
    pub fn asqd(&mut self, guid: u64, blob: &[u8]) {
        let blob_at = self.data(blob);
        self.asset(guid, b"asqd", 1, vec![0u8; 8], &[(0x00, blob_at)], None);
    }

    /// An `asqd` with no data pointer.
    pub fn asqd_without_data(&mut self, guid: u64) {
        self.asset(guid, b"asqd", 1, vec![0u8; 8], &[], None);
    }

    /// An `arig` of asset `version` (4 to 7; 40-byte header) with the studio block `studio` (see
    /// [`rmdl::studio`]) listing the sequences `seqs`.
    pub fn arig(&mut self, guid: u64, name: &str, version: u32, studio: &[u8], seqs: &[u64]) {
        let studio_at = self.data(studio);
        let name_at = self.string(name);
        let mut h = vec![0u8; 0x28];
        let mut pointers = vec![(0x00, studio_at), (0x08, name_at)];
        if !seqs.is_empty() {
            let list: Vec<u8> = seqs.iter().flat_map(|g| g.to_le_bytes()).collect();
            pointers.push((0x18, self.data(&list)));
        }
        if version == 4 {
            put(&mut h, 0x14, &(seqs.len() as i32).to_le_bytes());
        } else {
            put(&mut h, 0x12, &(seqs.len() as i16).to_le_bytes());
        }
        self.asset(guid, b"arig", version, h, &pointers, None);
    }
}

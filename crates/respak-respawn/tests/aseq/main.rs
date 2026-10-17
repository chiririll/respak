//! `aseq`, `asqd` and `arig` on synthetic paks (slice 19): versions, sections, tracks, rig lookup
//! and errors.

#[path = "../common/mod.rs"]
mod common;

use common::*;
use respak_core::capability::Track;
use respak_core::{Cx, ErrorKind, Registry, Value, Version};
use respak_respawn::{
    aseq_variant, studio_variant, AnimDesc, ArigHeader, AseqAsset, AseqHeader, AsqdAsset,
    AsqdHeader, RespawnPlugin, RigSelect, SectionData, SeqDesc, StudioLayout, StudioSelect,
    Weights,
};

const SEQ: u64 = 0x10;
const DATA: u64 = 0x20;
const RIG: u64 = 0x50;
/// The pak creation time from which rsx calls an aseq of asset version 12 "12.1".
const CREATED_12_1: u64 = 0x01DC_1DF8_05C2_8000;
const ROT: f32 = 0.000_191_753_45;

fn studio() -> Vec<u8> {
    rmdl::studio(18, false, 0)
}

/// A pak `p.rpak` with a sequence (asset `version`) and the `asqd` blobs, plus a rig listing it.
fn pak_with(
    version: u32,
    seq: &aseq::Seq,
    blobs: &[(u64, Vec<u8>)],
    extra: Option<&[u8]>,
    rig: bool,
) -> Vec<u8> {
    let mut p = PakBuilder::new();
    if rig {
        p.arig(RIG, "anim/rig", 5, &studio(), &[SEQ]);
    }
    for (guid, blob) in blobs {
        p.asqd(*guid, blob);
    }
    p.aseq(
        SEQ,
        "anim/walk.rseq",
        version,
        &aseq::seq_block(seq),
        &[0x77],
        extra,
    );
    let mut bytes = p.build(8);
    if version == 12 {
        bytes[8..16].copy_from_slice(&CREATED_12_1.to_le_bytes());
    }
    bytes
}

fn context(env: &Env, plugin: &RespawnPlugin) -> Cx {
    let mut r = Registry::new();
    r.add(plugin);
    respak_core::testing::cx(r, env.input.clone(), env.input.clone())
}

/// Reads asset `guid` of `kind` in `pak` through the mediator.
async fn read_with(
    env: &Env,
    plugin: &RespawnPlugin,
    pak: &str,
    guid: u64,
    kind: &str,
) -> (Cx, respak_core::Result<Value>) {
    let cx = context(env, plugin);
    let value = match cx
        .mediator
        .open(&format!("{pak}/0x{guid:016X}.{kind}"), &cx)
        .await
    {
        Ok(asset) => cx.mediator.identify(asset, &cx).await,
        Err(e) => Err(e),
    };
    (cx, value)
}

async fn load(env: &Env, bytes: Vec<u8>) -> (Cx, respak_core::Result<Value>) {
    env.input.write("p.rpak", bytes).await.unwrap();
    read_with(env, &RespawnPlugin::new(), "p.rpak", SEQ, "aseq").await
}

async fn loaded(env: &Env, bytes: Vec<u8>) -> (Cx, Value) {
    let (cx, value) = load(env, bytes).await;
    (cx, value.unwrap())
}

fn near(a: &[f32], b: &[f32]) {
    assert_eq!(a.len(), b.len(), "{a:?} != {b:?}");
    for (x, y) in a.iter().zip(b) {
        assert!((x - y).abs() < 1e-4, "{a:?} != {b:?}");
    }
}

fn dense<T: Clone>(t: &Track<T>) -> Vec<T> {
    match t {
        Track::Dense(v) => v.clone(),
        Track::Constant(_) => panic!("expected a dense track"),
    }
}

fn constant<T: Clone>(t: &Track<T>) -> T {
    match t {
        Track::Constant(v) => v.clone(),
        Track::Dense(_) => panic!("expected a constant track"),
    }
}

/// A position track of one axis (X) with `scale`: the bone block with a tracked position.
fn tracked_pos(scale: f32, track: Vec<u8>) -> Vec<u8> {
    let mut data = scale.to_le_bytes().to_vec();
    data.extend(aseq::value_ptr(4, 0b100, 0, 0));
    data.extend(track);
    aseq::bone(0x4, &data)
}

/// Bone 0 with a tracked position (X) and rotation (pitch), bone 1 with a constant rotation.
fn two_bone_section(six: bool) -> Vec<u8> {
    let mut data = 0.5f32.to_le_bytes().to_vec();
    data.extend(aseq::value_ptr(8, 0b100, 0, 0));
    data.extend(aseq::value_ptr(14, 0b010, 0, 0));
    data.extend(aseq::raw_track(&[10, 20, 30, 40]));
    data.extend(aseq::raw_track(&[1000, 2000, 3000, 4000]));
    aseq::section(
        six,
        &[
            (3, aseq::bone(0x6, &data)),
            (2, aseq::bone(0, &aseq::quat64([0.5; 4]))),
        ],
    )
}

fn one_anim(frames: i32) -> aseq::Seq {
    aseq::Seq::new("walk", vec![aseq::Anim::new("walk_anim", frames, DATA)])
}

mod arig;
mod decode;
mod errors;
mod versions;

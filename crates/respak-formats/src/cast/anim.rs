//! `Animation -> cast`.

use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use respak_core::capability::{Animation, Track};
use respak_core::value::{BlobData, BundleData};
use respak_core::{ConvertHandler, Cx, FormatId, Result, Target, TypedTarget, Value, ValueKey};

use super::{custom, stem_of, CastOptions, CastWriter, RootFixup};

/// Bone 0's position and rotation `[x, y, z, w]` after rsx's root fix-up: the rotation as Source
/// angles with yaw - 90 degrees, the position `(x, y)` to `(y, -x)` (`ParseAnimDesc_Origin`).
pub(super) fn root_fixup(position: [f32; 3], rotation: [f32; 4]) -> ([f32; 3], [f32; 4]) {
    // Lowering the Source yaw by 90 degrees is a turn about Z applied after the rest of the rotation.
    let (s, c) = (-std::f32::consts::FRAC_PI_4).sin_cos();
    (
        [position[1], -position[0], position[2]],
        quat_mul([0.0, 0.0, s, c], rotation),
    )
}

/// The Hamilton product `a * b` of two quaternions `[x, y, z, w]`.
fn quat_mul(a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    [
        a[3] * b[0] + a[0] * b[3] + a[1] * b[2] - a[2] * b[1],
        a[3] * b[1] - a[0] * b[2] + a[1] * b[3] + a[2] * b[0],
        a[3] * b[2] + a[0] * b[1] - a[1] * b[0] + a[2] * b[3],
        a[3] * b[3] - a[0] * b[0] - a[1] * b[1] - a[2] * b[2],
    ]
}

/// One value per frame from a track.
fn per_frame<T: Copy>(track: &Track<T>, frames: u32, what: &str) -> Result<Vec<T>> {
    match track {
        Track::Constant(v) => Ok(vec![*v; frames as usize]),
        Track::Dense(v) if v.len() == frames as usize => Ok(v.clone()),
        Track::Dense(v) => Err(custom(format!(
            "{what}: {} values for {frames} frames",
            v.len()
        ))),
    }
}

/// Capability edge `Animation -> cast`: a composite, `<stem>_<i>.cast` per written blend, each
/// with the skeleton and, per bone, position (`tx`, `ty`, `tz`), rotation (`rq`) and, when the
/// animation has scale, scale (`sx`, `sy`, `sz`) curves.
pub struct CastAnimationEncode;

#[async_trait]
impl ConvertHandler for CastAnimationEncode {
    /// Writes the blends chosen by [`CastOptions::blend`]. Bone 0 gets [`root_fixup`] when
    /// [`CastOptions::root_fixup`] is `Rsx`. Checks cancellation per blend and per bone. A
    /// `Constant` track is written as full-length curves like rsx (no collapsing). Additive
    /// animations write the bone weights as `ab`.
    async fn convert(&self, source: Value, target: &Target, cx: &Cx) -> Result<Value> {
        let options = TypedTarget::<CastOptions>::parse(target)?;
        let opts = options.options();
        let animation: &dyn Animation = source
            .as_animation()
            .ok_or_else(|| custom(format!("`{}` has no Animation capability", source.format())))?;
        let blends = opts.blend.resolve(animation.blend_count())?;
        let bones = animation.bones();
        let weights = animation.bone_weights();
        let stem = source
            .as_metadata()
            .and_then(|m| m.name())
            .map_or_else(|| "animation".to_owned(), stem_of);

        let key = ValueKey::converted(
            Arc::new(source.key().clone()),
            FormatId::new("cast"),
            options.normalized(),
        );
        let mut items: Vec<(String, Value)> = Vec::new();
        for blend in blends {
            cx.check_cancelled()?;
            let info = animation.info(blend)?;
            let frames = info.frame_count;
            let mode = if info.additive {
                "additive"
            } else {
                "absolute"
            };
            let mut writer = CastWriter::new();
            writer.animation(&info.name, info.fps, info.looping, bones);
            for (index, bone) in bones.iter().enumerate() {
                cx.check_cancelled()?;
                if frames == 0 {
                    break;
                }
                let track = animation.track(blend, index as u32, cx).await?;
                let what = format!("bone {}", bone.name);
                let mut translation = per_frame(&track.translation, frames, &what)?;
                let mut rotation = per_frame(&track.rotation, frames, &what)?;
                if index == 0 && opts.root_fixup == RootFixup::Rsx {
                    for (t, r) in translation.iter_mut().zip(rotation.iter_mut()) {
                        (*t, *r) = root_fixup(*t, *r);
                    }
                }
                let weight = info
                    .additive
                    .then(|| weights.get(index).copied().unwrap_or(1.0));
                for (axis, name) in ["tx", "ty", "tz"].into_iter().enumerate() {
                    let values: Vec<f32> = translation.iter().map(|v| v[axis]).collect();
                    writer.curve(&bone.name, name, mode, weight, frames, &values)?;
                }
                writer.curve(&bone.name, "rq", mode, weight, frames, &rotation.concat())?;
                if info.has_scale {
                    let scale = per_frame(&track.scale, frames, &what)?;
                    for (axis, name) in ["sx", "sy", "sz"].into_iter().enumerate() {
                        let values: Vec<f32> = scale.iter().map(|v| v[axis]).collect();
                        writer.curve(&bone.name, name, mode, weight, frames, &values)?;
                    }
                }
            }
            let value: Value = Arc::new(BlobData::with_format(
                FormatId::new("cast"),
                Bytes::from(writer.finish()),
                key.clone(),
            ));
            items.push((format!("{stem}_{blend}.cast"), value));
        }
        Ok(Arc::new(BundleData::named(items, key)))
    }
}

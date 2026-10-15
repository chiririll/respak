//! A material with its textures: `matl -> matl.json` over synthetic paks (slice 13).

use opendal::{services::Memory, Operator};
use respak_core::capability::PixelFormat;
use respak_core::{CancellationToken, ConvertRequest, Engine, Event, OutputResult};
use respak_formats::FormatsPlugin;
use respak_respawn::testing::{build_tex_pak, dxbc_blob, PakBuilder, TexSpec};
use respak_respawn::RespawnPlugin;
use serde_json::{json, Value};

const BC1: u16 = 0;
const SHDS: u64 = 0x5300;
const SHDR: u64 = 0x5301;
const MATERIAL: u64 = 0xA1;
const MATERIAL2: u64 = 0xA2;
const ALBEDO: u64 = 0xABCD;
const NORMAL: u64 = 0xBEEF;

struct Env {
    input: Operator,
    output: Operator,
}

fn mem() -> Operator {
    Operator::new(Memory::default()).unwrap()
}

fn engine(env: &Env, plugin: RespawnPlugin, cache: Option<Operator>) -> Engine {
    let mut b = Engine::builder().input(env.input.clone()).output(env.output.clone()).plugin(FormatsPlugin).plugin(plugin);
    if let Some(cache) = cache {
        b = b.cache(cache);
    }
    b.build().unwrap()
}

/// BC1 16x8 with three permanent mips, each filled with its shift + 1.
fn texture(guid: u64) -> TexSpec {
    let mut data = Vec::new();
    for shift in (0..3u32).rev() {
        let len = PixelFormat::Bc1.mip_len(16 >> shift, 8 >> shift).div_ceil(16).max(1) * 16;
        data.extend(std::iter::repeat_n(shift as u8 + 1, len as usize));
    }
    let mut t = TexSpec::new(guid, 10, 16, 8, BC1, 3, data);
    t.name = Some("albedo");
    t
}

async fn put(op: &Operator, path: &str, bytes: Vec<u8>) {
    op.write(path, bytes).await.unwrap();
}

/// `paks/common.rpak` with the textures, `paks/lvl.rpak` with the materials and their shader.
async fn standard_env(textures_in_common: bool) -> Env {
    let env = Env { input: mem(), output: mem() };
    let textures = [texture(ALBEDO), texture(NORMAL)];
    if textures_in_common {
        put(&env.input, "paks/common.rpak", build_tex_pak(8, &textures)).await;
    } else {
        put(&env.input, "elsewhere/tex.rpak", build_tex_pak(8, &textures)).await;
    }
    let mut lvl = PakBuilder::new();
    lvl.shdr(SHDR, &dxbc_blob(&[("albedoTexture", 2, 0), ("sampler0", 3, 0), ("normalTexture", 2, 1)]));
    lvl.shds(SHDS, SHDR);
    lvl.matl(MATERIAL, "material/wall", SHDS, &[ALBEDO, NORMAL]);
    lvl.matl(MATERIAL2, "material/floor", SHDS, &[ALBEDO]);
    put(&env.input, "paks/lvl.rpak", lvl.build(8)).await;
    env
}

fn request(from: &str, to: Value, path: &str) -> ConvertRequest {
    serde_json::from_value(json!({ "version": 1, "outputs": [{ "from": from, "to": to, "path": path }] })).unwrap()
}

async fn convert(engine: &Engine, from: &str, to: Value, path: &str) -> (OutputResult, Vec<Event>) {
    let mut events = engine.events();
    let report = engine.convert(&request(from, to, path), &CancellationToken::new()).await.unwrap();
    let mut seen = Vec::new();
    while let Ok(e) = events.try_recv() {
        seen.push(e);
    }
    (report.outputs.into_iter().next().unwrap().result, seen)
}

/// Warnings other than the mediator's note about which of several equal paths it chose.
fn warnings(events: &[Event]) -> Vec<&str> {
    events
        .iter()
        .filter_map(|e| if let Event::Warning { message } = e { Some(message.as_str()) } else { None })
        .filter(|m| !m.starts_with("path "))
        .collect()
}

async fn json_of(env: &Env, path: &str) -> Value {
    serde_json::from_slice(&env.output.read(path).await.unwrap().to_vec()).unwrap()
}

const WALL: &str = "paks/lvl.rpak/0x00000000000000A1.matl";
const FLOOR: &str = "paks/lvl.rpak/0x00000000000000A2.matl";

#[tokio::test]
async fn a_material_exports_its_json_and_textures_from_common_rpak() {
    let env = standard_env(true).await;
    let engine = engine(&env, RespawnPlugin::new(), None);
    let (result, events) = convert(&engine, WALL, json!({ "type": "matl.json" }), "out/wall").await;

    let OutputResult::Ok { files, dependencies } = result else { panic!("{result:?}") };
    assert_eq!(files, ["out/wall/material.json", "out/wall/textures/albedoTexture.png", "out/wall/textures/normalTexture.png"]);
    assert!(warnings(&events).is_empty(), "{:?}", warnings(&events));

    let doc = json_of(&env, "out/wall/material.json").await;
    assert_eq!(doc["name"], "material/wall");
    assert_eq!(doc["guid"], "0x00000000000000A1");
    assert_eq!((doc["width"].as_u64(), doc["height"].as_u64()), (Some(512), Some(256)));
    assert_eq!(doc["shaderSet"], "0x0000000000005300");
    assert_eq!(doc["textures"]["albedoTexture"], json!({ "slot": 0, "guid": "0x000000000000ABCD", "file": "textures/albedoTexture.png" }));
    assert_eq!(doc["textures"]["normalTexture"]["slot"], 1);

    let png = env.output.read("out/wall/textures/albedoTexture.png").await.unwrap().to_vec();
    assert_eq!(&png[1..4], b"PNG");

    // The report names what the converter asked for.
    assert_eq!(dependencies.len(), 2);
    assert!(dependencies.iter().all(|d| d.format == "png" && d.key.contains("paks/common.rpak/0x")), "{dependencies:?}");
}

#[tokio::test]
async fn the_textures_option_picks_the_texture_format() {
    let env = standard_env(true).await;
    let engine = engine(&env, RespawnPlugin::new(), None);
    let to = json!({ "type": "matl.json", "textures": { "type": "dds" } });
    let (result, _) = convert(&engine, WALL, to, "out/wall").await;
    let OutputResult::Ok { files, .. } = result else { panic!("{result:?}") };
    assert_eq!(files[1], "out/wall/textures/albedoTexture.dds");
    assert_eq!(&env.output.read(&files[1]).await.unwrap().to_vec()[..4], b"DDS ");
    let doc = json_of(&env, "out/wall/material.json").await;
    assert_eq!(doc["textures"]["albedoTexture"]["file"], "textures/albedoTexture.dds");
}

#[tokio::test]
async fn without_a_usable_shader_slots_are_numbered_and_a_warning_says_why() {
    let env = Env { input: mem(), output: mem() };
    put(&env.input, "paks/common.rpak", build_tex_pak(8, &[texture(ALBEDO)])).await;
    let mut lvl = PakBuilder::new();
    lvl.matl(MATERIAL, "material/wall", 0, &[0, ALBEDO]);
    put(&env.input, "paks/lvl.rpak", lvl.build(8)).await;
    let engine = engine(&env, RespawnPlugin::new(), None);

    let (result, events) = convert(&engine, WALL, json!({ "type": "matl.json" }), "out/wall").await;
    let OutputResult::Ok { files, .. } = result else { panic!("{result:?}") };
    assert_eq!(files, ["out/wall/material.json", "out/wall/textures/slot_1.png"]);
    assert!(warnings(&events).iter().any(|w| w.contains("slot_<n>")), "{:?}", warnings(&events));
}

#[tokio::test]
async fn a_corrupt_shader_falls_back_too() {
    let env = Env { input: mem(), output: mem() };
    put(&env.input, "paks/common.rpak", build_tex_pak(8, &[texture(ALBEDO)])).await;
    let mut lvl = PakBuilder::new();
    lvl.shdr(SHDR, b"this is not dxbc");
    lvl.shds(SHDS, SHDR);
    lvl.matl(MATERIAL, "material/wall", SHDS, &[ALBEDO]);
    put(&env.input, "paks/lvl.rpak", lvl.build(8)).await;
    let engine = engine(&env, RespawnPlugin::new(), None);

    let (result, events) = convert(&engine, WALL, json!({ "type": "matl.json" }), "out/wall").await;
    let OutputResult::Ok { files, .. } = result else { panic!("{result:?}") };
    assert_eq!(files[1], "out/wall/textures/slot_0.png");
    assert!(!warnings(&events).is_empty());
}

#[tokio::test]
async fn textures_outside_the_pak_and_common_rpak_need_the_search_list() {
    let env = standard_env(false).await;

    // Without a search list the textures are missing: the export still succeeds, with nulls.
    let engine_plain = engine(&env, RespawnPlugin::new(), None);
    let (result, events) = convert(&engine_plain, WALL, json!({ "type": "matl.json" }), "out/a").await;
    assert!(matches!(result, OutputResult::Ok { .. }), "{result:?}");
    let doc = json_of(&env, "out/a/material.json").await;
    assert_eq!(doc["textures"]["albedoTexture"]["file"], Value::Null);
    assert!(warnings(&events).iter().any(|w| w.contains("skipped")), "{:?}", warnings(&events));

    // With one they are found.
    let engine_search = engine(&env, RespawnPlugin::new().search(["elsewhere/*.rpak"]), None);
    let (result, events) = convert(&engine_search, WALL, json!({ "type": "matl.json" }), "out/b").await;
    let OutputResult::Ok { files, .. } = result else { panic!("{result:?}") };
    assert_eq!(files.len(), 3);
    assert!(warnings(&events).is_empty(), "{:?}", warnings(&events));
}

#[tokio::test]
async fn a_found_location_is_cached_for_the_next_run() {
    let env = standard_env(false).await;
    let cache = mem();
    let first = engine(&env, RespawnPlugin::new().search(["elsewhere/*.rpak"]), Some(cache.clone()));
    let (result, _) = convert(&first, WALL, json!({ "type": "matl.json" }), "out/a").await;
    assert!(matches!(result, OutputResult::Ok { .. }));
    let index = cache.read("respawn/v1/guid/000000000000abcd.txtr").await.unwrap().to_vec();
    assert_eq!(serde_json::from_slice::<String>(&index).unwrap(), "elsewhere/tex.rpak");

    // A new run without the search list finds the texture through the index.
    let second = engine(&env, RespawnPlugin::new(), Some(cache));
    let (result, events) = convert(&second, WALL, json!({ "type": "matl.json" }), "out/b").await;
    let OutputResult::Ok { files, .. } = result else { panic!("{result:?}") };
    assert_eq!(files.len(), 3);
    assert!(warnings(&events).is_empty(), "{:?}", warnings(&events));
}

#[tokio::test]
async fn materials_sharing_a_texture_each_get_their_own_copy() {
    let env = standard_env(true).await;
    let engine = engine(&env, RespawnPlugin::new(), None);
    let request: ConvertRequest = serde_json::from_value(json!({ "version": 1, "outputs": [
        { "from": WALL, "to": { "type": "matl.json" }, "path": "out/wall" },
        { "from": FLOOR, "to": { "type": "matl.json" }, "path": "out/floor" },
    ] }))
    .unwrap();
    let report = engine.convert(&request, &CancellationToken::new()).await.unwrap();
    assert!(report.outputs.iter().all(|o| matches!(o.result, OutputResult::Ok { .. })), "{report:?}");

    let wall = env.output.read("out/wall/textures/albedoTexture.png").await.unwrap().to_vec();
    let floor = env.output.read("out/floor/textures/albedoTexture.png").await.unwrap().to_vec();
    assert_eq!(wall, floor);
}

#[tokio::test]
async fn a_cancelled_export_writes_nothing() {
    let env = standard_env(true).await;
    let engine = engine(&env, RespawnPlugin::new(), None);
    let cancel = CancellationToken::new();
    cancel.cancel();
    let report = engine.convert(&request(WALL, json!({ "type": "matl.json" }), "out/wall"), &cancel).await.unwrap();
    assert!(matches!(report.outputs[0].result, OutputResult::Err { .. }));
    assert!(env.output.list("out/").await.map_or(true, |l| l.iter().all(|e| e.path() == "out/")));
}

fn dependency_keys(result: &OutputResult) -> Vec<String> {
    let OutputResult::Ok { dependencies, .. } = result else { panic!("{result:?}") };
    dependencies.iter().map(|d| d.key.clone()).collect()
}

#[tokio::test]
async fn lookup_order_is_common_rpak_then_the_search_list_then_the_index() {
    let env = standard_env(true).await;
    // The same texture also lives in two other paks.
    put(&env.input, "elsewhere/tex.rpak", build_tex_pak(8, &[texture(ALBEDO), texture(NORMAL)])).await;
    put(&env.input, "indexed/tex.rpak", build_tex_pak(8, &[texture(ALBEDO), texture(NORMAL)])).await;
    let cache = mem();
    cache.write("respawn/v1/guid/000000000000abcd.txtr", br#""indexed/tex.rpak""#.to_vec()).await.unwrap();
    let first = engine_for(&env, &cache);

    let (result, _) = convert(&first, FLOOR, json!({ "type": "matl.json" }), "out/a").await;
    assert!(dependency_keys(&result)[0].starts_with("paks/common.rpak/"), "{result:?}");

    // Remove common.rpak: the search list now wins over the index.
    env.input.delete("paks/common.rpak").await.unwrap();
    let fresh = engine_for(&env, &cache);
    let (result, _) = convert(&fresh, FLOOR, json!({ "type": "matl.json" }), "out/b").await;
    assert!(dependency_keys(&result)[0].starts_with("elsewhere/tex.rpak/"), "{result:?}");

    // Without the search list only the index is left (the search above refreshed it, so set it again).
    cache.write("respawn/v1/guid/000000000000abcd.txtr", br#""indexed/tex.rpak""#.to_vec()).await.unwrap();
    let indexed = engine(&env, RespawnPlugin::new(), Some(cache));
    let (result, _) = convert(&indexed, FLOOR, json!({ "type": "matl.json" }), "out/c").await;
    assert!(dependency_keys(&result)[0].starts_with("indexed/tex.rpak/"), "{result:?}");
}

fn engine_for(env: &Env, cache: &Operator) -> Engine {
    engine(env, RespawnPlugin::new().search(["elsewhere/*.rpak"]), Some(cache.clone()))
}

//! The frontend is built once (`npm run build` in `web/`) and embedded; say so clearly when it is missing.

fn main() {
    let index = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../web/dist/index.html");
    println!("cargo:rerun-if-changed={}", index.display());
    println!("cargo:rerun-if-changed=../../web/dist");
    if !index.is_file() {
        panic!(
            "\n\nweb/dist is missing: build the frontend first.\n\n    cd web && npm ci && npm run build\n\nThen build respak-web again (see docs/design/webui.md section 3).\n"
        );
    }
}

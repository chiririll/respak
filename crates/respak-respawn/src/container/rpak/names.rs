//! File names of a pak and its patches.

/// The stem of a pak file path: `dir/common(03).rpak` is `common`.
pub(super) fn pak_stem(file: &str) -> &str {
    let name = file.rsplit_once('/').map_or(file, |(_, f)| f);
    let name = name.strip_suffix(".rpak").unwrap_or(name);
    name.split('(').next().unwrap_or(name)
}

/// The file name of patch `index` of `stem`: `common.rpak` for 0, else `common(03).rpak`.
pub(super) fn patch_file_name(stem: &str, index: u32) -> String {
    if index == 0 {
        format!("{stem}.rpak")
    } else {
        format!("{stem}({index:02}).rpak")
    }
}

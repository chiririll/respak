/// A DXBC blob whose RDEF chunk declares `(name, resource type, bind point)`; type 2 is a texture.
pub fn dxbc_blob(resources: &[(&str, u32, u32)]) -> Vec<u8> {
    let table = 0x1C_u32;
    let names_at = table + 32 * resources.len() as u32;
    let mut rdef = vec![0u8; names_at as usize];
    rdef[8..12].copy_from_slice(&(resources.len() as u32).to_le_bytes());
    rdef[12..16].copy_from_slice(&table.to_le_bytes());
    let mut names = Vec::new();
    for (i, (name, ty, bind)) in resources.iter().enumerate() {
        let e = table as usize + i * 32;
        rdef[e..e + 4].copy_from_slice(&(names_at + names.len() as u32).to_le_bytes());
        rdef[e + 4..e + 8].copy_from_slice(&ty.to_le_bytes());
        rdef[e + 20..e + 24].copy_from_slice(&bind.to_le_bytes());
        names.extend_from_slice(name.as_bytes());
        names.push(0);
    }
    rdef.extend(names);

    // Two chunks: something else first, then RDEF.
    let other = [0xAAu8; 4];
    let mut out = b"DXBC".to_vec();
    out.extend([0u8; 16]);
    out.extend(1u32.to_le_bytes());
    out.extend(0u32.to_le_bytes());
    out.extend(2u32.to_le_bytes());
    let first = 0x28u32;
    let second = first + 8 + other.len() as u32;
    out.extend(first.to_le_bytes());
    out.extend(second.to_le_bytes());
    out.extend(b"SFI0");
    out.extend((other.len() as u32).to_le_bytes());
    out.extend(other);
    out.extend(b"RDEF");
    out.extend((rdef.len() as u32).to_le_bytes());
    out.extend(rdef);
    out
}

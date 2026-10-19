//! Sizes as people write and read them.

/// `500MiB`, `8GiB`, `2GB`, `1024` (bytes): binary units for `KiB`/`MiB`/`GiB`/`TiB`, decimal for `KB`/`MB`/`GB`/`TB`.
pub fn parse_size(text: &str) -> Result<u64, String> {
    let text = text.trim();
    let split = text
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(text.len());
    let (digits, unit) = text.split_at(split);
    let number: u64 = digits
        .parse()
        .map_err(|_| format!("`{text}` is not a size (try 500MiB or 8GiB)"))?;
    let factor: u64 = match unit.trim().to_ascii_lowercase().as_str() {
        "" | "b" => 1,
        "kb" => 1_000,
        "mb" => 1_000_000,
        "gb" => 1_000_000_000,
        "tb" => 1_000_000_000_000,
        "k" | "kib" => 1 << 10,
        "m" | "mib" => 1 << 20,
        "g" | "gib" => 1 << 30,
        "t" | "tib" => 1 << 40,
        other => return Err(format!("unknown size unit `{other}` in `{text}`")),
    };
    number
        .checked_mul(factor)
        .ok_or_else(|| format!("`{text}` is too large"))
}

/// `bytes` as `512 B`, `1.5 KiB`, `3.2 MiB`, ... (binary units, as `parse_size` reads them).
pub fn format_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["KiB", "MiB", "GiB", "TiB", "PiB"];
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let mut value = bytes as f64 / 1024.0;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    format!("{value:.1} {}", UNITS[unit])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes() {
        for (text, bytes) in [
            ("1024", 1024),
            ("1b", 1),
            ("8GiB", 8 << 30),
            ("500 MiB", 500 << 20),
            ("2k", 2 << 10),
            ("2KB", 2_000),
            ("2gb", 2_000_000_000),
            ("1TB", 1_000_000_000_000),
            ("1tib", 1 << 40),
        ] {
            assert_eq!(parse_size(text), Ok(bytes), "{text}");
        }
    }

    #[test]
    fn bad_sizes() {
        for text in ["", "GiB", "-1", "1.5GiB", "5 parsecs", "99999999999TiB"] {
            assert!(parse_size(text).is_err(), "{text}");
        }
    }

    #[test]
    fn sizes_are_friendly() {
        assert_eq!(format_size(0), "0 B");
        assert_eq!(format_size(1023), "1023 B");
        assert_eq!(format_size(1536), "1.5 KiB");
        assert_eq!(format_size(5 << 20), "5.0 MiB");
        assert_eq!(format_size(3 << 30), "3.0 GiB");
    }
}

use crate::map::MapPreset;
use crate::rules::seed_code as rules_seed;
use crate::types::Coord;

pub use rules_seed::{SeedDecodeError, Tier, tier_from_seed};

/// All the parameters needed to recreate a game from a seed code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeedParams {
    pub seed: u64,
    pub width: Coord,
    pub height: Coord,
    pub preset: Option<MapPreset>,
}

const DEFAULT_WIDTH: Coord = 80;
const DEFAULT_HEIGHT: Coord = 40;

fn encode_base36(n: u64) -> String {
    let mut buf = [0u8; rules_seed::MAX_BASE36_LEN];
    let len = rules_seed::encode_to_buf(n, &mut buf);
    // SAFETY: encode_to_buf only writes ASCII base36 chars.
    unsafe { String::from_utf8_unchecked(buf[..len].to_vec()) }
}

fn decode_base36(s: &str) -> Result<u64, String> {
    rules_seed::decode_from_bytes(s.as_bytes()).map_err(|e| match e {
        SeedDecodeError::Empty => "Empty seed value".to_string(),
        SeedDecodeError::InvalidChar(b) => format!("Invalid character in seed: '{}'", b as char),
        SeedDecodeError::Overflow => "Seed value too large".to_string(),
    })
}

fn preset_to_char(preset: MapPreset) -> char {
    match preset {
        MapPreset::Arena => 'a',
        MapPreset::Corridor => 'c',
        MapPreset::Labyrinth => 'l',
        MapPreset::SingleRoom => 's',
        MapPreset::OpenField => 'f',
    }
}

fn char_to_preset(ch: char) -> Result<MapPreset, String> {
    match ch {
        'a' => Ok(MapPreset::Arena),
        'c' => Ok(MapPreset::Corridor),
        'l' => Ok(MapPreset::Labyrinth),
        's' => Ok(MapPreset::SingleRoom),
        'f' => Ok(MapPreset::OpenField),
        _ => Err(format!("Unknown preset character: '{ch}'")),
    }
}

/// Encode game parameters into a shareable seed code.
///
/// Format: `<base36_seed>[-<W>x<H>][<preset_char>]`
/// - Default dimensions (80x40) and no preset: just the seed (`r7z3kq`)
/// - Custom dimensions: `r7z3kq-120x60`
/// - With preset: `r7z3kq-a`
/// - Both: `r7z3kq-120x60a`
pub fn encode(params: &SeedParams) -> String {
    let seed_str = encode_base36(params.seed);
    let has_custom_dims = params.width != DEFAULT_WIDTH || params.height != DEFAULT_HEIGHT;
    let has_preset = params.preset.is_some();

    if !has_custom_dims && !has_preset {
        return seed_str;
    }

    let mut result = seed_str;
    result.push('-');

    if has_custom_dims {
        result.push_str(&format!("{}x{}", params.width, params.height));
    }

    if let Some(preset) = params.preset {
        result.push(preset_to_char(preset));
    }

    result
}

/// Decode a seed code into game parameters.
///
/// Case-insensitive, whitespace-trimmed.
pub fn decode(code: &str) -> Result<SeedParams, String> {
    let code = code.trim().to_ascii_lowercase();
    if code.is_empty() {
        return Err("Empty seed code".to_string());
    }

    let (seed_part, suffix) = match code.find('-') {
        Some(pos) => (&code[..pos], Some(&code[pos + 1..])),
        None => (code.as_str(), None),
    };

    let seed = decode_base36(seed_part)?;

    let (width, height, preset) = match suffix {
        None => (DEFAULT_WIDTH, DEFAULT_HEIGHT, None),
        Some("") => return Err("Trailing dash with no dimensions or preset".to_string()),
        Some(s) => parse_suffix(s)?,
    };

    Ok(SeedParams {
        seed,
        width,
        height,
        preset,
    })
}

/// Parse the suffix after the dash: optional `WxH` followed by optional preset char.
fn parse_suffix(s: &str) -> Result<(Coord, Coord, Option<MapPreset>), String> {
    // Check if it starts with a digit (dimensions) or a letter (preset only).
    if s.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        // Has dimensions: parse WxH, then optional trailing preset char.
        let x_pos = s.find('x').ok_or("Expected 'x' separator in dimensions")?;
        let w_str = &s[..x_pos];
        let after_x = &s[x_pos + 1..];

        // Find where digits end in after_x.
        let digit_end = after_x
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(after_x.len());
        let h_str = &after_x[..digit_end];
        let remaining = &after_x[digit_end..];

        let width: Coord = w_str
            .parse()
            .map_err(|_| format!("Invalid width: '{w_str}'"))?;
        let height: Coord = h_str
            .parse()
            .map_err(|_| format!("Invalid height: '{h_str}'"))?;

        let preset = if remaining.is_empty() {
            None
        } else if remaining.len() == 1 {
            Some(char_to_preset(remaining.chars().next().unwrap())?)
        } else {
            return Err(format!("Unexpected trailing characters: '{remaining}'"));
        };

        Ok((width, height, preset))
    } else if s.len() == 1 {
        // Preset only, default dimensions.
        let preset = char_to_preset(s.chars().next().unwrap())?;
        Ok((DEFAULT_WIDTH, DEFAULT_HEIGHT, Some(preset)))
    } else {
        Err(format!("Invalid suffix: '{s}'"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_default_dims_no_preset() {
        let params = SeedParams {
            seed: 12345,
            width: 80,
            height: 40,
            preset: None,
        };
        let code = encode(&params);
        assert_eq!(code, encode_base36(12345));
        assert!(!code.contains('-'));
    }

    #[test]
    fn encode_custom_dims() {
        let params = SeedParams {
            seed: 42,
            width: 120,
            height: 60,
            preset: None,
        };
        let code = encode(&params);
        assert!(code.contains("-120x60"));
    }

    #[test]
    fn encode_with_preset() {
        let params = SeedParams {
            seed: 42,
            width: 80,
            height: 40,
            preset: Some(MapPreset::Arena),
        };
        let code = encode(&params);
        assert!(code.ends_with("-a"));
    }

    #[test]
    fn encode_custom_dims_and_preset() {
        let params = SeedParams {
            seed: 42,
            width: 120,
            height: 60,
            preset: Some(MapPreset::Labyrinth),
        };
        let code = encode(&params);
        assert!(code.ends_with("-120x60l"));
    }

    #[test]
    fn roundtrip_default() {
        let params = SeedParams {
            seed: 9999999,
            width: 80,
            height: 40,
            preset: None,
        };
        assert_eq!(decode(&encode(&params)).unwrap(), params);
    }

    #[test]
    fn roundtrip_custom_dims() {
        let params = SeedParams {
            seed: 42,
            width: 120,
            height: 60,
            preset: None,
        };
        assert_eq!(decode(&encode(&params)).unwrap(), params);
    }

    #[test]
    fn roundtrip_preset() {
        for preset in [
            MapPreset::Arena,
            MapPreset::Corridor,
            MapPreset::Labyrinth,
            MapPreset::SingleRoom,
            MapPreset::OpenField,
        ] {
            let params = SeedParams {
                seed: 123,
                width: 80,
                height: 40,
                preset: Some(preset),
            };
            assert_eq!(decode(&encode(&params)).unwrap(), params);
        }
    }

    #[test]
    fn roundtrip_dims_and_preset() {
        let params = SeedParams {
            seed: 7777,
            width: 100,
            height: 50,
            preset: Some(MapPreset::Corridor),
        };
        assert_eq!(decode(&encode(&params)).unwrap(), params);
    }

    #[test]
    fn roundtrip_seed_zero() {
        let params = SeedParams {
            seed: 0,
            width: 80,
            height: 40,
            preset: None,
        };
        assert_eq!(decode(&encode(&params)).unwrap(), params);
    }

    #[test]
    fn roundtrip_seed_max() {
        let params = SeedParams {
            seed: u64::MAX,
            width: 80,
            height: 40,
            preset: None,
        };
        assert_eq!(decode(&encode(&params)).unwrap(), params);
    }

    #[test]
    fn decode_case_insensitive() {
        let params = SeedParams {
            seed: 42,
            width: 80,
            height: 40,
            preset: None,
        };
        let code = encode(&params).to_uppercase();
        assert_eq!(decode(&code).unwrap(), params);
    }

    #[test]
    fn decode_trims_whitespace() {
        let params = SeedParams {
            seed: 42,
            width: 80,
            height: 40,
            preset: None,
        };
        let code = format!("  {} \n", encode(&params));
        assert_eq!(decode(&code).unwrap(), params);
    }

    #[test]
    fn decode_empty_string_errors() {
        assert!(decode("").is_err());
    }

    #[test]
    fn decode_invalid_chars_errors() {
        assert!(decode("abc!def").is_err());
    }

    #[test]
    fn decode_unknown_preset_char_errors() {
        assert!(decode("16-z").is_err());
    }

    #[test]
    fn decode_trailing_dash_errors() {
        assert!(decode("16-").is_err());
    }

    #[test]
    fn base36_encode_zero() {
        assert_eq!(encode_base36(0), "0");
    }

    #[test]
    fn base36_encode_max_u64() {
        let encoded = encode_base36(u64::MAX);
        // u64::MAX in base36 is 13 chars: 3w5e11264sgsf
        assert!(encoded.len() <= 13);
        assert_eq!(decode_base36(&encoded).unwrap(), u64::MAX);
    }
}

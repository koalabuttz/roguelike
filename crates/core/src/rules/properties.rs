//! Property system for emergent item interactions.
//!
//! Items are composed of 16 quantitative properties stored as nibbles
//! (4-bit values, range 0–15) packed into 8 bytes. This module defines
//! the property enum, the `PropertyBag` type, and nibble access helpers.
//!
//! **Step 1 (complete):** Properties defined and assigned via `default_properties()`
//! in `items.rs`. Every item has a property profile.
//!
//! **Step 2 (complete):** Interaction engine in `interactions.rs` operates on
//! property bags via `GameCommand::Combine`, producing emergent behaviors.
//!
//! **Step 3 (complete):** `Equipment::attack_bonus()` and `defense_bonus()` read
//! from property bags via `attack_from_bag()` / `defense_from_bag()`. Modified
//! items now affect combat stats.

use core::mem::size_of;

/// A physical or magical property that items (and eventually terrain) possess.
///
/// 16 variants packed as nibbles: properties 0–7 occupy bytes 0–3 (even index
/// = high nibble, odd = low nibble), properties 8–15 occupy bytes 4–7.
/// This grouping puts combat properties (0–3) in the first two bytes for
/// fast access on constrained platforms.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Property {
    // Combat properties (bytes 0–1)
    Sharp = 0, // Cutting damage potential
    Hard = 1,  // Structural integrity, blunt damage, defense
    Heavy = 2, // Mass, knockback
    Swift = 3, // Speed, turn advantage

    // Elemental properties (bytes 1–2, partial overlap)
    Hot = 4,  // Fire/heat energy
    Cold = 5, // Ice/frost energy
    Wet = 6,  // Water content

    // Material properties (bytes 3–4)
    Metal = 7,   // Metallic composition
    Organic = 8, // Plant/animal material, flammable

    // Effect properties (bytes 4–5)
    Venomous = 9,  // Poison/toxin
    Magical = 10,  // Arcane energy (multiplier property)
    Volatile = 11, // Explosive/unstable

    // Utility/special properties (bytes 6–7)
    Bright = 12,    // Light emission
    Corrosive = 13, // Acid, degrades materials
    Binding = 14,   // Adhesion, slowing
    Cursed = 15,    // Dark energy, risk/reward
}

/// All properties, for iteration. Mirrors the `ALL_KINDS` pattern in items/monsters.
pub const ALL_PROPERTIES: [Property; 16] = [
    Property::Sharp,
    Property::Hard,
    Property::Heavy,
    Property::Swift,
    Property::Hot,
    Property::Cold,
    Property::Wet,
    Property::Metal,
    Property::Organic,
    Property::Venomous,
    Property::Magical,
    Property::Volatile,
    Property::Bright,
    Property::Corrosive,
    Property::Binding,
    Property::Cursed,
];

/// Number of properties.
pub const PROPERTY_COUNT: usize = ALL_PROPERTIES.len();

/// 16 nibble-packed properties in 8 bytes.
///
/// Layout: byte `i` holds properties `2*i` (high nibble) and `2*i+1` (low nibble).
/// Example: byte 0 = `(Sharp << 4) | Hard`.
pub type PropertyBag = [u8; 8];

/// An empty property bag (all zeros).
pub const EMPTY: PropertyBag = [0u8; 8];

/// Read a nibble by raw index (0–15). Single source of truth for the
/// nibble layout — `get()` delegates here, and `interactions.rs` calls
/// this directly to avoid `u8 → Property` enum conversion.
pub const fn get_by_index(bag: &PropertyBag, idx: u8) -> u8 {
    let byte = bag[(idx / 2) as usize];
    if idx & 1 == 0 { byte >> 4 } else { byte & 0x0F }
}

/// Write a nibble by raw index (0–15). Clamps values above 15.
pub fn set_by_index(bag: &mut PropertyBag, idx: u8, val: u8) {
    let val = if val > 15 { 15 } else { val };
    let byte = &mut bag[(idx / 2) as usize];
    if idx & 1 == 0 {
        *byte = (*byte & 0x0F) | (val << 4);
    } else {
        *byte = (*byte & 0xF0) | val;
    }
}

/// Read a property's intensity (0–15) from a nibble-packed bag.
pub const fn get(bag: &PropertyBag, prop: Property) -> u8 {
    get_by_index(bag, prop as u8)
}

/// Write a property's intensity (0–15) into a nibble-packed bag.
/// Values above 15 are clamped.
pub fn set(bag: &mut PropertyBag, prop: Property, val: u8) {
    set_by_index(bag, prop as u8, val);
}

/// Count non-zero properties in a bag.
pub const fn count_nonzero(bag: &PropertyBag) -> u8 {
    let mut count = 0u8;
    let mut i = 0usize;
    while i < 8 {
        let byte = bag[i];
        if byte >> 4 != 0 {
            count += 1;
        }
        if byte & 0x0F != 0 {
            count += 1;
        }
        i += 1;
    }
    count
}

/// Sum of all property intensities in a bag (for carrying capacity checks).
pub const fn total_intensity(bag: &PropertyBag) -> u16 {
    let mut total = 0u16;
    let mut i = 0usize;
    while i < 8 {
        let byte = bag[i];
        total += (byte >> 4) as u16;
        total += (byte & 0x0F) as u16;
        i += 1;
    }
    total
}

/// 3-letter shorthand for display (matches the design document).
pub const fn short_name(prop: Property) -> &'static str {
    match prop {
        Property::Sharp => "SHP",
        Property::Hard => "HRD",
        Property::Heavy => "HVY",
        Property::Swift => "SWF",
        Property::Hot => "HOT",
        Property::Cold => "CLD",
        Property::Wet => "WET",
        Property::Metal => "MTL",
        Property::Organic => "ORG",
        Property::Venomous => "VNM",
        Property::Magical => "MAG",
        Property::Volatile => "VOL",
        Property::Bright => "BRT",
        Property::Corrosive => "CRS",
        Property::Binding => "BND",
        Property::Cursed => "CSD",
    }
}

/// Look up a Property by lowercase name (e.g., "sharp", "hot", "corrosive").
/// Case-insensitive, const fn with byte-by-byte comparison.
pub const fn from_name(s: &str) -> Option<Property> {
    // Use byte comparison for const fn compatibility.
    let b = s.as_bytes();
    match b.len() {
        3 => {
            if ci_eq(b, b"hot") {
                return Some(Property::Hot);
            }
            if ci_eq(b, b"wet") {
                return Some(Property::Wet);
            }
        }
        4 => {
            if ci_eq(b, b"hard") {
                return Some(Property::Hard);
            }
            if ci_eq(b, b"cold") {
                return Some(Property::Cold);
            }
        }
        5 => {
            if ci_eq(b, b"sharp") {
                return Some(Property::Sharp);
            }
            if ci_eq(b, b"heavy") {
                return Some(Property::Heavy);
            }
            if ci_eq(b, b"swift") {
                return Some(Property::Swift);
            }
            if ci_eq(b, b"metal") {
                return Some(Property::Metal);
            }
        }
        6 => {
            if ci_eq(b, b"cursed") {
                return Some(Property::Cursed);
            }
            if ci_eq(b, b"bright") {
                return Some(Property::Bright);
            }
        }
        7 => {
            if ci_eq(b, b"organic") {
                return Some(Property::Organic);
            }
            if ci_eq(b, b"magical") {
                return Some(Property::Magical);
            }
            if ci_eq(b, b"binding") {
                return Some(Property::Binding);
            }
        }
        8 => {
            if ci_eq(b, b"venomous") {
                return Some(Property::Venomous);
            }
            if ci_eq(b, b"volatile") {
                return Some(Property::Volatile);
            }
        }
        9 if ci_eq(b, b"corrosive") => {
            return Some(Property::Corrosive);
        }
        _ => {}
    }
    None
}

/// Case-insensitive byte comparison (const fn compatible).
const fn ci_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0;
    while i < a.len() {
        let ca = if a[i] >= b'A' && a[i] <= b'Z' {
            a[i] + 32
        } else {
            a[i]
        };
        let cb = if b[i] >= b'A' && b[i] <= b'Z' {
            b[i] + 32
        } else {
            b[i]
        };
        if ca != cb {
            return false;
        }
        i += 1;
    }
    true
}

/// Format a property bag as "SHP:6 HRD:7 MTL:8" (non-zero properties only).
/// Returns the number of bytes written to `buf`.
pub fn format_bag(bag: &PropertyBag, buf: &mut [u8]) -> usize {
    let mut pos = 0;
    for &prop in &ALL_PROPERTIES {
        let val = get(bag, prop);
        if val == 0 {
            continue;
        }
        if pos > 0 && pos < buf.len() {
            buf[pos] = b' ';
            pos += 1;
        }
        let label = short_name(prop).as_bytes();
        for &b in label {
            if pos < buf.len() {
                buf[pos] = b;
                pos += 1;
            }
        }
        if pos < buf.len() {
            buf[pos] = b':';
            pos += 1;
        }
        if val >= 10 && pos < buf.len() {
            buf[pos] = b'0' + val / 10;
            pos += 1;
        }
        if pos < buf.len() {
            buf[pos] = b'0' + val % 10;
            pos += 1;
        }
    }
    pos
}

// Compile-time guarantees.
const _: () = assert!(size_of::<Property>() == 1);
const _: () = assert!(size_of::<PropertyBag>() == 8);
const _: () = assert!(PROPERTY_COUNT == 16);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_bag_is_all_zeros() {
        for prop in ALL_PROPERTIES {
            assert_eq!(get(&EMPTY, prop), 0);
        }
    }

    #[test]
    fn set_and_get_roundtrip_all_properties() {
        for prop in ALL_PROPERTIES {
            let mut bag = EMPTY;
            set(&mut bag, prop, 7);
            assert_eq!(get(&bag, prop), 7, "roundtrip failed for {:?}", prop);
            // Other properties should still be zero.
            for other in ALL_PROPERTIES {
                if other != prop {
                    assert_eq!(get(&bag, other), 0, "{:?} leaked into {:?}", prop, other);
                }
            }
        }
    }

    #[test]
    fn set_clamps_above_15() {
        let mut bag = EMPTY;
        set(&mut bag, Property::Sharp, 255);
        assert_eq!(get(&bag, Property::Sharp), 15);
    }

    #[test]
    fn high_and_low_nibble_coexist() {
        let mut bag = EMPTY;
        set(&mut bag, Property::Sharp, 10); // high nibble of byte 0
        set(&mut bag, Property::Hard, 5); // low nibble of byte 0
        assert_eq!(get(&bag, Property::Sharp), 10);
        assert_eq!(get(&bag, Property::Hard), 5);
        assert_eq!(bag[0], 0xA5);
    }

    #[test]
    fn overwrite_preserves_neighbor() {
        let mut bag = EMPTY;
        set(&mut bag, Property::Sharp, 12);
        set(&mut bag, Property::Hard, 3);
        // Overwrite Sharp, Hard should survive.
        set(&mut bag, Property::Sharp, 7);
        assert_eq!(get(&bag, Property::Sharp), 7);
        assert_eq!(get(&bag, Property::Hard), 3);
    }

    #[test]
    fn count_nonzero_counts_correctly() {
        let mut bag = EMPTY;
        assert_eq!(count_nonzero(&bag), 0);
        set(&mut bag, Property::Sharp, 6);
        assert_eq!(count_nonzero(&bag), 1);
        set(&mut bag, Property::Metal, 8);
        assert_eq!(count_nonzero(&bag), 2);
        set(&mut bag, Property::Hard, 7); // same byte as Sharp
        assert_eq!(count_nonzero(&bag), 3);
    }

    #[test]
    fn total_intensity_sums_all() {
        let mut bag = EMPTY;
        set(&mut bag, Property::Sharp, 6);
        set(&mut bag, Property::Hard, 7);
        set(&mut bag, Property::Heavy, 4);
        set(&mut bag, Property::Metal, 8);
        assert_eq!(total_intensity(&bag), 25);
    }

    #[test]
    fn repr_u8_discriminants() {
        assert_eq!(Property::Sharp as u8, 0);
        assert_eq!(Property::Hard as u8, 1);
        assert_eq!(Property::Heavy as u8, 2);
        assert_eq!(Property::Swift as u8, 3);
        assert_eq!(Property::Hot as u8, 4);
        assert_eq!(Property::Cold as u8, 5);
        assert_eq!(Property::Wet as u8, 6);
        assert_eq!(Property::Metal as u8, 7);
        assert_eq!(Property::Organic as u8, 8);
        assert_eq!(Property::Venomous as u8, 9);
        assert_eq!(Property::Magical as u8, 10);
        assert_eq!(Property::Volatile as u8, 11);
        assert_eq!(Property::Bright as u8, 12);
        assert_eq!(Property::Corrosive as u8, 13);
        assert_eq!(Property::Binding as u8, 14);
        assert_eq!(Property::Cursed as u8, 15);
    }

    #[test]
    fn all_properties_covers_every_variant() {
        let mut count = 0usize;
        for &prop in &ALL_PROPERTIES {
            match prop {
                Property::Sharp
                | Property::Hard
                | Property::Heavy
                | Property::Swift
                | Property::Hot
                | Property::Cold
                | Property::Wet
                | Property::Metal
                | Property::Organic
                | Property::Venomous
                | Property::Magical
                | Property::Volatile
                | Property::Bright
                | Property::Corrosive
                | Property::Binding
                | Property::Cursed => {}
            }
            count += 1;
        }
        assert_eq!(count, PROPERTY_COUNT);
    }

    #[test]
    fn max_total_intensity() {
        // All 16 properties at max (15) = 240.
        let bag: PropertyBag = [0xFF; 8];
        assert_eq!(total_intensity(&bag), 240);
    }

    #[test]
    fn from_name_roundtrips_all_properties() {
        for &prop in &ALL_PROPERTIES {
            let name = format!("{:?}", prop).to_ascii_lowercase();
            assert_eq!(
                from_name(&name),
                Some(prop),
                "from_name({:?}) should return {:?}",
                name,
                prop
            );
        }
    }

    #[test]
    fn from_name_case_insensitive() {
        assert_eq!(from_name("SHARP"), Some(Property::Sharp));
        assert_eq!(from_name("Hot"), Some(Property::Hot));
        assert_eq!(from_name("corrosive"), Some(Property::Corrosive));
    }

    #[test]
    fn from_name_unknown() {
        assert_eq!(from_name("fire"), None);
        assert_eq!(from_name(""), None);
    }

    #[test]
    fn format_bag_shows_nonzero() {
        let mut bag = EMPTY;
        set(&mut bag, Property::Sharp, 6);
        set(&mut bag, Property::Hard, 7);
        set(&mut bag, Property::Metal, 8);
        let mut buf = [0u8; 80];
        let len = format_bag(&bag, &mut buf);
        let s = core::str::from_utf8(&buf[..len]).unwrap();
        assert!(s.contains("SHP:6"));
        assert!(s.contains("HRD:7"));
        assert!(s.contains("MTL:8"));
    }
}

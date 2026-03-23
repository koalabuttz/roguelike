//! Property system for emergent item interactions.
//!
//! Items are composed of 16 quantitative properties stored as nibbles
//! (4-bit values, range 0–15) packed into 8 bytes. This module defines
//! the property enum, the `PropertyBag` type, and nibble access helpers.
//!
//! **Step 1 (current):** Properties are defined and assigned to items via
//! `default_properties()` in `items.rs`, but nothing reads them at runtime.
//! Combat still uses `const fn attack_bonus(kind)`. The property data exists
//! as validated, tested dead code.
//!
//! **Step 2 (future):** An interaction engine in `interactions.rs` will
//! operate on property bags, producing emergent item behaviors.
//!
//! **Step 3 (future):** Combat reads from property bags instead of
//! `const fn(ItemKind)`.

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

/// Read a property's intensity (0–15) from a nibble-packed bag.
pub const fn get(bag: &PropertyBag, prop: Property) -> u8 {
    let idx = prop as u8;
    let byte = bag[(idx / 2) as usize];
    if idx & 1 == 0 {
        byte >> 4 // high nibble for even indices
    } else {
        byte & 0x0F // low nibble for odd indices
    }
}

/// Write a property's intensity (0–15) into a nibble-packed bag.
/// Values above 15 are clamped.
pub fn set(bag: &mut PropertyBag, prop: Property, val: u8) {
    let val = if val > 15 { 15 } else { val };
    let idx = prop as u8;
    let byte = &mut bag[(idx / 2) as usize];
    if idx & 1 == 0 {
        *byte = (*byte & 0x0F) | (val << 4); // set high nibble
    } else {
        *byte = (*byte & 0xF0) | val; // set low nibble
    }
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
}

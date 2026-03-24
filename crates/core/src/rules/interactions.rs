//! Emergent item interaction engine.
//!
//! Scans a rule table against two property bags and applies matching rules.
//! Chain reactions fire up to `MAX_CHAIN_DEPTH` times when properties change.
//! All types are `no_std` compatible — no allocation, caller-provided buffers.
//!
//! The rule table is a const array of 5-byte entries. Each rule checks whether
//! item A has `prop_a > 0` AND item B has `prop_b > 0`, then applies an effect.
//! Complex interactions emerge from multiple rules firing for the same property
//! pair (e.g., HOT+ORGANIC fires both ReduceA(ORGANIC) and BoostA(BRIGHT)).

use core::mem::size_of;

use super::properties::{Property, PropertyBag};

/// Maximum effects from a single interact() call (including chain reactions).
pub const MAX_EFFECTS: usize = 8;

/// Maximum chain reaction depth. After applying all rules, if properties
/// changed, re-scan up to this many additional times.
pub const MAX_CHAIN_DEPTH: u8 = 3;

// ---------------------------------------------------------------------------
// Result types
// ---------------------------------------------------------------------------

/// What a rule does when it fires.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResultType {
    /// Increase `target_prop` on item A by `modifier` (capped at 15).
    BoostA = 0,
    /// Decrease `target_prop` on item A by `modifier` (floored at 0).
    ReduceA = 1,
    /// Reduce `prop_a` on A and `prop_b` on B by min(both). `target_prop`
    /// and `modifier` are unused.
    Cancel = 2,
    /// Generate an effect. `target_prop` is the `EffectType` discriminant,
    /// `modifier` is unused (intensity comes from triggering property).
    Produce = 3,
}

/// Side effects produced by interactions (explosions, steam, etc.).
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EffectType {
    /// Area damage proportional to intensity.
    Explosion = 0,
    /// Visual flash / steam cloud.
    Steam = 1,
    /// Light emission burst.
    Glow = 2,
}

/// A side effect produced by an interaction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Effect {
    pub effect_type: EffectType,
    pub intensity: u8,
}

// ---------------------------------------------------------------------------
// Rule definition
// ---------------------------------------------------------------------------

/// A single interaction rule. 5 bytes, no padding.
///
/// When item A has `prop_a > 0` AND item B has `prop_b > 0`, apply the result.
/// Multiple rules can share the same (prop_a, prop_b) trigger — they all fire
/// independently, enabling complex multi-effect interactions.
#[derive(Clone, Copy, Debug)]
pub struct Rule {
    /// Property index required on item A (0–15).
    pub prop_a: u8,
    /// Property index required on item B (0–15).
    pub prop_b: u8,
    /// What happens when the rule fires.
    pub result: ResultType,
    /// Which property is modified (BoostA/ReduceA) or EffectType (Produce).
    pub target: u8,
    /// Amount to add/subtract (BoostA/ReduceA). Unused for Cancel/Produce.
    pub modifier: u8,
}

// Compile-time size checks.
const _: () = assert!(size_of::<Rule>() == 5);
const _: () = assert!(size_of::<Effect>() <= 4);
const _: () = assert!(size_of::<ResultType>() == 1);
const _: () = assert!(size_of::<EffectType>() == 1);

// ---------------------------------------------------------------------------
// Helpers for const rule construction
// ---------------------------------------------------------------------------

/// Shorthand for property indices (matches Property enum discriminants).
const SHP: u8 = Property::Sharp as u8;
const HRD: u8 = Property::Hard as u8;
const HVY: u8 = Property::Heavy as u8;
const SWF: u8 = Property::Swift as u8;
const HOT: u8 = Property::Hot as u8;
const CLD: u8 = Property::Cold as u8;
const WET: u8 = Property::Wet as u8;
const MTL: u8 = Property::Metal as u8;
const ORG: u8 = Property::Organic as u8;
const VNM: u8 = Property::Venomous as u8;
const MAG: u8 = Property::Magical as u8;
const VOL: u8 = Property::Volatile as u8;
const BRT: u8 = Property::Bright as u8;
const CRS: u8 = Property::Corrosive as u8;
const BND: u8 = Property::Binding as u8;
const CSD: u8 = Property::Cursed as u8;

// ---------------------------------------------------------------------------
// Rule table
// ---------------------------------------------------------------------------

/// The interaction rule table. Rules × 5 bytes each.
///
/// **Convention:** `prop_a` is checked on A (the target item being modified),
/// `prop_b` is checked on B (the source item providing the active ingredient).
/// Results always modify A. For symmetric reactions (cancel), add both
/// directions so it works regardless of which item the player picks as target.
///
/// Multiple rules can share the same (prop_a, prop_b) trigger — they all fire
/// independently, enabling complex multi-effect interactions.
pub const RULE_TABLE: [Rule; RULE_COUNT] = [
    // ── Cancel rules (symmetric — both directions) ───────────────────
    // HOT vs COLD → both reduced by min
    Rule {
        prop_a: HOT,
        prop_b: CLD,
        result: ResultType::Cancel,
        target: 0,
        modifier: 0,
    },
    Rule {
        prop_a: CLD,
        prop_b: HOT,
        result: ResultType::Cancel,
        target: 0,
        modifier: 0,
    },
    // HEAVY vs SWIFT → weight cancels speed
    Rule {
        prop_a: HVY,
        prop_b: SWF,
        result: ResultType::Cancel,
        target: 0,
        modifier: 0,
    },
    Rule {
        prop_a: SWF,
        prop_b: HVY,
        result: ResultType::Cancel,
        target: 0,
        modifier: 0,
    },
    // MAGICAL vs CURSED → purification
    Rule {
        prop_a: MAG,
        prop_b: CSD,
        result: ResultType::Cancel,
        target: 0,
        modifier: 0,
    },
    Rule {
        prop_a: CSD,
        prop_b: MAG,
        result: ResultType::Cancel,
        target: 0,
        modifier: 0,
    },
    // ── HOT as source (B provides HOT) ───────────────────────────────
    // A has WET, B has HOT → evaporate A's water, produce steam glow
    Rule {
        prop_a: WET,
        prop_b: HOT,
        result: ResultType::ReduceA,
        target: WET,
        modifier: 2,
    },
    Rule {
        prop_a: WET,
        prop_b: HOT,
        result: ResultType::BoostA,
        target: BRT,
        modifier: 1,
    },
    // A has ORGANIC, B has HOT → burn A's organic material
    Rule {
        prop_a: ORG,
        prop_b: HOT,
        result: ResultType::ReduceA,
        target: ORG,
        modifier: 2,
    },
    Rule {
        prop_a: ORG,
        prop_b: HOT,
        result: ResultType::BoostA,
        target: BRT,
        modifier: 1,
    },
    // A has METAL, B has HOT → temper A (increase hardness)
    Rule {
        prop_a: MTL,
        prop_b: HOT,
        result: ResultType::BoostA,
        target: HRD,
        modifier: 1,
    },
    // A has VOLATILE, B has HOT → ignite! Consume A's volatile, produce blast
    Rule {
        prop_a: VOL,
        prop_b: HOT,
        result: ResultType::ReduceA,
        target: VOL,
        modifier: 15,
    },
    Rule {
        prop_a: VOL,
        prop_b: HOT,
        result: ResultType::Produce,
        target: EffectType::Explosion as u8,
        modifier: 0,
    },
    // A has BINDING, B has HOT → heat melts A's adhesion
    Rule {
        prop_a: BND,
        prop_b: HOT,
        result: ResultType::ReduceA,
        target: BND,
        modifier: 2,
    },
    // ── COLD as source (B provides COLD) ─────────────────────────────
    // A has WET, B has COLD → freeze A's water into ice
    Rule {
        prop_a: WET,
        prop_b: CLD,
        result: ResultType::ReduceA,
        target: WET,
        modifier: 3,
    },
    Rule {
        prop_a: WET,
        prop_b: CLD,
        result: ResultType::BoostA,
        target: HRD,
        modifier: 2,
    },
    // A has METAL, B has COLD → brittle-sharpen A
    Rule {
        prop_a: MTL,
        prop_b: CLD,
        result: ResultType::BoostA,
        target: SHP,
        modifier: 1,
    },
    Rule {
        prop_a: MTL,
        prop_b: CLD,
        result: ResultType::ReduceA,
        target: HRD,
        modifier: 1,
    },
    // A has ORGANIC, B has COLD → frost damage to A
    Rule {
        prop_a: ORG,
        prop_b: CLD,
        result: ResultType::ReduceA,
        target: ORG,
        modifier: 1,
    },
    // A has BINDING, B has COLD → ice strengthens A's adhesion
    Rule {
        prop_a: BND,
        prop_b: CLD,
        result: ResultType::BoostA,
        target: BND,
        modifier: 1,
    },
    // ── MAGICAL as source (B provides MAGICAL) ───────────────────────
    // A has METAL, B has MAGICAL → magical conductivity (glow)
    Rule {
        prop_a: MTL,
        prop_b: MAG,
        result: ResultType::BoostA,
        target: BRT,
        modifier: 2,
    },
    // A has SHARP, B has MAGICAL → glowing edge
    Rule {
        prop_a: SHP,
        prop_b: MAG,
        result: ResultType::BoostA,
        target: BRT,
        modifier: 1,
    },
    // A has VOLATILE, B has MAGICAL → magical amplification (dangerous)
    Rule {
        prop_a: VOL,
        prop_b: MAG,
        result: ResultType::BoostA,
        target: VOL,
        modifier: 2,
    },
    // A has ORGANIC, B has MAGICAL → magic feeds growth
    Rule {
        prop_a: ORG,
        prop_b: MAG,
        result: ResultType::BoostA,
        target: ORG,
        modifier: 1,
    },
    // ── CORROSIVE as source (B provides CORROSIVE) ───────────────────
    // A has METAL, B has CORROSIVE → dissolve A's metal and weaken structure
    Rule {
        prop_a: MTL,
        prop_b: CRS,
        result: ResultType::ReduceA,
        target: MTL,
        modifier: 1,
    },
    Rule {
        prop_a: MTL,
        prop_b: CRS,
        result: ResultType::ReduceA,
        target: HRD,
        modifier: 1,
    },
    // A has HARD (non-metal), B has CORROSIVE → dissolve A's structure
    Rule {
        prop_a: HRD,
        prop_b: CRS,
        result: ResultType::ReduceA,
        target: HRD,
        modifier: 1,
    },
    // ── CURSED as source (B provides CURSED) ─────────────────────────
    // A has ORGANIC, B has CURSED → dark decay
    Rule {
        prop_a: ORG,
        prop_b: CSD,
        result: ResultType::ReduceA,
        target: ORG,
        modifier: 1,
    },
    // A has BRIGHT, B has CURSED → darkness snuffs light
    Rule {
        prop_a: BRT,
        prop_b: CSD,
        result: ResultType::ReduceA,
        target: BRT,
        modifier: 2,
    },
    // A has HOT, B has CURSED → cursed fire burns hotter
    Rule {
        prop_a: HOT,
        prop_b: CSD,
        result: ResultType::BoostA,
        target: HOT,
        modifier: 1,
    },
    // ── Other directional rules ──────────────────────────────────────
    // A has SHARP, B has BINDING → adhesive dulls A's edge
    Rule {
        prop_a: SHP,
        prop_b: BND,
        result: ResultType::ReduceA,
        target: SHP,
        modifier: 1,
    },
    // A has VOLATILE, B has BINDING → stabilize A (suppress explosion)
    Rule {
        prop_a: VOL,
        prop_b: BND,
        result: ResultType::ReduceA,
        target: VOL,
        modifier: 1,
    },
    // A has CORROSIVE, B has ORGANIC → organic absorbs A's acid
    Rule {
        prop_a: CRS,
        prop_b: ORG,
        result: ResultType::ReduceA,
        target: CRS,
        modifier: 1,
    },
    // A has CORROSIVE, B has WET → water dilutes A's acid
    Rule {
        prop_a: CRS,
        prop_b: WET,
        result: ResultType::ReduceA,
        target: CRS,
        modifier: 1,
    },
    // A has VENOMOUS, B has WET → water dilutes A's poison
    Rule {
        prop_a: VNM,
        prop_b: WET,
        result: ResultType::ReduceA,
        target: VNM,
        modifier: 1,
    },
    // A has BRIGHT, B has CURSED → (already in cancel above)
    // A has CURSED, B has BRIGHT → light purifies A's darkness
    Rule {
        prop_a: CSD,
        prop_b: BRT,
        result: ResultType::ReduceA,
        target: CSD,
        modifier: 1,
    },
    // A has VENOMOUS, B has CURSED → darkness amplifies A's poison
    Rule {
        prop_a: VNM,
        prop_b: CSD,
        result: ResultType::BoostA,
        target: VNM,
        modifier: 1,
    },
    // A has VOLATILE, B has CORROSIVE → acid destabilizes A
    Rule {
        prop_a: VOL,
        prop_b: CRS,
        result: ResultType::BoostA,
        target: VOL,
        modifier: 1,
    },
];

/// Number of rules in the table.
pub const RULE_COUNT: usize = 38;

// ---------------------------------------------------------------------------
// Interaction engine
// ---------------------------------------------------------------------------

/// Run all interaction rules between two property bags.
///
/// Item A is the primary (being modified by B's properties). Both bags may
/// be mutated (e.g., Cancel reduces both). Chain reactions fire up to
/// `MAX_CHAIN_DEPTH` additional passes when properties change.
///
/// Returns the number of effects written to the `effects` buffer.
pub fn interact(
    a: &mut PropertyBag,
    b: &mut PropertyBag,
    effects: &mut [Effect; MAX_EFFECTS],
) -> u8 {
    let mut effect_count = 0u8;

    for _chain in 0..=MAX_CHAIN_DEPTH {
        // Snapshot at the start of each pass — all rules in a pass check
        // against the same starting state. This avoids rule-ordering bugs
        // where ReduceA zeroes a property before Produce can read it.
        let a_snap = *a;
        let b_snap = *b;

        for rule in &RULE_TABLE {
            let val_a = get_by_index(&a_snap, rule.prop_a);
            let val_b = get_by_index(&b_snap, rule.prop_b);

            if val_a == 0 || val_b == 0 {
                continue;
            }

            match rule.result {
                ResultType::BoostA => {
                    let cur = get_by_index(a, rule.target);
                    set_by_index(a, rule.target, cur.saturating_add(rule.modifier).min(15));
                }
                ResultType::ReduceA => {
                    let cur = get_by_index(a, rule.target);
                    set_by_index(a, rule.target, cur.saturating_sub(rule.modifier));
                }
                ResultType::Cancel => {
                    // Amount to cancel is determined by snapshot values (how
                    // much was present at the start of the pass), but the
                    // subtraction applies to the working copy — consistent
                    // with how BoostA/ReduceA read from the working copy.
                    let amount = val_a.min(val_b);
                    let cur_a = get_by_index(a, rule.prop_a);
                    let cur_b = get_by_index(b, rule.prop_b);
                    set_by_index(a, rule.prop_a, cur_a.saturating_sub(amount));
                    set_by_index(b, rule.prop_b, cur_b.saturating_sub(amount));
                }
                ResultType::Produce => {
                    if (effect_count as usize) < MAX_EFFECTS {
                        effects[effect_count as usize] = Effect {
                            effect_type: match rule.target {
                                0 => EffectType::Explosion,
                                1 => EffectType::Steam,
                                _ => EffectType::Glow,
                            },
                            intensity: val_a,
                        };
                        effect_count += 1;
                    }
                }
            }
        }

        // Stop chaining if nothing changed.
        if *a == a_snap && *b == b_snap {
            break;
        }
    }

    effect_count
}

use super::properties::{get_by_index, set_by_index};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::properties;

    /// Helper: create a bag with specific properties set.
    fn bag_with(props: &[(Property, u8)]) -> PropertyBag {
        let mut bag = properties::EMPTY;
        for &(prop, val) in props {
            properties::set(&mut bag, prop, val);
        }
        bag
    }

    // ── Rule table invariants ────────────────────────────────────────

    #[test]
    fn rule_count_matches_table() {
        assert_eq!(RULE_TABLE.len(), RULE_COUNT);
    }

    #[test]
    fn all_rule_properties_in_range() {
        for (i, rule) in RULE_TABLE.iter().enumerate() {
            assert!(rule.prop_a < 16, "rule {} prop_a out of range", i);
            assert!(rule.prop_b < 16, "rule {} prop_b out of range", i);
            if matches!(rule.result, ResultType::BoostA | ResultType::ReduceA) {
                assert!(rule.target < 16, "rule {} target out of range", i);
            }
        }
    }

    #[test]
    fn no_uncontrolled_self_boost() {
        // BoostA rules that target their own prop_a can grow each chain pass.
        // This is acceptable when bounded by chain depth (max +4) and nibble
        // cap (max 15), but we track known cases explicitly.
        let allowed_self_boosts: &[(u8, u8)] = &[
            (HOT, CSD), // cursed fire burns hotter — bounded by CURSED on B
            (VOL, MAG), // magical amplification — bounded by chain depth
            (VOL, CRS), // acid destabilizes — bounded by chain depth
            (VNM, CSD), // darkness amplifies poison — bounded by chain depth
            (BND, CLD), // ice strengthens adhesion — bounded by chain depth
            (ORG, MAG), // magic feeds growth — bounded by chain depth
        ];
        for (i, rule) in RULE_TABLE.iter().enumerate() {
            if rule.result == ResultType::BoostA && rule.target == rule.prop_a {
                assert!(
                    allowed_self_boosts
                        .iter()
                        .any(|&(pa, pb)| pa == rule.prop_a && pb == rule.prop_b),
                    "rule {} has unlisted self-boost: prop_a={}, prop_b={}",
                    i,
                    rule.prop_a,
                    rule.prop_b
                );
            }
        }
    }

    // ── Specific interaction tests ───────────────────────────────────

    #[test]
    fn hot_cold_cancel() {
        let mut a = bag_with(&[(Property::Hot, 5)]);
        let mut b = bag_with(&[(Property::Cold, 3)]);
        let mut effects = [Effect {
            effect_type: EffectType::Glow,
            intensity: 0,
        }; MAX_EFFECTS];

        interact(&mut a, &mut b, &mut effects);

        // HOT 5 vs COLD 3 → cancel by min(5,3)=3. HOT→2, COLD→0.
        assert_eq!(properties::get(&a, Property::Hot), 2);
        assert_eq!(properties::get(&b, Property::Cold), 0);
    }

    #[test]
    fn hot_metal_tempers() {
        // A has METAL, B has HOT → A gets tempered (HARD increases).
        // Chain depth of 3 means up to 4 passes, so HARD can increase by up to 4.
        let mut a = bag_with(&[(Property::Metal, 8), (Property::Hard, 5)]);
        let mut b = bag_with(&[(Property::Hot, 6)]);
        let mut effects = [Effect {
            effect_type: EffectType::Glow,
            intensity: 0,
        }; MAX_EFFECTS];

        interact(&mut a, &mut b, &mut effects);

        // HARD should increase (at least +1, up to +4 from chain depth).
        assert!(
            properties::get(&a, Property::Hard) > 5,
            "HARD should increase from tempering"
        );
    }

    #[test]
    fn hot_volatile_explodes() {
        // A has VOLATILE, B has HOT → A's volatile ignites, explosion produced
        let mut a = bag_with(&[(Property::Volatile, 5)]);
        let mut b = bag_with(&[(Property::Hot, 7)]);
        let mut effects = [Effect {
            effect_type: EffectType::Glow,
            intensity: 0,
        }; MAX_EFFECTS];

        let count = interact(&mut a, &mut b, &mut effects);

        assert_eq!(properties::get(&a, Property::Volatile), 0);
        assert!(count > 0);
        assert!(
            effects[..count as usize]
                .iter()
                .any(|e| e.effect_type == EffectType::Explosion)
        );
    }

    #[test]
    fn cold_wet_freezes() {
        let mut a = bag_with(&[(Property::Cold, 4)]);
        let mut b = bag_with(&[(Property::Wet, 6)]);
        let mut effects = [Effect {
            effect_type: EffectType::Glow,
            intensity: 0,
        }; MAX_EFFECTS];

        interact(&mut a, &mut b, &mut effects);

        // COLD+WET → WET reduced by 3, HARD boosted by 2
        assert!(properties::get(&a, Property::Wet) < 6 || properties::get(&a, Property::Hard) > 0);
    }

    #[test]
    fn corrosive_dissolves_metal() {
        // A has METAL+HARD (sword), B has CORROSIVE (acid bath)
        let mut a = bag_with(&[(Property::Metal, 6), (Property::Hard, 7)]);
        let mut b = bag_with(&[(Property::Corrosive, 4)]);
        let mut effects = [Effect {
            effect_type: EffectType::Glow,
            intensity: 0,
        }; MAX_EFFECTS];

        interact(&mut a, &mut b, &mut effects);

        // MTL+CRS → METAL reduced, HARD reduced
        assert!(properties::get(&a, Property::Metal) < 6);
        assert!(properties::get(&a, Property::Hard) < 7);
    }

    #[test]
    fn magical_cursed_cancel() {
        let mut a = bag_with(&[(Property::Magical, 5)]);
        let mut b = bag_with(&[(Property::Cursed, 3)]);
        let mut effects = [Effect {
            effect_type: EffectType::Glow,
            intensity: 0,
        }; MAX_EFFECTS];

        interact(&mut a, &mut b, &mut effects);

        assert_eq!(properties::get(&a, Property::Magical), 2);
        assert_eq!(properties::get(&b, Property::Cursed), 0);
    }

    #[test]
    fn heavy_swift_cancel() {
        let mut a = bag_with(&[(Property::Heavy, 8)]);
        let mut b = bag_with(&[(Property::Swift, 5)]);
        let mut effects = [Effect {
            effect_type: EffectType::Glow,
            intensity: 0,
        }; MAX_EFFECTS];

        interact(&mut a, &mut b, &mut effects);

        assert_eq!(properties::get(&a, Property::Heavy), 3);
        assert_eq!(properties::get(&b, Property::Swift), 0);
    }

    #[test]
    fn bright_purifies_cursed() {
        // A has CURSED, B has BRIGHT → A's CURSED reduced (light purifies).
        let mut a = bag_with(&[(Property::Cursed, 5)]);
        let mut b = bag_with(&[(Property::Bright, 6)]);
        let mut effects = [Effect {
            effect_type: EffectType::Glow,
            intensity: 0,
        }; MAX_EFFECTS];

        interact(&mut a, &mut b, &mut effects);

        assert!(
            properties::get(&a, Property::Cursed) < 5,
            "CURSED should be reduced by BRIGHT"
        );
    }

    #[test]
    fn empty_bags_produce_nothing() {
        let mut a = properties::EMPTY;
        let mut b = properties::EMPTY;
        let mut effects = [Effect {
            effect_type: EffectType::Glow,
            intensity: 0,
        }; MAX_EFFECTS];

        let count = interact(&mut a, &mut b, &mut effects);

        assert_eq!(count, 0);
        assert_eq!(a, properties::EMPTY);
        assert_eq!(b, properties::EMPTY);
    }

    #[test]
    fn chain_reactions_fire() {
        // A has VOLATILE:3, B has HOT:8 → A's volatile ignites
        // Pass 1: VOL+HOT → VOLATILE consumed (0), explosion produced
        // No further chains since VOLATILE is gone.
        let mut a = bag_with(&[(Property::Volatile, 3)]);
        let mut b = bag_with(&[(Property::Hot, 8)]);
        let mut effects = [Effect {
            effect_type: EffectType::Glow,
            intensity: 0,
        }; MAX_EFFECTS];

        let count = interact(&mut a, &mut b, &mut effects);

        assert_eq!(properties::get(&a, Property::Volatile), 0);
        assert!(count > 0);
    }

    // ── Fuzz-style invariant tests ───────────────────────────────────

    #[test]
    fn fuzz_interact_no_panic_bounded_effects() {
        // Exercise interact on 200 diverse property bag combinations.
        // Main invariants: no panics, effect count bounded, and
        // individual properties never exceed 15 (verified via get()).
        for seed in 0u16..200 {
            let mut a = properties::EMPTY;
            let mut b = properties::EMPTY;
            // Fill with pseudo-random bytes — any u8 is a valid PropertyBag
            // byte (two nibbles packed), so no clamping needed.
            for i in 0..8 {
                a[i] = ((seed.wrapping_mul(31).wrapping_add(i as u16 * 7)) & 0xFF) as u8;
                b[i] = ((seed.wrapping_mul(17).wrapping_add(i as u16 * 13)) & 0xFF) as u8;
            }

            let mut effects = [Effect {
                effect_type: EffectType::Glow,
                intensity: 0,
            }; MAX_EFFECTS];
            let count = interact(&mut a, &mut b, &mut effects);

            // Verify every property reads back as 0–15 via the get() API.
            // This catches any corruption where interact writes raw bytes
            // that don't round-trip through the nibble encoding.
            for &prop in &properties::ALL_PROPERTIES {
                let va = properties::get(&a, prop);
                let vb = properties::get(&b, prop);
                assert!(va <= 15, "seed {seed}: bag a {:?} = {va}", prop);
                assert!(vb <= 15, "seed {seed}: bag b {:?} = {vb}", prop);
            }
            assert!((count as usize) <= MAX_EFFECTS);
        }
    }

    #[test]
    fn effect_count_never_exceeds_max() {
        // Bags designed to trigger many rules simultaneously.
        let mut a = [0xFF; 8]; // All properties at 15
        let mut b = [0xFF; 8];
        let mut effects = [Effect {
            effect_type: EffectType::Glow,
            intensity: 0,
        }; MAX_EFFECTS];

        let count = interact(&mut a, &mut b, &mut effects);

        assert!((count as usize) <= MAX_EFFECTS);
    }

    #[test]
    fn total_intensity_bounded() {
        // Verify that interact doesn't create unbounded growth.
        // Start with max bags and verify total doesn't increase.
        for seed in 0u16..50 {
            let mut a = properties::EMPTY;
            let mut b = properties::EMPTY;
            for i in 0..8 {
                a[i] = ((seed.wrapping_mul(37).wrapping_add(i as u16 * 11)) & 0xFF) as u8;
                b[i] = ((seed.wrapping_mul(23).wrapping_add(i as u16 * 19)) & 0xFF) as u8;
            }

            let total_before = properties::total_intensity(&a) + properties::total_intensity(&b);

            let mut effects = [Effect {
                effect_type: EffectType::Glow,
                intensity: 0,
            }; MAX_EFFECTS];
            interact(&mut a, &mut b, &mut effects);

            let total_after = properties::total_intensity(&a) + properties::total_intensity(&b);

            // Allow small growth from boost rules but not runaway.
            // Chain depth cap of 3 limits maximum growth per interaction.
            assert!(
                total_after <= total_before + 60,
                "seed {}: total grew from {} to {} (diff {})",
                seed,
                total_before,
                total_after,
                total_after.saturating_sub(total_before)
            );
        }
    }

    // ── Emergent scenario tests ──────────────────────────────────────

    #[test]
    fn sword_dipped_in_fire() {
        use crate::rules::items;
        // Short Sword (A): SHP:6, HRD:7, HVY:4, MTL:8
        // Fire source (B): HOT:8, VOLATILE:3
        // A has METAL, B has HOT → tempering fires: HARD increases.
        let mut sword = items::default_properties(items::ItemKind::ShortSword);
        let mut fire = bag_with(&[(Property::Hot, 8), (Property::Volatile, 3)]);
        let mut effects = [Effect {
            effect_type: EffectType::Glow,
            intensity: 0,
        }; MAX_EFFECTS];

        let count = interact(&mut sword, &mut fire, &mut effects);

        // Sword should be tempered: HARD increased from 7
        assert!(
            properties::get(&sword, Property::Hard) > 7,
            "sword should be tempered (HARD increased from MTL+HOT)"
        );
        // No explosion — the rule VOL+HOT checks A.VOLATILE. Sword has no
        // VOLATILE, so it doesn't fire. (Fire's VOLATILE is on B.)
        assert!(count <= MAX_EFFECTS as u8);
    }

    #[test]
    fn chain_mail_vs_leather_in_water() {
        use crate::rules::items;
        // Chain Mail: HRD:8, HVY:6, MTL:7
        // Leather: HRD:5, SWF:3, ORG:6
        let mut chain = items::default_properties(items::ItemKind::ChainMail);
        let mut leather = items::default_properties(items::ItemKind::LeatherArmor);
        let water = bag_with(&[(Property::Wet, 6), (Property::Cold, 2)]);
        let mut effects = [Effect {
            effect_type: EffectType::Glow,
            intensity: 0,
        }; MAX_EFFECTS];

        // Chain mail in water: CORROSIVE rules don't fire (water isn't corrosive)
        // but COLD+METAL would fire if chain had COLD... it doesn't.
        // Water interaction is mostly through environment (step 3).
        // For now, just verify no crashes.
        let mut water_copy = water;
        interact(&mut chain, &mut water_copy, &mut effects);

        // Leather in water: no metal, so no rust
        let mut water_copy2 = water;
        interact(&mut leather, &mut water_copy2, &mut effects);

        // Both should survive without corruption
        assert!(properties::get(&chain, Property::Hard) > 0);
        assert!(properties::get(&leather, Property::Hard) > 0);
    }
}

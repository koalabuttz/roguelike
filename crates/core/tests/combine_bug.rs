use roguelike_core::command::GameCommand;
use roguelike_core::rules::items::{self, ItemKind};
use roguelike_core::rules::properties::{self, Property};
use roguelike_core::tier_micro::game::MicroGameState;

/// Full inventory + consumable source: consuming the source frees a slot
/// for the split target, so the combine succeeds.
#[test]
fn combine_full_inventory_succeeds_with_consumable_source() {
    let mut g = MicroGameState::new_default(42);

    // Slot 0: stack of 2 HealthPotions.
    g.inventory.add(ItemKind::HealthPotion);
    g.inventory.add(ItemKind::HealthPotion);

    // Slot 1: StrengthPotion (consumable source — consumed during combine).
    g.inventory.add(ItemKind::StrengthPotion);

    // Slots 2..26: fill with non-stackable items.
    for _ in 2..26 {
        assert!(g.inventory.add(ItemKind::LeatherArmor));
    }

    // Inventory is FULL (26 slots). The combine should succeed because
    // consuming the StrengthPotion frees slot 1 for the modified potion.
    let result = g.step(GameCommand::Combine(0, 1));
    assert!(
        result.action_taken,
        "combine should succeed — consuming source frees a slot"
    );

    // The original stack should have decremented by 1.
    let slot0 = g.inventory.get(0).expect("slot 0 should still exist");
    assert_eq!(slot0.kind, ItemKind::HealthPotion);
    assert_eq!(slot0.count, 1, "one potion was split off");

    // The StrengthPotion (source) should be consumed.
    // The modified potion should exist somewhere in the inventory.
    let mut found_source = false;
    let mut found_modified = false;
    for i in 0..26 {
        if let Some(slot) = g.inventory.get(i) {
            if slot.kind == ItemKind::StrengthPotion {
                found_source = true;
            }
            if slot.kind == ItemKind::HealthPotion && slot.props != items::default_properties(ItemKind::HealthPotion) {
                found_modified = true;
            }
        }
    }
    assert!(!found_source, "source should be consumed");
    assert!(found_modified, "modified potion should exist");
}

/// Full inventory + non-consumable source + stacked target: no slot can be
/// freed, so the combine aborts safely without losing items.
#[test]
fn combine_full_inventory_aborts_with_equipment_source() {
    let mut g = MicroGameState::new_default(42);

    // Slot 0: stack of 2 HealthPotions (target).
    g.inventory.add(ItemKind::HealthPotion);
    g.inventory.add(ItemKind::HealthPotion);

    // Slot 1: ShortSword (non-consumable source — not consumed).
    g.inventory.add(ItemKind::ShortSword);

    // Slots 2..26: fill remaining.
    for _ in 2..26 {
        assert!(g.inventory.add(ItemKind::LeatherArmor));
    }

    let result = g.step(GameCommand::Combine(0, 1));
    assert!(
        !result.action_taken,
        "combine should abort — no slot can be freed"
    );

    // Verify nothing was lost.
    let slot0 = g.inventory.get(0).expect("slot 0 should exist");
    assert_eq!(slot0.count, 2, "stack unchanged");
    let slot1 = g.inventory.get(1).expect("slot 1 should exist");
    assert_eq!(slot1.kind, ItemKind::ShortSword);
}

/// Combining a single consumable with a source that produces matching props
/// should not lose the item (the old self-cannibalization bug).
#[test]
fn combine_no_self_cannibalization() {
    let mut g = MicroGameState::new_default(42);

    // Slot 0: HealthPotion with modified props (HOT:5).
    g.inventory.add(ItemKind::HealthPotion);
    let mut hot_props = items::default_properties(ItemKind::HealthPotion);
    properties::set(&mut hot_props, Property::Hot, 5);
    g.inventory.set_props(0, hot_props);

    // Slot 1: default HealthPotion.
    g.inventory.add(ItemKind::HealthPotion);

    let total_before = count_health_potions(&g);

    // Combine slot 0 (target) with slot 1 (source).
    // The interaction may cause target props to converge toward source props.
    g.step(GameCommand::Combine(0, 1));

    let total_after = count_health_potions(&g);

    // Regardless of what the interaction does to properties, we should
    // never lose an item. At worst the combine has no effect.
    // Source is consumed (consumable), so we expect total - 1.
    assert!(
        total_after >= total_before - 1,
        "item was lost: had {total_before}, now {total_after}"
    );
}

fn count_health_potions(g: &MicroGameState) -> usize {
    let mut count = 0;
    for i in 0..26 {
        if let Some(slot) = g.inventory.get(i) {
            if slot.kind == ItemKind::HealthPotion {
                count += slot.count as usize;
            }
        }
    }
    count
}

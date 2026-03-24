use roguelike_core::command::GameCommand;
use roguelike_core::rules::items::ItemKind;
use roguelike_core::tier_micro::game::MicroGameState;

#[test]
fn combine_with_full_inventory_aborts_safely() {
    let mut g = MicroGameState::new_default(42);

    // Slot 0: stack of 2 HealthPotions.
    g.inventory.add(ItemKind::HealthPotion);
    g.inventory.add(ItemKind::HealthPotion);

    // Slot 1: StrengthPotion (consumable source).
    g.inventory.add(ItemKind::StrengthPotion);

    // Slots 2..26: fill with non-stackable items.
    for _ in 2..26 {
        assert!(g.inventory.add(ItemKind::LeatherArmor));
    }

    // Inventory is FULL (26 slots occupied). Combining a stacked target
    // would need a new slot for the modified item, which doesn't exist.
    // The combine should abort without losing any items.
    let result = g.step(GameCommand::Combine(0, 1));
    assert!(!result.action_taken, "combine should abort when inventory is full");

    // Verify no items were lost — slot 0 still has 2 potions.
    let slot0 = g.inventory.get(0).expect("slot 0 should still exist");
    assert_eq!(slot0.kind, ItemKind::HealthPotion);
    assert_eq!(slot0.count, 2, "stack should be unchanged");

    // Slot 1 still has the StrengthPotion (not consumed since combine aborted).
    let slot1 = g.inventory.get(1).expect("slot 1 should still exist");
    assert_eq!(slot1.kind, ItemKind::StrengthPotion);
}

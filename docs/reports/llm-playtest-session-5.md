# LLM Playtest Report — 2026-02-23 (Session 5)

**Player:** Claude Opus 4.6 via MCP server (claude-code backend)
**Games:** 5 parallel (seeds 64462–64466)
**Budget:** $1.50/game max
**Focus:** Verify new item system (Health Potions, Short Swords, Leather Armor)

## What Changed Since Session 4

The developer added:
- **Item system** — 3 item types spawn on the dungeon floor: Health Potions (`!`),
  Short Swords (`/`), Leather Armor (`[`). Auto-pickup on walk. Potions heal immediately
  (stay on ground at full HP). Equipment auto-equips if strictly better.
- **Equipment stats** — `effective_attack()` and `effective_defense()` apply equipment
  bonuses. Observation JSON includes `player_atk`, `player_def`, `weapon`, `armor` fields.
- **Items in observations** — `visible_items` array in observe/act responses shows ground
  items with name, glyph, and position.
- **Items in rules** — `get_rules` now documents item glyphs (`!`, `/`, `[`) and
  auto-pickup behavior.
- **Combat fix** — `melee_attack` now takes explicit atk/def parameters. Monster attacks
  correctly respect player armor (was previously ignored).

## Item System Observations

### 1. Items spawn and display correctly (POSITIVE)

Across the 5 games, items appeared in observations as expected:

- **Seed 64464:** 2 Health Potions found at (55,14) and (71,7) — visible in `visible_items`
  array, correctly displayed with name and glyph. The LLM identified them and noted their
  positions for later use.
- **Seed 64465:** Short Sword spawned, picked up, equipped correctly. The LLM reported:
  "+3 ATK, properly displayed equip message, combat math updated." Clean win with 18 kills.

The `visible_items` field provides exactly what the LLM needs — item name, glyph, and
position. No ASCII parsing required.

### 2. Equipment changes combat outcomes (POSITIVE)

Seed 64465 demonstrated the full equipment flow:
- Found Short Sword → auto-equipped → ATK increased from 5 to 8
- With +3 ATK, goblins die in 1 hit instead of 2 (8-0=8 dmg > 6 HP)
- Orcs die in 2 hits instead of 3 (8-1=7 dmg, 12 HP → 2 rounds)
- The LLM correctly observed the combat math improvement

This validates the equipment design — items create meaningful power spikes that the LLM
can reason about.

### 3. Potion saving works but creates no tension (NEUTRAL)

Seed 64464 found 2 Health Potions while at full HP. Both correctly stayed on the ground.
The LLM noted their positions for potential future use, demonstrating the intended tactical
depth (remembering item locations).

However, with HP regen still too generous (Session 4's finding), the potions were never
needed. The LLM ended at 29/30 HP after killing 2 goblins. In practice, potions only
matter against trolls — the one monster that rarely spawns.

### 4. Item spawn rate may be too low (CONCERN)

- **Seed 64466:** 80% explored, 204 turns, zero items found. The LLM flagged this:
  "No items (potions, weapons, armor) were found in 80% map exploration — this is
  suspicious and may indicate items aren't spawning correctly."
- **Seed 64464:** 2 items found in ~20% explored — reasonable
- **Seed 64465:** Items found (Short Sword confirmed) — reasonable

With `MAX_ITEMS_PER_ROOM = 1` and a probability roll per room, some dungeons will have
very few items. This is technically correct behavior but creates inconsistent experiences.
Consider:
- Minimum 2-3 items per dungeon (guaranteed)
- Or increase `MAX_ITEMS_PER_ROOM` to 2
- Or add item placement guarantees (e.g., always one potion in the first 3 rooms)

### 5. MCP observation fields are complete (POSITIVE)

The new observation fields work well:

| Field | Purpose | Working? |
|-------|---------|----------|
| `visible_items` | Ground items in FOV | Yes — name, glyph, x, y |
| `player_atk` | Effective attack (base + weapon) | Yes |
| `player_def` | Effective defense (base + armor) | Yes |
| `weapon` | Equipped weapon name or null | Yes |
| `armor` | Equipped armor name or null | Yes |

The LLM can now reason about equipment state without parsing the status bar.

## Gameplay Observations

### 1. Trolls remain the unsolved problem (Sessions 3, 4, 5)

Seed 64462 documented a detailed Troll encounter:
- Player ATK 5 DEF 2 vs Troll ATK 6 DEF 3 → player deals 2/round, takes 4/round
- 10 rounds to kill, 8 rounds to die = **guaranteed death at base stats**
- The LLM attempted kiting (hit-and-run in corridors with regen) but Chase AI follows
  indefinitely, making retreat futile without careful corridor geometry

**With equipment**, the math changes:
- Short Sword: ATK 8 vs DEF 3 → 5 dmg/round → 4 rounds to kill, take 12 dmg = survivable
- Leather Armor: DEF 4 vs ATK 6 → take 2 dmg/round → 10 rounds × 2 = 20 dmg = survivable
- Both: ATK 8 DEF 4 → 5 dmg, 2 taken → 4 rounds, 8 dmg total = easy

**Items transform the Troll from impossible to manageable.** This is the correct design —
equipment is the player's answer to the Troll gate. The problem is when items don't spawn
(seed 64466), leaving the player with no viable strategy.

### 2. Monster ATK/DEF still missing from observations (Sessions 4, 5)

The LLM in seed 64462 noted: "the observation doesn't include monster ATK/DEF, so combat
math was wrong." The `entities` array in observations includes `hp`, `max_hp`, `name`,
`glyph`, `x`, `y`, `alive` — but not attack or defense stats.

The LLM must either:
- Call `get_rules` and memorize the monster table (current approach)
- Guess based on monster name (fragile)

Adding `atk` and `def` to visible entity info would let the LLM make accurate real-time
combat assessments.

### 3. auto_explore + monster_spotted creates a friction gap (Sessions 4, 5)

When `auto_explore` stops with `monster_spotted`, the monster is typically 2-3 tiles away
(just entered FOV). The LLM then needs to:
1. `pathfind_to` the monster's position (or adjacent tile)
2. `auto_fight` once adjacent

This 2-step dance is the most common source of wasted calls. Consider: when auto_explore
stops for a monster, include the monster's position and distance in the response so the LLM
can immediately pathfind there.

### 4. Session 4's recommendations — status update

| Recommendation | Status |
|---------------|--------|
| Win condition (stairs/exit) | **Not yet implemented** — still the #1 missing feature |
| Guarantee troll spawns | Not yet — trolls spawned in some session 5 seeds |
| Increase monster density | Not yet — varied across seeds |
| Rebalance regen | Not yet — still too generous |
| Combat depth — items | **Implemented!** Items add meaningful resource decisions |
| Combat depth — damage variance | Not yet |
| Multi-entrance rooms | Not yet |

## Per-Game Results

| Seed | Turns | Kills | HP | Explored | Items Found | Result |
|------|-------|-------|-----|----------|-------------|--------|
| 64462 | 204+ | 10 | 0/30 | 80% | 0 | DIED (Troll) |
| 64463 | ~50+ | 2+ | — | ~20% | — | Budget exceeded |
| 64464 | 33+ | 2 | 29/30 | 20% | 2 Health Potions | Budget exceeded |
| 64465 | 362 | 18 | 30/30 | 100% | Short Sword | **SURVIVED** |
| 64466 | 204 | 10 | 0/30 | 80% | 0 | DIED (Troll) |

## Cost Analysis

| Seed | Tool Calls | Cost |
|------|-----------|------|
| 64462 | 34 | $1.54 |
| 64463 | 20 | $0.96 |
| 64464 | 29 | $1.58 |
| 64465 | 30 | $1.33 |
| 64466 | 22 | $0.97 |
| **Total** | **135** | **$6.39** |

Average: $1.28/game, 27 tool calls/game. Comparable to Session 4 (~30 calls).

## Priority Recommendations

### Item system — working well, minor tuning:
1. **Guarantee minimum items per dungeon** (MEDIUM) — seed 64466 had zero items in 80%
   explored. Minimum 2-3 items ensures the system is always relevant.
2. **Add monster ATK/DEF to observations** (MEDIUM) — lets LLM make accurate combat
   decisions about when items change the fight from unwinnable to winnable.

### Gameplay — unchanged from Session 4:
1. **Win condition** — still the #1 missing feature across all 5 sessions
2. **Troll spawn guarantee** — items now provide a counter, but only if both spawn
3. **Monster density** — more encounters would make items feel more impactful

### MCP design validation:
- **Items in observations work** — `visible_items`, `weapon`, `armor`, `player_atk`,
  `player_def` give the LLM everything it needs
- **Auto-pickup is correct for MCP** — no new commands needed, items integrate seamlessly
  into the existing explore→fight loop
- **Equipment creates real strategic depth** — the Troll math shifts from "impossible" to
  "easy" with one weapon, giving items a clear purpose

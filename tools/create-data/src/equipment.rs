//! Create's equipment items (`AllItems`, `AllArmorMaterials`, `content/equipment`): the armour's
//! numbers as item components over vanilla templates. Their behaviour is the native mod's.

use serde_json::{Value, json};

/// `AllArmorMaterials.COPPER`'s equip sound (`COPPER_ARMOR_EQUIP` plays `ARMOR_EQUIP_GOLD`).
const COPPER_EQUIP: &str = "minecraft:item.armor.equip_gold";

/// The armour's attribute modifiers, in the form of the game's own armour items'
/// `minecraft:attribute_modifiers`.
fn armor(slot: &str, kind: &str, defense: f64, toughness: f64, knockback: f64) -> Value {
    let id = format!("minecraft:armor.{kind}");
    let mut v = vec![
        json!({ "type": "minecraft:armor", "amount": defense, "id": id,
                "operation": "add_value", "slot": slot }),
        json!({ "type": "minecraft:armor_toughness", "amount": toughness, "id": id,
                "operation": "add_value", "slot": slot }),
    ];
    if knockback > 0.0 {
        v.push(
            json!({ "type": "minecraft:knockback_resistance", "amount": knockback, "id": id,
                       "operation": "add_value", "slot": slot }),
        );
    }
    Value::Array(v)
}

/// The components of a backtank's item (`BacktankItem`, a chestplate of copper or netherite;
/// no durability, so one to a stack), or None for another block. Its air is the native mod's.
///
/// Create's armour is drawn with its own textures (`BaseArmorItem.getArmorTexture`:
/// `create:textures/models/armor/<material>_diving_layer_<n>.png`) and the backtank's model
/// on the back (`BacktankArmorLayer`), not with the game's copper or netherite armour, so the
/// `equippable` names the equipment asset of Create's texture (`create:copper_diving`), which no
/// pack declares: the game's armour layer draws no layer of it, its head layer leaves a helmet
/// to the armour layer (an item with an `asset_id` is armour), and the native mod draws
/// Create's look.
pub fn backtank_item(name: &str) -> Option<Value> {
    Some(match name {
        // AllArmorMaterials.COPPER: 4 on the chest, enchantability 7.
        "copper_backtank" => json!({
            "minecraft:max_stack_size": 1,
            "minecraft:equippable": { "slot": "chest", "equip_sound": COPPER_EQUIP,
                                      "asset_id": COPPER_DIVING },
            "minecraft:attribute_modifiers": armor("chest", "chestplate", 4.0, 0.0, 0.0),
            "minecraft:enchantable": { "value": 7 }
        }),
        // Netherite armour on the chest: the numbers of the game's own `minecraft:netherite_chestplate`
        // (its 26.3 default components: armour 8, toughness 3, knockback resistance 0.1,
        // enchantable 15, damage resistant to `#minecraft:is_fire`), which a block's item can't
        // take as a template.
        "netherite_backtank" => json!({
            "minecraft:max_stack_size": 1,
            "minecraft:equippable": { "slot": "chest",
                                      "equip_sound": "minecraft:item.armor.equip_netherite",
                                      "asset_id": NETHERITE_DIVING },
            "minecraft:attribute_modifiers":
                armor("chest", "chestplate", 8.0, 3.0, f64::from(0.1f32)),
            "minecraft:enchantable": { "value": 15 },
            "minecraft:damage_resistant": { "types": "#minecraft:is_fire" }
        }),
        _ => return None,
    })
}

/// The equipment assets of Create's diving gear: the `textureLoc` its `BaseArmorItem` is made
/// with (`AllItems`), which no pack declares (see [`backtank_item`]).
const COPPER_DIVING: &str = "create:copper_diving";
const NETHERITE_DIVING: &str = "create:netherite_diving";

/// The equippable of a diving helmet or boots: the template's slot and sound, and Create's
/// texture as its look (`asset_id`) in place of the game's armour (see [`backtank_item`]).
fn diving(slot: &str, sound: &str, asset: &str) -> Value {
    json!({ "slot": slot, "equip_sound": sound, "asset_id": asset })
}

/// The declaration of an equipment item that is not a block, or None.
pub fn item(name: &str) -> Option<Value> {
    const NETHERITE_EQUIP: &str = "minecraft:item.armor.equip_netherite";
    // DivingHelmetItem.getEnchantmentLevel/getAllEnchantments: Aqua Affinity always.
    let aqua = json!({ "minecraft:aqua_affinity": 1 });
    Some(match name {
        // COPPER_DIVING_HELMET: Create's `Type.HELMET.getDurability(7)`, 77 (a helmet lasts 11
        // per point of its material's durability, as the game's own helmets do).
        "copper_diving_helmet" => json!({
            "template": "minecraft:copper_helmet",
            "components": {
                "minecraft:max_damage": 77,
                "minecraft:enchantable": { "value": 7 },
                "minecraft:equippable": diving("head", COPPER_EQUIP, COPPER_DIVING),
                "minecraft:attribute_modifiers": armor("head", "helmet", 2.0, 0.0, 0.0),
                "minecraft:enchantments": aqua
            }
        }),
        // NETHERITE_DIVING_HELMET: netherite armour, `getDurability(37)`, fire resistant: the
        // game's netherite helmet, as its template, drawn as Create's.
        "netherite_diving_helmet" => json!({
            "template": "minecraft:netherite_helmet",
            "components": {
                "minecraft:enchantments": aqua,
                "minecraft:equippable": diving("head", NETHERITE_EQUIP, NETHERITE_DIVING)
            }
        }),
        // COPPER_DIVING_BOOTS: `Type.BOOTS.getDurability(7)`, 91 (13 per point for boots).
        "copper_diving_boots" => json!({
            "template": "minecraft:copper_boots",
            "components": {
                "minecraft:max_damage": 91,
                "minecraft:enchantable": { "value": 7 },
                "minecraft:equippable": diving("feet", COPPER_EQUIP, COPPER_DIVING),
                "minecraft:attribute_modifiers": armor("feet", "boots", 1.0, 0.0, 0.0)
            }
        }),
        // NETHERITE_DIVING_BOOTS: `getDurability(37)`: the game's netherite boots, as template,
        // drawn as Create's.
        "netherite_diving_boots" => json!({
            "template": "minecraft:netherite_boots",
            "components": { "minecraft:equippable": diving("feet", NETHERITE_EQUIP, NETHERITE_DIVING) }
        }),
        // ExtendoGripItem: `durability(MAX_DAMAGE)` (200), uncommon.
        "extendo_grip" => json!({
            "template": "minecraft:stick",
            "max_stack_size": 1,
            "rarity": "uncommon",
            "components": { "minecraft:max_damage": 200, "minecraft:damage": 0 }
        }),
        // SandPaperItem: `durability(8)`, enchantability 1.
        "sand_paper" | "red_sand_paper" => json!({
            "template": "minecraft:stick",
            "max_stack_size": 1,
            "components": {
                "minecraft:max_damage": 8,
                "minecraft:damage": 0,
                "minecraft:enchantable": { "value": 1 }
            }
        }),
        // Filters (`FilterItem.regular`, `.attribute`, `.address`), the schedule and the
        // crafting blueprint: plain items whose settings the native mod keeps in
        // `minecraft:custom_data`; the blueprint's frame entity is gap 3.
        "filter" | "attribute_filter" | "package_filter" | "schedule" | "crafting_blueprint"
        | "tree_fertilizer" => json!({ "template": "minecraft:stick" }),
        // ShoppingListItem, LinkedControllerItem: `stacksTo(1)`.
        "shopping_list" | "linked_controller" => {
            json!({ "template": "minecraft:stick", "max_stack_size": 1 })
        }
        // SuperGlueItem: `stacksTo(1).durability(99)`.
        "super_glue" => json!({
            "template": "minecraft:stick",
            "max_stack_size": 1,
            "components": { "minecraft:max_damage": 99, "minecraft:damage": 0 }
        }),
        // SymmetryWandItem: `stacksTo(1)`, uncommon.
        "wand_of_symmetry" => json!({
            "template": "minecraft:stick",
            "max_stack_size": 1,
            "rarity": "uncommon"
        }),
        // WorldshaperItem: `ZapperItem` stacks to 1, epic.
        "handheld_worldshaper" => json!({
            "template": "minecraft:stick",
            "max_stack_size": 1,
            "rarity": "epic"
        }),
        // PotatoCannonItem: `durability(100)` (one to a stack), a projectile weapon's
        // enchantability 1 (`ProjectileWeaponItem.getEnchantmentValue`). A stick has no
        // `damage`: an item wears only with both components (`isDamageableItem`).
        "potato_cannon" => json!({
            "template": "minecraft:stick",
            "max_stack_size": 1,
            "components": {
                "minecraft:max_damage": 100,
                "minecraft:damage": 0,
                "minecraft:enchantable": { "value": 1 }
            }
        }),
        // CARDBOARD_SWORD: `AllToolMaterials.CARDBOARD` (1 use, +2 damage, enchantability 1,
        // repaired with cardboard), `SwordItem.createAttributes(CARDBOARD, 3, 1)`: 3 + 2 attack
        // damage and +1 attack speed; one to a stack, burns 1000 ticks.
        "cardboard_sword" => json!({
            "template": "minecraft:wooden_sword",
            "max_stack_size": 1,
            "components": {
                "minecraft:max_damage": 1,
                "minecraft:enchantable": { "value": 1 },
                "minecraft:repairable": { "items": "create:cardboard" },
                "minecraft:attribute_modifiers": [
                    { "type": "minecraft:attack_damage", "amount": 5.0,
                      "id": "minecraft:base_attack_damage", "operation": "add_value",
                      "slot": "mainhand" },
                    { "type": "minecraft:attack_speed", "amount": 1.0,
                      "id": "minecraft:base_attack_speed", "operation": "add_value",
                      "slot": "mainhand" }
                ],
                "minecraft:cooking_fuel": { "burn_time": 1000,
                                            "speed_multiplier": "minecraft:cooking/speed_default" }
            }
        }),
        _ => return cardboard_armor(name),
    })
}

/// `AllArmorMaterials.CARDBOARD` (defence 1 each, enchantability 4, leather's equip sound, no
/// toughness, repaired with cardboard) on `Type.getDurability(4)` (11, 16, 15, 13 × 4); each
/// piece burns 1000 ticks. Its stealth (`CardboardArmorHandler`) is the native mod's.
fn cardboard_armor(name: &str) -> Option<Value> {
    let (piece, slot, durability) = match name {
        "cardboard_helmet" => ("helmet", "head", 44),
        "cardboard_chestplate" => ("chestplate", "chest", 64),
        "cardboard_leggings" => ("leggings", "legs", 60),
        "cardboard_boots" => ("boots", "feet", 52),
        _ => return None,
    };
    Some(json!({
        "template": format!("minecraft:leather_{piece}"),
        "components": {
            "minecraft:max_damage": durability,
            "minecraft:enchantable": { "value": 4 },
            "minecraft:repairable": { "items": "create:cardboard" },
            "minecraft:equippable": { "slot": slot,
                                      "equip_sound": "minecraft:item.armor.equip_leather",
                                      "asset_id": "create:cardboard" },
            "minecraft:attribute_modifiers": armor(slot, piece, 1.0, 0.0, 0.0),
            "minecraft:cooking_fuel": { "burn_time": 1000,
                                        "speed_multiplier": "minecraft:cooking/speed_default" }
        }
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backtanks_are_chestplates_of_their_metal() {
        let copper = backtank_item("copper_backtank").unwrap();
        assert_eq!(copper["minecraft:equippable"]["slot"], "chest");
        assert_eq!(copper["minecraft:attribute_modifiers"][0]["amount"], 4.0);
        let netherite = backtank_item("netherite_backtank").unwrap();
        assert_eq!(netherite["minecraft:attribute_modifiers"][1]["amount"], 3.0);
        assert!(netherite.get("minecraft:damage_resistant").is_some());
        assert!(backtank_item("shaft").is_none());
    }

    #[test]
    fn create_s_diving_gear_is_not_drawn_as_the_game_s_armour() {
        let backtanks = ["copper_backtank", "netherite_backtank"].map(backtank_item);
        let worn = ["copper_diving_helmet", "copper_diving_boots"]
            .map(|n| item(n).unwrap()["components"].clone());
        let netherite = ["netherite_diving_helmet", "netherite_diving_boots"]
            .map(|n| item(n).unwrap()["components"].clone());
        for (i, c) in backtanks
            .into_iter()
            .flatten()
            .chain(worn)
            .chain(netherite)
            .enumerate()
        {
            let e = &c["minecraft:equippable"];
            assert!(e["slot"].is_string(), "still worn: {c}");
            // Create's texture, never the game's copper or netherite armour.
            let asset = if i == 1 || i >= 4 {
                NETHERITE_DIVING
            } else {
                COPPER_DIVING
            };
            assert_eq!(e["asset_id"], asset, "{c}");
        }
    }

    #[test]
    fn copper_gear_wears_as_create_s_copper() {
        let helmet = item("copper_diving_helmet").unwrap();
        assert_eq!(helmet["components"]["minecraft:max_damage"], 77);
        let boots = item("copper_diving_boots").unwrap();
        assert_eq!(boots["components"]["minecraft:max_damage"], 91);
        assert_eq!(
            item("extendo_grip").unwrap()["components"]["minecraft:max_damage"],
            200
        );
        assert!(item("wrench").is_none());
    }

    #[test]
    fn handheld_items_take_create_s_numbers() {
        assert_eq!(
            item("super_glue").unwrap()["components"]["minecraft:max_damage"],
            99
        );
        assert_eq!(item("wand_of_symmetry").unwrap()["rarity"], "uncommon");
        assert_eq!(item("handheld_worldshaper").unwrap()["rarity"], "epic");
        assert_eq!(
            item("potato_cannon").unwrap()["components"]["minecraft:max_damage"],
            100
        );
        let sword = item("cardboard_sword").unwrap();
        assert_eq!(
            sword["components"]["minecraft:attribute_modifiers"][0]["amount"],
            5.0
        );
        let chest = item("cardboard_chestplate").unwrap();
        assert_eq!(chest["components"]["minecraft:max_damage"], 64);
        assert_eq!(
            chest["components"]["minecraft:equippable"]["asset_id"],
            "create:cardboard"
        );
        assert_eq!(item("filter").unwrap()["template"], "minecraft:stick");
    }
}

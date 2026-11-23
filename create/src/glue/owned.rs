//! Owned copies of the stacks the API lends: a player's inventory, a living entity's hands and
//! equipment, a dropped item's and a place's stack, and a level's entities and players. None
//! where the API lends none.

use mod_api::{
    EntityRef, EquipmentSlot, Hand, Host, ItemEntityRef, ItemPlace, ItemStack, LevelRef, LivingRef,
    PlayerRef, ServerHost,
};

pub trait OwnedInventory {
    /// `Inventory.items`: the non-empty main inventory stacks in slot order.
    fn inventory(self, host: &dyn Host) -> Option<Vec<ItemStack>>;
    /// Every occupied inventory slot with its stack, by the inventory's slot number.
    fn inventory_slots(self, host: &dyn Host) -> Option<Vec<(u32, ItemStack)>>;
}

// Horizon hands out owned copies itself: these forward to its own methods.
impl OwnedInventory for PlayerRef {
    fn inventory(self, host: &dyn Host) -> Option<Vec<ItemStack>> {
        PlayerRef::inventory(self, host)
    }

    fn inventory_slots(self, host: &dyn Host) -> Option<Vec<(u32, ItemStack)>> {
        PlayerRef::inventory_slots(self, host)
    }
}

pub trait OwnedEquipment {
    fn hand_item(self, host: &dyn Host, hand: Hand) -> Option<ItemStack>;
    fn equipment(self, host: &dyn Host, slot: EquipmentSlot) -> Option<ItemStack>;
}

impl OwnedEquipment for LivingRef {
    fn hand_item(self, host: &dyn Host, hand: Hand) -> Option<ItemStack> {
        LivingRef::hand_item(self, host, hand)
    }

    fn equipment(self, host: &dyn Host, slot: EquipmentSlot) -> Option<ItemStack> {
        LivingRef::equipment(self, host, slot)
    }
}

pub trait OwnedStack {
    fn stack(self, host: &dyn Host) -> Option<ItemStack>;
}

impl OwnedStack for ItemEntityRef {
    fn stack(self, host: &dyn Host) -> Option<ItemStack> {
        ItemEntityRef::stack(self, host)
    }
}

pub trait OwnedPlace {
    fn stack(&self, host: &mut dyn ServerHost) -> Option<ItemStack>;
}

impl OwnedPlace for ItemPlace {
    fn stack(&self, host: &mut dyn ServerHost) -> Option<ItemStack> {
        ItemPlace::stack(self, host)
    }
}

pub trait LevelLists {
    /// `Level.getEntities`: the entities whose boxes meet `min`..`max`.
    fn entities_in(self, host: &dyn Host, min: [f64; 3], max: [f64; 3]) -> Option<Vec<EntityRef>>;
    /// `ServerLevel.players`.
    fn online_players(self, host: &dyn Host) -> Option<Vec<PlayerRef>>;
}

impl LevelLists for LevelRef {
    fn entities_in(self, host: &dyn Host, min: [f64; 3], max: [f64; 3]) -> Option<Vec<EntityRef>> {
        LevelRef::entities_in(self, host, min, max)
    }

    fn online_players(self, host: &dyn Host) -> Option<Vec<PlayerRef>> {
        LevelRef::online_players(self, host)
    }
}

//! Registry keys' names as owned text, empty when the host names none.

use mod_api::{
    BlockEntityTypeKey, BlockKey, EffectKey, EnchantmentKey, EntityTypeKey, FluidKey, Host,
    ItemKey, MenuKey,
};

pub trait Named {
    fn name(self, host: &dyn Host) -> String;
}

macro_rules! named {
    ($($key:ty),*) => {$(
        impl Named for $key {
            fn name(self, host: &dyn Host) -> String {
                self.id(host)
            }
        }
    )*};
}

macro_rules! registry_named {
    ($($key:ty),*) => {$(
        impl Named for $key {
            fn name(self, host: &dyn Host) -> String {
                self.id(host)
            }
        }
    )*};
}

named!(BlockEntityTypeKey, BlockKey, EffectKey, FluidKey, ItemKey);
registry_named!(EnchantmentKey, EntityTypeKey, MenuKey);

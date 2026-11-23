//! What clients see of a network's stress changing (`KineticEffectHandler`): two ticks after a
//! turning block's network becomes overstressed, smoke puffs out of it and its turning parts
//! flash red; when it recovers, a little cloud and a green flash (`overStressedEffect`).

use super::super::kinetics;
use create_engine::processing::Rng;
use mod_api::{
    BlockEntityDataReceivedEvent, BlockEntityDataSource, BlockEntityPart, BlockPart, ClientHost,
    HostSide, OnChunkUnloadedEvent, ParticleKey, ParticleOptions, Registrar, Tag,
};
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::OnceLock;

static SMOKE: OnceLock<ParticleKey> = OnceLock::new();
static CLOUD: OnceLock<ParticleKey> = OnceLock::new();

#[derive(Default)]
struct Client {
    /// Whether each block's network was overstressed when it was last heard of.
    over: HashMap<[i32; 3], bool>,
    /// Effects to play: the block, whether it is overstressed, ticks left
    /// (`overStressedTime`).
    pending: Vec<([i32; 3], bool, u8)>,
    /// `overStressedEffect` of each block flashing: 1 (red) or -1 (green), fading by a tenth a
    /// tick.
    flash: HashMap<[i32; 3], f32>,
    rng: Option<Rng>,
}

thread_local! {
    static CLIENT: RefCell<Client> = RefCell::new(Client::default());
}

pub(super) fn register(r: &mut dyn Registrar) {
    let _ = SMOKE.set(r.particle_key("minecraft:smoke"));
    let _ = CLOUD.set(r.particle_key("minecraft:cloud"));
    let Some(kind) = kinetics::kind() else {
        return;
    };
    if let Err(e) = r.block_entity_part(&kind, Effects) {
        crate::create_log!("[create] the stress effects' block entity part was refused: {e:?}");
    }
}

pub(super) fn client_reset() {
    CLIENT.with_borrow_mut(|c| *c = Client::default());
}

/// `KineticEffectHandler.tick` on the client: an effect two ticks after it was triggered
/// (`spawnEffect`: smoke, 5 at up to .2 a tick; a cloud, 2 at up to .075).
pub(super) fn client_tick(host: &mut dyn ClientHost) {
    let due: Vec<([i32; 3], bool)> = CLIENT.with_borrow_mut(|c| {
        let mut due = Vec::new();
        c.pending.retain_mut(|(pos, over, left)| {
            *left -= 1;
            if *left == 0 {
                due.push((*pos, *over));
            }
            *left > 0
        });
        for &(pos, over) in &due {
            c.flash.insert(pos, if over { 1.0 } else { -1.0 });
        }
        c.flash.retain(|_, e| {
            *e -= *e * 0.1;
            e.abs() >= 1.0 / 128.0
        });
        due
    });
    for (pos, over) in due {
        let (key, motion, amount) = if over {
            (SMOKE.get(), 0.2, 5)
        } else {
            (CLOUD.get(), 0.075, 2)
        };
        let Some(&key) = key else {
            continue;
        };
        let center = pos.map(|v| f64::from(v) + 0.5);
        for _ in 0..amount {
            let v = CLIENT.with_borrow_mut(|c| {
                let rng = c.rng.get_or_insert_with(|| Rng::new(0x0E57_2E55));
                [0; 3].map(|_| f64::from((rng.next_float() - 0.5) * 2.0 * motion))
            });
            host.add_particle(&ParticleOptions::simple(key), center, v);
        }
    }
}

/// Hears each block's network turning overstressed or recovering.
struct Effects;

#[mod_api::horizon_part]
impl BlockPart for Effects {}

#[mod_api::horizon_part]
impl BlockEntityPart for Effects {
    /// `KineticBlockEntity.read` on the client: an update that turns the overstress on or off
    /// while the block should turn (`speed != 0`) triggers its effect.
    fn block_entity_data_received(&self, host: &dyn mod_api::ClientHost, event: &mut BlockEntityDataReceivedEvent<'_>) {
        let Some(Tag::Byte(over)) = event.tag.get(&kinetics::OVER.into()) else {
            return;
        };
        let Some(pos) = event.entity.position(host) else {
            return;
        };
        let over = *over != 0;
        let theory = match event.tag.get(&kinetics::THEORY.into()) {
            Some(Tag::Float(bits)) => f32::from_bits(*bits),
            _ => 0.0,
        };
        let update = matches!(event.source, BlockEntityDataSource::UpdatePacket);
        CLIENT.with_borrow_mut(|c| {
            let before = c.over.insert(pos, over);
            if update && before.is_some_and(|b| b != over) && theory != 0.0 {
                // `triggerOverStressedEffect`: a second change before it plays cancels it.
                if let Some(i) = c.pending.iter().position(|p| p.0 == pos) {
                    c.pending.swap_remove(i);
                } else {
                    c.pending.push((pos, over, 2));
                }
            }
        });
    }

    fn on_chunk_unloaded(&self, host: HostSide, event: &mut OnChunkUnloadedEvent<'_>) {
        if let HostSide::Client(_) = host {
            CLIENT.with_borrow_mut(|c| {
                c.over.remove(&event.pos);
                c.flash.remove(&event.pos);
            });
        }
    }
}

/// `KineticBlockEntityRenderer.kineticRotationTransform`'s colour for the block at `pos`: white
/// mixed toward red (overstressed) or spring green (recovered) by the fading effect.
pub fn flash_color(pos: [i32; 3]) -> Option<u32> {
    let e = CLIENT.with_borrow(|c| c.flash.get(&pos).copied())?;
    let (to, weight) = if e > 0.0 {
        ([255.0, 0.0, 0.0], e)
    } else {
        ([0.0, 255.0, 187.0], -e)
    };
    let [r, g, b] = to.map(|c: f32| (255.0 + (c - 255.0) * weight) as u32);
    Some(0xFF00_0000 | r << 16 | g << 8 | b)
}

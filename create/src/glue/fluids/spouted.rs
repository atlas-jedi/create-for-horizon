//! What clients draw of spouts (`SpoutRenderer`): the fluid in the spout, the nozzle's three
//! pieces squeezing while it fills, and the stream it pours, from the spout's contents (`TANK`)
//! and the game time its cycle started (`SPOUT_START`).
//!
//! And what clients see of it working (`SpoutBlockEntity.tick` and `read` on the client): its
//! fluid dripping out while a cycle runs (`spawnProcessingParticles`), and splashing on what it
//! filled (`spawnSplash`), told by the game time of the fill (`SPLASH`).

use super::super::states;
use super::client::fluid_box;
use super::{SPOUT_START, TANK};
use create_engine::Block;
use create_engine::processing::Rng;
use create_engine::spout::FILLING_TIME;
use create_engine::tank::{Codes, Fluid};
use mod_api::{
    BlockEntityClientTickEvent, BlockEntityClientTicksEvent, BlockEntityPart, BlockPart,
    ClientHost, Compound, HostSide, ModBlockEntity, OnChunkUnloadedEvent, Registrar, Tag,
};
use std::cell::RefCell;
use std::collections::HashMap;

/// The key of the game time of a spout's last fill in its block entity data, for its clients
/// (`SpoutBlockEntity.write`'s `Splash`, sent once with the update after a fill).
pub const SPLASH: &str = "SpoutSplash";

/// `SPLASH_PARTICLE_COUNT`.
const SPLASH_PARTICLES: usize = 20;

/// Ticks after a fill a client that hears of it late still splashes.
const SPLASH_WINDOW: i64 = 20;

/// What a client last saw of a spout: the fluid it shows (`getRenderedFluid`, kept once the
/// tank empties) and the fill it last splashed for.
#[derive(Default)]
struct Seen {
    fluid: Option<Fluid>,
    splash: Option<i64>,
}

thread_local! {
    static SEEN: RefCell<HashMap<[i32; 3], Seen>> = RefCell::new(HashMap::new());
    static RNG: RefCell<Rng> = RefCell::new(Rng::new(0x5B07));
}

/// The game time the spout's cycle started, while it runs.
fn start(entity: &ModBlockEntity) -> Option<i64> {
    start_in(&entity.nbt)
}

fn start_in(nbt: &Compound) -> Option<i64> {
    match nbt.get(&SPOUT_START.into()) {
        Some(Tag::IntArray(v)) => v.first().map(|t| i64::from(*t)),
        _ => None,
    }
}

/// The fluid a spout's data shows in its tank, while it holds some.
fn shown_fluid(nbt: &Compound) -> Option<Fluid> {
    match nbt.get(&TANK.into()) {
        Some(Tag::IntArray(v)) => match v.as_slice() {
            &[fluid, amount, _] if amount > 0 => {
                Codes::fluid(&super::fluid_keys(nbt, TANK), fluid)
            }
            _ => None,
        },
        _ => None,
    }
}

/// The game time of the spout's last fill its data shows.
fn splash_in(nbt: &Compound) -> Option<i64> {
    match nbt.get(&SPLASH.into()) {
        Some(Tag::Long(t)) => Some(*t),
        _ => None,
    }
}

/// The spouts' effects on the `create:kinetic` block entity type.
pub(super) fn register(r: &mut dyn Registrar) {
    let Some(kind) = super::super::kinetics::kind() else {
        return;
    };
    if let Err(e) = r.block_entity_part(&kind, Effects) {
        crate::create_log!("[create] the spouts' block entity part was refused: {e:?}");
    }
}

fn random() -> f64 {
    RNG.with_borrow_mut(|r| f64::from(r.next_float()))
}

/// Spouts' drips and splashes on their clients.
struct Effects;

#[mod_api::horizon_part]
impl BlockPart for Effects {}

#[mod_api::horizon_part]
impl BlockEntityPart for Effects {
    fn block_entity_client_ticks(
        &self,
        _host: &dyn ClientHost,
        event: &mut BlockEntityClientTicksEvent<'_>,
    ) {
        if event.block == "create:spout" {
            event.ticks = true;
        }
    }

    /// `SpoutBlockEntity.tick` on the client: a fluid particle half a block under the spout's
    /// middle, falling at 0.1, each tick while `processingTicks >= 8`; and `read`'s
    /// `spawnSplash`: 20 particles where the filled item sits (1 11/16 under the middle), thrown
    /// up and out at up to 0.125.
    fn block_entity_client_tick(
        &self,
        host: &mut dyn ClientHost,
        event: &mut BlockEntityClientTickEvent<'_>,
    ) {
        let event = &mut crate::glue::compat::ClientTick::new(host, &*event);
        let Some((state, pos, level, tank, start, splash)) = event.subject.with_view(|v| {
            let nbt = v.nbt();
            (
                v.state(),
                v.position(),
                v.level(),
                shown_fluid(nbt),
                start_in(nbt),
                splash_in(nbt),
            )
        }) else {
            return;
        };
        if state.block() != states::key(Block::Spout) {
            return;
        }
        let host = event.subject.host();
        let Some(now) = level.game_time(&*host) else {
            return;
        };
        let (fluid, splashes) = SEEN.with_borrow_mut(|seen| {
            let s = seen.entry(pos).or_default();
            if tank.is_some() {
                s.fluid = tank;
            }
            let fresh = splash
                .is_some_and(|t| s.splash != Some(t) && (0..=SPLASH_WINDOW).contains(&(now - t)));
            if splash.is_some() {
                s.splash = splash;
            }
            (s.fluid, fresh)
        });
        let Some(options) = fluid.and_then(super::flows::fluid_particle_of) else {
            return;
        };
        let center = pos.map(|v| f64::from(v) + 0.5);
        let processing = start.map(|s| FILLING_TIME - (now - s) as i32);
        if processing.is_some_and(create_engine::spout::drips) {
            let at = [center[0], center[1] - 0.5, center[2]];
            host.add_particle(&options, at, [0.0, -0.1, 0.0]);
        }
        if splashes {
            let at = [center[0], center[1] - (2.0 - 5.0 / 16.0), center[2]];
            for _ in 0..SPLASH_PARTICLES {
                let m = [0; 3].map(|_| (random() - 0.5) * 2.0 * 0.125);
                host.add_particle(&options, at, [m[0], m[1].abs(), m[2]]);
            }
        }
    }

    fn on_chunk_unloaded(&self, host: HostSide, event: &mut OnChunkUnloadedEvent<'_>) {
        if let HostSide::Client(_) = host {
            SEEN.with_borrow_mut(|s| {
                s.remove(&event.pos);
            });
        }
    }
}

/// Whether a spout moves this frame (its cycle runs).
pub(super) fn animated(entity: &ModBlockEntity) -> bool {
    start(entity).is_some()
}

pub(super) fn draw(host: &dyn ClientHost, event: &mut crate::glue::Submit<'_>) {
    let tank = match event.entity.nbt.get(&TANK.into()) {
        Some(Tag::IntArray(v)) => match v.as_slice() {
            &[fluid, amount, capacity] if amount > 0 && capacity > 0 => {
                let keys = super::fluid_keys(&event.entity.nbt, TANK);
                Codes::fluid(&keys, fluid).map(|f| (f, amount, capacity))
            }
            _ => None,
        },
        _ => None,
    };
    if let Some((fluid, amount, capacity)) = tank {
        let level = (amount as f32 / capacity as f32).clamp(0.175, 1.0);
        let (min, max) = (2.5 / 16.0, 2.5 / 16.0 + 11.0 / 16.0);
        let top = min + 11.0 / 16.0 * level;
        fluid_box(
            host,
            &mut *event.draw,
            fluid,
            amount as u32,
            [min, min, min],
            [max, top, max],
        );
    }
    // `processingTicks` and the frame's part of a tick.
    let pt = start(event.entity).map(|s| {
        let draw = &*event.draw;
        let ticks = FILLING_TIME as f32 - (draw.game_time() - s) as f32;
        ticks - draw.partial()
    });
    let progress = pt.map_or(0.0, |pt| (1.0 - (pt - 5.0) / 10.0).clamp(0.0, 1.0));
    let radius = if pt.is_some() {
        (2.0 * progress - 1.0).powi(2) - 1.0
    } else {
        0.0
    };
    if let (Some((fluid, amount, _)), Some(_)) = (tank, pt) {
        let r = radius.abs() / 32.0;
        if r > 0.0 {
            fluid_box(
                host,
                &mut *event.draw,
                fluid,
                amount as u32,
                [0.5 - r, -1.2, 0.5 - r],
                [0.5 + r, 0.0, 0.5 + r],
            );
        }
    }
    let squeeze = match pt {
        None => 0.0,
        Some(pt) if pt < 0.0 => 0.0,
        Some(pt) if pt < 2.0 => -(pt / 2.0),
        Some(pt) if pt < 10.0 => -1.0,
        Some(_) => radius,
    };
    let draw = &mut *event.draw;
    draw.push();
    for name in ["part_spout_top", "part_spout_middle", "part_spout_bottom"] {
        if let Some(part) = states::partial(name) {
            draw.block(part, &[("facing", "up")]);
        }
        draw.translate([0.0, -3.0 * squeeze / 32.0, 0.0]);
    }
    draw.pop();
}

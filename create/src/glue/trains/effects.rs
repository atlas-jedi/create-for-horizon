//! What a client hears and sees of the trains it knows as they run (`CarriageSounds`,
//! `CarriageParticles`, `create_engine::carriage_effects`): the first carriage's steam puffs at
//! its nearest bogey, each carriage's steam let off as it stops, its wheels sparking on the rails
//! (`CRIT`, the standard bogeys' contact particle) and smoking as it stops (`POOF`), and a
//! derailed carriage's smoke (`spawnDerailParticles`). The rolling-wheel loops are left out
//! (`HORIZON_GAPS.md`). Nothing runs without trains known.

use crate::glue::names::Named as _;
use create_engine::carriage_effects::{
    DERAIL_RISE, DERAIL_SMOKE_CHANCE, Heard, PARTICLE_RANGE, Particles, Sounds, derail_offset,
};
use mod_api::{ClientHost, ParticleKey, ParticleOptions, Registrar, SoundPlay, SoundSource};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::sync::OnceLock;

static CRIT: OnceLock<ParticleKey> = OnceLock::new();
static POOF: OnceLock<ParticleKey> = OnceLock::new();
static COSY_SMOKE: OnceLock<ParticleKey> = OnceLock::new();

pub fn register(r: &mut dyn Registrar) {
    let _ = CRIT.set(r.particle_key("minecraft:crit"));
    let _ = POOF.set(r.particle_key("minecraft:poof"));
    let _ = COSY_SMOKE.set(r.particle_key("minecraft:campfire_cosy_smoke"));
}

/// A carriage's effects on this client: its sounds, its particles and its derailed smoke's
/// offset (`derailParticleOffset`, rolled as its entity is made).
struct Car {
    sounds: Sounds,
    particles: Particles,
    smoke: [f64; 3],
}

impl Default for Car {
    fn default() -> Self {
        Car {
            sounds: Sounds::default(),
            particles: Particles::default(),
            smoke: derail_offset([float(), float(), float()]),
        }
    }
}

/// A train's effects on this client: its steam (`accumulatedSteamRelease`) and its carriages'.
#[derive(Default)]
struct Train {
    steam: f32,
    cars: Vec<Car>,
}

thread_local! {
    static TRAINS: RefCell<HashMap<u32, Train>> = RefCell::new(HashMap::new());
    /// The client's `RandomSource` for the particles.
    static SEED: Cell<u64> = const { Cell::new(0x9E37_79B9_7F4A_7C15) };
}

/// The client left its world.
pub fn reset() {
    TRAINS.with_borrow_mut(HashMap::clear);
}

fn next() -> u64 {
    let mut x = SEED.get();
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    SEED.set(x);
    x
}

/// `nextFloat`.
fn float() -> f32 {
    (next() >> 40) as f32 / (1u64 << 24) as f32
}

/// The client's tick: each carriage of each train it knows hears and shows itself
/// (`CarriageContraptionEntity.tickContraption` on a client).
pub fn client_tick(host: &mut dyn ClientHost) {
    if !super::client::knows_trains() {
        if !TRAINS.with_borrow(HashMap::is_empty) {
            reset();
        }
        return;
    }
    let Some(camera) = host.camera() else {
        return;
    };
    let player = host.local_player().map(|p| p.entity());
    let camera_motion = player.and_then(|e| e.velocity(&*host)).unwrap_or([0.0; 3]);
    // On a carriage seat: a rider of a seat's stand-in, an item display.
    let seated = player.and_then(|e| e.vehicle(&*host)).is_some_and(|v| {
        v.kind(&*host)
            .is_some_and(|k| k.name(&*host) == "minecraft:item_display")
    });
    let known = super::client::known_trains();
    TRAINS.with_borrow_mut(|t| t.retain(|id, _| known.contains(id)));
    let mut sounds: Vec<create_engine::carriage_effects::OneShot> = Vec::new();
    let mut puffs: Vec<create_engine::carriage_effects::Puff> = Vec::new();
    let mut smoke: Vec<[f64; 3]> = Vec::new();
    super::client::each_train(&*host, |id, train, cars| {
        TRAINS.with_borrow_mut(|all| {
            let t = all.entry(id).or_default();
            t.cars.resize_with(cars.len(), Car::default);
            for (c, car) in cars.iter().enumerate() {
                let heard = Heard {
                    camera: camera.position,
                    camera_motion,
                    motion: car.motion,
                    leading: car.leading,
                    trailing: car.trailing,
                    first: c == 0,
                    seated,
                    stalled: train.stalled,
                };
                let state = &mut t.cars[c];
                sounds.extend(state.sounds.tick(&heard, &mut t.steam));
                let near = (0..3)
                    .map(|k| (car.leading[k] - camera.position[k]).powi(2))
                    .sum::<f64>()
                    < PARTICLE_RANGE * PARTICLE_RANGE;
                if near {
                    let mut f = float;
                    let mut i4 = || (next() % 4) as i32;
                    puffs.extend(state.particles.tick(
                        car.motion,
                        &car.bogeys,
                        t.steam,
                        train.stalled,
                        &mut f,
                        &mut i4,
                    ));
                }
                if train.derailed && float() < DERAIL_SMOKE_CHANCE {
                    smoke.push(std::array::from_fn(|k| car.at[k] + state.smoke[k]));
                }
            }
        });
    });
    for s in sounds {
        if s.volume <= 0.0 {
            continue;
        }
        let play = SoundPlay::at(s.sound, SoundSource::Neutral, s.at)
            .with_volume(s.volume)
            .with_pitch(s.pitch);
        host.play_local_sound(&play, 0);
    }
    if let (Some(&crit), Some(&poof)) = (CRIT.get(), POOF.get()) {
        for p in puffs {
            let options = ParticleOptions::simple(if p.spark { crit } else { poof });
            host.add_particle(&options, p.at, p.velocity);
        }
    }
    if let Some(&cosy) = COSY_SMOKE.get() {
        for at in smoke {
            host.add_particle(&ParticleOptions::simple(cosy), at, [0.0, DERAIL_RISE, 0.0]);
        }
    }
}

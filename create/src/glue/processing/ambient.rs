//! What the processing machines do on a client alone (their client `tick` and `tickAudio`):
//! the mixer's crunching and the crumbs it spills while it works the bottom
//! (`MechanicalMixerBlockEntity`), the millstone's crumbs and noise (`MillstoneBlockEntity`), the
//! crushing wheels' crumbs and noise (`CrushingWheelControllerBlockEntity`) and the blaze burner's
//! flames and smoke (`BlazeBurnerBlockEntity.spawnParticles`). The millstone's and the crushing
//! wheels' noise is Create's `SoundScapes`: the sources of one kind and pitch band within 16
//! blocks share one sound, played at where they stand on average, louder the more there are.
//!
//! The game has no `animateTick` for a mod's blocks: the crackle of a burner and the sparks
//! between turning crushing wheels (`BlazeBurnerBlock.animateTick`,
//! `CrushingWheelControllerBlock.animateTick`) come from their client ticks, as often as the game
//! would call `animateTick` for a block that far from the player
//! (`processing::animate_tick_chance`). A lit blaze burner without a blaze has no block entity,
//! so its smoke and crackle (`LitBlazeBurnerBlock.animateTick`) are left out.

use super::{basin as basin_glue, client, stacks};
use crate::glue::kinetics;
use crate::glue::states;
use create_engine::basin::shown_ticks;
use create_engine::fan::Heat;
use create_engine::processing::{
    AMBIENT_DISTANCE, Rng, animate_tick_chance, machine_pitch, pitch_group, scape_volume,
};
use create_engine::tank::Fluid;
use create_engine::Block;
use mod_api::{
    BlockEntityClientTickEvent, BlockEntityClientTicksEvent, BlockEntityPart, BlockPart,
    ClientHost, Compound, ParticleKey, ParticleOptions, Registrar, SoundPlay, SoundSource, Tag,
};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

/// The particle types the machines use, on both sides.
pub struct Keys {
    pub item: ParticleKey,
    pub flame: ParticleKey,
    pub soul_fire_flame: ParticleKey,
    pub large_smoke: ParticleKey,
    pub crit: ParticleKey,
}

pub static KEYS: OnceLock<Keys> = OnceLock::new();

/// The key of a millstone's block entity data its clients read what it mills from: `I` its
/// input's item while it holds one, `F` (1) while an output is full (it waits).
pub const MILL_KEY: &str = "Milling";
/// The key of a crushing wheel controller's block entity data its clients read what it
/// crushes from: `I` the item in its input (`""` while it draws an entity in).
pub const CRUSH_KEY: &str = "Crushing";

/// `SoundScapes.UPDATE_INTERVAL`.
const UPDATE_INTERVAL: u64 = 5;

/// `SoundScapes.AmbienceGroup`: the processing machines' kinds of ambient noise.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Group {
    Crushing,
    Milling,
}

impl Group {
    /// `SoundScapes.crushing` / `milling`: each repeating sound (`RepeatingSound`): its event
    /// (Create's `crushing_1` to `_3` are the game's netherrack hit, gravel place and netherite
    /// block break), relative volume and pitch, and every how many ticks it repeats.
    fn sounds(self) -> &'static [(&'static str, f32, f32, u64)] {
        const CRUSHING: [(&str, f32, f32, u64); 3] = [
            ("minecraft:block.netherrack.hit", 1.545, 0.75, 1),
            ("minecraft:block.gravel.place", 0.425, 0.75, 2),
            ("minecraft:block.netherite_block.break", 2.0, 1.75, 2),
        ];
        match self {
            Group::Crushing => &CRUSHING,
            Group::Milling => &CRUSHING[..2],
        }
    }
}

/// One sound shared by the sources of a group and pitch band (`SoundScape`): the pitch of the
/// source that started it, and where they stand on average, worked out again every 5 ticks.
struct Scape {
    pitch: f32,
    mean: Option<[f64; 3]>,
}

#[derive(Default)]
struct Shared {
    /// `AnimationTickHolder.getTicks`.
    ticks: u64,
    /// `SoundScapes.counter`: the sources heard since the last update, by group and band.
    counter: HashMap<(Group, u8), HashSet<[i32; 3]>>,
    /// `SoundScapes.activeSounds`.
    active: HashMap<(Group, u8), Scape>,
}

thread_local! {
    static SHARED: RefCell<Shared> = RefCell::new(Shared::default());
    static RNG: RefCell<Rng> = RefCell::new(Rng::new(0xA3B1_E7));
}

/// `level.random.nextFloat()` on the client.
fn random() -> f64 {
    RNG.with_borrow_mut(|r| f64::from(r.next_float()))
}

/// `VecHelper.offsetRandomly(ZERO, r, radius)`.
fn offset_randomly(radius: f64) -> [f64; 3] {
    [0; 3].map(|_| (random() - 0.5) * 2.0 * radius)
}

/// `VecHelper.rotate(v, degrees, Y)`.
fn rotate_y(v: [f64; 3], degrees: f64) -> [f64; 3] {
    let (s, c) = degrees.to_radians().sin_cos();
    [v[0] * c + v[2] * s, v[1], v[2] * c - v[0] * s]
}

/// Asks for the particle types; attaches the machines' client ticks to the kinetic type.
pub fn register(r: &mut dyn Registrar) {
    let _ = KEYS.set(Keys {
        item: r.particle_key("minecraft:item"),
        flame: r.particle_key("minecraft:flame"),
        soul_fire_flame: r.particle_key("minecraft:soul_fire_flame"),
        large_smoke: r.particle_key("minecraft:large_smoke"),
        crit: r.particle_key("minecraft:crit"),
    });
    let kind = r.block_entity_type_key(kinetics::TYPE);
    if let Err(e) = r.block_entity_part(&kind, MachineTicks) {
        crate::create_log!("[create] the machines' client ticks were refused: {e:?}");
    }
}

/// The client left its world (`SoundScapes.invalidateAll`; the sounds stop with the level).
pub fn reset() {
    SHARED.with_borrow_mut(|s| *s = Shared::default());
}

/// Where the local player stands (`cameraEntity.position()`).
fn camera(host: &dyn ClientHost) -> Option<[f64; 3]> {
    host.local_player()?.entity().position(host)
}

/// `SoundScapes.play`: a source of `group` at `pos` asking for `pitch`, heard within 16 blocks
/// of the camera (`closerThan` its block); the first of a band starts its sound.
fn play_scape(host: &dyn ClientHost, group: Group, pos: [i32; 3], pitch: f32) {
    let Some(cam) = camera(host) else {
        return;
    };
    let d2: f64 = (0..3)
        .map(|i| (cam[i].floor() - f64::from(pos[i])).powi(2))
        .sum();
    if d2 >= AMBIENT_DISTANCE * AMBIENT_DISTANCE {
        return;
    }
    let band = pitch_group(pitch);
    SHARED.with_borrow_mut(|s| {
        s.counter.entry((group, band)).or_default().insert(pos);
        s.active
            .entry((group, band))
            .or_insert(Scape { pitch, mean: None });
    });
}

/// `SoundScapes.tick`, each client tick: every sound repeats on its own beat at where its
/// sources stand on average; every 5 ticks a band no source asked for again goes quiet and the
/// count starts over.
pub fn tick(host: &mut dyn ClientHost) {
    let cam = camera(&*host);
    let plays = SHARED.with_borrow_mut(|s| {
        s.ticks += 1;
        let ticks = s.ticks;
        let mut plays = Vec::new();
        let Shared {
            counter, active, ..
        } = s;
        for (key, scape) in active.iter_mut() {
            if ticks % UPDATE_INTERVAL == 0 {
                scape.mean = None;
            }
            let sources = counter.get(key);
            let count = sources.map_or(0, HashSet::len);
            let mean = *scape.mean.get_or_insert_with(|| {
                let Some(sources) = sources.filter(|s| !s.is_empty()) else {
                    return [0.0; 3];
                };
                let n = sources.len() as f64;
                let sum = sources.iter().fold([0.0; 3], |a, p| {
                    [0, 1, 2].map(|i| a[i] + f64::from(p[i]) + 0.5)
                });
                sum.map(|v| v / n)
            });
            // `getVolume`: no camera, no volume.
            let volume = cam.map_or(0.0, |c| {
                let d = (0..3).map(|i| (c[i] - mean[i]).powi(2)).sum::<f64>().sqrt();
                scape_volume(count, d)
            });
            for &(sound, relative_volume, relative_pitch, delay) in key.0.sounds() {
                if ticks % delay.max(1) == 0 && volume * relative_volume > 0.0 {
                    plays.push((
                        sound,
                        mean,
                        volume * relative_volume,
                        scape.pitch * relative_pitch,
                    ));
                }
            }
        }
        if ticks % UPDATE_INTERVAL == 0 {
            active.retain(|key, _| counter.get(key).is_some_and(|s| !s.is_empty()));
            counter.values_mut().for_each(HashSet::clear);
        }
        plays
    });
    // `RepeatingSound.tick`: `playLocalSound` on the ambient source.
    for (sound, at, volume, pitch) in plays {
        let play = SoundPlay::at(sound, SoundSource::Ambient, at)
            .with_volume(volume)
            .with_pitch(pitch);
        let _ = host.play_local_sound(&play, 0);
    }
}

/// Whether a client tick of this many ticks' odds lands (`animate_tick_chance` for the block at
/// `pos`, from the player's block).
fn animated(host: &dyn ClientHost, pos: [i32; 3]) -> bool {
    let Some(cam) = camera(host) else {
        return false;
    };
    let offset = [0, 1, 2].map(|i| pos[i] - cam[i].floor() as i32);
    random() < animate_tick_chance(offset)
}

/// The machines' client ticks.
struct MachineTicks;

#[mod_api::horizon_part]
impl BlockPart for MachineTicks {}

#[mod_api::horizon_part]
impl BlockEntityPart for MachineTicks {
    fn block_entity_client_ticks(
        &self,
        _host: &dyn ClientHost,
        event: &mut BlockEntityClientTicksEvent<'_>,
    ) {
        if matches!(
            event.block,
            "create:mechanical_mixer"
                | "create:millstone"
                | "create:crushing_wheel_controller"
                | "create:blaze_burner"
        ) {
            event.ticks = true;
        }
    }

    fn block_entity_client_tick(
        &self,
        host: &mut dyn ClientHost,
        event: &mut BlockEntityClientTickEvent<'_>,
    ) {
        let Some(id) = states::id(event.state) else {
            return;
        };
        let Some((block, info)) =
            crate::glue::client_table(&*host, |t| t.get(id).map(|(b, i)| (b, *i))).flatten()
        else {
            return;
        };
        let nbt = &event.entity.nbt;
        let pos = event.pos;
        match block {
            Block::MechanicalMixer => mixer_tick(host, pos, nbt),
            Block::Millstone => millstone_tick(host, pos, nbt),
            Block::CrushingWheelController => crusher_tick(host, pos, nbt),
            Block::BlazeBurner => burner_tick(host, pos, info.heat.unwrap_or(Heat::None)),
            _ => {}
        }
    }
}

/// The client's block entity data at `pos`.
fn data_at(host: &dyn ClientHost, pos: [i32; 3]) -> Option<Compound> {
    let level = host.level(host.dimension())?;
    host.block_entity(level, pos)?.data(host)
}

/// The client's game time.
fn game_time(host: &dyn ClientHost) -> i64 {
    host.level(host.dimension())
        .and_then(|l| l.game_time(host))
        .unwrap_or(0)
}

/// `MechanicalMixerBlockEntity.tick` on the client at the bottom of its cycle (`runningTicks ==
/// 20`, where it waits for the server): its `tickAudio` crunches (`AllSoundEvents.MIXING` at
/// .75, every tick, every other tick under 65 rpm), and `renderParticles` spills a crumb of each
/// stack and fluid in the basin.
fn mixer_tick(host: &mut dyn ClientHost, pos: [i32; 3], nbt: &Compound) {
    let Some(Tag::Compound(c)) = nbt.get(&basin_glue::MIXER_KEY.into()) else {
        return;
    };
    let (Some(Tag::Int(told)), Some(Tag::Long(at))) = (c.get(&"T".into()), c.get(&"S".into()))
    else {
        return;
    };
    if shown_ticks(*told, game_time(&*host) - at) != 20 {
        return;
    }
    let speed = kinetics::speeds_of(nbt)[0];
    let ticks = SHARED.with_borrow(|s| s.ticks);
    if !(speed.abs() < 65.0 && ticks % 2 == 0) {
        // `MIXING`: Create's compound of gilded blackstone and netherrack breaking, each at
        // .125 of the volume and half the pitch it is played at.
        let centre = pos.map(|v| f64::from(v) + 0.5);
        for sound in [
            "minecraft:block.gilded_blackstone.break",
            "minecraft:block.netherrack.break",
        ] {
            let play = SoundPlay::at(sound, SoundSource::Blocks, centre)
                .with_volume(0.125 * 0.75)
                .with_pitch(0.5);
            let _ = host.play_local_sound(&play, 0);
        }
    }
    let Some(basin) = data_at(&*host, [pos[0], pos[1] - 2, pos[2]]) else {
        return;
    };
    let mut options = Vec::new();
    for (id, _) in client::basin_items(&basin) {
        if let Some(o) = stacks::stack(&*host, &id, 1).and_then(|s| host.item_particle_options(&s))
        {
            options.push(o);
        }
    }
    for (fluid, _) in client::basin_fluids(&basin) {
        options.extend(fluid_particle(fluid));
    }
    for o in options {
        spill(host, pos, speed, &o);
    }
}

/// `MechanicalMixerBlockEntity.spillParticle`: a crumb flung from the head's rim a quarter block
/// out, turning the mixer's way, below it in the basin.
fn spill(host: &mut dyn ClientHost, pos: [i32; 3], speed: f32, o: &ParticleOptions) {
    let offset = rotate_y([0.0, 0.0, 0.25], random() * 360.0);
    let turn = if speed > 0.0 { 25.0 } else { -25.0 };
    let mut target = rotate_y(offset, turn);
    target[1] += 0.25;
    let r = offset_randomly(1.0 / 128.0);
    let motion = [0, 1, 2].map(|i| target[i] - offset[i] + r[i]);
    let at = [
        f64::from(pos[0]) + 0.5 + offset[0],
        f64::from(pos[1]) + 0.5 + offset[1] - 1.75,
        f64::from(pos[2]) + 0.5 + offset[2],
    ];
    host.add_particle(o, at, motion);
}

/// `FluidFX.getFluidParticle`: a crumb of the fluid's still sprite in 0.8 of its tint
/// (`FluidStackParticle`), as the pipes draw it.
fn fluid_particle(fluid: Fluid) -> Option<ParticleOptions> {
    let keys = KEYS.get()?;
    let tint = match fluid {
        Fluid::Water => 0x3F_76E4,
        Fluid::Potion(p) => p.color(),
        _ => 0xFF_FFFF,
    };
    let [r, g, b] = [16, 8, 0].map(|s| 0.8 * ((tint >> s) & 0xFF) as f32 / 255.0);
    let sprite = match fluid {
        Fluid::Water => "block/water_still",
        Fluid::Lava => "block/lava_still",
        Fluid::Honey => "create:block/fluid/honey_still",
        Fluid::Chocolate => "create:block/fluid/chocolate_still",
        Fluid::Milk => "create:block/fluid/milk_still",
        Fluid::Tea => "create:block/fluid/tea_still",
        Fluid::Potion(_) => "create:block/fluid/potion_still",
    };
    let mut options = ParticleOptions::item(keys.item, vec![sprite.into()]);
    options.color = [r, g, b, 1.0];
    Some(options)
}

/// What a millstone's or controller's data says it holds (`I`), and whether it waits (`F`).
fn shown_input(nbt: &Compound, key: &str) -> Option<(String, bool)> {
    let Some(Tag::Compound(c)) = nbt.get(&key.into()) else {
        return None;
    };
    let Some(Tag::String(id)) = c.get(&"I".into()) else {
        return None;
    };
    let waits = c.get(&"F".into()) == Some(&Tag::Byte(1));
    Some((id.to_string().ok()?, waits))
}

/// `MillstoneBlockEntity.tick` on the client and its `tickAudio`: turning with an input, it
/// sounds (`SoundScapes.MILLING`); with room in its outputs, its timer runs and it throws a crumb
/// of its input off its rim each tick, turning its way.
fn millstone_tick(host: &mut dyn ClientHost, pos: [i32; 3], nbt: &Compound) {
    let speed = kinetics::speeds_of(nbt)[0];
    if speed == 0.0 {
        return;
    }
    let Some((item, full)) = shown_input(nbt, MILL_KEY) else {
        return;
    };
    play_scape(&*host, Group::Milling, pos, machine_pitch(speed));
    if full {
        return;
    }
    let Some(o) = stacks::stack(&*host, &item, 1).and_then(|s| host.item_particle_options(&s))
    else {
        return;
    };
    // `spawnParticles`.
    let offset = rotate_y([0.0, 0.0, 0.5], random() * 360.0);
    let turn = if speed > 0.0 { 25.0 } else { -25.0 };
    let target = rotate_y(offset, turn);
    let r = offset_randomly(1.0 / 128.0);
    let motion = [0, 1, 2].map(|i| target[i] - offset[i] + r[i]);
    let at = [0, 1, 2].map(|i| f64::from(pos[i]) + 0.5 + offset[i]);
    host.add_particle(&o, at, motion);
}

/// `CrushingWheelControllerBlockEntity.tick` on the client between wheels that crush (`VALID`):
/// holding an entity or an input it sounds (`SoundScapes.CRUSHING`, always at pitch .85: its
/// speed is a wheel's / 50); crushing an input it throws 4 crumbs of it about its block each tick
/// (Create throws a block item's block particles, here its item's crumbs: the game gives no
/// block's particle sprite). And now and then a spark (`animateTick`'s `CRIT`).
fn crusher_tick(host: &mut dyn ClientHost, pos: [i32; 3], nbt: &Compound) {
    let valid = host
        .level(host.dimension())
        .and_then(|l| l.block_property(&*host, pos, "valid"))
        .is_some_and(|v| v == "true");
    if !valid {
        return;
    }
    if animated(&*host, pos)
        && let Some(keys) = KEYS.get()
    {
        let at = [0, 1, 2].map(|i| f64::from(pos[i]) + random());
        host.add_particle(&ParticleOptions::simple(keys.crit), at, [0.0; 3]);
    }
    let Some((item, _)) = shown_input(nbt, CRUSH_KEY) else {
        return;
    };
    play_scape(&*host, Group::Crushing, pos, machine_pitch(0.0));
    if item.is_empty() {
        return;
    }
    let Some(o) = stacks::stack(&*host, &item, 1).and_then(|s| host.item_particle_options(&s))
    else {
        return;
    };
    for _ in 0..4 {
        let at = [0, 1, 2].map(|i| f64::from(pos[i]) + random());
        host.add_particle(&o, at, [0.0; 3]);
    }
}

/// `BlazeBurnerBlockEntity.spawnParticles` each client tick for a burner with a blaze: a tick in
/// four, smoke (always under nothing solid, else one time in eight) and a flame, a soul flame when
/// seething, rising from its rim; and its crackle (`animateTick`: one call in ten, the campfire's
/// crackle).
fn burner_tick(host: &mut dyn ClientHost, pos: [i32; 3], heat: Heat) {
    if heat == Heat::None {
        return;
    }
    let Some(keys) = KEYS.get() else {
        return;
    };
    let centre = pos.map(|v| f64::from(v) + 0.5);
    if animated(&*host, pos) && random() < 0.1 {
        let play = SoundPlay::at("minecraft:block.campfire.crackle", SoundSource::Blocks, centre)
            .with_volume(0.5 + random() as f32)
            .with_pitch(random() as f32 * 0.7 + 0.6);
        let _ = host.play_local_sound(&play, 0);
    }
    let r = offset_randomly(0.125);
    let v = [centre[0] + r[0], centre[1], centre[2] + r[2]];
    if (random() * 4.0) as u32 != 0 {
        return;
    }
    let above = [pos[0], pos[1] + 1, pos[2]];
    let empty = host.level(host.dimension()).is_none_or(|l| {
        l.block_state(&*host, above).is_none_or(|s| {
            l.block_shape(&*host, s, above, mod_api::BlockShapeKind::Collision)
                .is_none_or(|b| b.is_empty())
        })
    });
    if empty || (random() * 8.0) as u32 == 0 {
        host.add_particle(&ParticleOptions::simple(keys.large_smoke), v, [0.0; 3]);
    }
    let y_motion = if empty { 0.0625 } else { random() * 0.0125 };
    let o = offset_randomly(0.5);
    let o = [o[0], o[1] * 0.25, o[2]];
    let len = (o[0] * o[0] + o[1] * o[1] + o[2] * o[2]).sqrt();
    // `Vec3.normalize`: a zero vector stays zero.
    let unit = if len < 1e-4 { [0.0; 3] } else { o.map(|c| c / len) };
    let out = (if empty { 0.25 } else { 0.5 }) + random() * 0.125;
    let v2 = [
        centre[0] + unit[0] * out,
        centre[1] + unit[1] * out + 0.5,
        centre[2] + unit[2] * out,
    ];
    let flame = if heat >= Heat::Seething {
        keys.soul_fire_flame
    } else if heat >= Heat::Fading {
        keys.flame
    } else {
        return;
    };
    host.add_particle(&ParticleOptions::simple(flame), v2, [0.0, y_motion, 0.0]);
}

/// `BlazeBurnerBlockEntity.spawnParticleBurst`: 20 flames (soul flames for special fuel) thrown
/// off the burner's rim as fuel goes in, sent from the server (Create draws them on the feeding
/// player's client).
pub fn burner_burst(lv: &mut crate::glue::server::ServerLevel<'_>, pos: [i32; 3], soul: bool) {
    let Some(keys) = KEYS.get() else {
        return;
    };
    let options = ParticleOptions::simple(if soul {
        keys.soul_fire_flame
    } else {
        keys.flame
    });
    let mut rng = Rng::new(lv.seed_at(pos) ^ 0xB1A2E);
    let mut next = || f64::from(rng.next_float());
    for _ in 0..20 {
        let o = [0; 3].map(|_| (next() - 0.5) * 2.0 * 0.5);
        let o = [o[0], o[1] * 0.25, o[2]];
        let len = (o[0] * o[0] + o[1] * o[1] + o[2] * o[2]).sqrt();
        let unit = if len < 1e-4 { [0.0; 3] } else { o.map(|c| c / len) };
        let out = 0.5 + next() * 0.125;
        let at = [
            f64::from(pos[0]) + 0.5 + unit[0] * out,
            f64::from(pos[1]) + 0.5 + unit[1] * out + 0.125,
            f64::from(pos[2]) + 0.5 + unit[2] * out,
        ];
        let motion = unit.map(|c| c / 32.0);
        let _ = lv
            .level
            .send_particles(&mut *lv.host, &options, None, at, motion, 1.0, 0);
    }
}

/// Item crumbs sent from the server to the players near `at`, each moving by its own
/// `motion(rng)` (a count of 0 sends one particle moving exactly so).
pub fn send_crumbs(
    lv: &mut crate::glue::server::ServerLevel<'_>,
    stack: &mod_api::ItemStack,
    at: [f64; 3],
    count: usize,
    mut motion: impl FnMut(&mut Rng) -> [f64; 3],
    seed: u64,
) {
    let Some(keys) = KEYS.get() else {
        return;
    };
    let mut one = stack.clone();
    if one.set_count(1).is_err() {
        return;
    }
    let options = ParticleOptions::item_stack(keys.item, one);
    let mut rng = Rng::new(seed);
    for _ in 0..count {
        let m = motion(&mut rng);
        let _ = lv
            .level
            .send_particles(&mut *lv.host, &options, None, at, m, 1.0, 0);
    }
}

/// A random offset of up to `radius` along each axis (`VecHelper.offsetRandomly(ZERO, r, ..)`).
pub fn offset_with(rng: &mut Rng, radius: f64) -> [f64; 3] {
    [0; 3].map(|_| (f64::from(rng.next_float()) - 0.5) * 2.0 * radius)
}

/// What a client of the millstone or controller at `pos` is shown of what it works (`MILL_KEY`,
/// `CRUSH_KEY`), told when it changes.
pub fn show_input(
    lv: &mut crate::glue::server::ServerLevel<'_>,
    pos: [i32; 3],
    key: &'static str,
    shown: Option<(String, bool)>,
) {
    let dim = lv.dim;
    // Every 40 ticks the block entity's data is looked at even when nothing changed here: a
    // machine placed again where one was starts with none.
    let recheck = lv.game_time() % 40 == 0;
    let changed = SHOWN.with_borrow_mut(|s| {
        let k = (dim, pos, key);
        let same = s.get(&k) == Some(&shown) || (shown.is_none() && !s.contains_key(&k));
        if same && !recheck {
            return false;
        }
        match &shown {
            Some(v) => s.insert(k, Some(v.clone())),
            None => s.remove(&k),
        };
        true
    });
    if !changed {
        return;
    }
    let value = shown.map(|(item, waits)| {
        let mut c = Compound::from([("I".into(), Tag::String(item.as_str().into()))]);
        if waits {
            c.insert("F".into(), Tag::Byte(1));
        }
        Tag::Compound(c)
    });
    basin_glue::set_shown(lv, pos, &[(key, value)]);
}

thread_local! {
    /// What each millstone's and controller's clients were last shown.
    static SHOWN: RefCell<HashMap<(usize, [i32; 3], &'static str), Option<(String, bool)>>> =
        RefCell::new(HashMap::new());
}

/// Forgets what was kept for the world.
pub fn clear() {
    SHOWN.with_borrow_mut(HashMap::clear);
}

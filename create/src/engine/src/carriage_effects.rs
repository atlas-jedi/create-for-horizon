//! What carriages sound and show as they run (`CarriageSounds`, `CarriageParticles`) and their
//! arrival's chimes (`ArrivalSoundQueue`, `CarriageContraptionEntity.tickArrivalSound`).
//!
//! - A client hears each train's steam puffs at its first carriage's nearest bogey as it runs,
//!   and each carriage lets its steam off as it comes to a stop; it sees each running bogey's
//!   wheels spark on the rails and smoke as the train stops. The rolling loops are left out
//!   (`HORIZON_GAPS.md`: a playing sound can't follow its train nor change pitch).
//! - The server plays the bells, note blocks and steam whistles carriages carry, front to back
//!   (back to front backing in), as a train ends a journey longer than 100 blocks.

use std::collections::BTreeMap;

pub type Vec3 = [f64; 3];

fn sub(a: Vec3, b: Vec3) -> Vec3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn add(a: Vec3, b: Vec3) -> Vec3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn scale(a: Vec3, s: f64) -> Vec3 {
    [a[0] * s, a[1] * s, a[2] * s]
}
fn length(a: Vec3) -> f64 {
    (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt()
}
/// `Vec3.normalize`: zero stays zero.
fn normalize(a: Vec3) -> Vec3 {
    let l = length(a);
    if l < 1.0e-4 {
        [0.0; 3]
    } else {
        scale(a, 1.0 / l)
    }
}
fn dot(a: Vec3, b: Vec3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// `VecHelper.rotate` about x, degrees.
pub fn rotate_x(v: Vec3, deg: f64) -> Vec3 {
    let (s, c) = deg.to_radians().sin_cos();
    [v[0], v[1] * c - v[2] * s, v[2] * c + v[1] * s]
}

/// `VecHelper.rotate` about y, degrees.
pub fn rotate_y(v: Vec3, deg: f64) -> Vec3 {
    let (s, c) = deg.to_radians().sin_cos();
    [v[0] * c + v[2] * s, v[1], v[2] * c - v[0] * s]
}

/// `Mth.clampedLerp(start, end, delta)`.
fn clamped_lerp(start: f64, end: f64, delta: f64) -> f64 {
    if delta < 0.0 {
        start
    } else if delta > 1.0 {
        end
    } else {
        start + delta * (end - start)
    }
}

/// Catnip's `LerpedFloat`, linear, chased by `Chaser.exp(max)`: each tick the gap times the
/// speed, no more than `max` (`EXP` has none), landing on the target within `Mth.equal`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Lerped {
    pub value: f32,
}

impl Lerped {
    /// `chase(target, speed, Chaser.exp(max))` then `tickChaser`.
    pub fn chase(&mut self, target: f64, speed: f64, max: f64) {
        let target = target as f32;
        if (target - self.value).abs() < 1.0e-5 {
            self.value = target;
            return;
        }
        let c = f64::from(self.value);
        let step = ((f64::from(target) - c) * speed).clamp(-max, max);
        self.value = (c + step) as f32;
    }
}

/// A sound a client plays where it is (`SoundEntry.playAt`, `playLocalSound`).
#[derive(Clone, Debug, PartialEq)]
pub struct OneShot {
    pub sound: &'static str,
    pub at: Vec3,
    pub volume: f32,
    pub pitch: f32,
}

/// What a carriage's sounds read of it and the camera each client tick.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Heard {
    /// The camera's eyes (`getEyePosition`) and the camera entity's motion
    /// (`getDeltaMovement`).
    pub camera: Vec3,
    pub camera_motion: Vec3,
    /// The carriage's motion this tick (`position - prevPosition`).
    pub motion: Vec3,
    /// Its rotation anchors (`leadingAnchor`, `trailingAnchor`: its bogeys, or its one bogey's
    /// wheels).
    pub leading: Vec3,
    pub trailing: Vec3,
    /// Its train's first carriage (`carriageIndex == 0`).
    pub first: bool,
    /// The camera rides a carriage (`getVehicle() instanceof CarriageContraptionEntity`).
    pub seated: bool,
    /// An actor stalls its train (`isStalled`): it has not arrived anywhere.
    pub stalled: bool,
}

/// A carriage's `CarriageSounds` (no loops): how fast and near it is, as heard.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sounds {
    pub distance: Lerped,
    pub speed: Lerped,
    pub approach: Lerped,
    pub seat: Lerped,
    /// Stopped and its steam let off (`arrived`, true at first).
    pub arrived: bool,
    pub tick: i32,
}

impl Default for Sounds {
    fn default() -> Self {
        Sounds {
            distance: Lerped::default(),
            speed: Lerped::default(),
            approach: Lerped::default(),
            seat: Lerped::default(),
            arrived: true,
            tick: 0,
        }
    }
}

/// `STEAM`, and the steam let off as a carriage stops.
pub const STEAM: &str = "create:steam";
pub const EXTINGUISH: &str = "minecraft:block.lava.extinguish";
pub const TRAPDOOR_CLOSE: &str = "minecraft:block.wooden_trapdoor.close";

impl Sounds {
    /// `CarriageSounds.tick` without its looping sounds: the first carriage builds its train's
    /// steam (`accumulatedSteamRelease`, up to 10, halved each tick it stands) and puffs it every
    /// four ticks (louder every eighth, and a deeper one every sixteenth) at its nearest anchor;
    /// a carriage coming to a stop with steam built lets it off (a hiss, a clunk and a sigh).
    pub fn tick(&mut self, h: &Heard, steam: &mut f32) -> Vec<OneShot> {
        let mut out = Vec::new();
        self.tick += 1;
        let moved = length(h.motion);
        let combined = sub(h.motion, h.camera_motion);
        if self.arrived && moved > 0.01 {
            self.arrived = false;
        }
        if self.arrived && h.first {
            *steam /= 2.0;
        }
        self.arrived |= h.stalled;
        if h.first {
            *steam = (*steam + (moved / 10.0).min(0.5) as f32).min(10.0);
        }
        let to_lead = sub(h.leading, h.camera);
        let to_trail = sub(h.trailing, h.camera);
        let (d1, d2) = (length(to_lead), length(to_trail));
        let to_carriage = if d1 > d2 { to_trail } else { to_lead };
        let distance = d1.min(d2);
        let at = add(h.camera, to_carriage);
        let facing = dot(normalize(to_carriage), normalize(combined));
        self.speed.chase(moved, 0.25, 0.05);
        self.distance.chase(
            clamped_lerp(100.0, 0.0, (distance - 3.0) / 64.0),
            0.25,
            50.0,
        );
        self.approach
            .chase(clamped_lerp(50.0, 200.0, 0.5 * (facing + 1.0)), 0.25, 10.0);
        self.seat
            .chase(if h.seated { 1.0 } else { 0.0 }, 0.1, f64::MAX);
        let volume = self
            .speed
            .value
            .min(self.distance.value / 100.0)
            .min(self.approach.value / 300.0 + 0.0125);
        if h.first {
            let v = volume * (1.0 - self.seat.value * 0.35) * 0.75;
            if (3 + self.tick) % 4 == 0 {
                let loud = if (self.tick + 7) % 8 == 0 { 0.75 } else { 0.45 };
                out.push(OneShot {
                    sound: STEAM,
                    at,
                    volume: v * loud,
                    pitch: 1.17,
                });
            }
            if self.tick % 16 == 0 {
                out.push(OneShot {
                    sound: STEAM,
                    at,
                    volume: v * 1.5,
                    pitch: 0.8,
                });
            }
        }
        if !self.arrived && self.speed.value < 0.002 && *steam > 1.0 {
            self.arrived = true;
            let release = *steam / 10.0;
            out.push(OneShot {
                sound: EXTINGUISH,
                at,
                volume: 0.25 * release,
                pitch: 0.78,
            });
            out.push(OneShot {
                sound: TRAPDOOR_CLOSE,
                at,
                volume: 0.2 * release,
                pitch: 1.5,
            });
            out.push(OneShot {
                sound: STEAM,
                at,
                volume: 0.75 * release,
                pitch: 0.5,
            });
        }
        out
    }
}

/// A bogey as `CarriageParticles` sees it: its anchor, and its `yaw` and `pitch`
/// (`CarriageBogey.updateAngles`: the yaw `-(atan2(dz, dx) + 90)` of its leading wheel from its
/// trailing one, the pitch their slope).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Bogey {
    pub anchor: Vec3,
    pub yaw: f64,
    pub pitch: f64,
}

impl Bogey {
    /// A bogey at `anchor` whose wheels line up along `front` (its leading wheel's way).
    pub fn facing(anchor: Vec3, front: Vec3) -> Bogey {
        let flat = (front[0] * front[0] + front[2] * front[2]).sqrt();
        Bogey {
            anchor,
            yaw: -(front[2].atan2(front[0]).to_degrees() + 90.0),
            pitch: front[1].atan2(flat).to_degrees(),
        }
    }
}

/// A particle a client adds (`level.addParticle`): a contact spark (`contactParticle`, the
/// standard bogeys' `CRIT`) or smoke (`smokeParticle`, `POOF`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Puff {
    pub spark: bool,
    pub at: Vec3,
    pub velocity: Vec3,
}

/// A carriage's `CarriageParticles`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Particles {
    pub arrived: bool,
    pub depressurise: i32,
    pub previous: f64,
    pub brakes: Lerped,
}

impl Default for Particles {
    fn default() -> Self {
        Particles {
            arrived: true,
            depressurise: 0,
            previous: 0.0,
            brakes: Lerped::default(),
        }
    }
}

/// `CarriageParticles.tick`'s reach: a carriage whose leading anchor is farther from the camera
/// shows nothing.
pub const PARTICLE_RANGE: f64 = 64.0;

impl Particles {
    /// `CarriageParticles.tick`: its bogeys' wheels spark on the rails while it runs faster than
    /// an eighth of a block a tick, more as it brakes; as it stops it lets its train's steam off
    /// for a while (`depressurise`, two ticks per unit of steam), smoke puffing at its wheels.
    /// A stalled train (`isStalled`) has not arrived. `float()` is `RandomSource.nextFloat`,
    /// `int4()` is `nextInt(4)`.
    pub fn tick(
        &mut self,
        motion: Vec3,
        bogeys: &[Bogey],
        steam: f32,
        stalled: bool,
        float: &mut dyn FnMut() -> f32,
        int4: &mut dyn FnMut() -> i32,
    ) -> Vec<Puff> {
        let mut out = Vec::new();
        let moved = length(motion);
        if self.arrived && moved > 0.01 {
            self.arrived = false;
        }
        self.arrived |= stalled;
        let stopped = moved < 0.002;
        if stopped {
            if !self.arrived {
                self.arrived = true;
                self.depressurise = (20.0 * steam / 10.0) as i32;
            }
        } else {
            self.depressurise = 0;
        }
        if self.depressurise > 0 {
            self.depressurise -= 1;
        }
        let braking = self.previous > moved + moved / 512.0;
        self.brakes
            .chase(if braking { 1.0 } else { 0.0 }, 0.25, 0.625);
        self.previous = moved;
        for b in bogeys {
            let spark = self.depressurise == 0 || self.depressurise > 10;
            let mut cutoff: f32 = if moved < 1.0 / 8.0 { 0.0 } else { 1.0 / 8.0 };
            if moved > 1.0 / 6.0 {
                cutoff = cutoff.max(self.brakes.value * 1.15);
            }
            for j in [1.0, -1.0] {
                if float() > cutoff && (spark || int4() == 0) {
                    continue;
                }
                for i in [1.0, -1.0] {
                    if float() > cutoff && (spark || int4() == 0) {
                        continue;
                    }
                    let v = [j * 1.15, if spark { -0.6 } else { 0.32 }, i];
                    let m = [
                        j * if spark { 0.5 } else { 0.25 },
                        if spark { 0.49 } else { -0.29 },
                        0.0,
                    ];
                    let turn = |p: Vec3| rotate_y(rotate_x(p, b.pitch), b.yaw);
                    out.push(Puff {
                        spark,
                        at: add(b.anchor, turn(v)),
                        velocity: add(turn(m), scale(motion, 0.75)),
                    });
                }
            }
        }
        out
    }
}

/// `spawnDerailParticles`: a derailed carriage smokes (`CAMPFIRE_COSY_SMOKE`) a twentieth of its
/// ticks, at its place moved by an offset it keeps (`derailParticleOffset`), rising
/// [`DERAIL_RISE`] a tick.
pub const DERAIL_SMOKE_CHANCE: f32 = 1.0 / 20.0;
pub const DERAIL_RISE: f64 = 0.04;

/// `derailParticleOffset` from three `nextFloat` rolls (`VecHelper.offsetRandomly(ZERO, r, 1.5)`
/// flattened to a quarter: up to 1.5 blocks either way across, 0.375 up or down).
pub fn derail_offset(rolls: [f32; 3]) -> Vec3 {
    let d = |r: f32| (f64::from(r) - 0.5) * 2.0 * 1.5;
    [d(rolls[0]), d(rolls[1]) * 0.25, d(rolls[2])]
}

/// `ArrivalSoundQueue.isPlayable`: what an arrival plays.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Chime {
    /// A bell (`BellBlock`); the haunted bell tolls its own.
    Bell { haunted: bool },
    /// A note block's instrument (by its sound event's path) and note.
    Note { sound: &'static str, note: i32 },
    /// A steam whistle: large, and its pitch (`(small ? 12 : 0) - Pitch`).
    Whistle { large: bool, pitch: i32 },
}

impl Chime {
    /// `ArrivalSoundQueue.play`'s sound event, volume and pitch (`playSimple`: five times the
    /// volume, at the carriage).
    pub fn sound(self) -> (&'static str, f32, f32) {
        match self {
            Chime::Bell { haunted: true } => ("create:haunted_bell_use", 5.0, 1.0),
            Chime::Bell { haunted: false } => ("minecraft:block.bell.use", 5.0, 1.0),
            Chime::Note { sound, note } => (sound, 5.0, 2f32.powf((note - 12) as f32 / 12.0)),
            Chime::Whistle { large, pitch } => (
                if large {
                    "create:whistle_train_low"
                } else {
                    "create:whistle_train"
                },
                5.0,
                2f32.powf(pitch as f32 / 12.0),
            ),
        }
    }
}

/// `NoteBlockInstrument.getSoundEvent` by its `instrument` value.
pub fn note_sound(instrument: &str) -> Option<&'static str> {
    Some(match instrument {
        "harp" => "minecraft:block.note_block.harp",
        "basedrum" => "minecraft:block.note_block.basedrum",
        "snare" => "minecraft:block.note_block.snare",
        "hat" => "minecraft:block.note_block.hat",
        "bass" => "minecraft:block.note_block.bass",
        "flute" => "minecraft:block.note_block.flute",
        "bell" => "minecraft:block.note_block.bell",
        "guitar" => "minecraft:block.note_block.guitar",
        "chime" => "minecraft:block.note_block.chime",
        "xylophone" => "minecraft:block.note_block.xylophone",
        "iron_xylophone" => "minecraft:block.note_block.iron_xylophone",
        "cow_bell" => "minecraft:block.note_block.cow_bell",
        "didgeridoo" => "minecraft:block.note_block.didgeridoo",
        "bit" => "minecraft:block.note_block.bit",
        "banjo" => "minecraft:block.note_block.banjo",
        "pling" => "minecraft:block.note_block.pling",
        "trumpet" => "minecraft:block.note_block.trumpet",
        "trumpet_exposed" => "minecraft:block.note_block.trumpet_exposed",
        "trumpet_weathered" => "minecraft:block.note_block.trumpet_weathered",
        "trumpet_oxidized" => "minecraft:block.note_block.trumpet_oxidized",
        "zombie" => "minecraft:block.note_block.imitate.zombie",
        "skeleton" => "minecraft:block.note_block.imitate.skeleton",
        "creeper" => "minecraft:block.note_block.imitate.creeper",
        "dragon" => "minecraft:block.note_block.imitate.ender_dragon",
        "wither_skeleton" => "minecraft:block.note_block.imitate.wither_skeleton",
        "piglin" => "minecraft:block.note_block.imitate.piglin",
        "custom_head" => "minecraft:ui.button.click",
        _ => return None,
    })
}

/// A train's `ArrivalSoundQueue`s on one count: what each tick of its arrival plays, by
/// carriage and block. Carriage `c` counts from where its first bogey stood from the station's
/// track as it was assembled (`setSoundQueueOffset(bogeyLocations + 1)`), each block from its
/// carriage's first bogey along the assembly direction (`capture`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ArrivalQueue {
    pub sources: BTreeMap<i32, Vec<(usize, [i32; 3], Chime)>>,
}

impl ArrivalQueue {
    /// The queue of a train heading `heading` (a unit along x or z as assembled) whose
    /// carriages' chimes are `chimes` (each carriage's blocks from its first bogey), its
    /// carriages `first bogey to first bogey` apart as `gaps` say (`bogeySpacing` and the
    /// spacing to the next).
    pub fn new(heading: [i32; 2], gaps: &[i32], chimes: &[Vec<([i32; 3], Chime)>]) -> Self {
        let mut q = ArrivalQueue::default();
        let mut offset = 1;
        for (c, car) in chimes.iter().enumerate() {
            if c > 0 {
                offset += gaps.get(c - 1).copied().unwrap_or(0);
            }
            for (o, chime) in car {
                let along = -(o[0] * heading[0] + o[2] * heading[1]);
                q.sources
                    .entry(offset + along)
                    .or_default()
                    .push((c, *o, *chime));
            }
        }
        q
    }

    /// `tickArrivalSound`'s first count: the front's (`firstTick`), backing in the back's
    /// (`lastTick`); None with nothing to play.
    pub fn start(&self, reversed: bool) -> Option<i32> {
        if reversed {
            self.sources.keys().next_back().copied()
        } else {
            self.sources.keys().next().copied()
        }
    }

    /// One count (`ArrivalSoundQueue.tick` of each carriage, every other tick): what plays at
    /// `tick`, and the next count while there is more to play.
    pub fn step(&self, tick: i32, reversed: bool) -> (&[(usize, [i32; 3], Chime)], Option<i32>) {
        let play = self.sources.get(&tick).map_or(&[][..], Vec::as_slice);
        let (Some(&min), Some(&max)) =
            (self.sources.keys().next(), self.sources.keys().next_back())
        else {
            return (play, None);
        };
        let more = if reversed { tick > min } else { tick < max };
        (
            play,
            more.then_some(if reversed { tick - 1 } else { tick + 1 }),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steam_puffs_build_while_running_and_let_off_as_it_stops() {
        let mut s = Sounds::default();
        let mut steam = 0.0f32;
        let running = Heard {
            camera: [0.0, 1.62, 3.0],
            motion: [0.5, 0.0, 0.0],
            leading: [1.0, 0.0, 0.0],
            trailing: [-1.0, 0.0, 0.0],
            first: true,
            ..Heard::default()
        };
        let mut puffs = Vec::new();
        for _ in 0..256 {
            puffs.extend(s.tick(&running, &mut steam));
        }
        assert!(!s.arrived);
        // Every fourth tick, plus a deep one every sixteenth.
        let high = puffs.iter().filter(|p| p.pitch == 1.17).count();
        let deep = puffs.iter().filter(|p| p.pitch == 0.8).count();
        assert_eq!((high, deep), (64, 16));
        assert!(puffs.iter().all(|p| p.sound == STEAM && p.volume > 0.0));
        // A twentieth of its speed a tick, up to 10.
        assert!((steam - 10.0).abs() < 1e-6, "{steam}");
        // At the nearest anchor.
        assert_eq!(puffs[0].at, [1.0, 0.0, 0.0]);
        // Stopping: once its speed has settled, the steam is let off once.
        let stopped = Heard {
            motion: [0.0; 3],
            ..running
        };
        let mut off = Vec::new();
        for _ in 0..200 {
            off.extend(
                s.tick(&stopped, &mut steam)
                    .into_iter()
                    .filter(|p| p.pitch != 1.17),
            );
        }
        let hiss: Vec<_> = off.iter().filter(|p| p.sound == EXTINGUISH).collect();
        assert_eq!(hiss.len(), 1);
        assert!(off.iter().any(|p| p.sound == TRAPDOOR_CLOSE));
        assert!(s.arrived && steam < 1.0);
        // Another carriage builds no steam and puffs none.
        let mut s = Sounds::default();
        let mut steam = 0.0f32;
        let second = Heard {
            first: false,
            ..running
        };
        let out: Vec<_> = (0..64).flat_map(|_| s.tick(&second, &mut steam)).collect();
        assert!(out.is_empty() && steam == 0.0);
    }

    #[test]
    fn a_far_carriage_is_quiet() {
        let mut s = Sounds::default();
        let mut steam = 0.0f32;
        let far = Heard {
            camera: [0.0, 0.0, 100.0],
            motion: [0.5, 0.0, 0.0],
            leading: [1.0, 0.0, 0.0],
            trailing: [-1.0, 0.0, 0.0],
            first: true,
            ..Heard::default()
        };
        let out: Vec<_> = (0..64).flat_map(|_| s.tick(&far, &mut steam)).collect();
        assert!(out.iter().all(|p| p.volume == 0.0), "{out:?}");
    }

    #[test]
    fn running_wheels_spark_on_the_rails_and_smoke_as_it_stops() {
        // A bogey heading +x: its rails are across it (z), the sparks below its axles.
        let b = Bogey::facing([10.0, 64.0, 0.5], [1.0, 0.0, 0.0]);
        let mut p = Particles::default();
        let mut always = || 0.0f32;
        let mut never4 = || 1;
        let out = p.tick([0.5, 0.0, 0.0], &[b], 0.0, false, &mut always, &mut never4);
        assert_eq!(out.len(), 4);
        for puff in &out {
            assert!(puff.spark);
            assert!((puff.at[1] - 63.4).abs() < 1e-9, "{puff:?}");
            assert!(
                ((puff.at[2] - 0.5).abs() - 1.15).abs() < 1e-9,
                "on a rail: {puff:?}"
            );
            assert!(
                ((puff.at[0] - 10.0).abs() - 1.0).abs() < 1e-9,
                "at an axle: {puff:?}"
            );
            assert!((puff.velocity[1] - 0.49).abs() < 1e-9);
            assert!(
                (puff.velocity[0] - 0.375).abs() < 1e-9,
                "with the train: {puff:?}"
            );
        }
        // Slow: no sparks (nothing passes a cutoff of 0 unless the roll is exactly 0).
        let mut p = Particles::default();
        let mut half = || 0.5f32;
        let out = p.tick([0.05, 0.0, 0.0], &[b], 0.0, false, &mut half, &mut never4);
        assert!(out.is_empty());
        // Stopping with steam built: smoke for a while (it starts 20 ticks per 10 steam).
        let mut p = Particles {
            arrived: false,
            ..Particles::default()
        };
        let out = p.tick([0.0; 3], &[b], 10.0, false, &mut half, &mut never4);
        assert_eq!(p.depressurise, 19);
        assert!(out.iter().all(|o| o.spark), "above 10: still sparks");
        let mut smoked = 0;
        for _ in 0..20 {
            smoked += p
                .tick([0.0; 3], &[b], 10.0, false, &mut half, &mut never4)
                .iter()
                .filter(|o| !o.spark)
                .count();
        }
        assert!(smoked > 0);
        assert_eq!(p.depressurise, 0);
    }

    #[test]
    fn a_stalled_train_lets_no_steam_off_and_a_derailed_one_smokes_nearby() {
        // Stalled by an actor: stopping is no arrival.
        let mut p = Particles {
            arrived: false,
            ..Particles::default()
        };
        let mut half = || 0.5f32;
        let mut never4 = || 1;
        p.tick([0.0; 3], &[], 10.0, true, &mut half, &mut never4);
        assert_eq!(p.depressurise, 0);
        let mut s = Sounds {
            arrived: false,
            ..Sounds::default()
        };
        let mut steam = 10.0f32;
        let stalled = Heard {
            first: true,
            stalled: true,
            ..Heard::default()
        };
        assert!(
            s.tick(&stalled, &mut steam)
                .iter()
                .all(|o| o.sound == STEAM)
        );
        let o = derail_offset([0.0, 1.0, 0.5]);
        assert_eq!(o, [-1.5, 0.375, 0.0]);
    }

    #[test]
    fn the_bogey_s_angles_follow_create() {
        // `atan2(dz, dx) + 90`, negated: heading +x is yaw -90, +z is -180.
        assert!((Bogey::facing([0.0; 3], [1.0, 0.0, 0.0]).yaw + 90.0).abs() < 1e-9);
        assert!((Bogey::facing([0.0; 3], [0.0, 0.0, 1.0]).yaw + 180.0).abs() < 1e-9);
        let up = Bogey::facing([0.0; 3], [1.0, 1.0, 0.0]);
        assert!((up.pitch - 45.0).abs() < 1e-9);
    }

    #[test]
    fn an_arrival_plays_front_to_back() {
        let bell = Chime::Bell { haunted: false };
        let whistle = Chime::Whistle {
            large: false,
            pitch: 12,
        };
        // Heading +x: a block further along -x is further back. Two carriages, the second's
        // first bogey 7 behind the first's.
        let chimes = vec![
            vec![([1, 2, 0], bell), ([-2, 1, 0], whistle)],
            vec![([0, 1, 0], bell)],
        ];
        let q = ArrivalQueue::new([1, 0], &[7], &chimes);
        assert_eq!(q.sources.keys().copied().collect::<Vec<_>>(), vec![0, 3, 8]);
        let mut tick = q.start(false);
        let mut played = Vec::new();
        while let Some(t) = tick {
            let (now, next) = q.step(t, false);
            played.extend(now.iter().map(|(c, o, _)| (*c, *o)));
            tick = next;
        }
        assert_eq!(
            played,
            vec![(0, [1, 2, 0]), (0, [-2, 1, 0]), (1, [0, 1, 0])]
        );
        // Backing in: back to front.
        let (now, next) = q.step(q.start(true).unwrap(), true);
        assert_eq!(now[0].0, 1);
        assert_eq!(next, Some(7));
        assert!(ArrivalQueue::default().start(false).is_none());
    }

    #[test]
    fn chimes_sound_as_create_plays_them() {
        assert_eq!(
            Chime::Bell { haunted: false }.sound(),
            ("minecraft:block.bell.use", 5.0, 1.0)
        );
        let (s, v, p) = Chime::Note {
            sound: note_sound("flute").unwrap(),
            note: 24,
        }
        .sound();
        assert_eq!((s, v), ("minecraft:block.note_block.flute", 5.0));
        assert!((p - 2.0).abs() < 1e-6);
        let (s, _, p) = Chime::Whistle {
            large: true,
            pitch: -12,
        }
        .sound();
        assert_eq!(s, "create:whistle_train_low");
        assert!((p - 0.5).abs() < 1e-6);
        assert!(note_sound("nothing").is_none());
    }
}

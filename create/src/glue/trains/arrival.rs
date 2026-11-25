//! A train's arrival chimes (`CarriageContraptionEntity.tickArrivalSound`,
//! `ArrivalSoundQueue`): a train that set off for a station more than 100 blocks away plays its
//! carriages' bells, note blocks and steam whistles one block a count, every other tick, from its
//! front to its back (backing in, back to front), once it is within 60 blocks of where it goes.
//! Each plays at its carriage (`playSound(null, entity, ...)`: here at the carriage's place as
//! it starts, as a carriage is no entity), five times as loud as the block would.

use super::super::server::ServerLevel;
use super::super::world::with_world;
use create_engine::carriage_effects::{ArrivalQueue, Chime, note_sound};
use mod_api::{Host, SoundSource};
use std::cell::RefCell;
use std::collections::HashMap;

/// An arrival being played: the train's queue, the count it is at and whether it backs in.
struct Playing {
    queue: ArrivalQueue,
    tick: i32,
    reversed: bool,
}

thread_local! {
    static PLAYING: RefCell<HashMap<(usize, u32), Playing>> = RefCell::new(HashMap::new());
}

pub fn forget() {
    PLAYING.with_borrow_mut(HashMap::clear);
}

/// `ArrivalSoundQueue.isPlayable` and `play`'s sound for a carriage block.
fn chime(host: &dyn Host, blocks: &[([i32; 3], u32)], o: [i32; 3], state: u32) -> Option<Chime> {
    let (name, props) = super::state_parts(host, super::car_state(state)?)?;
    let prop = |k: &str| props.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str());
    match name.as_str() {
        // `BellBlock`: the vanilla bell and Create's peculiar and haunted bells.
        "minecraft:bell" | "create:peculiar_bell" => Some(Chime::Bell { haunted: false }),
        "create:haunted_bell" => Some(Chime::Bell { haunted: true }),
        "minecraft:note_block" => Some(Chime::Note {
            sound: note_sound(prop("instrument")?)?,
            note: prop("note")?.parse().ok()?,
        }),
        "create:steam_whistle" => {
            let (large, pitch) = super::horn::whistle_at(host, blocks, o, state)?;
            Some(Chime::Whistle { large, pitch })
        }
        _ => None,
    }
}

/// The level's tick, after the trains moved: a train due its arrival (`announceArrival` and
/// near) starts it when its leading carriage that way (the last one backing) is where entities
/// tick; then, on odd ticks, each arrival plays its count.
pub fn tick(lv: &mut ServerLevel<'_>) {
    let dim = lv.dim;
    let due: Vec<(u32, bool, [i32; 3])> = with_world(|w| {
        let r = &w.dims[dim].railway;
        r.trains
            .iter()
            .filter(|t| t.train.bound && !t.train.carriages.is_empty() && t.announce_due())
            .map(|t| {
                let reversed = t.train.speed < 0.0;
                let c = if reversed {
                    t.train.carriages.len() - 1
                } else {
                    0
                };
                let (at, _) = t.train.carriage_pose(&r.net.graph, c);
                (t.id, reversed, at.map(|v| v.floor() as i32))
            })
            .collect()
    })
    .unwrap_or_default();
    for (id, reversed, at) in due {
        if !lv.level.entity_ticking(&*lv.host, at).unwrap_or(false) {
            continue;
        }
        let Some((cars, heading, gaps)) = with_world(|w| {
            let t = w.dims[dim].railway.trains.iter_mut().find(|t| t.id == id)?;
            t.train.announce = false;
            // Carriage to carriage, first bogey to first bogey.
            let gaps: Vec<i32> = (1..t.train.carriages.len())
                .map(|c| {
                    t.train.carriages[c - 1].bogey_spacing
                        + t.train.spacing.get(c - 1).copied().unwrap_or(0)
                })
                .collect();
            Some((t.cars.clone(), t.heading, gaps))
        })
        .flatten() else {
            continue;
        };
        super::super::persist::mark_unsaved(dim, [at[0] >> 4, at[2] >> 4]);
        let chimes: Vec<Vec<([i32; 3], Chime)>> = cars
            .iter()
            .map(|car| {
                car.iter()
                    .filter_map(|(o, s)| Some((*o, chime(&*lv.host, car, *o, *s)?)))
                    .collect()
            })
            .collect();
        let queue = ArrivalQueue::new(heading, &gaps, &chimes);
        let Some(tick) = queue.start(reversed) else {
            continue;
        };
        PLAYING.with_borrow_mut(|p| {
            p.insert(
                (dim, id),
                Playing {
                    queue,
                    tick,
                    reversed,
                },
            )
        });
    }
    if PLAYING.with_borrow(|p| p.keys().all(|k| k.0 != dim)) || lv.game_time() % 2 == 0 {
        return;
    }
    let keys: Vec<(usize, u32)> =
        PLAYING.with_borrow(|p| p.keys().filter(|k| k.0 == dim).copied().collect());
    for key in keys {
        let step = PLAYING.with_borrow_mut(|p| {
            let playing = p.get_mut(&key)?;
            let (now, next) = playing.queue.step(playing.tick, playing.reversed);
            let now: Vec<(usize, Chime)> = now.iter().map(|(c, _, chime)| (*c, *chime)).collect();
            match next {
                Some(t) => playing.tick = t,
                None => {
                    p.remove(&key);
                }
            }
            Some(now)
        });
        let Some(now) = step else {
            continue;
        };
        if now.is_empty() {
            continue;
        }
        let places: Option<Vec<[f64; 3]>> = with_world(|w| {
            let r = &w.dims[dim].railway;
            let t = r.trains.iter().find(|t| t.id == key.1 && t.train.bound)?;
            Some(
                (0..t.train.carriages.len())
                    .map(|c| t.train.carriage_pose(&r.net.graph, c).0)
                    .collect(),
            )
        })
        .flatten();
        let Some(places) = places else {
            // The train went (or left the track): its arrival ends.
            PLAYING.with_borrow_mut(|p| p.remove(&key));
            continue;
        };
        for (c, chime) in now {
            let Some(&at) = places.get(c) else { continue };
            let (sound, volume, pitch) = chime.sound();
            lv.level.play_sound(
                &mut *lv.host,
                sound,
                at,
                SoundSource::Neutral,
                volume,
                pitch,
            );
        }
    }
}

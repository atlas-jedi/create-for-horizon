//! Who runs a train's schedule: a conductor, a mob sitting on a carriage seat behind train
//! controls (`CarriageContraption.conductorSeats`) or a blaze burner with its blaze behind them
//! (`blockConductors`, `ConductorBlockInteractionBehavior.BlazeBurner`), handed a schedule by a
//! click (`ScheduleItem.handScheduleTo`, `ScheduleItemEntityInteraction`); or a station holding
//! a schedule (`StationBlockEntity.applyAutoSchedule`): one dropped on it, given to each train
//! that arrives there, taken back by a click. A scheduled train runs only with a conductor
//! facing the way it goes (`Train.updateConductors` each tick, `hasForwardConductor`,
//! `hasBackwardConductor`).

use super::super::server::ServerLevel;
use super::super::world::with_world;
use super::schedule_screen::{schedule_of, with_schedule};
use crate::glue::items;
use crate::glue::names::Named as _;
use create_engine::schedule::{Runtime, Schedule};
use create_engine::train_controls::Target;
use mod_api::{
    EntityInteractEvent, EntityRef, Hand, Host, InteractionResult, PlayerRef, ServerHost,
    SoundSource, Text,
};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

/// A sound at the player (`AllSoundEvents.CONFIRM` / `DENY`, `playOnServer`).
fn sound_at(host: &mut dyn ServerHost, player: PlayerRef, id: &str, volume: f32, pitch: f32) {
    let Some(level) = player.level(&*host) else {
        return;
    };
    if let Some(at) = player.entity().position(&*host) {
        level.play_sound(host, id, at, SoundSource::Players, volume, pitch);
    }
}

fn say(host: &mut dyn ServerHost, player: PlayerRef, key: &str) {
    super::action_bar(host, player, Text::translate(key, Vec::new()));
}

/// `ScheduleRuntime.setSchedule`: the train runs `schedule` from its saved progress; a station's
/// is an auto-schedule. Its status forgets its navigation and conductor troubles
/// (`newSchedule`).
pub fn set_schedule(dim: usize, train: u32, schedule: Schedule, auto: bool) -> bool {
    with_world(|w| {
        let r = &mut w.dims[dim].railway;
        let t = r.trains.iter_mut().find(|t| t.id == train)?;
        t.runtime = Runtime::for_schedule(&schedule, auto);
        t.status.new_schedule();
        t.train.nav = None;
        t.schedule = Some(schedule);
        Some(t.column)
    })
    .flatten()
    .map(|c| super::super::persist::mark_unsaved(dim, [c.0, c.1]))
    .is_some()
}

/// `ScheduleRuntime.returnSchedule`: the train's schedule taken off it (`discardSchedule`),
/// with its progress; None for an auto-schedule, which is not given back.
pub fn take_schedule(dim: usize, train: u32) -> Option<Option<Schedule>> {
    with_world(|w| {
        let r = &mut w.dims[dim].railway;
        let t = r.trains.iter_mut().find(|t| t.id == train)?;
        let mut s = t.schedule.take()?;
        s.saved_progress = t.runtime.current;
        let auto = t.runtime.auto;
        t.runtime = Runtime::new();
        t.runtime.paused = true;
        t.train.nav = None;
        super::super::persist::mark_unsaved(dim, [t.column.0, t.column.1]);
        Some((!auto).then_some(s))
    })
    .flatten()
}

/// A train's carriages as their conductors see them, worked out once per train as Create does
/// as it assembles them (`CarriageContraption.assemble`): each carriage's train controls by
/// place and facing (a unit along x or z), and the ways its blaze burners drive it
/// (`blockConductors`). `key` tells a train given the same id apart.
struct Crew {
    key: (usize, usize, [i32; 2]),
    heading: [i32; 2],
    controls: Vec<HashMap<[i32; 3], [i32; 2]>>,
    blocks: Vec<[bool; 2]>,
}

impl Crew {
    /// `inControl` both ways for carriage `car`'s block `at`: `[forward, backward]`.
    fn sides(&self, car: usize, at: [i32; 3]) -> [bool; 2] {
        let Some(controls) = self.controls.get(car) else {
            return [false; 2];
        };
        create_engine::train_status::in_control(at, self.heading, |p| controls.get(&p).copied())
    }
}

thread_local! {
    static CREWS: RefCell<HashMap<(usize, u32), Crew>> = RefCell::new(HashMap::new());
}

/// A horizontal `facing` as a unit along x or z.
fn facing_unit(name: &str) -> Option<[i32; 2]> {
    Some(match name {
        "north" => [0, -1],
        "south" => [0, 1],
        "west" => [-1, 0],
        "east" => [1, 0],
        _ => return None,
    })
}

/// `ConductorBlockInteractionBehavior.BlazeBurner.isValidConductor`: a blaze burner with its
/// blaze (`HEAT_LEVEL` not `NONE`).
pub fn is_blaze_conductor(host: &dyn Host, state: mod_api::BlockState) -> bool {
    state.block() == super::super::states::key(create_engine::Block::BlazeBurner)
        && super::property(host, state, "blaze").is_some_and(|b| b != "none")
}

fn crew_of(host: &dyn Host, cars: &[Vec<([i32; 3], u32)>], heading: [i32; 2]) -> Crew {
    let controls_block = super::super::states::key(create_engine::Block::Controls);
    let mut crew = Crew {
        key: (cars.len(), cars.iter().map(Vec::len).sum(), heading),
        heading,
        controls: Vec::with_capacity(cars.len()),
        blocks: Vec::with_capacity(cars.len()),
    };
    for car in cars {
        let mut controls = HashMap::new();
        let mut blazes = Vec::new();
        for (o, id) in car {
            let Some(st) = super::car_state(*id) else {
                continue;
            };
            if st.block() == controls_block {
                if let Some(f) = super::property(host, st, "facing").and_then(|f| facing_unit(&f)) {
                    controls.insert(*o, f);
                }
            } else if is_blaze_conductor(host, st) {
                blazes.push(*o);
            }
        }
        let sides = blazes.iter().fold([false; 2], |a, b| {
            let s = create_engine::train_status::in_control(*b, heading, |p| {
                controls.get(&p).copied()
            });
            [a[0] || s[0], a[1] || s[1]]
        });
        crew.controls.push(controls);
        crew.blocks.push(sides);
    }
    crew
}

/// `f` of the crew of train `train` in level `dim`, worked out the first time.
fn with_crew<R>(host: &dyn Host, dim: usize, train: u32, f: impl FnOnce(&Crew) -> R) -> Option<R> {
    let key = with_world(|w| {
        let t = w.dims[dim].railway.trains.iter().find(|t| t.id == train)?;
        Some((t.cars.len(), t.cars.iter().map(Vec::len).sum::<usize>(), t.heading))
    })
    .flatten()?;
    let known = CREWS.with_borrow(|c| c.get(&(dim, train)).is_some_and(|c| c.key == key));
    if !known {
        let (cars, heading) = with_world(|w| {
            let t = w.dims[dim].railway.trains.iter().find(|t| t.id == train)?;
            Some((t.cars.clone(), t.heading))
        })
        .flatten()?;
        let crew = crew_of(host, &cars, heading);
        CREWS.with_borrow_mut(|c| c.insert((dim, train), crew));
    }
    CREWS.with_borrow(|c| c.get(&(dim, train)).map(f))
}

/// `CarriageContraption.inControl` for the carriage seat at `local` of carriage `car`: train
/// controls next to it along the train, facing it (`conductorSeats`).
fn conductor_seat(
    dim: usize,
    train: u32,
    car: usize,
    local: [i32; 3],
    host: &dyn ServerHost,
) -> bool {
    with_crew(host, dim, train, |c| c.sides(car, local)).is_some_and(|s| s[0] || s[1])
}

/// `Train.updateConductors` before the railway's tick: each carriage's conductors
/// (`checkConductors`), its blaze burners in control and the mobs on its seats in control, each
/// the ways it drives. A carriage whose riders saved with it are not seated again yet
/// (`serialisedPassengers`), or which stands where entities don't tick (its entity unloaded),
/// keeps what it had.
pub fn update(lv: &ServerLevel<'_>) {
    let dim = lv.dim;
    let trains: Vec<(u32, Vec<Option<[i32; 3]>>)> = with_world(|w| {
        let r = &w.dims[dim].railway;
        r.trains
            .iter()
            .map(|t| {
                let at = (0..t.train.carriages.len())
                    .map(|c| {
                        t.train.bound.then(|| {
                            let (pivot, _) = t.train.carriage_pose(&r.net.graph, c);
                            pivot.map(|v| v.floor() as i32)
                        })
                    })
                    .collect();
                (t.id, at)
            })
            .collect()
    })
    .unwrap_or_default();
    CREWS.with_borrow_mut(|c| c.retain(|(d, id), _| *d != dim || trains.iter().any(|t| t.0 == *id)));
    if trains.is_empty() {
        return;
    }
    let seated = super::riding::mob_seats(&*lv.host, dim);
    let waiting: HashSet<(u32, u16)> = super::riding::carriages_waiting_for_riders(dim);
    let mut found: Vec<(u32, Vec<Option<[bool; 2]>>)> = Vec::with_capacity(trains.len());
    for (id, at) in trains {
        let sides = with_crew(&*lv.host, dim, id, |crew| {
            at.iter()
                .enumerate()
                .map(|(c, pos)| {
                    let loaded = pos.is_some_and(|p| {
                        lv.level.entity_ticking(&*lv.host, p).unwrap_or(false)
                    });
                    if !loaded || waiting.contains(&(id, c as u16)) {
                        return None;
                    }
                    let mut s = crew.blocks.get(c).copied().unwrap_or_default();
                    for (_, _, local) in seated
                        .iter()
                        .filter(|(t, car, _)| *t == id && usize::from(*car) == c)
                    {
                        let seat = crew.sides(c, *local);
                        s = [s[0] || seat[0], s[1] || seat[1]];
                    }
                    Some(s)
                })
                .collect::<Vec<_>>()
        });
        if let Some(sides) = sides {
            found.push((id, sides));
        }
    }
    with_world(|w| {
        for t in &mut w.dims[dim].railway.trains {
            let Some((_, sides)) = found.iter().find(|f| f.0 == t.id) else {
                continue;
            };
            t.conductors.resize(sides.len(), [false; 2]);
            for (kept, now) in t.conductors.iter_mut().zip(sides) {
                if let Some(now) = now {
                    *kept = *now;
                }
            }
        }
    });
}

/// `ConductorBlockInteractionBehavior.handlePlayerInteraction` (main hand only): a click on a
/// blaze burner of a carriage. Behind controls, it is the train's conductor: a train paused by
/// its driver runs on; one with a schedule gives it back to an empty hand; a schedule in hand is
/// handed to a train without one. Not behind controls, it says so.
pub fn blaze_clicked(host: &mut dyn ServerHost, dim: usize, player: PlayerRef, at: Target) {
    let car = usize::from(at.car);
    let in_control = with_crew(&*host, dim, at.train, |c| c.sides(car, at.local))
        .is_some_and(|s| s[0] || s[1]);
    if !in_control {
        say(host, player, "create.schedule.non_controlling_seat");
        sound_at(host, player, "create:deny", 1.0, 1.0);
        return;
    }
    let Some((has_schedule, paused, completed, auto)) = with_world(|w| {
        let t = w.dims[dim].railway.trains.iter().find(|t| t.id == at.train)?;
        Some((
            t.schedule.is_some(),
            t.runtime.paused,
            t.runtime.completed,
            t.runtime.auto,
        ))
    })
    .flatten() else {
        return;
    };
    let held = items::held(&*host, player, Hand::Main).filter(|s| s.count() > 0);
    if has_schedule {
        if paused && !completed {
            with_world(|w| {
                if let Some(t) = w.dims[dim]
                    .railway
                    .trains
                    .iter_mut()
                    .find(|t| t.id == at.train)
                {
                    t.runtime.paused = false;
                }
            });
            sound_at(host, player, "create:confirm", 1.0, 1.0);
            say(host, player, "create.schedule.continued");
            return;
        }
        if held.is_some() {
            sound_at(host, player, "create:deny", 1.0, 1.0);
            say(host, player, "create.schedule.remove_with_empty_hand");
            return;
        }
        // `playItemPickup`, then the schedule into the hand (`returnSchedule`: none for an
        // auto-schedule).
        sound_at(host, player, "minecraft:entity.item.pickup", 0.2, 1.5);
        say(
            host,
            player,
            if auto {
                "create.schedule.auto_removed_from_train"
            } else {
                "create.schedule.removed_from_train"
            },
        );
        let stack = take_schedule(dim, at.train)
            .flatten()
            .and_then(|s| schedule_stack(&*host, &s));
        items::set_held(host, player, Hand::Main, stack);
        return;
    }
    let Some(stack) = held.filter(|s| s.item().name(&*host) == "create:schedule") else {
        return;
    };
    let Some(schedule) = schedule_of(&stack) else {
        return;
    };
    if schedule.entries.is_empty() {
        sound_at(host, player, "create:deny", 1.0, 1.0);
        say(host, player, "create.schedule.no_stops");
        return;
    }
    set_schedule(dim, at.train, schedule, false);
    crate::glue::advancements::award(host, player, "conductor");
    sound_at(host, player, "create:confirm", 1.0, 1.0);
    let mut style = mod_api::text::TextStyle::default();
    style.color = Some("green".to_owned());
    super::action_bar(
        host,
        player,
        Text::translate("create.schedule.applied_to_train", Vec::new()).with_style(style),
    );
    items::shrink_held(host, player, Hand::Main, 1);
}

/// A schedule item holding `s`.
fn schedule_stack(host: &dyn Host, s: &Schedule) -> Option<mod_api::ItemStack> {
    let key = mod_api::Identifier::parse("create:schedule")
        .ok()
        .and_then(|i| host.item_key(&i))?;
    Some(with_schedule(&mod_api::ItemStack::of(key), s))
}

/// `ScheduleItemEntityInteraction`: a click on a mob sitting on a carriage seat. With a
/// schedule, it is handed to the train if the mob is its conductor (`handScheduleTo`); with an
/// empty hand, a train paused by its driver runs on, else its schedule comes back.
pub fn interact(
    host: &mut dyn ServerHost,
    event: &mut EntityInteractEvent<'_>,
) -> Option<InteractionResult> {
    let player = host.players().acting_player()?;
    let target: EntityRef = host.entity(event.entity as u32)?;
    let seat = super::riding::seat_of(&*host, target)?;
    let dimension = player
        .level(&*host)?
        .dimension()
        .id(&*host)?;
    let dim = ServerLevel::new(host, &dimension)?.dim;
    let holding = event
        .stack
        .filter(|s| s.item().name(&*host) == "create:schedule")
        .cloned();
    let conductor = conductor_seat(dim, seat.train, usize::from(seat.car), seat.local, &*host);
    let has_schedule = with_world(|w| {
        w.dims[dim]
            .railway
            .trains
            .iter()
            .find(|t| t.id == seat.train)
            .map(|t| (t.schedule.is_some(), t.runtime.paused, t.runtime.completed))
    })
    .flatten()?;
    if let Some(stack) = holding {
        let schedule = schedule_of(&stack)?;
        if !conductor {
            say(host, player, "create.schedule.non_controlling_seat");
            sound_at(host, player, "create:deny", 1.0, 1.0);
            return Some(InteractionResult::SuccessServer);
        }
        if has_schedule.0 {
            sound_at(host, player, "create:deny", 1.0, 1.0);
            say(host, player, "create.schedule.remove_with_empty_hand");
            return Some(InteractionResult::SuccessServer);
        }
        if schedule.entries.is_empty() {
            sound_at(host, player, "create:deny", 1.0, 1.0);
            say(host, player, "create.schedule.no_stops");
            return Some(InteractionResult::SuccessServer);
        }
        set_schedule(dim, seat.train, schedule, false);
        crate::glue::advancements::award(host, player, "conductor");
        sound_at(host, player, "create:confirm", 1.0, 1.0);
        let mut style = mod_api::text::TextStyle::default();
        style.color = Some("green".to_owned());
        super::action_bar(
            host,
            player,
            Text::translate("create.schedule.applied_to_train", Vec::new()).with_style(style),
        );
        items::shrink_held(host, player, event.hand, 1);
        return Some(InteractionResult::SuccessServer);
    }
    if event.hand == Hand::Off || !has_schedule.0 || !conductor {
        return None;
    }
    let (_, paused, completed) = has_schedule;
    if paused && !completed {
        with_world(|w| {
            if let Some(t) = w.dims[dim]
                .railway
                .trains
                .iter_mut()
                .find(|t| t.id == seat.train)
            {
                t.runtime.paused = false;
            }
        });
        sound_at(host, player, "create:confirm", 1.0, 1.0);
        say(host, player, "create.schedule.continued");
        return Some(InteractionResult::SuccessServer);
    }
    if event.stack.is_some_and(|s| s.count() > 0) {
        sound_at(host, player, "create:deny", 1.0, 1.0);
        say(host, player, "create.schedule.remove_with_empty_hand");
        return Some(InteractionResult::SuccessServer);
    }
    let auto = with_world(|w| {
        w.dims[dim]
            .railway
            .trains
            .iter()
            .find(|t| t.id == seat.train)
            .is_some_and(|t| t.runtime.auto)
    })
    .unwrap_or(false);
    give_back_schedule(host, player, dim, seat.train);
    say(
        host,
        player,
        if auto {
            "create.schedule.auto_removed_from_train"
        } else {
            "create.schedule.removed_from_train"
        },
    );
    Some(InteractionResult::SuccessServer)
}

/// A schedule item holding `s` into `player`'s inventory (`placeItemBackInInventory`), with
/// `playItemPickup`'s sound.
pub fn give_schedule(host: &mut dyn ServerHost, player: PlayerRef, s: &Schedule) {
    if let Some(stack) = schedule_stack(&*host, s) {
        items::give_back(host, player, stack);
    }
    sound_at(host, player, "minecraft:entity.item.pickup", 0.2, 1.5);
}

static HAPPY_VILLAGER: std::sync::OnceLock<mod_api::ParticleKey> = std::sync::OnceLock::new();
static END_ROD: std::sync::OnceLock<mod_api::ParticleKey> = std::sync::OnceLock::new();

pub fn register(r: &mut dyn mod_api::Registrar) {
    let _ = HAPPY_VILLAGER.set(r.particle_key("minecraft:happy_villager"));
    let _ = END_ROD.set(r.particle_key("minecraft:end_rod"));
}

thread_local! {
    /// The stations a train stood at as of the last tick, by level (`trainPresent`).
    static PRESENT: std::cell::RefCell<std::collections::HashMap<usize, std::collections::HashSet<[i32; 3]>>> =
        std::cell::RefCell::new(std::collections::HashMap::new());
}

pub fn forget() {
    PRESENT.with_borrow_mut(std::collections::HashMap::clear);
    CREWS.with_borrow_mut(HashMap::clear);
}

/// `applyAutoSchedule`: the train standing at station `pos` runs the schedule it holds. With
/// `now`, whatever stood there; else only when it newly arrived (the level's tick).
pub fn apply_auto(dim: usize, pos: [i32; 3], now: bool) -> bool {
    let _ = now;
    let applied = with_world(|w| {
        let r = &mut w.dims[dim].railway;
        let s = r.auto_schedules.get(&pos)?.clone();
        if s.entries.is_empty() {
            return None;
        }
        let t = r
            .trains
            .iter_mut()
            .find(|t| t.train.bound && t.train.at_station == Some(pos))?;
        t.runtime = Runtime::for_schedule(&s, true);
        t.status.new_schedule();
        t.train.nav = None;
        t.schedule = Some(s);
        Some(t.column)
    })
    .flatten();
    if let Some(c) = applied {
        super::super::persist::mark_unsaved(dim, [c.0, c.1]);
        crate::glue::advancements::award_later(dim, pos, "conductor");
    }
    applied.is_some()
}

/// Each tick: a train newly standing at a station holding a schedule takes it (`newlyArrived`
/// in `StationBlockEntity.tick`), with its sound and particles.
pub fn tick(lv: &mut ServerLevel<'_>) {
    let dim = lv.dim;
    let now: std::collections::HashSet<[i32; 3]> = with_world(|w| {
        w.dims[dim]
            .railway
            .trains
            .iter()
            .filter(|t| t.train.bound)
            .filter_map(|t| t.train.at_station)
            .collect()
    })
    .unwrap_or_default();
    let newly: Vec<[i32; 3]> = PRESENT.with_borrow_mut(|p| {
        let before = p.entry(dim).or_default();
        let newly = now.difference(before).copied().collect();
        *before = now;
        newly
    });
    for pos in newly {
        if !apply_auto(dim, pos, false) {
            continue;
        }
        let at = [
            f64::from(pos[0]) + 0.5,
            f64::from(pos[1]) + 1.0,
            f64::from(pos[2]) + 0.5,
        ];
        lv.level.play_sound(
            &mut *lv.host,
            "create:confirm",
            at,
            SoundSource::Blocks,
            1.0,
            1.0,
        );
        for (key, at, spread, speed, count) in [
            (&HAPPY_VILLAGER, at, [0.35, 0.05, 0.35], 1.0, 8),
            (
                &END_ROD,
                [at[0], at[1] + 0.25, at[2]],
                [0.05, 1.0, 0.05],
                0.005,
                10,
            ),
        ] {
            if let Some(&k) = key.get() {
                let options = mod_api::ParticleOptions::simple(k);
                let _ = lv.level.send_particles(
                    &mut *lv.host,
                    &options,
                    None,
                    at,
                    spread,
                    speed,
                    count,
                );
            }
        }
    }
    // The schedules of stations taken away drop where they stood.
    let dropped = with_world(|w| std::mem::take(&mut w.dims[dim].railway.dropped_schedules))
        .unwrap_or_default();
    for (pos, s) in dropped {
        drop_schedule(lv, pos, &s);
    }
}

/// A schedule item holding `s` dropped at block `pos` (`Containers.dropItemStack`).
pub fn drop_schedule(lv: &mut ServerLevel<'_>, pos: [i32; 3], s: &Schedule) {
    let Some(key) = mod_api::Identifier::parse("create:schedule")
        .ok()
        .and_then(|i| lv.host.item_key(&i))
    else {
        return;
    };
    let stack = with_schedule(&mod_api::ItemStack::of(key), s);
    let p = pos.map(|v| f64::from(v) + 0.5);
    let _ = lv
        .level
        .spawn_item(&mut *lv.host, p[0], p[1], p[2], stack, 10);
}

/// The train's schedule into `player`'s inventory (`placeItemBackInInventory`), with
/// `playItemPickup`'s sound; an auto-schedule is just dropped.
pub fn give_back_schedule(host: &mut dyn ServerHost, player: PlayerRef, dim: usize, train: u32) {
    let Some(taken) = take_schedule(dim, train) else {
        return;
    };
    match taken {
        Some(s) => give_schedule(host, player, &s),
        None => sound_at(host, player, "minecraft:entity.item.pickup", 0.2, 1.5),
    }
}

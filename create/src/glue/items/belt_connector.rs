//! `BeltConnectorItem.useOn`: sneaking forgets the first shaft; a click on a shaft remembers
//! it, a click on a second shaft that connects lays the belt (one belt connector used up outside
//! creative) and forgets it. Refusals say nothing, as in Create: its client draws the line of
//! `BeltConnectorHandler` (`tick`), green dust toward a shaft that connects to the first, red
//! toward one that doesn't. Remembering a shaft, or laying a belt with connectors left, starts
//! the item's 5-tick cooldown; a belt laid gives the `belt` advancement.

use super::super::server::ServerLevel;
use super::super::states;
use super::super::world::with_world;
use super::{creative, custom, held, set_custom, set_held, sneaking};
use crate::glue::names::Named as _;
use create_engine::Block;
use create_engine::belt_connector::{self as bc, FIRST_SHAFT_KEY};
use create_engine::kinetics::Axis;
use mod_api::{
    BlockState, ClientHost, CrosshairTarget, Hand, HostBlockStates, HostSide, InteractionResult,
    ItemPart, ItemStack, LevelRef, ParticleKey, ParticleOptions, Registrar, ServerHost,
    SoundSource, Tag, UseOnEvent,
};
use std::cell::RefCell;
use std::sync::OnceLock;

/// The dust the line between the shafts is drawn with.
static DUST: OnceLock<ParticleKey> = OnceLock::new();

thread_local! {
    /// The line's random source (the client level's `random` in Create).
    static RNG: RefCell<create_engine::processing::Rng> =
        RefCell::new(create_engine::processing::Rng::new(0xBE17_C0DE));
}

/// Asks for the dust particle.
pub(super) fn register(r: &mut dyn Registrar) {
    let _ = DUST.set(r.particle_key("minecraft:dust"));
}

pub(super) struct Connector;

#[mod_api::horizon_part]
impl ItemPart for Connector {
    fn use_on(&self, host: HostSide, event: &mut UseOnEvent<'_>) -> Option<InteractionResult> {
        match host {
            HostSide::Server(host) => use_on(host, event),
            HostSide::Client(host) => predict(host, event),
        }
    }
}

/// `if (world.isClientSide) return validAxis ? SUCCESS : FAIL` (after the sneaking forget).
fn predict(host: &mut dyn ClientHost, event: &UseOnEvent<'_>) -> Option<InteractionResult> {
    let player = host.player(event.player)?;
    if sneaking(&*host, player) {
        return Some(InteractionResult::Success);
    }
    let level = host.level(host.dimension())?;
    let shaft = level
        .block_state(&*host, event.pos)
        .and_then(states::id)
        .and_then(|id| {
            super::super::client_table(
                &*host,
                |t| matches!(t.get(id), Some((Block::Shaft, i)) if i.axis.is_some()),
            )
        })
        .unwrap_or(false);
    Some(if shaft {
        InteractionResult::Success
    } else {
        InteractionResult::Fail
    })
}

/// The world as the belt connector asks it, on the server.
struct Ground<'l, 'a> {
    lv: &'l ServerLevel<'a>,
}

impl bc::Ground for Ground<'_, '_> {
    fn loaded(&self, pos: [i32; 3]) -> bool {
        self.lv.is_loaded(pos)
    }

    fn shaft(&self, pos: [i32; 3]) -> Option<Axis> {
        let state = self.lv.block(pos);
        with_world(|w| match w.table.get(state) {
            Some((Block::Shaft, info)) => info.axis,
            _ => None,
        })
        .flatten()
    }

    fn speed(&self, pos: [i32; 3]) -> f32 {
        let dim = self.lv.dim;
        with_world(|w| w.speed(dim, pos)).flatten().unwrap_or(0.0)
    }

    fn replaceable(&self, pos: [i32; 3]) -> bool {
        self.lv
            .block_state(pos)
            .and_then(|s| self.lv.host.block_states().replaceable(s))
            .unwrap_or(false)
    }
}

/// The belt connector's first shaft (`BELT_FIRST_SHAFT`, a `BlockPos`: three ints).
fn first_shaft(stack: &ItemStack) -> Option<[i32; 3]> {
    match custom(stack).get(&FIRST_SHAFT_KEY.into()) {
        Some(Tag::IntArray(v)) if v.len() == 3 => Some([v[0], v[1], v[2]]),
        _ => None,
    }
}

/// The world as the belt connector's line asks it, on the client (the same questions the
/// server's `Ground` answers, from what the client holds).
struct Sight<'h> {
    host: &'h dyn ClientHost,
    level: LevelRef,
}

impl bc::Ground for Sight<'_> {
    fn loaded(&self, pos: [i32; 3]) -> bool {
        self.level.block_state(self.host, pos).is_some()
    }

    fn shaft(&self, pos: [i32; 3]) -> Option<Axis> {
        let id = self
            .level
            .block_state(self.host, pos)
            .and_then(states::id)?;
        super::super::client_table(self.host, |t| match t.get(id) {
            Some((Block::Shaft, info)) => info.axis,
            _ => None,
        })
        .flatten()
    }

    fn speed(&self, pos: [i32; 3]) -> f32 {
        super::super::kinetics::shown_theory(self.host, self.level, pos).0
    }

    fn replaceable(&self, pos: [i32; 3]) -> bool {
        self.level
            .block_state(self.host, pos)
            .and_then(|s| self.host.block_states().replaceable(s))
            .unwrap_or(false)
    }
}

/// `hasProperty(AXIS)`: the axis of any block that has one (a log too), not only a shaft's.
fn axis_of(host: &dyn ClientHost, state: BlockState) -> Option<Axis> {
    let facts = host.block_states();
    match state.property(&facts, "axis").ok().flatten()? {
        "x" => Some(Axis::X),
        "y" => Some(Axis::Y),
        "z" => Some(Axis::Z),
        _ => None,
    }
}

fn axis_index(axis: Axis) -> usize {
    match axis {
        Axis::X => 0,
        Axis::Y => 1,
        Axis::Z => 2,
    }
}

/// `Math.signum`: 0 stays 0.
fn signum(v: f64) -> f64 {
    if v > 0.0 {
        1.0
    } else if v < 0.0 {
        -1.0
    } else {
        0.0
    }
}

/// `BeltConnectorHandler.tick`'s line from the first shaft toward `selected`, its shaft turning
/// about `axis`: the step a block (each axis -1, 0 or 1) and how many blocks it runs. The part
/// of the way along the shaft's own axis is dropped; a way that is neither straight nor at 45°
/// is drawn along the nearest direction a belt can take. None where Create draws nothing: a
/// vertical shaft asked for a diagonal.
fn line(first: [i32; 3], selected: [i32; 3], axis: Axis) -> Option<([f64; 3], f32)> {
    let along = axis_index(axis);
    let mut diff = [0, 1, 2].map(|i| f64::from(selected[i] - first[i]));
    diff[along] = 0.0;
    let [x, y, z] = diff.map(f64::abs);
    let length = x.max(y).max(z) as f32;
    // `Vec3.normalize`: none under 1e-4.
    let norm = (diff[0] * diff[0] + diff[1] * diff[1] + diff[2] * diff[2]).sqrt();
    let mut step = if norm < 1.0e-4 {
        [0.0; 3]
    } else {
        diff.map(|v| v / norm)
    };
    let sames = i32::from(x == y) + i32::from(y == z) + i32::from(z == x);
    if sames == 0 {
        // The closest of the ways a belt runs from this shaft, the first found on a tie.
        let mut closest = f32::MAX;
        let mut best = [0.0; 3];
        for i in -1..=1 {
            for j in -1..=1 {
                for k in -1..=1 {
                    let v = [i, j, k].map(f64::from);
                    if v[along] != 0.0 || axis == Axis::Y && i != 0 && k != 0 || v == [0.0; 3] {
                        continue;
                    }
                    let d = [0, 1, 2]
                        .map(|a| (step[a] - v[a]).powi(2))
                        .iter()
                        .sum::<f64>()
                        .sqrt();
                    if d < f64::from(closest) {
                        closest = d as f32;
                        best = v;
                    }
                }
            }
        }
        step = best;
    }
    if axis == Axis::Y && step[0] != 0.0 && step[2] != 0.0 {
        return None;
    }
    Some((step.map(signum), length))
}

/// `BeltConnectorHandler.tick`, each client tick with no screen open: a belt connector in a
/// hand that remembers its first shaft draws dust from that shaft toward the shaft looked at
/// (or the block in front of the face looked at), one speck in ten for each sixteenth of a
/// block, green when the two connect (`canConnect`), red when they don't. Looking at an entity,
/// a green speck now and then marks the first shaft; looking at nothing, or at a block that
/// can be replaced, draws nothing.
pub(super) fn tick(host: &mut dyn ClientHost) {
    if !host.playing() {
        return;
    }
    let (Some(player), Some(level), Some(&dust)) = (
        host.local_player(),
        host.level(host.dimension()),
        DUST.get(),
    ) else {
        return;
    };
    for hand in Hand::BOTH {
        let Some(stack) = held(&*host, player, hand) else {
            continue;
        };
        if stack.item().name(&*host) != bc::ITEM {
            continue;
        }
        let Some(first) = first_shaft(&stack) else {
            continue;
        };
        let Some(axis) = level
            .block_state(&*host, first)
            .and_then(|s| axis_of(&*host, s))
        else {
            continue;
        };
        draw_line(host, level, dust, first, axis);
        return;
    }
}

fn draw_line(
    host: &mut dyn ClientHost,
    level: LevelRef,
    dust: ParticleKey,
    first: [i32; 3],
    axis: Axis,
) {
    let roll = || RNG.with_borrow_mut(|r| r.next_float());
    let target = host.crosshair_target();
    let Some(CrosshairTarget::Block { pos, hit, .. }) = target else {
        // Create's miss is a block hit too, in the air its reach ends in: nothing is drawn.
        if matches!(target, Some(CrosshairTarget::Entity { .. })) && roll() < 1.0 / 50.0 {
            let at = [0, 1, 2].map(|i| f64::from(first[i]) + 0.5 + f64::from((roll() - 0.5) * 0.5));
            let options = ParticleOptions::dust(dust, [0.3, 0.9, 0.5], 1.0);
            host.add_particle(&options, at, [0.0; 3]);
        }
        return;
    };
    let mut selected = [pos.x, pos.y, pos.z];
    let sight = Sight {
        host: &*host,
        level,
    };
    if bc::Ground::replaceable(&sight, selected) {
        return;
    }
    if bc::Ground::shaft(&sight, selected).is_none() {
        selected = [0, 1, 2].map(|i| selected[i] + hit.face[i]);
    }
    if !bc::closer_than(selected, first, bc::MAX_LENGTH) {
        return;
    }
    let connects = bc::valid_axis(&sight, selected) && bc::can_connect(&sight, first, selected);
    let Some((step, length)) = line(first, selected, axis) else {
        return;
    };
    let rgb = if connects {
        [0.3, 0.9, 0.5]
    } else {
        [0.9, 0.3, 0.5]
    };
    let options = ParticleOptions::dust(dust, rgb, 1.0);
    let mut f = 0.0f32;
    while f < length {
        if roll() < 0.1 {
            let at = [0, 1, 2].map(|i| f64::from(first[i]) + step[i] * f64::from(f) + 0.5);
            host.add_particle(&options, at, [0.0; 3]);
        }
        f += 0.0625;
    }
}

fn use_on(host: &mut dyn ServerHost, event: &UseOnEvent<'_>) -> Option<InteractionResult> {
    let player = host.player(event.player)?;
    let dimension = player.dimension(&*host)?.to_string();
    let back = sneaking(&*host, player);
    let mut lv = ServerLevel::new(host, &dimension)?;
    let pos = event.pos;
    let first = first_shaft(event.stack);
    let click = bc::click(&Ground { lv: &lv }, back, first, pos, true);
    let hand = event.hand;
    let set_first = |lv: &mut ServerLevel<'_>, p: Option<[i32; 3]>| {
        if let Some(mut s) = held(&*lv.host, player, hand) {
            set_custom(
                &mut s,
                &[(FIRST_SHAFT_KEY, p.map(|p| Tag::IntArray(p.to_vec())))],
            );
            set_held(lv.host, player, hand, Some(s));
        }
    };
    let answer = match click {
        bc::Click::Forget => {
            set_first(&mut lv, None);
            InteractionResult::Success
        }
        bc::Click::Fail { forget } => {
            if forget {
                set_first(&mut lv, None);
            }
            InteractionResult::Fail
        }
        bc::Click::Remember => {
            set_first(&mut lv, Some(pos));
            player.set_item_cooldown(lv.host, "create:belt_connector", 5);
            InteractionResult::Success
        }
        bc::Click::Refused => InteractionResult::Success,
        bc::Click::Connect(start) => {
            create_belts(&mut lv, start, pos);
            super::super::advancements::award(lv.host, player, "belt");
            if !creative(&*lv.host, player) {
                super::shrink_held(lv.host, player, hand, 1);
            }
            set_first(&mut lv, None);
            if held(&*lv.host, player, hand).is_some() {
                player.set_item_cooldown(lv.host, "create:belt_connector", 5);
            }
            InteractionResult::Success
        }
    };
    Some(answer)
}

/// `createBelts`: the wool sound, then each block of the chain becomes its belt segment
/// (`switchToBlockState`, waterlogged in a water source; what can't be replaced, the shafts, is
/// broken without drops first); an unbreakable block on the way stops it and takes every belt
/// block laid away again.
pub(crate) fn create_belts(lv: &mut ServerLevel<'_>, start: [i32; 3], end: [i32; 3]) {
    let at = bc::sound_at(start, end).map(|v| f64::from(v) + 0.5);
    lv.level.play_sound(
        &mut *lv.host,
        "minecraft:block.wool.place",
        at,
        SoundSource::Blocks,
        0.5,
        1.0,
    );
    let layout = {
        let ground = Ground { lv };
        let Some(axis) = bc::Ground::shaft(&ground, start) else {
            return;
        };
        bc::layout(start, end, axis, |p| bc::Ground::shaft(&ground, p))
    };
    let Some(belt) = lv.host.block_state_default(states::key(Block::Belt)) else {
        return;
    };
    // `getFluidState().getType() == Fluids.WATER`: a water source.
    let water = |lv: &ServerLevel<'_>, s: BlockState| {
        lv.host
            .state_fluid_state(s)
            .is_some_and(|f| f.is_source() && f.fluid.name(&*lv.host) == "minecraft:water")
    };
    let mut failed = false;
    for (pos, seg) in &layout {
        let pos = *pos;
        let Some(state) = lv.block_state(pos) else {
            failed = true;
            break;
        };
        let facts = lv.host.block_states();
        if facts.destroy_speed(state).is_some_and(|h| h < 0.0) {
            failed = true;
            break;
        }
        let replaceable = facts.replaceable(state).unwrap_or(false);
        let waterlogged = water(lv, state);
        if !replaceable {
            // `destroyBlock(pos, false)`: what stays is its fluid.
            lv.destroy_block(pos, false);
        }
        let facts = lv.host.block_states();
        let mut s = belt;
        for (k, v) in [
            ("facing", super::super::kinetics::dir_name(seg.facing)),
            ("slope", bc::slope_name(seg.slope)),
            ("part", bc::part_name(seg.part)),
            ("waterlogged", if waterlogged { "true" } else { "false" }),
        ] {
            s = s.with(&facts, k, v).unwrap_or(s);
        }
        if lv.block_state(pos) != Some(s) {
            super::wrench::switch_state(lv, pos, s);
        }
    }
    if !failed {
        return;
    }
    for (pos, _) in layout {
        let id = lv.block(pos);
        let is_belt = with_world(|w| w.table.get(id).is_some_and(|(b, _)| b == Block::Belt));
        if is_belt == Some(true) {
            lv.destroy_block(pos, false);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_line_runs_as_create_s_handler_draws_it() {
        // Straight along z from an x shaft, and up a 45° slope.
        assert_eq!(
            line([0, 0, 0], [0, 0, 5], Axis::X),
            Some(([0.0, 0.0, 1.0], 5.0))
        );
        assert_eq!(
            line([0, 0, 0], [0, 3, -3], Axis::X),
            Some(([0.0, 1.0, -1.0], 3.0))
        );
        // The way along the shaft's own axis is dropped.
        assert_eq!(
            line([0, 0, 0], [2, 0, 5], Axis::X),
            Some(([0.0, 0.0, 1.0], 5.0))
        );
        // Crooked: drawn along the nearest way a belt runs.
        assert_eq!(
            line([0, 0, 0], [0, 2, 5], Axis::X),
            Some(([0.0, 0.0, 1.0], 5.0))
        );
        // A vertical shaft: flat only, never diagonal.
        assert_eq!(line([0, 0, 0], [3, 0, 3], Axis::Y), None);
        assert_eq!(
            line([0, 0, 0], [3, 0, 1], Axis::Y),
            Some(([1.0, 0.0, 0.0], 3.0))
        );
        // Straight up from a horizontal shaft.
        assert_eq!(
            line([0, 0, 0], [0, 4, 0], Axis::Z),
            Some(([0.0, 1.0, 0.0], 4.0))
        );
        // The shaft itself: nothing to draw.
        assert_eq!(line([0, 0, 0], [0, 0, 0], Axis::X), Some(([0.0; 3], 0.0)));
    }
}

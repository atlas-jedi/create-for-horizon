//! Funnels' and tunnels' flaps: the server notes each flap (`FunnelBlockEntity.flap`,
//! `BeltTunnelBlockEntity.flap`) and writes them to its block entity after the tick (as
//! `FunnelFlapPacket`, `TunnelFlapPacket` bring them), with a funnel's sound (`FUNNEL_FLAP`,
//! which Create's client plays as the packet arrives); clients swing them back to rest
//! (`LerpedFloat` chasing 0) and draw them
//! (`FlapStuffs.renderFlaps`).

use super::super::server::ServerLevel;
use super::info_at;
use create_engine::Block;
use create_engine::kinetics::{Axis, Dir};
use mod_api::{Compound, Tag};
use std::cell::RefCell;

/// The block entity key holding a funnel's or tunnel's latest flaps, synced to its clients:
/// a count of the ticks it flapped, then each flap as its side times two, plus one inward.
pub const KEY: &str = "Flaps";

/// The side a funnel's flap is noted on (a tunnel's are its sides, 0 to 5).
pub const FUNNEL: u8 = 6;

thread_local! {
    /// Flaps noted this tick: level, block, side and inward.
    #[allow(clippy::type_complexity)]
    static FLAPS: RefCell<Vec<(usize, [i32; 3], u8, bool)>> = const { RefCell::new(Vec::new()) };
    /// Funnels heard this tick (`FunnelFlapPacket`): level and block.
    static SOUNDS: RefCell<Vec<(usize, [i32; 3])>> = const { RefCell::new(Vec::new()) };
}

/// Whether the funnel at `pos` has a flap (`hasFlap`: facing sideways); None for no funnel.
fn has_flap(lv: &ServerLevel<'_>, pos: [i32; 3]) -> Option<bool> {
    let (block, info) = info_at(lv, pos)?;
    if !create_engine::funnel::is_funnel(block) {
        return None;
    }
    Some(info.facing.is_some_and(|f| f.axis() != Axis::Y))
}

/// `FunnelBlockEntity.flap` where Create asks `hasFlap` first (`AbstractFunnelBlock.tryInsert`)
/// or on a belt funnel (always sideways): the funnel at `pos` flaps in or out, and is heard,
/// when it has a flap.
pub fn funnel(lv: &ServerLevel<'_>, pos: [i32; 3], inward: bool) {
    if has_flap(lv, pos) == Some(true) {
        note_funnel(lv.dim, pos, Some(inward));
    }
}

/// `FunnelBlockEntity.flap(false)` from `activateExtractor`, which does not ask `hasFlap`: the
/// funnel at `pos` is heard whatever its facing; one facing sideways also flaps out.
pub fn funnel_extracted(lv: &ServerLevel<'_>, pos: [i32; 3]) {
    if let Some(flap) = has_flap(lv, pos) {
        note_funnel(lv.dim, pos, flap.then_some(false));
    }
}

/// A funnel's `flap` in level `dim`: its sound, and its flap in (`Some(true)`) or out when it
/// swings one.
fn note_funnel(dim: usize, pos: [i32; 3], flap: Option<bool>) {
    if let Some(inward) = flap {
        FLAPS.with_borrow_mut(|f| f.push((dim, pos, FUNNEL, inward)));
    }
    SOUNDS.with_borrow_mut(|s| s.push((dim, pos)));
}

/// `BeltTunnelInteractionHandler.flapTunnel`: the tunnel over the belt segment `segment` flaps
/// on `side`, when one is there.
pub fn tunnel(lv: &ServerLevel<'_>, segment: [i32; 3], side: Dir, inward: bool) {
    let pos = [segment[0], segment[1] + 1, segment[2]];
    if !info_at(lv, pos)
        .is_some_and(|(b, _)| matches!(b, Block::AndesiteTunnel | Block::BrassTunnel))
    {
        return;
    }
    let dim = lv.dim;
    FLAPS.with_borrow_mut(|f| f.push((dim, pos, side as u8, inward)));
}

/// This tick's flaps of this level go to their block entities (`FunnelFlapPacket`,
/// `TunnelFlapPacket`), for their clients to swing them; each funnel heard plays
/// `FUNNEL_FLAP` at its centre, once a tick.
pub fn send(lv: &mut ServerLevel<'_>) {
    let dim = lv.dim;
    let mut heard: Vec<[i32; 3]> = SOUNDS.with_borrow_mut(|s| {
        if s.is_empty() {
            return Vec::new();
        }
        let mine = s.iter().filter(|e| e.0 == dim).map(|e| e.1).collect();
        s.retain(|e| e.0 != dim);
        mine
    });
    heard.sort_unstable();
    heard.dedup();
    for pos in heard {
        let at = pos.map(|v| f64::from(v) + 0.5);
        for (sound, volume, pitch) in create_engine::funnel::FLAP_SOUNDS {
            let _ = lv.level.play_sound(
                &mut *lv.host,
                sound,
                at,
                mod_api::SoundSource::Blocks,
                volume,
                pitch,
            );
        }
    }
    let mut mine: Vec<([i32; 3], u8, bool)> = FLAPS.with_borrow_mut(|f| {
        if f.is_empty() {
            return Vec::new();
        }
        let mine = f
            .iter()
            .filter(|e| e.0 == dim)
            .map(|e| (e.1, e.2, e.3))
            .collect();
        f.retain(|e| e.0 != dim);
        mine
    });
    mine.sort_by_key(|e| (e.0, e.1));
    mine.dedup_by_key(|e| (e.0, e.1));
    for group in mine.chunk_by(|a, b| a.0 == b.0) {
        let pos = group[0].0;
        let Some(e) = lv.host.block_entity(lv.level, pos) else {
            continue;
        };
        let serial = match e.data(&*lv.host).and_then(|d| d.get(&KEY.into()).cloned()) {
            Some(Tag::IntArray(v)) => v.first().copied().unwrap_or(0).wrapping_add(1),
            _ => 1,
        };
        let mut v = vec![serial];
        v.extend(
            group
                .iter()
                .map(|(_, side, inward)| i32::from(*side) * 2 + i32::from(*inward)),
        );
        let _ = e.merge_data(
            &mut *lv.host,
            Compound::from([(KEY.into(), Tag::IntArray(v))]),
        );
    }
}

/// What a block entity's `KEY` holds: the count, and each flap's side and whether inward.
pub fn read(data: &Compound) -> Option<(i32, Vec<(u8, bool)>)> {
    let Some(Tag::IntArray(v)) = data.get(&KEY.into()) else {
        return None;
    };
    let (serial, flaps) = v.split_first()?;
    Some((
        *serial,
        flaps.iter().map(|f| ((f / 2) as u8, f % 2 == 1)).collect(),
    ))
}

/// `FlapStuffs.flapAngle`: segment `segment`'s swing at `flapness`.
pub fn flap_angle(flapness: f32, segment: usize) -> f32 {
    let intensity = if segment == 3 {
        1.5
    } else {
        segment as f32 + 1.0
    };
    let abs = flapness.abs();
    let mut angle = ((1.0 - abs) * std::f32::consts::PI * intensity).sin() * 30.0 * flapness;
    if flapness < 0.0 {
        angle *= 0.5;
    }
    angle
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flaps_swing_both_ways() {
        assert_eq!(flap_angle(0.0, 0), 0.0);
        assert!(flap_angle(-0.5, 0) < 0.0);
        let data = Compound::from([(KEY.into(), Tag::IntArray(vec![3, 13, 4]))]);
        assert_eq!(read(&data), Some((3, vec![(6, true), (2, false)])));
    }
}

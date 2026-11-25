//! Compares two snapshots block by block.

use crate::scenario::{Kinetic, Snapshot, parse_state};
use crate::transport::{DroppedItem, Placed};

/// Float tolerance: Create's figures are f32 sums of small products.
const EPS: f32 = 1e-3;

fn close(a: f32, b: f32) -> bool {
    (a - b).abs() <= EPS * a.abs().max(b.abs()).max(1.0)
}

pub fn diff(java: &Snapshot, rust: &Snapshot) -> Vec<String> {
    let (java, rust) = normalized(java, rust);
    diff_exact(&java, &rust)
}

/// Both snapshots with what chance decides taken out.
fn normalized<'a>(
    java: &'a Snapshot,
    rust: &'a Snapshot,
) -> (
    std::borrow::Cow<'a, Snapshot>,
    std::borrow::Cow<'a, Snapshot>,
) {
    use std::borrow::Cow;
    match crate::processing::Rolls::of(java, rust) {
        Some(rolls) => {
            let (java, rust) = rolls.normalize(java, rust);
            (Cow::Owned(java), Cow::Owned(rust))
        }
        None => (Cow::Borrowed(java), Cow::Borrowed(rust)),
    }
}

pub(crate) fn diff_exact(java: &Snapshot, rust: &Snapshot) -> Vec<String> {
    let mut out = Vec::new();
    for (side, snapshot) in [("java", java), ("rust", rust)] {
        if snapshot
            .unavailable
            .iter()
            .any(|s| s == crate::fluid_world::OUTSIDE)
        {
            out.push(format!(
                "{side} adapter unavailable: {}",
                crate::fluid_world::OUTSIDE
            ));
        }
    }
    if java.item_outcomes != rust.item_outcomes {
        out.push(format!(
            "item outcomes: java {:?} rust {:?}",
            java.item_outcomes, rust.item_outcomes
        ));
    }
    if let (Some(j), Some(r)) = (&java.drops, &rust.drops)
        && !drops_match(j, r)
    {
        out.push(format!("drops: java {j:?} rust {r:?}"));
    }
    if let (Some(j), Some(r)) = (&java.networks, &rust.networks) {
        let normalize = |groups: &[Vec<String>]| {
            let mut groups = groups.to_vec();
            for g in &mut groups {
                g.sort();
            }
            groups.sort();
            groups
        };
        if normalize(&j.partitions) != normalize(&r.partitions) {
            out.push("network partitions differ".into());
        }
        if j.sizes != r.sizes {
            out.push("network sizes differ".into());
        }
    }
    let keys: std::collections::BTreeSet<&String> =
        java.blocks.keys().chain(rust.blocks.keys()).collect();
    for k in keys {
        match (java.blocks.get(k), rust.blocks.get(k)) {
            (Some(j), None) => out.push(format!("{k}: java has {}, rust has air", j.state)),
            (None, Some(r)) => out.push(format!("{k}: java has air, rust has {}", r.state)),
            (Some(j), Some(r)) => {
                if j.fluid_storage != r.fluid_storage {
                    out.push(format!(
                        "{k}: fluid storage: java {:?} rust {:?}",
                        j.fluid_storage, r.fluid_storage
                    ));
                }
                if j.fluid_filter != r.fluid_filter {
                    out.push(format!(
                        "{k}: fluid filter: java {:?} rust {:?}",
                        j.fluid_filter, r.fluid_filter
                    ));
                }
                if j.fluid_machine != r.fluid_machine {
                    out.push(format!(
                        "{k}: fluid machine: java {:?} rust {:?}",
                        j.fluid_machine, r.fluid_machine
                    ));
                }
                out.extend(
                    fluid_network_diff(j.fluid_network.as_ref(), r.fluid_network.as_ref())
                        .into_iter()
                        .map(|d| format!("{k}: {d}")),
                );
                if j.world_fluid != r.world_fluid {
                    out.push(format!(
                        "{k}: world fluid: java {:?} rust {:?}",
                        j.world_fluid, r.world_fluid
                    ));
                }
                if j.fluid_tank != r.fluid_tank {
                    out.push(format!(
                        "{k}: fluid tank: java {:?} rust {:?}",
                        j.fluid_tank, r.fluid_tank
                    ));
                }
                if !transport_equal(j.transport.as_ref(), r.transport.as_ref()) {
                    out.push(format!(
                        "{k}: transport: java {:?} rust {:?}",
                        j.transport, r.transport
                    ));
                }
                if j.conveyor_links != r.conveyor_links {
                    out.push(format!(
                        "{k}: conveyor links: java {:?} rust {:?}",
                        j.conveyor_links, r.conveyor_links
                    ));
                }
                if !crate::persistence::payload::equal(&j.payload, &r.payload) {
                    out.push(format!(
                        "{k}: saved subsystem payload: java {:?} rust {:?}",
                        j.payload, r.payload
                    ));
                }
                if !crate::persistence::equal(&j.saved, &r.saved) {
                    out.push(format!(
                        "{k}: saved kinetic data: java {:?} rust {:?}",
                        j.saved, r.saved
                    ));
                }
                if j.belt != r.belt {
                    out.push(format!("{k}: belt: java {:?} rust {:?}", j.belt, r.belt));
                }
                if j.tunnel != r.tunnel {
                    out.push(format!(
                        "{k}: tunnel: java {:?} rust {:?}",
                        j.tunnel, r.tunnel
                    ));
                }
                if j.funnel != r.funnel {
                    out.push(format!(
                        "{k}: funnel: java {:?} rust {:?}",
                        j.funnel, r.funnel
                    ));
                }
                if !crate::logistics::chute_equal(j.chute.as_ref(), r.chute.as_ref()) {
                    out.push(format!("{k}: chute: java {:?} rust {:?}", j.chute, r.chute));
                }
                if !crate::logistics::ejector_equal(j.ejector.as_ref(), r.ejector.as_ref()) {
                    out.push(format!(
                        "{k}: ejector: java {:?} rust {:?}",
                        j.ejector, r.ejector
                    ));
                }
                if !crate::processing::equal(j.processing.as_ref(), r.processing.as_ref()) {
                    out.push(format!(
                        "{k}: processing: java {:?} rust {:?}",
                        j.processing, r.processing
                    ));
                }
                if j.bracket != r.bracket {
                    out.push(format!(
                        "{k}: bracket: java {:?} rust {:?}",
                        j.bracket, r.bracket
                    ));
                }
                if let Some(d) = state_diff(&j.state, &r.state)
                    && !crate::allow::trig_state(&j.state, &r.state, r.measured_trig.as_ref())
                {
                    out.push(format!("{k}: state {d}"));
                }
                match (&j.kinetic, &r.kinetic) {
                    (Some(a), Some(b)) => {
                        out.extend(kinetic_diff(a, b).into_iter().map(|d| format!("{k}: {d}")))
                    }
                    (Some(_), None) => {
                        out.push(format!("{k}: java kinetic, rust not ({})", j.state))
                    }
                    (None, Some(_)) => {
                        out.push(format!("{k}: rust kinetic, java not ({})", j.state))
                    }
                    (None, None) => {}
                }
            }
            (None, None) => {}
        }
    }
    let ticks: std::collections::BTreeSet<_> = java
        .checkpoints
        .keys()
        .chain(rust.checkpoints.keys())
        .collect();
    for t in ticks {
        match (java.checkpoints.get(t), rust.checkpoints.get(t)) {
            (Some(j), Some(r)) => out.extend(
                diff_exact(j, r)
                    .into_iter()
                    .map(|d| format!("tick {t}: {d}")),
            ),
            _ => out.push(format!("tick {t}: missing checkpoint")),
        }
    }
    for n in &rust.notes {
        out.push(format!("rust note: {n}"));
    }
    out
}

/// The same item entities, in any order. A drop placed exactly on one side has one at the same
/// place and motion within the tolerance on the other. The rest agree in how many of each item
/// joined in each tick from each origin, a drop lying in the range its origin spreads it over.
/// Without a placement on either side, an origin is any block whose widest range, flown a tick,
/// takes the drops of both sides.
fn drops_match(a: &[DroppedItem], b: &[DroppedItem]) -> bool {
    use std::collections::BTreeMap;
    let mut a: Vec<&DroppedItem> = a.iter().collect();
    let mut b: Vec<&DroppedItem> = b.iter().collect();
    if !take_exact(&mut a, &mut b) || !take_exact(&mut b, &mut a) {
        return false;
    }
    type Sides<'a> = (Vec<&'a DroppedItem>, Vec<&'a DroppedItem>);
    let mut groups: BTreeMap<(u32, &str), Sides> = BTreeMap::new();
    for d in a {
        groups
            .entry((d.tick, d.stack.item.as_str()))
            .or_default()
            .0
            .push(d);
    }
    for d in b {
        groups
            .entry((d.tick, d.stack.item.as_str()))
            .or_default()
            .1
            .push(d);
    }
    let placed = |v: &[&DroppedItem]| v.iter().any(|d| d.placed.is_some());
    groups.into_values().all(|(a, b)| {
        if placed(&b) {
            origins_match(&a, &b)
        } else if placed(&a) {
            origins_match(&b, &a)
        } else {
            blocks_match(&a, &b)
        }
    })
}

/// Where a drop can be from where its origin put it: the least and most per axis, or anywhere.
fn reach(placed: Option<Placed>) -> Option<[[f64; 2]; 3]> {
    use crate::transport::{FIRST_FLIGHT, SPREAD_MIDDLE};
    match placed? {
        Placed::Exact => Some([[0.0; 2]; 3]),
        Placed::Flown => None,
        Placed::Spread(by) => {
            let range = by.range();
            Some(std::array::from_fn(|i| {
                [
                    range[i][0] - SPREAD_MIDDLE[i] + FIRST_FLIGHT[i][0],
                    range[i][1] - SPREAD_MIDDLE[i] + FIRST_FLIGHT[i][1],
                ]
            }))
        }
    }
}

/// `free`'s drops go to the origins `placed`'s drops name, each to one whose reach it lies in.
fn origins_match(free: &[&DroppedItem], placed: &[&DroppedItem]) -> bool {
    let near =
        |a: &[f64; 3], b: &[f64; 3]| a.iter().zip(b).all(|(x, y)| close(*x as f32, *y as f32));
    let mut origins: Vec<&DroppedItem> = Vec::new();
    let mut items = Vec::new();
    for d in placed {
        let k = origins
            .iter()
            .position(|o| o.placed == d.placed && near(&o.pos, &d.pos))
            .unwrap_or_else(|| {
                origins.push(d);
                origins.len() - 1
            });
        items.push((-i64::from(d.stack.count), vec![k]));
    }
    for d in free {
        let takes = |o: &&DroppedItem| {
            (d.placed.is_none() || d.placed == o.placed)
                && reach(o.placed).is_none_or(|r| {
                    (0..3).all(|i| {
                        let off = d.pos[i] - o.pos[i];
                        off >= r[i][0] - f64::from(EPS) && off <= r[i][1] + f64::from(EPS)
                    })
                })
        };
        let keys = (0..origins.len()).filter(|&k| takes(&origins[k])).collect();
        items.push((i64::from(d.stack.count), keys));
    }
    assignable(&items, origins.len())
}

/// Neither side says how its drops were placed: each goes to a block whose widest range, flown
/// a tick, it lies in.
fn blocks_match(a: &[&DroppedItem], b: &[&DroppedItem]) -> bool {
    use crate::transport::{FIRST_FLIGHT, Spreader};
    let range = Spreader::DropItemStack.range();
    let mut blocks: Vec<[i32; 3]> = Vec::new();
    let mut items = Vec::new();
    for (sign, side) in [(1, a), (-1, b)] {
        for d in side {
            let span: [Vec<i32>; 3] = std::array::from_fn(|i| {
                let lo = (d.pos[i] - range[i][1] - FIRST_FLIGHT[i][1]).ceil() as i32;
                let hi = (d.pos[i] - range[i][0] - FIRST_FLIGHT[i][0]).floor() as i32;
                (lo..=hi).collect()
            });
            let mut keys = Vec::new();
            for &x in &span[0] {
                for &y in &span[1] {
                    for &z in &span[2] {
                        let p = [x, y, z];
                        let k = blocks.iter().position(|b| *b == p).unwrap_or_else(|| {
                            blocks.push(p);
                            blocks.len() - 1
                        });
                        keys.push(k);
                    }
                }
            }
            items.push((sign * i64::from(d.stack.count), keys));
        }
    }
    assignable(&items, blocks.len())
}

/// Whether each item (a count and the keys it may go to) can go to one of its keys so that the
/// counts at every key sum to zero.
fn assignable(items: &[(i64, Vec<usize>)], keys: usize) -> bool {
    let mut group: Vec<usize> = (0..keys).collect();
    fn root(group: &mut [usize], k: usize) -> usize {
        let mut r = k;
        while group[r] != r {
            r = group[r];
        }
        group[k] = r;
        r
    }
    for (_, ks) in items {
        for w in ks.windows(2) {
            let (x, y) = (root(&mut group, w[0]), root(&mut group, w[1]));
            group[x] = y;
        }
    }
    let mut parts: std::collections::BTreeMap<usize, Vec<(i64, &[usize])>> = Default::default();
    for (n, ks) in items {
        let Some(&k) = ks.first() else {
            return false;
        };
        let r = root(&mut group, k);
        parts.entry(r).or_default().push((*n, ks));
    }
    parts.into_values().all(|mut part| {
        if part.iter().map(|p| p.0).sum::<i64>() != 0 {
            return false;
        }
        part.sort_by_key(|p| p.1.len());
        let mut last = vec![0; keys];
        for (i, (_, ks)) in part.iter().enumerate() {
            for &k in *ks {
                last[k] = i;
            }
        }
        let mut closing = vec![Vec::new(); part.len()];
        for (i, (_, ks)) in part.iter().enumerate() {
            for &k in *ks {
                if last[k] == i && !closing[i].contains(&k) {
                    closing[i].push(k);
                }
            }
        }
        fn place(
            part: &[(i64, &[usize])],
            closing: &[Vec<usize>],
            at: &mut [i64],
            i: usize,
        ) -> bool {
            let Some((n, ks)) = part.get(i) else {
                return true;
            };
            for &k in *ks {
                at[k] += n;
                if closing[i].iter().all(|&c| at[c] == 0) && place(part, closing, at, i + 1) {
                    return true;
                }
                at[k] -= n;
            }
            false
        }
        place(&part, &closing, &mut vec![0; keys], 0)
    })
}

/// Pairs each of `known`'s exactly placed drops with one of `other`'s at the same tick, stack,
/// place and motion, taking both out. False when one has none.
fn take_exact(known: &mut Vec<&DroppedItem>, other: &mut Vec<&DroppedItem>) -> bool {
    let near =
        |a: &[f64; 3], b: &[f64; 3]| a.iter().zip(b).all(|(x, y)| close(*x as f32, *y as f32));
    let mut i = 0;
    while i < known.len() {
        let k = known[i];
        if k.placed != Some(Placed::Exact) {
            i += 1;
            continue;
        }
        let Some(j) = other.iter().position(|o| {
            o.tick == k.tick
                && o.stack == k.stack
                && near(&o.pos, &k.pos)
                && near(&o.motion, &k.motion)
        }) else {
            return false;
        };
        other.remove(j);
        known.remove(i);
    }
    true
}

/// The block names differ, or a property both sides name has another value.
fn state_diff(j: &str, r: &str) -> Option<String> {
    let (jn, jp) = parse_state(j);
    let (rn, rp) = parse_state(r);
    if jn != rn {
        return Some(format!("java {j} rust {r}"));
    }
    let differing: Vec<String> = rp
        .iter()
        .filter(|(k, _)| {
            !crate::catalogue::entries()
                .iter()
                .find(|e| e.id == jn)
                .is_some_and(|e| e.observation_scope.contains_key(k))
        })
        .filter_map(|(k, v)| {
            let jv = &jp.iter().find(|(jk, _)| jk == k)?.1;
            (jv != v).then(|| format!("{k}: java {jv} rust {v}"))
        })
        .collect();
    (!differing.is_empty()).then(|| differing.join(", "))
}

fn kinetic_diff(j: &Kinetic, r: &Kinetic) -> Vec<String> {
    let mut out = Vec::new();
    let mut f = |name: &str, a: f32, b: f32| {
        if !close(a, b) {
            out.push(format!("{name}: java {a} rust {b}"));
        }
    };
    f("speed", j.speed, r.speed);
    f("theoretical", j.theoretical, r.theoretical);
    f("stress", j.stress, r.stress);
    f("capacity", j.capacity, r.capacity);
    f("impact", j.impact, r.impact);
    f("added_capacity", j.added_capacity, r.added_capacity);
    f("generated", j.generated, r.generated);
    if let (Some(jf), Some(rf)) = (&j.faces, &r.faces) {
        for (face, a) in jf {
            let b = rf.get(face).copied().unwrap_or(f32::NAN);
            if !close(*a, b) {
                out.push(format!("{face} face: java {a} rust {b}"));
            }
        }
    }
    match (&j.valve, &r.valve) {
        (Some(a), Some(b))
            if a.angle_unavailable == b.angle_unavailable
                && a.selected == b.selected
                && a.in_use == b.in_use
                && match (a.angle, b.angle) {
                    (Some(a), Some(b)) => close(a, b),
                    (None, None) => true,
                    _ => false,
                } => {}
        (None, None) => {}
        _ => out.push(format!("valve: java {:?} rust {:?}", j.valve, r.valve)),
    }
    match (&j.sequencer, &r.sequencer) {
        (Some(a), Some(b)) => {
            if (a.current, a.duration, a.timer, a.powered_before)
                != (b.current, b.duration, b.timer, b.powered_before)
                || !close(a.progress, b.progress)
            {
                out.push(format!("sequencer: java {a:?} rust {b:?}"));
            }
        }
        (None, None) => {}
        _ => out.push(format!(
            "sequencer: java {:?} rust {:?}",
            j.sequencer, r.sequencer
        )),
    }
    if j.has_source != r.has_source {
        out.push(format!(
            "has_source: java {} rust {}",
            j.has_source, r.has_source
        ));
    }
    if j.comparator != r.comparator {
        out.push(format!(
            "comparator: java {:?} rust {:?}",
            j.comparator, r.comparator
        ));
    }
    for (name, a, b) in [
        ("speed_setting", j.speed_setting, r.speed_setting),
        ("chain_modifier", j.chain_modifier, r.chain_modifier),
        ("sequence_angle", j.sequence_angle, r.sequence_angle),
        (
            "sequence_distance",
            j.sequence_distance,
            r.sequence_distance,
        ),
    ] {
        match (a, b) {
            (Some(a), Some(b)) if close(a, b) => {}
            (None, None) => {}
            _ => out.push(format!("{name}: java {a:?} rust {b:?}")),
        }
    }
    if j.overstressed != r.overstressed {
        out.push(format!(
            "overstressed: java {} rust {}",
            j.overstressed, r.overstressed
        ));
    }
    out
}

fn transport_equal(
    a: Option<&crate::transport::Contents>,
    b: Option<&crate::transport::Contents>,
) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => {
            a.kind == b.kind
                && a.outputs == b.outputs
                && a.items.len() == b.items.len()
                && a.items.iter().zip(&b.items).all(|(a, b)| {
                    a.stack == b.stack
                        && a.locked == b.locked
                        && a.pending == b.pending
                        && a.stalled == b.stalled
                        && close(a.position, b.position)
                })
        }
        _ => false,
    }
}

fn fluid_network_diff(
    j: Option<&crate::fluid_network::Observation>,
    r: Option<&crate::fluid_network::Observation>,
) -> Vec<String> {
    let (j, r) = match (j, r) {
        (Some(j), Some(r)) => (j, r),
        (None, None) => return vec![],
        _ => return vec!["fluid network presence differs".into()],
    };
    let mut out = Vec::new();
    match (j.pump_pressure, r.pump_pressure) {
        (Some(a), Some(b)) if close(a, b) => {}
        (None, None) => {}
        (a, b) => out.push(format!("pump pressure: java {a:?} rust {b:?}")),
    }
    let keys: std::collections::BTreeSet<_> =
        j.connections.keys().chain(r.connections.keys()).collect();
    for side in keys {
        match (j.connections.get(side), r.connections.get(side)) {
            (Some(a), Some(b)) => {
                for (i, name) in ["inbound", "outward"].iter().enumerate() {
                    if !close(a.pressure[i], b.pressure[i]) {
                        out.push(format!(
                            "{side} {name} pressure: java {} rust {}",
                            a.pressure[i], b.pressure[i]
                        ));
                    }
                }
                if a.buffer != b.buffer {
                    out.push(format!(
                        "{side} open-end buffer: java {:?} rust {:?}",
                        a.buffer, b.buffer
                    ));
                }
                match (&a.flow, &b.flow) {
                    (Some(a), Some(b))
                        if a.fluid == b.fluid
                            && a.inbound == b.inbound
                            && a.complete == b.complete
                            && close(a.progress, b.progress) => {}
                    (None, None) => {}
                    (a, b) => out.push(format!("{side} flow: java {a:?} rust {b:?}")),
                }
            }
            _ => out.push(format!("{side} fluid connection presence differs")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::observation::Networks;
    #[test]
    fn scope_excludes_only_declared_outputs() {
        assert!(
            state_diff(
                "create:fluid_valve[enabled=true,axis=x]",
                "create:fluid_valve[enabled=false,axis=x]"
            )
            .is_some()
        );
        assert!(
            state_diff(
                "create:fluid_valve[enabled=true,axis=x]",
                "create:fluid_valve[enabled=false,axis=y]"
            )
            .is_some()
        );
        assert!(
            state_diff(
                "create:display_board[up=true,facing=north]",
                "create:display_board[up=false,facing=south]"
            )
            .is_some()
        );
        assert!(crate::catalogue::validate("create:shaft[waterlogged=true]").is_ok());
        assert!(
            state_diff(
                "create:shaft[waterlogged=true]",
                "create:shaft[waterlogged=false]"
            )
            .is_some()
        );
        for e in crate::catalogue::entries() {
            for (property, reason) in &e.observation_scope {
                assert!(e.properties.contains_key(property));
                assert!(!reason.is_empty());
            }
        }
    }
    #[test]
    fn partitions_compare_membership_independently_of_order() {
        let mut j = Snapshot {
            networks: Some(Networks {
                partitions: vec![vec!["a".into(), "b".into()], vec!["c".into()]],
                ..Networks::default()
            }),
            ..Snapshot::default()
        };
        let r = Snapshot {
            networks: Some(Networks {
                partitions: vec![vec!["c".into()], vec!["b".into(), "a".into()]],
                ..Networks::default()
            }),
            ..Snapshot::default()
        };
        assert!(diff(&j, &r).is_empty());
        j.networks.as_mut().unwrap().partitions =
            vec![vec!["a".into(), "c".into()], vec!["b".into()]];
        assert_eq!(diff(&j, &r), ["network partitions differ"]);
    }
    fn dropped(
        tick: u32,
        item: &str,
        count: u8,
        pos: [f64; 3],
        placed: Option<Placed>,
    ) -> DroppedItem {
        DroppedItem {
            tick,
            stack: crate::transport::Stack {
                item: item.into(),
                count,
            },
            pos,
            motion: [0.1, -0.2, 0.0],
            placed,
        }
    }
    fn dropping(v: Vec<DroppedItem>) -> Snapshot {
        Snapshot {
            drops: Some(v),
            ..Snapshot::default()
        }
    }
    const POPPED: Option<Placed> = Some(Placed::Spread(crate::transport::Spreader::PopResource));
    #[test]
    fn drops_compare_by_how_they_were_placed() {
        let spilled = Some(Placed::Spread(crate::transport::Spreader::DropItemStack));
        let j = dropping(vec![
            dropped(4, "a", 1, [1.5, 9.0625, 0.5], None),
            dropped(3, "b", 2, [2.3, 9.2, 0.5], None),
            dropped(3, "b", 1, [2.7, 9.6, 0.5], None),
            dropped(3, "d", 5, [3.15, 9.7, 0.5], None),
            dropped(5, "c", 1, [7.2, 9.0625, 0.5], None),
        ]);
        let mut r = dropping(vec![
            dropped(3, "b", 3, [2.5, 9.375, 0.5], POPPED),
            dropped(3, "d", 5, [3.5, 9.375, 0.5], spilled),
            dropped(4, "a", 1, [1.500_000_1, 9.0625, 0.5], Some(Placed::Exact)),
            dropped(5, "c", 1, [6.9, 9.0625, 0.5], Some(Placed::Flown)),
        ]);
        assert!(diff(&j, &r).is_empty());
        assert!(diff(&r, &j).is_empty());
        assert!(diff(&j, &j).is_empty());
        let moved = |i: usize, axis: usize, v: f64| {
            let mut r = r.clone();
            r.drops.as_mut().unwrap()[i].pos[axis] = v;
            diff(&j, &r)
        };
        assert!(moved(2, 0, 1.6)[0].starts_with("drops:"));
        assert!(moved(0, 0, 3.1)[0].starts_with("drops:"));
        assert!(moved(0, 1, 9.0)[0].starts_with("drops:"));
        assert!(moved(1, 0, 4.0)[0].starts_with("drops:"));
        assert!(moved(3, 0, 60.0).is_empty());
        r.drops.as_mut().unwrap()[3].stack.count = 2;
        assert!(diff(&j, &r)[0].starts_with("drops:"));
        r.drops = None;
        assert!(diff(&j, &r).is_empty());
    }
    #[test]
    fn a_popped_drop_flown_a_tick_may_leave_its_block() {
        // Seeds 12711 and 12839: a redstone block replaces a depot holding iron ingots, whose
        // `DepotBehaviour.destroy` pops them; pushed out of the redstone block in their first
        // tick, they land in the depot's block or the next one.
        for (origin, seen) in [
            (
                [2, 9, 1],
                [
                    [
                        2.511_777_123_974_070_5,
                        9.265_220_808_560_668,
                        1.701_683_975_510_219,
                    ],
                    [
                        2.571_631_306_256_904_3,
                        9.673_597_911_216_772,
                        0.975_950_484_810_394_6,
                    ],
                ],
            ),
            (
                [-1, 9, 1],
                [
                    [
                        -0.595_811_486_877_739_8,
                        9.751_575_756_579_465,
                        1.756_844_983_720_924_3,
                    ],
                    [
                        0.022_908_018_130_948_45,
                        9.276_099_891_234_324,
                        1.398_586_957_782_868,
                    ],
                ],
            ),
        ] {
            let java =
                seen.map(|p| dropping(vec![dropped(70, "minecraft:iron_ingot", 24, p, None)]));
            assert!(diff(&java[0], &java[1]).is_empty());
            assert!(diff(&java[1], &java[0]).is_empty());
            let middle: [f64; 3] =
                std::array::from_fn(|i| f64::from(origin[i]) + crate::transport::SPREAD_MIDDLE[i]);
            let rust = |dz: f64| {
                let at = [middle[0], middle[1], middle[2] + dz];
                dropping(vec![dropped(70, "minecraft:iron_ingot", 24, at, POPPED)])
            };
            for j in &java {
                assert!(diff(j, &rust(0.0)).is_empty());
                assert!(diff(&rust(0.0), j).is_empty());
            }
            assert!(java.iter().any(|j| !diff(j, &rust(1.0)).is_empty()));
            assert!(java.iter().any(|j| !diff(j, &rust(-1.0)).is_empty()));
        }
    }
    #[test]
    fn unplaced_drops_share_an_origin_or_differ() {
        let java = |x, count| {
            dropping(vec![
                dropped(7, "a", count, [x, 9.3, 0.5], None),
                dropped(7, "a", 3, [x + 0.1, 9.4, 0.6], None),
            ])
        };
        assert!(diff(&java(0.9, 2), &java(1.1, 2)).is_empty());
        assert!(diff(&java(0.9, 2), &java(2.3, 2))[0].starts_with("drops:"));
        assert!(diff(&java(0.9, 2), &java(1.1, 1))[0].starts_with("drops:"));
        let split = |n| {
            dropping(vec![
                dropped(7, "a", n, [1.2, 9.2, 0.4], None),
                dropped(7, "a", 5 - n, [0.7, 9.5, 0.6], None),
            ])
        };
        assert!(diff(&java(0.9, 2), &split(2)).is_empty());
        assert!(diff(&split(3), &java(0.9, 2)).is_empty());
        assert!(diff(&java(0.9, 2), &split(4))[0].starts_with("drops:"));
    }
    #[test]
    fn checkpoints_and_air_positions_are_compared() {
        let mut j = Snapshot::default();
        let mut r = Snapshot::default();
        j.checkpoints.insert(1, Snapshot::default());
        r.checkpoints.insert(
            1,
            Snapshot {
                blocks: [(
                    "2,8,0".into(),
                    crate::scenario::BlockSnap {
                        state: "create:shaft[axis=x]".into(),
                        ..Default::default()
                    },
                )]
                .into(),
                ..Default::default()
            },
        );
        assert!(diff(&j, &r)[0].starts_with("tick 1: 2,8,0:"));
    }
}

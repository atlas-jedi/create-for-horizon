//! Andesite and brass tunnels on the server: casing their belt when placed (`TunnelItem`), their shape from
//! what lies beside their segment, and brass tunnels sharing a stack out among their sides
//! (`BrassTunnelBlockEntity.tick`).

use super::super::server::{ServerLevel, update_all};
use super::super::world::with_world;
use super::belt::{belt_segment, handle_insertion};
use super::info_at;
use create_engine::Block;
use create_engine::belt::Slope;
use create_engine::board::{self, FilterClick};
use create_engine::kinetics::{Axis, Dir};
use create_engine::tunnel;
use mod_api::HostBlockStates;

/// A tunnel's axis and shape at `pos`; None for any other block.
pub fn tunnel_state(lv: &ServerLevel<'_>, pos: [i32; 3]) -> Option<(Axis, tunnel::Shape)> {
    let (block, info) = info_at(lv, pos)?;
    matches!(block, Block::AndesiteTunnel | Block::BrassTunnel)
        .then_some((info.axis?, info.tunnel?))
}

/// The brass tunnel's axis at `pos`, if a brass tunnel is there.
fn brass_axis(lv: &ServerLevel<'_>, pos: [i32; 3]) -> Option<Axis> {
    let (block, info) = info_at(lv, pos)?;
    (block == Block::BrassTunnel).then_some(info.axis?)
}

/// `BeltTunnelBlockEntity.flaps` (`updateTunnelConnections`): the open sides of the tunnel at
/// `pos` (axis `axis`, shape `shape`) that no tunnel continues and no extended belt funnel
/// facing back into it closes.
fn flaps(lv: &ServerLevel<'_>, pos: [i32; 3], axis: Axis, shape: tunnel::Shape) -> Vec<Dir> {
    tunnel::sides(axis, shape)
        .into_iter()
        .filter(|d| {
            let o = d.offset();
            let next = [pos[0] + o[0], pos[1], pos[2] + o[2]];
            if tunnel_state(lv, next).is_some() {
                return false;
            }
            !super::funnel::funnel_at(lv, next).is_some_and(|f| {
                f.belt == Some(create_engine::funnel::BeltShape::Extended)
                    && f.facing == d.opposite()
            })
        })
        .collect()
}

/// `flaps` not empty.
fn has_flaps(lv: &ServerLevel<'_>, pos: [i32; 3], axis: Axis, shape: tunnel::Shape) -> bool {
    !flaps(lv, pos, axis, shape).is_empty()
}

/// `BrassTunnelBlockEntity.hasDistributionBehaviour` of the brass tunnel at `pos`: it has flaps,
/// and is connected with a brass tunnel beside it or has a flap across its axis.
pub fn has_distribution(lv: &ServerLevel<'_>, pos: [i32; 3]) -> bool {
    let Some(axis) = brass_axis(lv, pos) else {
        return false;
    };
    let Some((_, shape)) = tunnel_state(lv, pos) else {
        return false;
    };
    let f = flaps(lv, pos, axis, shape);
    let dim = lv.dim;
    let connected = with_world(|w| {
        w.dims[dim]
            .brass
            .get(&pos)
            .is_some_and(|b| b.connected_left || b.connected_right)
    })
    .unwrap_or(false);
    !f.is_empty() && (connected || f.iter().any(|d| d.axis() != axis))
}

/// `BrassTunnelBlockEntity.canInsert`: whether an item entering from `side` is accepted by the
/// tunnel's own filter and (when distributing) its held slot.
pub fn can_insert(lv: &ServerLevel<'_>, pos: [i32; 3], side: Dir, item: &str) -> bool {
    let distributing = has_distribution(lv, pos);
    let filter_passes = own_filter_passes(lv, pos, side, item);
    let dim = lv.dim;
    with_world(|w| {
        w.dims[dim]
            .brass
            .get(&pos)
            .is_some_and(|b| b.can_insert(distributing, filter_passes))
    })
    .unwrap_or(false)
}

/// `canTakeItems`: the brass tunnel at `pos` holds no stack and does not hand out with its
/// synchronized group.
pub fn can_take(lv: &ServerLevel<'_>, pos: [i32; 3]) -> bool {
    let dim = lv.dim;
    with_world(|w| {
        w.dims[dim]
            .brass
            .get(&pos)
            .is_some_and(create_engine::tunnel::Brass::can_take)
    })
    .unwrap_or(false)
}

/// `BrassTunnelItemHandler.insertItem`: whether the brass tunnel at `pos` passes inserts to its
/// belt below, accepts them to distribute, or holds them back.
pub fn handler_target(lv: &ServerLevel<'_>, pos: [i32; 3]) -> Option<tunnel::HandlerTarget> {
    let distributing = has_distribution(lv, pos);
    let dim = lv.dim;
    with_world(|w| {
        w.dims[dim]
            .brass
            .get(&pos)
            .map(|b| b.handler_target(distributing))
    })
    .flatten()
}

/// `setStackToDistribute`: the brass tunnel at `pos` holds `item` (come in by side `from`) to
/// hand out, in its block entity's slot (for hoppers and funnels) and its state.
pub fn set_stack(
    lv: &mut ServerLevel<'_>,
    pos: [i32; 3],
    item: &create_engine::belt::Item,
    from: Option<Dir>,
) {
    let dim = lv.dim;
    let stack = super::belt::belt_stack(lv, item);
    with_world(|w| {
        let d = &mut w.dims[dim];
        let Some(b) = d.brass.get_mut(&pos) else {
            return;
        };
        b.stack = (item.count > 0).then(|| (item.item.clone(), item.count));
        b.stack_progress = item.progress.clone();
        b.entered_from = from;
        b.progress = -1;
        b.new_item = item.count > 0;
        w.mark_unsaved(dim, pos);
    });
    super::items::set_slot(lv, pos, 0, stack);
}

/// The stack a brass tunnel's slot holds changed by a hopper, a funnel or another mod (its
/// `BrassTunnelItemHandler`): its state takes it as `setStackToDistribute(stack, null)`; a
/// write of the tunnel's own is no change.
pub fn slot_changed(lv: &ServerLevel<'_>, pos: [i32; 3]) {
    let dim = lv.dim;
    let now = super::items::slot(lv, pos, 0).map(|s| {
        let (name, progress) = super::belt::belt_name(&*lv.host, &s);
        (name, s.count().min(255) as u8, progress)
    });
    with_world(|w| {
        let d = &mut w.dims[dim];
        let Some(b) = d.brass.get_mut(&pos) else {
            return;
        };
        let held = now.as_ref().map(|(n, c, _)| (n.clone(), *c));
        if held == b.stack {
            return;
        }
        b.stack = held;
        b.stack_progress = now.and_then(|n| n.2);
        b.entered_from = None;
        b.progress = -1;
        b.new_item = b.stack.is_some();
        w.mark_unsaved(dim, pos);
    });
}

/// `BrassTunnelBlock.useWithoutItem` (`grabAllStacksOfGroup`). True when there were any.
pub fn grab_stacks(lv: &mut ServerLevel<'_>, pos: [i32; 3], player: u128) -> bool {
    let Some(axis) = brass_axis(lv, pos) else {
        return false;
    };
    let dim = lv.dim;
    let mut taken = Vec::new();
    for t in brass_group(lv, pos, axis) {
        let held = with_world(|w| w.dims[dim].brass.get(&t).is_some_and(|b| b.stack.is_some()));
        if held != Some(true) {
            continue;
        }
        let held = with_world(|w| w.dims[dim].brass.get(&t).and_then(|b| b.stack.clone()));
        let kept = held.flatten().and_then(|(item, n)| {
            super::belt::belt_stack(lv, &create_engine::belt::Item::new(&item, n, 0, false))
        });
        if let Some(stack) = super::items::slot(lv, t, 0).or(kept) {
            taken.push(stack);
        }
        let empty = create_engine::belt::Item::new("minecraft:air", 0, 0, false);
        set_stack(lv, t, &empty, None);
    }
    if taken.is_empty() {
        return false;
    }
    for stack in taken {
        super::items::give(&mut *lv.host, player, stack);
    }
    let seed = super::items::random_angle(lv, pos) as f32 / 360.0;
    lv.level.play_sound(
        &mut *lv.host,
        "minecraft:entity.item.pickup",
        pos.map(|v| f64::from(v) + 0.5),
        mod_api::SoundSource::Players,
        0.2,
        1.0 + seed,
    );
    true
}

/// `BeltTunnelItem.updateCustomBlockEntityTag`: the belt segment under a tunnel placed from its
/// item takes the tunnel's casing while it has none, as the block is placed; the tunnel takes
/// its shape (`getStateForPlacement`) and looks at its connections (`onPlace`) at once.
pub struct TunnelItem;

#[mod_api::horizon_part]
impl mod_api::ItemPart for TunnelItem {
    fn on_block_placed_by(
        &self,
        host: &mut dyn mod_api::ServerHost,
        event: &mut mod_api::OnBlockPlacedByEvent<'_>,
    ) {
        let Some(dimension) = crate::glue::compat::placed_level(&*host, event)
            .and_then(|l| l.dimension().id(&*host)) else {
            return;
        };
        let dimension = dimension.to_string();
        let Some(mut lv) = ServerLevel::new(host, &dimension) else {
            return;
        };
        let pos = event.pos;
        let kind = match info_at(&lv, pos) {
            Some((Block::BrassTunnel, _)) => "brass",
            Some((Block::AndesiteTunnel, _)) => "andesite",
            _ => return,
        };
        let below = [pos[0], pos[1] - 1, pos[2]];
        if info_at(&lv, below).and_then(|(_, i)| i.belt).is_none() {
            return;
        }
        let cased = lv.block_state(below).is_some_and(|s| {
            s.property(&lv.host.block_states(), "casing").ok().flatten() == Some("true")
        });
        if !cased {
            super::belt::set_casing(&mut lv, below, Some(kind));
        }
        shape_tunnel(&mut lv, pos, true);
        // `onPlace`: its connections, before the save that can follow the placement at once.
        update_brass_connections(&lv, vec![pos]);
        super::super::persist::publish(&mut lv);
    }
}

/// `BrassTunnelBlockEntity.getAdjacent`: the brass tunnel of the same axis beside the one at
/// `pos` across its axis (`left`: counterclockwise from the axis' positive way).
fn brass_adjacent(lv: &ServerLevel<'_>, pos: [i32; 3], axis: Axis, left: bool) -> Option<[i32; 3]> {
    let next = tunnel::adjacent_pos(pos, axis, left);
    (brass_axis(lv, next) == Some(axis)).then_some(next)
}

/// `BrassTunnelBlockEntity.updateTunnelConnections` → `updateFilterPresence`: the tunnel at
/// `pos` keeps the side filters of the open sides of `open` (its axis and shape).
fn keep_side_filters(lv: &mut ServerLevel<'_>, pos: [i32; 3], open: (Axis, tunnel::Shape)) {
    let dim = lv.dim;
    let sides = tunnel::sides(open.0, open.1);
    if with_world(|w| w.keep_side_filters(dim, pos, &sides)).unwrap_or(false) {
        show_tunnel_filters(lv, pos);
    }
}

/// `BeltTunnelBlock.canSurvive` and `getTunnelState` for the tunnel at `pos`: it breaks without a
/// cased, flat belt below, else takes the shape its segment's outputs give (`at_once`: as placed
/// or `updateTunnel`; otherwise `updateShape`, keeping a state whose window still fits).
fn shape_tunnel(lv: &mut ServerLevel<'_>, pos: [i32; 3], at_once: bool) {
    let belt_below = |lv: &ServerLevel<'_>, p: [i32; 3]| {
        info_at(lv, [p[0], p[1] - 1, p[2]]).and_then(|(_, i)| i.belt)
    };
    let cased = |lv: &ServerLevel<'_>, p: [i32; 3]| {
        let Some(state) = lv.block_state([p[0], p[1] - 1, p[2]]) else {
            return false;
        };
        let facts = lv.host.block_states();
        state.property(&facts, "casing").ok().flatten() == Some("true")
    };
    let Some(now) = tunnel_state(lv, pos) else {
        return;
    };
    let seg = belt_below(lv, pos).filter(|s| s.slope == Slope::Horizontal);
    let Some(seg) = seg.filter(|_| cased(lv, pos)) else {
        lv.destroy_block(pos, true);
        return;
    };
    let axis = seg.facing.axis();
    // `hasValidOutput` beside the belt segment: a belt across it, or anything taking items
    // from that side (`DirectBeltInputBehaviour.canInsertFromSide`: a depot, an ejector, a
    // floor funnel, a chute, an item drain, a processing machine).
    let output = |lv: &ServerLevel<'_>, side: Dir| {
        let o = side.offset();
        let p = [pos[0] + o[0], pos[1] - 1, pos[2] + o[2]];
        if let Some((Block::Belt, i)) = info_at(lv, p) {
            return i.belt.is_some_and(|b| b.facing.axis() == side.axis());
        }
        super::belt::direct_input_at(lv, p) && super::belt::direct_input_accepts(lv, p, side)
    };
    let left = tunnel::left(axis);
    let (on_left, on_right) = (output(lv, left), output(lv, left.opposite()));
    // `canHaveWindow`: tunnels on both ends.
    let fw = if axis == Axis::X {
        Dir::East
    } else {
        Dir::South
    };
    // Or an extended belt funnel facing back into it, but not on both.
    let ends = [fw, fw.opposite()].map(|d| {
        let o = d.offset();
        let next = [pos[0] + o[0], pos[1], pos[2] + o[2]];
        let funnel = super::funnel::funnel_at(lv, next).is_some_and(|f| {
            f.belt == Some(create_engine::funnel::BeltShape::Extended) && f.facing == d.opposite()
        });
        (tunnel_state(lv, next).is_some() || funnel, funnel)
    });
    let window = ends[0].0 && ends[1].0 && !(ends[0].1 && ends[1].1);
    let fresh = (axis, tunnel::shape_for(on_left, on_right, window));
    let (axis2, shape2) = if at_once {
        fresh
    } else {
        tunnel::updated(now, fresh)
    };
    // `updateShape` runs `updateTunnelConnections` before its state is set, `updateTunnel` after.
    keep_side_filters(lv, pos, if at_once { (axis2, shape2) } else { now });
    if (axis2, shape2) == now {
        return;
    }
    let Some(state) = lv.block_state(pos) else {
        return;
    };
    let next = {
        let facts = lv.host.block_states();
        let axis_name = if axis2 == Axis::X { "x" } else { "z" };
        state
            .with(&facts, "axis", axis_name)
            .and_then(|s| s.with(&facts, "shape", shape2.name()))
    };
    if let Ok(next) = next {
        lv.set_state(pos, next, update_all());
    }
}

/// Andesite tunnels: those whose surroundings changed break without a cased, flat belt below (`canSurvive`) or take the shape
/// the belts and depots beside their segment give (`updateShape`, `getTunnelState`). First,
/// brass tunnels show their clients the stacks they distribute (`show_stacks`).
pub fn tick_tunnels(lv: &mut ServerLevel<'_>) {
    show_stacks(lv);
    let dim = lv.dim;
    let Some((new, kept, fresh, initialized)) = with_world(|w| {
        let d = &mut w.dims[dim];
        (
            std::mem::take(&mut d.tunnels_new),
            std::mem::take(&mut d.tunnel_checks),
            std::mem::take(&mut d.tunnel_fresh),
            std::mem::take(&mut d.brass_initialize),
        )
    }) else {
        return;
    };
    if new.is_empty() && kept.is_empty() && fresh.is_empty() && initialized.is_empty() {
        return;
    }
    let mut checks: Vec<([i32; 3], bool)> = fresh
        .into_iter()
        .map(|p| (p, true))
        .chain(kept.into_iter().map(|p| (p, false)))
        .collect();
    // A fresh look wins over a kept one for the same tunnel.
    checks.sort_by_key(|c| (c.0, !c.1));
    checks.dedup_by_key(|c| c.0);
    let check_positions: Vec<[i32; 3]> = checks.iter().map(|c| c.0).collect();
    for (pos, at_once) in checks {
        shape_tunnel(lv, pos, at_once);
    }
    // `BrassTunnelBlockEntity.updateTunnelConnections`: newly placed tunnels run before neighbour
    // shape updates (matching `onPlace` before `updateShape`), so a joining tunnel takes the
    // group's mode; a loaded one looks at its connections again (`initialize`).
    for &p in &initialized {
        if let Some(open) = tunnel_state(lv, p) {
            keep_side_filters(lv, p, open);
        }
    }
    let mut conn_checks = new;
    for p in initialized.into_iter().chain(check_positions) {
        if !conn_checks.contains(&p) {
            conn_checks.push(p);
        }
    }
    update_brass_connections(lv, conn_checks);
}

/// `BrassTunnelBlockEntity.updateTunnelConnections` for the brass tunnels at `roots`, in order:
/// their connections across their axis, and the mode handed on to a tunnel newly joined.
fn update_brass_connections(lv: &ServerLevel<'_>, roots: Vec<[i32; 3]>) {
    let dim = lv.dim;
    let brass_info = collect_brass_info(lv, &roots);
    if brass_info.is_empty() {
        return;
    }
    with_world(|w| {
        let d = &mut w.dims[dim];
        let mut changed = Vec::new();
        for p in roots {
            tunnel::update_connections(
                p,
                &mut d.brass,
                &|q| brass_info.get(&q).copied(),
                &mut changed,
            );
        }
        for p in changed {
            w.mark_unsaved(dim, p);
        }
    });
}

/// Collects `(axis, has_flaps)` for every brass tunnel reachable from `roots` across its axis.
fn collect_brass_info(
    lv: &ServerLevel<'_>,
    roots: &[[i32; 3]],
) -> std::collections::HashMap<[i32; 3], (Axis, bool)> {
    let mut brass_info = std::collections::HashMap::new();
    let mut stack = roots.to_vec();
    while let Some(p) = stack.pop() {
        if brass_info.contains_key(&p) {
            continue;
        }
        let Some(axis) = brass_axis(lv, p) else {
            continue;
        };
        let Some((_, shape)) = tunnel_state(lv, p) else {
            continue;
        };
        brass_info.insert(p, (axis, has_flaps(lv, p, axis, shape)));
        for left in [true, false] {
            stack.push(tunnel::adjacent_pos(p, axis, left));
        }
    }
    brass_info
}

/// A player's click on a brass tunnel's side filter slot (`SidedFilteringBehaviour` through
/// `ValueSettingsInputHandler`, at `BrassTunnelFilterSlot`): the held item becomes that side's
/// filter (a filter item taken from the hand, the one it held given back), an empty hand clears
/// it. True when the click was a slot's.
pub fn use_tunnel_filter(
    lv: &mut ServerLevel<'_>,
    pos: [i32; 3],
    click: &mod_api::BlockClick<'_>,
) -> bool {
    use crate::glue::items::filters;
    let Some((axis, shape)) = tunnel_state(lv, pos) else {
        return false;
    };
    let Some(p) = lv.host.player(click.player) else {
        return false;
    };
    if !crate::glue::interaction::values::may_use_value_box(&*lv.host, p) {
        return false;
    }
    let held = click.stack.map(|s| super::items::id_of(&*lv.host, s));
    let Some(side) = Dir::from_offset(click.hit.face) else {
        return false;
    };
    let local = [0, 1, 2].map(|i| click.hit.point[i] - f64::from(pos[i]));
    let slot = tunnel::filter_slot(axis, shape, side);
    match board::filter_click(slot, local, held.as_deref()) {
        FilterClick::Miss => return false,
        FilterClick::Keep => return true,
        FilterClick::Set => {}
    }
    let filter = filters::take_filter(lv.host, p, click.hand);
    let dim = lv.dim;
    let old = with_world(|w| {
        let d = &mut w.dims[dim];
        d.unsaved.push((pos[0] >> 4, pos[2] >> 4));
        match filter {
            Some(f) => d.tunnel_filters.insert((pos, side), f),
            None => d.tunnel_filters.remove(&(pos, side)),
        }
    })
    .flatten();
    show_tunnel_filters(lv, pos);
    filters::return_filter(lv, p, pos, old);
    true
}

/// The block entity key listing the items a brass tunnel's side filter slots show, by side
/// (`Dir` order; an empty string: none), synced for its clients to draw.
pub const SIDE_FILTERS: &str = "SideFilters";

/// Shows the brass tunnel at `pos`'s side filters (`SIDE_FILTERS`).
pub fn show_tunnel_filters(lv: &mut ServerLevel<'_>, pos: [i32; 3]) {
    use crate::glue::items::filters;
    let dim = lv.dim;
    let ids: Vec<mod_api::Tag> = Dir::ALL
        .iter()
        .map(|side| {
            let f =
                with_world(|w| w.dims[dim].tunnel_filters.get(&(pos, *side)).cloned()).flatten();
            let id = f
                .and_then(|f| filters::filter_stack(&*lv.host, &f))
                .map(|s| super::items::id_of(&*lv.host, &s))
                .unwrap_or_default();
            mod_api::Tag::String(id.as_str().into())
        })
        .collect();
    if let Some(e) = lv.host.block_entity(lv.level, pos) {
        let data = mod_api::Compound::from([(SIDE_FILTERS.into(), mod_api::Tag::List(ids))]);
        let _ = e.merge_data(&mut *lv.host, data);
    }
}

/// The block entity key holding the stack a brass tunnel distributes, synced for its clients'
/// goggles: `Item` (as a belt names it; empty: none) and `Count`.
pub const DISTRIBUTING: &str = "Distributing";

/// `BrassTunnelBlockEntity.sendData` of `stackToDistribute`: each brass tunnel of this level
/// whose stack is not what its clients were last told writes it to `DISTRIBUTING`.
fn show_stacks(lv: &mut ServerLevel<'_>) {
    use mod_api::{Compound, Tag};
    let dim = lv.dim;
    let stale: Vec<([i32; 3], Option<(String, u8)>)> = with_world(|w| {
        w.dims[dim]
            .brass
            .iter()
            .filter(|(_, b)| b.shown.as_ref() != Some(&b.stack))
            .map(|(p, b)| (*p, b.stack.clone()))
            .collect()
    })
    .unwrap_or_default();
    for (pos, stack) in stale {
        let Some(e) = lv.host.block_entity(lv.level, pos) else {
            continue;
        };
        let (item, count) = stack.clone().unwrap_or_default();
        let shown = Compound::from([
            ("Item".into(), Tag::String(item.as_str().into())),
            ("Count".into(), Tag::Int(i32::from(count))),
        ]);
        let data = Compound::from([(DISTRIBUTING.into(), Tag::Compound(shown))]);
        if e.merge_data(&mut *lv.host, data).is_ok() {
            with_world(|w| {
                if let Some(b) = w.dims[dim].brass.get_mut(&pos) {
                    b.shown = Some(stack);
                }
            });
        }
    }
}

/// `BrassTunnelBlockEntity.addToGoggleTooltip` on a client: the stacks the brass tunnel at `pos`
/// and the brass tunnels of its axis beside it hold (`grabAllStacksOfGroup(true)`: its own, then
/// leftward, then rightward), as their block entities last showed them (`DISTRIBUTING`).
pub(crate) fn goggle_lines(
    host: &dyn mod_api::ClientHost,
    pos: [i32; 3],
    lines: &mut Vec<Vec<(String, u32)>>,
) {
    use mod_api::Tag;
    let Some(level) = host.level(host.dimension()) else {
        return;
    };
    let brass_axis = |p: [i32; 3]| {
        let id = super::super::states::id(level.block_state(host, p)?)?;
        super::super::client_table(host, |t| t.get(id).map(|(b, i)| (b, i.axis)))
            .flatten()
            .and_then(|(b, axis)| if b == Block::BrassTunnel { axis } else { None })
    };
    let Some(axis) = brass_axis(pos) else {
        return;
    };
    let mut group = vec![pos];
    for left in [true, false] {
        let mut at = pos;
        loop {
            let next = tunnel::adjacent_pos(at, axis, left);
            if brass_axis(next) != Some(axis) {
                break;
            }
            group.push(next);
            at = next;
        }
    }
    let stacks: Vec<(String, u32)> = group
        .into_iter()
        .filter_map(|p| {
            let data = host.block_entity(level, p)?.data(host)?;
            let Some(Tag::Compound(c)) = data.get(&DISTRIBUTING.into()) else {
                return None;
            };
            let (Some(Tag::String(item)), Some(Tag::Int(count))) =
                (c.get(&"Item".into()), c.get(&"Count".into()))
            else {
                return None;
            };
            let item = item.to_string_lossy();
            if item.is_empty() || *count <= 0 {
                return None;
            }
            // `Component.translatable(item.getDescriptionId())`.
            let name = super::items::stack_of(host, &item, 1)
                .map_or_else(|| item.to_string(), |s| host.hover_name(&s));
            Some((name, *count as u32))
        })
        .collect();
    let tr = |key: &str| host.translate(key);
    lines.extend(tunnel::goggle_lines(&stacks, &tr));
}

/// `selectionMode.withCallback`: the brass tunnel at `pos` and those connected with it take
/// mode `mode`.
pub fn set_brass_mode(lv: &ServerLevel<'_>, pos: [i32; 3], mode: tunnel::Mode) {
    let brass_info = collect_brass_info(lv, &[pos]);
    let dim = lv.dim;
    with_world(|w| {
        let mut changed = Vec::new();
        tunnel::set_group_mode(
            pos,
            mode,
            &mut w.dims[dim].brass,
            &|q| brass_info.get(&q).copied(),
            &mut changed,
        );
        for p in changed {
            w.mark_unsaved(dim, p);
        }
    });
}

/// `BrassTunnelBlockEntity.testFlapFilter`: whether `stack` (item `item`) may pass side `side` of
/// the brass tunnel at `pos`: its filter there, else the filter of the block beside it there (a
/// brass funnel's or smart chute's), else anything.
pub fn flap_filter_passes(lv: &ServerLevel<'_>, pos: [i32; 3], side: Dir, item: &str) -> bool {
    let dim = lv.dim;
    let o = side.offset();
    let beside = [pos[0] + o[0], pos[1] + o[1], pos[2] + o[2]];
    with_world(|w| {
        let d = &w.dims[dim];
        let f = d
            .tunnel_filters
            .get(&(pos, side))
            .or_else(|| d.chute_filters.get(&beside));
        crate::glue::items::filters::passes_item(&*lv.host, f, item)
    })
    .unwrap_or(true)
}

/// `SidedFilteringBehaviour.test(side, stack)`: the brass tunnel at `pos`'s own filter on side
/// `side` passes item `item` (none passes anything).
pub fn own_filter_passes(lv: &ServerLevel<'_>, pos: [i32; 3], side: Dir, item: &str) -> bool {
    let dim = lv.dim;
    with_world(|w| {
        let f = w.dims[dim].tunnel_filters.get(&(pos, side));
        crate::glue::items::filters::passes_item(&*lv.host, f, item)
    })
    .unwrap_or(true)
}

/// `flapFilterEmpty`: no filter on side `side` of the brass tunnel at `pos`, nor beside it.
fn flap_filter_empty(lv: &ServerLevel<'_>, pos: [i32; 3], side: Dir) -> bool {
    let dim = lv.dim;
    let o = side.offset();
    let beside = [pos[0] + o[0], pos[1] + o[1], pos[2] + o[2]];
    with_world(|w| {
        let d = &w.dims[dim];
        !d.tunnel_filters.contains_key(&(pos, side)) && !d.chute_filters.contains_key(&beside)
    })
    .unwrap_or(true)
}

/// The brass tunnels connected with the one at `pos` across its axis, itself first
/// (`gatherValidOutputs`' walk with `getAdjacent`).
fn brass_group(lv: &ServerLevel<'_>, pos: [i32; 3], axis: Axis) -> Vec<[i32; 3]> {
    let mut group = vec![pos];
    for left in [true, false] {
        let mut at = pos;
        while let Some(next) = brass_adjacent(lv, at, axis, left) {
            at = next;
            group.push(at);
        }
    }
    group
}

/// `addValidOutputsOf`: the sides of the brass tunnel at `t` its stack may go out by, as
/// (tunnel, side): for the distributing tunnel itself (`own`) the sides across the belt first,
/// then ahead; never back, never ahead of a still belt, not into a belt funnel pulling from that
/// side; a side with something taking items there (`DirectBeltInputBehaviour`), or the way ahead
/// when nothing solid stops it (the stack is thrown off the belt there).
fn valid_outputs_of(lv: &ServerLevel<'_>, t: [i32; 3], own: bool) -> Vec<([i32; 3], Dir)> {
    let dim = lv.dim;
    let below = [t[0], t[1] - 1, t[2]];
    let Some((c, _, seg)) = belt_segment(lv, below) else {
        return Vec::new();
    };
    let Some((axis, shape)) = tunnel_state(lv, t) else {
        return Vec::new();
    };
    let speed = with_world(|w| w.speed(dim, c)).flatten().unwrap_or(0.0);
    let movement = create_engine::belt::movement_facing(speed, seg.facing);
    let open = tunnel::sides(axis, shape);
    let order = [Dir::South, Dir::West, Dir::North, Dir::East];
    let mut out = Vec::new();
    for side_pass in [true, false] {
        if !own && side_pass {
            continue;
        }
        for d in order {
            if d == movement && speed == 0.0 {
                continue;
            }
            if own && side_pass == (d.axis() == movement.axis()) {
                continue;
            }
            if d == movement.opposite() || !open.contains(&d) {
                continue;
            }
            let o = d.offset();
            let offset = [below[0] + o[0], below[1], below[2] + o[2]];
            let funnel = super::funnel::funnel_at(lv, [offset[0], offset[1] + 1, offset[2]]);
            if funnel.is_some_and(|f| {
                f.belt == Some(create_engine::funnel::BeltShape::Pulling) && f.facing == d
            }) {
                continue;
            }
            if super::belt::direct_input_at(lv, offset) {
                if super::belt::direct_input_accepts(lv, offset, d) {
                    out.push((t, d));
                }
                continue;
            }
            if d == movement && !solid_toward(lv, offset, d.opposite()) {
                out.push((t, d));
            }
        }
    }
    out
}

/// `BlockHelper.hasBlockSolidSide` of the block at `pos` on side `face`.
fn solid_toward(lv: &ServerLevel<'_>, pos: [i32; 3], face: Dir) -> bool {
    let face = match face {
        Dir::Down => mod_api::BlockFace::Down,
        Dir::Up => mod_api::BlockFace::Up,
        Dir::North => mod_api::BlockFace::North,
        Dir::South => mod_api::BlockFace::South,
        Dir::West => mod_api::BlockFace::West,
        Dir::East => mod_api::BlockFace::East,
    };
    lv.level
        .block_face_sturdy(&*lv.host, pos, face, mod_api::SupportType::Full)
        == Some(true)
}

/// `insertIntoTunnel`: `n` of the stack out of side `d` of the brass tunnel `t` (None: that side
/// takes none of it, else what is left). Its filter must pass it; something taking items there
/// takes what it does (and the flap swings out); else, ahead, the stack is thrown off the belt.
fn insert_into_tunnel(
    lv: &mut ServerLevel<'_>,
    t: [i32; 3],
    d: Dir,
    item: &create_engine::belt::Item,
    simulate: bool,
) -> Option<u8> {
    if !flap_filter_passes(lv, t, d, &item.item) {
        return None;
    }
    let dim = lv.dim;
    let below = [t[0], t[1] - 1, t[2]];
    let (c, index, seg) = belt_segment(lv, below)?;
    let o = d.offset();
    let p = [below[0] + o[0], below[1], below[2] + o[2]];
    if super::belt::direct_input_at(lv, p) {
        if !super::belt::direct_input_accepts(lv, p, d) {
            return None;
        }
        // `sideOutput.handleInsertion(stack, side, simulate)`: what it takes, or would.
        let left = handle_insertion(lv, p, d, item, false, simulate).unwrap_or(item.count);
        if left == 0 && !simulate {
            super::flaps::tunnel(lv, below, d, false);
        }
        return Some(left);
    }
    let speed = with_world(|w| w.speed(dim, c)).flatten().unwrap_or(0.0);
    let movement = create_engine::belt::movement_facing(speed, seg.facing);
    if d != movement || solid_toward(lv, p, d.opposite()) {
        return None;
    }
    if !simulate {
        super::flaps::tunnel(lv, below, d, true);
        // Thrown off ahead of the segment, forward and a little up.
        let length = with_world(|w| {
            let dd = &w.dims[dim];
            dd.belts.get(&c).map(|b| b.chain.len() as i32)
        })
        .flatten()
        .unwrap_or(1);
        let aware = create_engine::belt::direction_aware_speed(speed, seg.facing);
        let extra = if aware > 0.0 { 1 } else { 0 };
        let at = create_engine::belt::vector_for_offset(c, &seg, length, (index + extra) as f32);
        let v = create_engine::belt::movement_speed(speed)
            .abs()
            .max(1.0 / 8.0);
        let dv = d.offset();
        let motion = [
            f64::from(dv[0]) * f64::from(v),
            1.0 / 8.0,
            f64::from(dv[2]) * f64::from(v),
        ];
        if let Some(stack) = super::belt::belt_stack(lv, item) {
            super::items::drop_stack(lv, [at[0], at[1] + 6.0 / 16.0, at[2]], motion, stack, 10);
        }
    }
    Some(0)
}

/// The turn of a brass tunnel in the shared transport walk (`BrassTunnelBlockEntity.tick`, in
/// the level's block-entity order: a belt or depot ticked before it holds what it hands out
/// until its next turn): two ticks after a stack arrives (then every `brassTunnelTimer` while
/// some is left), the stack is split among the outputs of the tunnel and those connected with
/// it, filtered sides first, as its mode says; synchronizing tunnels hand out only while every
/// one of their group holds a stack.
pub(super) fn tick_brass(lv: &mut ServerLevel<'_>, t: [i32; 3]) {
    let dim = lv.dim;
    let Some(mut b) = with_world(|w| w.dims[dim].brass.get(&t).cloned()).flatten() else {
        return;
    };
    let below = [t[0], t[1] - 1, t[2]];
    let rpm = belt_segment(lv, below)
        .and_then(|(c, ..)| with_world(|w| w.speed(dim, c)).flatten())
        .unwrap_or(0.0);
    let save = |b: &tunnel::Brass| {
        with_world(|w| {
            let d = &mut w.dims[dim];
            let changed = d
                .brass
                .get(&t)
                .is_some_and(|x| x.to_saved() != b.to_saved());
            if let Some(x) = d.brass.get_mut(&t) {
                *x = b.clone();
            }
            if changed {
                w.mark_unsaved(dim, t);
            }
        });
    };
    let phase = b.begin_tick(rpm != 0.0);
    if matches!(phase, tunnel::BrassPhase::Idle | tunnel::BrassPhase::Wait) {
        save(&b);
        return;
    }
    let Some((axis, _)) = tunnel_state(lv, t) else {
        return;
    };
    if phase == tunnel::BrassPhase::Gather {
        let group = brass_group(lv, t, axis);
        let held: Vec<bool> = group
            .iter()
            .map(|p| {
                with_world(|w| w.dims[dim].brass.get(p).is_some_and(|x| x.stack.is_some()))
                    .unwrap_or(false)
            })
            .collect();
        let (sync_update, can_gather) = b.gather_sync(&held);
        if let Some(synced) = sync_update {
            with_world(|w| {
                let mut changed = Vec::new();
                let d = &mut w.dims[dim];
                for p in &group {
                    if let Some(x) = d.brass.get_mut(p)
                        && x.synced != synced
                    {
                        x.synced = synced;
                        changed.push(*p);
                    }
                }
                for p in changed {
                    w.mark_unsaved(dim, p);
                }
            });
        }
        if !can_gather {
            save(&b);
            return;
        }
        let Some((item, count)) = b.stack.clone() else {
            save(&b);
            return;
        };
        let mut one = create_engine::belt::Item::new(&item, count, 180, false);
        one.progress = b.stack_progress.clone();
        let mut targets = tunnel::Targets::default();
        for (i, g) in group.iter().enumerate() {
            for (g2, d) in valid_outputs_of(lv, *g, i == 0) {
                if insert_into_tunnel(lv, g2, d, &one, true).is_none() {
                    continue;
                }
                if flap_filter_empty(lv, g2, d) {
                    targets.plain.push((g2, d));
                } else {
                    targets.filtered.push((g2, d));
                }
            }
        }
        b.gathered(targets);
        save(&b);
        return;
    }
    let Some((item, count)) = b.stack.clone() else {
        b.progress = -1;
        save(&b);
        return;
    };
    let mut one = create_engine::belt::Item::new(&item, count, 180, false);
    one.progress = b.stack_progress.clone();
    let lists = b.targets.to_hand_out(t, b.entered_from, |p| {
        with_world(|w| w.dims[dim].brass.contains_key(&p)).unwrap_or(false)
    });
    let mut left = count;
    for outputs in lists {
        if left == 0 || outputs.is_empty() {
            continue;
        }
        let random = super::items::random_angle(lv, t) as usize;
        let dealt = tunnel::distribute(
            left,
            outputs.len(),
            b.previous_output,
            b.mode,
            random,
            |i, n, simulate| {
                let (g, d) = outputs[i];
                let mut part = one.clone();
                part.count = n;
                insert_into_tunnel(lv, g, d, &part, simulate)
            },
        );
        // A forced mode with an output refusing hands out nothing and stays on the same
        // output (`distribute` returns early).
        if let Some(l) = dealt {
            left = l;
            b.previous_output = (b.previous_output + 1) % outputs.len().max(1);
        }
    }
    let changed = left != count;
    b.stack = (left > 0).then(|| (item.clone(), left));
    if b.stack.is_none() {
        b.entered_from = None;
        b.stack_progress = None;
    }
    b.progress = -1;
    save(&b);
    if changed {
        let rest = if left == 0 {
            None
        } else {
            let mut part = one.clone();
            part.count = left;
            super::items::slot(lv, t, 0)
                .and_then(|s| super::items::with_count(&s, u32::from(left)))
                .or_else(|| super::belt::belt_stack(lv, &part))
        };
        super::items::set_slot(lv, t, 0, rest);
    }
}

//! The track's client side: a track block's own outline (`drawCustomBlockSelection`), aiming at
//! a curve (`TrackBlockOutline.pickCurves`, its outline `drawCurveSelection`, no block outline
//! while one is aimed at), breaking a curve on its own and using items on it
//! (`CurvedTrackInteraction`), the preview of a track-targeting item's pick
//! (`TrackTargetingClient`) and the signal sections shown while a signal is held
//! (`tickSignalOverlay`, `TrackGraphVisualizer.visualiseSignalEdgeGroups`), and the sprint key
//! held as a track click goes out (`TrackBlockItem.sendExtenderPacket`).
//!
//! A client knows the curves from the track blocks' data (their `Curves`, the curves drawn from
//! each); the graph is the server's, so a pick's preview asks it whether the pick holds
//! (`withGraphLocation`) and where its marker goes, and draws it once told.

use super::super::states;
use super::tracks::{self, CHANNEL, CURVES};
use crate::glue::items;
use crate::glue::names::Named as _;
use create_engine::Block;
use create_engine::bezier::{Curve, Vec3};
use create_engine::network::{PointKind, Target};
use create_engine::track_curves::Saved;
use create_engine::track_outline::{self, Selection};
use mod_api::{
    BlockHighlightEvent, ClientHost, Compound, Flow, Hand, HostBlockStates, InteractionKey,
    InteractionKeyMappingTriggeredEvent, LightCoords, LightLayer, ModelInstance, Overlay,
    RenderLevelStage, RenderLevelStageEvent, WorldDepth, WorldLine,
};
use std::cell::RefCell;
use std::collections::HashMap;

/// The crack's breaker id on this client (`destroyBlockProgress`'s player id).
const BREAKER: i32 = 0x0C2E_7AC0;

/// `renderShape`'s colours, at alpha 0.4: black, white where valid, red-orange (1, 0.25, 0.125)
/// where not.
const BLACK: u32 = 0x6600_0000;
const WHITE: u32 = 0x66FF_FFFF;
const RED: u32 = 0x66FF_4020;

#[derive(Default)]
struct Client {
    /// Each track block's curves drawn from it (`TRACKS_WITH_TURNS`), from its data.
    turns: HashMap<[i32; 3], Vec<(Saved, Curve)>>,
    /// The segment aimed at (`TrackBlockOutline.result`).
    selection: Option<Selection>,
    /// `CurvedTrackInteraction`'s breaking.
    break_ticks: i32,
    break_timeout: i32,
    break_progress: f32,
    break_pos: Option<[i32; 3]>,
    /// The pick last asked of the server, and its answer when it holds: the track block, the
    /// marker's kind and transform (`TrackTargetingClient.lastLocation`).
    asked: Option<Vec<u8>>,
    preview: Option<([i32; 3], PointKind, [f32; 12])>,
    /// The track block outlined in place of its block's outline, and its shape.
    highlight: Option<([i32; 3], create_engine::track::TrackShape)>,
    /// The signal sections the server last sent (`tickSignalOverlay`): each line's ends and its
    /// colour.
    overlay: Vec<(Vec3, Vec3, u32)>,
}

thread_local! {
    static CLIENT: RefCell<Client> = RefCell::new(Client::default());
}

pub fn reset() {
    CLIENT.with_borrow_mut(|c| *c = Client::default());
}

/// A track block entity's data reached this client: its curves, if it says.
pub fn data_received(pos: [i32; 3], tag: &Compound) {
    let curves = super::saved::curves_of(tag.get(&CURVES.into()));
    CLIENT.with_borrow_mut(|c| {
        if curves.is_empty() {
            c.turns.remove(&pos);
        } else {
            let list = curves.into_iter().map(|s| (s, s.curve())).collect();
            c.turns.insert(pos, list);
        }
    });
}

/// The block entity at `pos` left this client.
pub fn unloaded(pos: [i32; 3]) {
    CLIENT.with_borrow_mut(|c| {
        c.turns.remove(&pos);
    });
}

fn block_at(host: &dyn ClientHost, pos: [i32; 3]) -> Option<mod_api::BlockState> {
    host.level(host.dimension())?.block_state(host, pos)
}

fn is_track(host: &dyn ClientHost, pos: [i32; 3]) -> bool {
    block_at(host, pos).is_some_and(|s| s.block() == states::key(Block::Track))
}

fn distance_sqr(a: [f64; 3], b: [f64; 3]) -> f64 {
    (0..3).map(|k| (a[k] - b[k]).powi(2)).sum()
}

/// A client tick: the curve aimed at, its breaking, and the pick's preview; with no signal in
/// the main hand, no signal sections are shown.
pub fn client_tick(host: &mut dyn ClientHost) {
    pick(&*host);
    interact(host);
    hover(host);
    if !holding_signal(&*host) {
        CLIENT.with_borrow_mut(|c| c.overlay.clear());
    }
}

fn holding_signal(host: &dyn ClientHost) -> bool {
    host.local_player()
        .and_then(|p| items::held(host, p, Hand::Main))
        .is_some_and(|s| s.item().name(host) == "create:track_signal")
}

/// `TrackBlockOutline.pickCurves`: of the curves this client knows, the segment the view meets
/// within reach and before what the crosshair is on.
fn pick(host: &dyn ClientHost) {
    let selection = (|| {
        let player = host.local_player()?;
        let entity = player.entity();
        // `cameraEntity instanceof LocalPlayer`.
        if host.camera_entity() != Some(entity) {
            return None;
        }
        let eye = entity.eye_position(host)?;
        let look = entity.view_vector(host)?;
        let range = host
            .living(entity)
            .and_then(|l| l.attribute_value(host, "minecraft:block_interaction_range"))
            .unwrap_or(4.5);
        // `mc.hitResult.getLocation()`: a miss lies at the end of the reach.
        let reach_sqr = match host.crosshair_target() {
            Some(mod_api::CrosshairTarget::Block { hit, .. }) => distance_sqr(hit.point, eye),
            Some(mod_api::CrosshairTarget::Entity { point, .. }) => distance_sqr(point, eye),
            _ => range * range,
        };
        CLIENT.with_borrow_mut(|c| {
            c.turns.retain(|p, _| is_track(host, *p));
            let curves = c
                .turns
                .iter()
                .flat_map(|(p, list)| list.iter().map(move |(s, curve)| (*p, s.to, curve)));
            track_outline::pick(curves, eye, look, reach_sqr, range)
        })
    })();
    CLIENT.with_borrow_mut(|c| c.selection = selection);
}

/// The curve aimed at, if any.
fn selection() -> Option<Selection> {
    CLIENT.with_borrow(|c| c.selection)
}

/// `CurvedTrackInteraction.clientTick`: holding attack on a curve breaks it, as fast as its track
/// block breaks (an eighth of that a tick, an eighth of the way a tick in creative), with the
/// block's hit sound every four ticks and its crack on the track block; broken, the server takes
/// it up. Let go, the crack goes two ticks later.
fn interact(host: &mut dyn ClientHost) {
    let Some(player) = host.local_player() else {
        return;
    };
    if player.may_build(&*host) != Some(true) {
        return;
    }
    let attack = crate::glue::keys::down(&*host, "key.attack");
    if attack && let Some(sel) = selection() {
        let pos = sel.track;
        let state = block_at(&*host, pos);
        if state.is_none_or(|s| host.block_state_string(s).as_deref() == Some("minecraft:air")) {
            reset_break(host);
            return;
        }
        let Some(state) = state else {
            return;
        };
        let ticks = CLIENT.with_borrow(|c| c.break_ticks);
        if ticks % 4 == 0
            && let Some(sound) = host.block_states().sound(state)
        {
            let at = sel.anchor.map(|v| v.floor() + 0.5);
            let play = mod_api::SoundPlay::at(&sound.hit, mod_api::SoundSource::Blocks, at)
                .with_volume((sound.volume + 1.0) / 8.0)
                .with_pitch(sound.pitch * 0.5);
            let _ = host.play_local_sound(&play, 0);
        }
        let creative = player
            .abilities(&*host)
            .is_some_and(|a| a.instabuild);
        let step = if creative {
            0.125
        } else {
            destroy_progress(&*host, player, state) / 8.0
        };
        let (progress, old) = CLIENT.with_borrow_mut(|c| {
            c.break_ticks += 1;
            c.break_timeout = 2;
            c.break_progress += step;
            (c.break_progress, c.break_pos.replace(pos))
        });
        if let Some(level) = host.level(host.dimension()) {
            if let Some(o) = old.filter(|o| *o != pos) {
                let _ = level.set_client_break_progress(host, BREAKER, o, -1);
            }
            let stage = (progress * 10.0) as i32 - 1;
            let _ = level.set_client_break_progress(host, BREAKER, pos, stage);
        }
        if progress >= 1.0 {
            let mut m = vec![b'D'];
            let sound = sel.anchor.map(|v| v.floor() as i32);
            for v in pos.into_iter().chain(sel.to).chain(sound) {
                m.extend(v.to_le_bytes());
            }
            m.push(0);
            crate::glue::net::send_to_server(host, CHANNEL, &m);
            reset_break(host);
        }
        return;
    }
    let timeout = CLIENT.with_borrow_mut(|c| {
        if c.break_timeout == 0 {
            return None;
        }
        c.break_timeout -= 1;
        Some(c.break_timeout)
    });
    if timeout == Some(0) {
        reset_break(host);
    }
}

/// `resetBreakProgress`.
fn reset_break(host: &mut dyn ClientHost) {
    let pos = CLIENT.with_borrow_mut(|c| {
        c.break_progress = 0.0;
        c.break_ticks = 0;
        c.break_pos.take()
    });
    if let Some(pos) = pos
        && let Some(level) = host.level(host.dimension())
    {
        let _ = level.set_client_break_progress(host, BREAKER, pos, -1);
    }
}

/// `BlockBehaviour.getDestroyProgress` for the local player and `state`: their digging speed
/// over the block's hardness, a thirtieth with the right tool, a hundredth without.
fn destroy_progress(
    host: &dyn ClientHost,
    player: mod_api::PlayerRef,
    state: mod_api::BlockState,
) -> f32 {
    let hardness = host.block_states().destroy_speed(state).unwrap_or(1.0);
    if hardness == -1.0 {
        return 0.0;
    }
    let correct = player
        .has_correct_tool_for_drops(host, state)
        .unwrap_or(true);
    let divisor = if correct { 30.0 } else { 100.0 };
    destroy_speed(host, player, state) / hardness / divisor
}

/// `Player.getDestroySpeed(state)`: the held tool's speed (a vanilla pickaxe's on a block a
/// pickaxe mines; 1 otherwise), with mining efficiency, haste or conduit power, mining fatigue,
/// the block break speed, under water and off the ground as vanilla counts them.
fn destroy_speed(
    host: &dyn ClientHost,
    player: mod_api::PlayerRef,
    state: mod_api::BlockState,
) -> f32 {
    let entity = player.entity();
    let living = host.living(entity);
    let attribute = |id: &str| {
        living
            .and_then(|l| l.attribute_value(host, id))
            .unwrap_or(1.0) as f32
    };
    let held = items::held(host, player, Hand::Main).map(|s| s.item().name(host));
    let pickaxe = host.block_in_tag(state.block(), "minecraft:mineable/pickaxe") == Some(true);
    let mut f = match held.as_deref() {
        Some("minecraft:wooden_pickaxe") if pickaxe => 2.0,
        Some("minecraft:stone_pickaxe") if pickaxe => 4.0,
        Some("minecraft:copper_pickaxe") if pickaxe => 5.0,
        Some("minecraft:iron_pickaxe") if pickaxe => 6.0,
        Some("minecraft:diamond_pickaxe") if pickaxe => 8.0,
        Some("minecraft:netherite_pickaxe") if pickaxe => 9.0,
        Some("minecraft:golden_pickaxe") if pickaxe => 12.0,
        _ => 1.0,
    };
    if f > 1.0 {
        f += living
            .and_then(|l| l.attribute_value(host, "minecraft:mining_efficiency"))
            .unwrap_or(0.0) as f32;
    }
    let amplifier = |id: &str| {
        let key = host.effect_key(id)?;
        Some(living?.effect(host, key)?.amplifier)
    };
    let dig = amplifier("minecraft:haste").max(amplifier("minecraft:conduit_power"));
    if let Some(a) = dig {
        f *= 1.0 + (a + 1) as f32 * 0.2;
    }
    if let Some(a) = amplifier("minecraft:mining_fatigue") {
        f *= match a {
            0 => 0.3,
            1 => 0.09,
            2 => 0.0027,
            _ => 8.1e-4,
        };
    }
    f *= attribute("minecraft:block_break_speed");
    let eye_in_water = entity
        .eye_fluid(host)
        .flatten()
        .is_some_and(|f| f == "minecraft:water" || f == "minecraft:flowing_water");
    if eye_in_water {
        f *= attribute("minecraft:submerged_mining_speed");
    }
    if entity.on_ground(host) == Some(false) {
        f /= 5.0;
    }
    f
}

/// `CurvedTrackInteraction.onClickInput` while a curve is aimed at: use with a track says a curve
/// can't start one (`track.turn_start`), with a station, signal or observer picks the segment
/// (`useOnCurve`), with a sneaking wrench takes the curve up; attack is the curve's (its breaking
/// is the tick's). The click goes no further.
pub fn interaction_key(
    host: &mut dyn ClientHost,
    event: &mut InteractionKeyMappingTriggeredEvent,
) -> Flow {
    let Some(sel) = selection() else {
        return Flow::Continue;
    };
    let Some(player) = host.local_player() else {
        return Flow::Continue;
    };
    match event.key {
        InteractionKey::Use => {
            let Some(stack) = items::held(&*host, player, Hand::Main) else {
                return Flow::Continue;
            };
            let id = stack.item().name(&*host);
            let message = if id == "create:track" {
                vec![b'T']
            } else if tracks::is_targeting(&*host, &stack) {
                let look = player.entity().view_vector(&*host).unwrap_or([0.0; 3]);
                let d = sel.direction;
                let front = look[0] * d[0] + look[1] * d[1] + look[2] * d[2] < 0.0;
                let slot = player.selected_slot(&*host).unwrap_or(0) as i32;
                let mut m = vec![b'S'];
                for v in sel.track.into_iter().chain(sel.to) {
                    m.extend(v.to_le_bytes());
                }
                m.push(u8::from(front));
                m.extend(sel.segment.to_le_bytes());
                m.extend(slot.to_le_bytes());
                m
            } else if id == "create:wrench" && items::sneaking(&*host, player) {
                let mut m = vec![b'D'];
                let sound = sel.anchor.map(|v| v.floor() as i32);
                for v in sel.track.into_iter().chain(sel.to).chain(sound) {
                    m.extend(v.to_le_bytes());
                }
                m.push(1);
                reset_break(host);
                m
            } else {
                return Flow::Continue;
            };
            crate::glue::net::send_to_server(host, CHANNEL, &message);
            event.swing_hand = true;
            Flow::Cancel
        }
        InteractionKey::Attack => {
            event.swing_hand = true;
            Flow::Cancel
        }
        _ => Flow::Continue,
    }
}

/// While a curve is aimed at, the block behind it is not outlined (`pickCurves` makes the
/// crosshair's hit a miss).
pub fn block_highlight(host: &dyn ClientHost, event: &mut BlockHighlightEvent) -> Flow {
    if selection().is_some() {
        return Flow::Cancel;
    }
    // `drawCustomBlockSelection`: a track block shows its track's outline instead.
    let shape = tracks::shape_of(host, event.state);
    let outline = shape.filter(|s| !track_outline::block_outline(*s).is_empty());
    CLIENT.with_borrow_mut(|c| c.highlight = outline.map(|s| (event.pos, s)));
    if outline.is_some() {
        Flow::Cancel
    } else {
        Flow::Continue
    }
}

/// `TrackBlockItem.sendExtenderPacket`: a track click with the sprint key held asks for the
/// largest curve.
pub fn right_click_block(host: &mut dyn ClientHost, click: &mod_api::BlockClick<'_>) {
    let Some(stack) = click.stack else {
        return;
    };
    if stack.item().name(&*host) != "create:track" {
        return;
    }
    if crate::glue::keys::down(&*host, "key.sprint") {
        let main = u8::from(click.hand == Hand::Main);
        crate::glue::net::send_to_server(host, CHANNEL, &[b'X', main]);
    }
}

/// `TrackTargetingClient.clientTick`: with a station, signal or observer in the main hand, the
/// pick it holds, else the curve's segment aimed at, else the track under the crosshair, looked
/// along; asked of the server when it changes.
fn hover(host: &mut dyn ClientHost) {
    let hovered = (|| {
        let player = host.local_player()?;
        let stack = items::held(&*host, player, Hand::Main)?;
        let kind = tracks::targeting_kind(&*host, &stack)?;
        if let Some(t) = tracks::stack_pick(&stack) {
            return Some((kind, t));
        }
        let look = player.entity().view_vector(&*host)?;
        if let Some(sel) = selection() {
            let d = sel.direction;
            let front = look[0] * d[0] + look[1] * d[1] + look[2] * d[2] < 0.0;
            return Some((kind, Target::on_curve(sel.track, sel.to, sel.segment, front)));
        }
        let Some(mod_api::CrosshairTarget::Block { pos, state, .. }) = host.crosshair_target()
        else {
            return None;
        };
        let pos = [pos.x, pos.y, pos.z];
        let shape = tracks::shape_of(&*host, state)?;
        // A junction never holds a pick (`no_junctions`): nothing to show.
        Some((kind, Target::pick(pos, shape, look)?))
    })();
    let Some((kind, target)) = hovered else {
        CLIENT.with_borrow_mut(|c| {
            c.asked = None;
            c.preview = None;
        });
        return;
    };
    let key = tracks::encode_hover(kind, &target);
    let fresh = CLIENT.with_borrow_mut(|c| {
        if c.asked.as_deref() == Some(&key[..]) {
            return false;
        }
        c.asked = Some(key.clone());
        c.preview = None;
        true
    });
    if fresh {
        let mut m = vec![b'H'];
        m.extend_from_slice(&key);
        crate::glue::net::send_to_server(host, CHANNEL, &m);
    }
}

/// The signal sections around this player, from the server (`O`).
fn overlay_from(d: &[u8]) {
    let int = |k: usize| Some(i32::from_le_bytes(d.get(k..k + 4)?.try_into().ok()?));
    let float = |k: usize| Some(f32::from_le_bytes(d.get(k..k + 4)?.try_into().ok()?));
    let (Some(x), Some(y), Some(z), Some(n)) = (int(0), int(4), int(8), int(12)) else {
        return;
    };
    let origin = [f64::from(x), f64::from(y), f64::from(z)];
    let mut lines = Vec::new();
    for i in 0..n.max(0) as usize {
        let at = 16 + i * 28;
        let point = |k: usize| -> Option<Vec3> {
            let mut p = [0.0; 3];
            for (j, v) in p.iter_mut().enumerate() {
                *v = origin[j] + f64::from(float(at + 4 * (k * 3 + j))?);
            }
            Some(p)
        };
        let (Some(a), Some(b), Some(rgb)) = (point(0), point(1), int(at + 24)) else {
            return;
        };
        lines.push((a, b, rgb as u32));
    }
    CLIENT.with_borrow_mut(|c| c.overlay = lines);
}

/// `LineOutline` of width 1/8 (Catnip's `Outliner.showLine(..).lineWidth(1 / 8f)`): a square
/// beam along each line, its sides lit as entities are (two lights from above, `0.4` ambient).
fn beams(lines: &[(Vec3, Vec3, u32)], room: usize) -> Vec<mod_api::WorldQuad> {
    let half = f64::from(1.0f32 / 16.0);
    let norm = |v: Vec3| -> Vec3 {
        let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
        if l < 1e-9 {
            [0.0; 3]
        } else {
            v.map(|c| c / l)
        }
    };
    let cross = |a: Vec3, b: Vec3| -> Vec3 {
        [
            a[1] * b[2] - a[2] * b[1],
            a[2] * b[0] - a[0] * b[2],
            a[0] * b[1] - a[1] * b[0],
        ]
    };
    let dot = |a: Vec3, b: Vec3| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let lights = [norm([0.2, 1.0, -0.7]), norm([-0.2, 1.0, 0.7])];
    let shade = |n: Vec3, rgb: u32| -> u32 {
        let light =
            (lights.iter().map(|l| dot(*l, n).max(0.0)).sum::<f64>() * 0.6 + 0.4).min(1.0);
        let c = |shift: u32| ((f64::from((rgb >> shift) & 0xFF) * light).round() as u32) << shift;
        0xFF00_0000 | c(16) | c(8) | c(0)
    };
    let mut out = Vec::new();
    for &(a, b, rgb) in lines {
        if out.len() + 6 > room {
            break;
        }
        let d = norm([b[0] - a[0], b[1] - a[1], b[2] - a[2]]);
        if d == [0.0; 3] {
            continue;
        }
        let up = if d[1].abs() > 0.99 {
            [1.0, 0.0, 0.0]
        } else {
            [0.0, 1.0, 0.0]
        };
        let u = norm(cross(d, up));
        let v = cross(u, d);
        let at = |p: Vec3, su: f64, sv: f64| -> Vec3 {
            std::array::from_fn(|k| p[k] + (u[k] * su + v[k] * sv) * half)
        };
        // The four sides, then the two ends.
        for (n, s1, s2) in [
            (v, (1.0, 1.0), (-1.0, 1.0)),
            (u, (1.0, -1.0), (1.0, 1.0)),
            (v.map(|c| -c), (-1.0, -1.0), (1.0, -1.0)),
            (u.map(|c| -c), (-1.0, 1.0), (-1.0, -1.0)),
        ] {
            let corners = [
                at(a, s1.0, s1.1),
                at(a, s2.0, s2.1),
                at(b, s2.0, s2.1),
                at(b, s1.0, s1.1),
            ];
            out.push(mod_api::WorldQuad::new(corners, shade(n, rgb), WorldDepth::Tested));
        }
        for (p, n) in [(a, d.map(|c| -c)), (b, d)] {
            let corners = [at(p, 1.0, 1.0), at(p, -1.0, 1.0), at(p, -1.0, -1.0), at(p, 1.0, -1.0)];
            out.push(mod_api::WorldQuad::new(corners, shade(n, rgb), WorldDepth::Tested));
        }
    }
    out
}

/// The server's word on [`CHANNEL`]: whether the pick asked of it holds, and its marker (`V`);
/// the signal sections around this player (`O`).
pub fn server_message(data: &[u8]) {
    if let Some((b'O', rest)) = data.split_first() {
        overlay_from(rest);
        return;
    }
    let Some((b'V', rest)) = data.split_first().map(|(k, r)| (*k, r)) else {
        return;
    };
    let Some((kind, target, len)) = tracks::decode_hover(rest) else {
        return;
    };
    let key = &rest[..len];
    let valid = rest.get(len) == Some(&1);
    let pose: Option<[f32; 12]> = rest.get(len + 1..len + 49).and_then(|b| {
        let mut m = [0.0f32; 12];
        for (k, v) in m.iter_mut().enumerate() {
            *v = f32::from_le_bytes(b[4 * k..4 * k + 4].try_into().ok()?);
        }
        Some(m)
    });
    CLIENT.with_borrow_mut(|c| {
        if c.asked.as_deref() != Some(key) {
            return;
        }
        c.preview = valid
            .then_some(pose)
            .flatten()
            .map(|m| (target.track, kind, m));
    });
}

fn model(host: &dyn ClientHost, id: &str) -> Option<mod_api::ModelRef> {
    let id = mod_api::Identifier::parse(id).ok()?;
    host.render_assets().model(&id).ok()
}

/// `drawCurveSelection` (the segment's outline, black, or red while holding a track) and
/// `TrackTargetingClient.render` (the pick's marker, a sixteenth larger).
pub fn render_level_stage(host: &dyn ClientHost, event: &mut RenderLevelStageEvent<'_>) {
    match event.stage {
        RenderLevelStage::AfterOpaqueBlocks => {
            let Some((track, kind, pose)) = CLIENT.with_borrow(|c| c.preview) else {
                return;
            };
            let id = match kind {
                PointKind::Station => super::station_draw::OVERLAY,
                PointKind::Signal => super::station_draw::SIGNAL,
                PointKind::Observer => super::station_draw::OBSERVER,
            };
            let Some(m) = model(host, id) else {
                return;
            };
            let light = host
                .level(host.dimension())
                .and_then(|l| {
                    let b = l.brightness(host, LightLayer::Block, track)?;
                    let s = l.brightness(host, LightLayer::Sky, track)?;
                    LightCoords::new(b.clamp(0, 15) as u8, s.clamp(0, 15) as u8)
                })
                .unwrap_or(LightCoords::FULL_BRIGHT);
            let origin = track.map(f64::from);
            let instance = ModelInstance::new(origin, pose, 0xFFFF_FFFF, light, Overlay::NONE);
            let _ = event.draw.draw_model_instances(m, &[instance]);
        }
        RenderLevelStage::AfterTranslucentBlocks => {
            let highlight = CLIENT.with_borrow_mut(|c| c.highlight.take());
            let Some(player) = host.local_player() else {
                return;
            };
            let holding_track = items::held(host, player, Hand::Main)
                .is_some_and(|s| s.item().name(host) == "create:track");
            let line = |(a, b): (Vec3, Vec3), colour: u32| {
                WorldLine::new(a, b, colour, 2.5, WorldDepth::Tested)
            };
            let mut lines: Vec<WorldLine> = Vec::new();
            if let Some((pos, shape)) = highlight {
                // White where a track can start from it, red where it can't (a junction).
                let colour = match (holding_track, shape.is_junction()) {
                    (false, _) => BLACK,
                    (true, false) => WHITE,
                    (true, true) => RED,
                };
                let origin = pos.map(f64::from);
                lines.extend(track_outline::block_outline(shape).into_iter().map(|(a, b)| {
                    let at = |v: Vec3| -> Vec3 { std::array::from_fn(|k| v[k] + origin[k]) };
                    line((at(a), at(b)), colour)
                }));
            }
            // `drawCurveSelection`: not with the HUD hidden or for a spectator.
            if let Some(sel) = selection()
                && host.hud_mode() == mod_api::HudMode::AllVisible
                && player.is_spectator(host) != Some(true)
            {
                let colour = if holding_track { RED } else { BLACK };
                lines.extend(
                    track_outline::outline(&sel)
                        .into_iter()
                        .map(|l| line(l, colour)),
                );
            }
            if !lines.is_empty() {
                let _ = event.draw.lines(&lines);
            }
            // `tickSignalOverlay`, while a signal is held.
            let overlay = CLIENT.with_borrow(|c| c.overlay.clone());
            if !overlay.is_empty() && holding_signal(host) {
                let usage = event.draw.usage();
                let room = usage.quad_capacity.saturating_sub(usage.quads) as usize;
                let quads = beams(&overlay, room);
                if !quads.is_empty() {
                    let _ = event.draw.quads(&quads);
                }
            }
        }
        _ => {}
    }
}

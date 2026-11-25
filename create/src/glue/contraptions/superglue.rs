//! The super glue item (`SuperGlueSelectionHandler`, `SuperGlueSelectionPacket`,
//! `SuperGlueRemovalPacket`): a right click on a block picks the first corner, a second click
//! glues the area between them when the blocks there hold together (one charge of the glue
//! outside creative), sneaking forgets the first corner, and a left click with the glue in hand
//! (on a block or in the air) takes off the glue area looked at. The areas are kept and saved as `actors` keeps them.
//!
//! Create picks the corners on its client and draws the selection and glue outlines there;
//! here the clicks are taken on the server (the corner a player picked is the server's), which
//! tells a player holding the glue of their first corner and the glue areas near them
//! (`create:glue`), and their client outlines them as Create's does: each area in Create's
//! passive green, the one looked at highlighted, and the selection from the first corner to the
//! block looked at, their faces covered with `AllSpecialTextures.GLUE` (the glue's textured
//! world pipeline, `pipelines`).

use super::super::items::{action_bar, creative, held, sneaking};
use super::super::persist::mark_unsaved;
use super::super::server::ServerLevel;
use super::super::world::with_world;
use super::{center, found};
use crate::glue::names::Named as _;
use crate::glue::pipelines::Textured;
use create_engine::glue;
use mod_api::{
    EquipmentSlot, Flow, Hand, HostSide, InteractionResult, ItemPart, LeftClickBlockEvent,
    PlayerRef, Registrar, RightClickBlockEvent, ServerHost, SoundSource, TriState, UseOnEvent,
};
use std::cell::RefCell;
use std::collections::HashMap;

pub const ITEM: &str = "create:super_glue";

/// The glue a player holding super glue sees: their first corner and the areas near them,
/// from the server to that player.
pub(super) const CHANNEL: &str = "create:glue";

/// `SuperGlueRemovalPacket`: a client's attack with the glue in hand while it looks at a glue
/// area (in the air as on a block), to the server, which takes off the area the player's look
/// picks.
pub(super) const REMOVE_CHANNEL: &str = "create:glue_remove";

/// `SuperGlueSelectionHandler`'s colours: `PASSIVE`, `HIGHLIGHT`, `FAIL`.
const PASSIVE: u32 = 0xFF4D_9162;
const HIGHLIGHT: u32 = 0xFF68_C586;
const FAIL: u32 = 0xFFC5_B548;

/// What this client was told: its first corner and the glue areas near it.
type Seen = (Option<[i32; 3]>, Vec<([i32; 3], [i32; 3])>);

thread_local! {
    /// Each player's first corner (`firstPos`), by player.
    static FIRST: RefCell<HashMap<u128, [i32; 3]>> = RefCell::new(HashMap::new());
    /// The client's: what the server last told it.
    static SEEN: RefCell<Option<Seen>> = const { RefCell::new(None) };
}

pub(super) fn forget() {
    FIRST.with_borrow_mut(HashMap::clear);
}

pub(super) fn client_reset() {
    SEEN.with_borrow_mut(|s| *s = None);
}

/// `SuperGlueSelectionHandler.tick`'s scan (`inflate(32, 16, 32)`), from the server: each
/// player of the level holding the glue is told of their first corner and the areas near them,
/// every half second (and at once when they change, `send_glue`).
pub(super) fn tick(lv: &mut ServerLevel<'_>) {
    if lv.game_time() % 10 != 0 {
        return;
    }
    for player in lv.players() {
        if is_glue(&*lv.host, player) {
            send_glue(lv, player);
        }
    }
}

/// Tells `player` their first corner and the glue areas near them.
fn send_glue(lv: &mut ServerLevel<'_>, player: PlayerRef) {
    let Some(at) = player.entity().position(&*lv.host) else {
        return;
    };
    let first = FIRST.with_borrow(|f| f.get(&player.uuid()).copied());
    let dim = lv.dim;
    let areas: Vec<glue::Glue> = with_world(|w| {
        w.dims[dim]
            .glue
            .iter()
            .filter(|g| near(g, at))
            .copied()
            .collect()
    })
    .unwrap_or_default();
    let mut words = vec![i32::from(first.is_some())];
    words.extend(first.unwrap_or_default());
    for g in &areas {
        words.extend(g.min);
        words.extend(g.max);
    }
    let data: Vec<u8> = words.iter().flat_map(|v| v.to_le_bytes()).collect();
    super::super::net::send_to_player(lv.host, player, CHANNEL, &data);
}

/// What the server told this client (`CHANNEL`).
pub(super) fn from_server(data: &[u8]) {
    let w: Vec<i32> = data
        .as_chunks::<4>()
        .0
        .iter()
        .map(|b| i32::from_le_bytes(*b))
        .collect();
    let [has, x, y, z, ref rest @ ..] = w[..] else {
        return;
    };
    let first = (has != 0).then_some([x, y, z]);
    let areas = rest
        .as_chunks::<6>()
        .0
        .iter()
        .map(|a| ([a[0], a[1], a[2]], [a[3], a[4], a[5]]))
        .collect();
    SEEN.with_borrow_mut(|s| *s = Some((first, areas)));
}

/// `SuperGlueSelectionHandler.tick`'s outlines, while the main hand holds the glue: the glue
/// areas near (the one the crosshair is in highlighted), and the selection from the first
/// corner to the block looked at (`getCurrentSelectionBox`; `FAIL` 24 blocks or more away, or
/// sneaking to discard).
pub(super) fn render(
    host: &dyn mod_api::ClientHost,
    event: &mut mod_api::RenderLevelStageEvent<'_>,
) {
    use mod_api::{CrosshairTarget, RenderLevelStage};
    if event.stage != RenderLevelStage::AfterTranslucentBlocks {
        return;
    }
    let Some(player) = host.local_player() else {
        return;
    };
    let holding = held(host, player, Hand::Main).is_some_and(|s| s.item().name(host) == ITEM);
    if !holding {
        return;
    }
    let Some((first, areas)) = SEEN.with_borrow(Clone::clone) else {
        return;
    };
    let hovered = match host.crosshair_target() {
        Some(CrosshairTarget::Block { pos, .. }) => Some([pos.x, pos.y, pos.z]),
        _ => None,
    };
    let mut lines = Vec::new();
    // `withFaceTextures(GLUE, ..)`: the highlighted area's and the selection's faces.
    let mut faces = Vec::new();
    let camera = host.camera().map(|c| c.position);
    if first.is_none() {
        let lit = selected(host, player, &areas);
        for (i, a) in areas.iter().enumerate() {
            let (colour, width) = if Some(i) == lit {
                box_faces(&mut faces, *a, HIGHLIGHT, camera);
                (HIGHLIGHT, 4.0)
            } else {
                (PASSIVE, 1.5)
            };
            box_lines(&mut lines, *a, colour, width);
        }
    }
    if let (Some(f), Some(h)) = (first, hovered) {
        let d2: i64 = (0..3).map(|i| i64::from(f[i] - h[i]).pow(2)).sum();
        let ok = d2 < 24 * 24 && !sneaking(host, player);
        let min = [0, 1, 2].map(|i| f[i].min(h[i]));
        let max = [0, 1, 2].map(|i| f[i].max(h[i]));
        let colour = if ok { HIGHLIGHT } else { FAIL };
        box_faces(&mut faces, (min, max), colour, camera);
        box_lines(&mut lines, (min, max), colour, 4.0);
    }
    if !faces.is_empty()
        && let Some(glue) = crate::glue::pipelines::resolve(host, Textured::Glue)
    {
        let _ = event.draw.pipeline_quads(glue, &faces);
    }
    if !lines.is_empty() {
        let _ = event.draw.lines(&lines);
    }
}

/// `selected`: of the areas the client was told of, the one the local player's look enters
/// nearest the eye, within their block reach and one (`getTraceTarget`).
fn selected(
    host: &dyn mod_api::ClientHost,
    player: PlayerRef,
    areas: &[([i32; 3], [i32; 3])],
) -> Option<usize> {
    let entity = player.entity();
    let (eye, look) = entity.eye_position(host).zip(entity.view_vector(host))?;
    let range = host
        .living(entity)
        .and_then(|l| l.attribute_value(host, "minecraft:block_interaction_range"))
        .unwrap_or(4.5)
        + 1.0;
    let target = [0, 1, 2].map(|i| eye[i] + look[i] * range);
    let boxes: Vec<glue::Glue> = areas
        .iter()
        .map(|&(min, max)| glue::Glue { min, max })
        .collect();
    glue::pick(&boxes, eye, target).map(|(i, _)| i)
}

/// `SuperGlueSelectionHandler.onMouseInput(attack)`: an attack with the glue in the main hand,
/// no first corner picked, while an area is looked at (`selected`) asks the server to take it
/// off (`SuperGlueRemovalPacket`) and is not an attack. True when it was taken.
pub(super) fn attack_key(host: &mut dyn mod_api::ClientHost) -> bool {
    let Some(player) = host.local_player() else {
        return false;
    };
    let holding = held(&*host, player, Hand::Main).is_some_and(|s| s.item().name(&*host) == ITEM);
    if !holding {
        return false;
    }
    let Some((None, areas)) = SEEN.with_borrow(Clone::clone) else {
        return false;
    };
    if selected(&*host, player, &areas).is_none() {
        return false;
    }
    super::super::net::send_to_server(host, REMOVE_CHANNEL, &[])
}

/// `AABBOutline.renderBoxFaces` with no highlighted face: the six faces of the blocks `min`
/// to `max`, 1/128 out (in, with the camera inside), the texture once a block, in `colour` at
/// half its alpha.
fn box_faces(
    faces: &mut Vec<mod_api::PipelineQuad>,
    (min, max): ([i32; 3], [i32; 3]),
    colour: u32,
    camera: Option<[f64; 3]>,
) {
    let mut lo = min.map(f64::from);
    let mut hi = max.map(|v| f64::from(v) + 1.0);
    let inside = camera.is_some_and(|c| (0..3).all(|i| lo[i] <= c[i] && c[i] <= hi[i]));
    let inflate = if inside { -1.0 / 128.0 } else { 1.0 / 128.0 };
    for i in 0..3 {
        lo[i] -= inflate;
        hi[i] += inflate;
    }
    let colour = (((colour >> 24) / 2) << 24) | (colour & 0xFF_FFFF);
    let (sx, sy, sz) = (
        (hi[0] - lo[0]) as f32,
        (hi[1] - lo[1]) as f32,
        (hi[2] - lo[2]) as f32,
    );
    let [x0, y0, z0] = lo;
    let [x1, y1, z1] = hi;
    for (corners, u, v) in [
        (
            [[x0, y0, z1], [x0, y0, z0], [x1, y0, z0], [x1, y0, z1]],
            sx,
            sz,
        ),
        (
            [[x0, y1, z0], [x0, y1, z1], [x1, y1, z1], [x1, y1, z0]],
            sx,
            sz,
        ),
        (
            [[x1, y1, z0], [x1, y0, z0], [x0, y0, z0], [x0, y1, z0]],
            sx,
            sy,
        ),
        (
            [[x0, y1, z1], [x0, y0, z1], [x1, y0, z1], [x1, y1, z1]],
            sx,
            sy,
        ),
        (
            [[x0, y1, z0], [x0, y0, z0], [x0, y0, z1], [x0, y1, z1]],
            sz,
            sy,
        ),
        (
            [[x1, y1, z1], [x1, y0, z1], [x1, y0, z0], [x1, y1, z0]],
            sz,
            sy,
        ),
    ] {
        faces.push(mod_api::PipelineQuad::new(
            corners,
            [[0.0, 0.0], [0.0, v], [u, v], [u, 0.0]],
            colour,
        ));
    }
}

/// The twelve edges of the blocks `min` to `max` (inclusive), a hair outside them.
fn box_lines(
    lines: &mut Vec<mod_api::WorldLine>,
    (min, max): ([i32; 3], [i32; 3]),
    colour: u32,
    width: f32,
) {
    use mod_api::{WorldDepth, WorldLine};
    let lo = min.map(|v| f64::from(v) - 0.005);
    let hi = max.map(|v| f64::from(v) + 1.005);
    let c = |x: usize, y: usize, z: usize| {
        [
            if x == 0 { lo[0] } else { hi[0] },
            if y == 0 { lo[1] } else { hi[1] },
            if z == 0 { lo[2] } else { hi[2] },
        ]
    };
    for i in 0..2 {
        for j in 0..2 {
            for (a, b) in [
                (c(0, i, j), c(1, i, j)),
                (c(i, 0, j), c(i, 1, j)),
                (c(i, j, 0), c(i, j, 1)),
            ] {
                lines.push(WorldLine::new(a, b, colour, width, WorldDepth::Tested));
            }
        }
    }
}

pub(super) fn register(r: &mut dyn Registrar) {
    let item = r.item_key(ITEM);
    r.item_part(item, GlueItem);
}

struct GlueItem;

#[mod_api::horizon_part]
impl ItemPart for GlueItem {
    fn use_on(&self, host: HostSide, event: &mut UseOnEvent<'_>) -> Option<InteractionResult> {
        // `onMouseInput` reads the main hand only.
        if event.hand != Hand::Main {
            return None;
        }
        match host {
            HostSide::Server(host) => use_on(host, event),
            HostSide::Client(_) => Some(InteractionResult::Success),
        }
    }
}

/// `SuperGlueSelectionHandler.onMouseInput` takes a right click with the glue in the main hand
/// before the block hit is used (a deployer would take the glue, a depot would hold it), unless
/// the block is a chassis (its glueable sides take the glue as a slime ball) or the player
/// sneaks (which only forgets the first corner).
pub(super) fn right_click_block(host: &mut HostSide, event: &mut RightClickBlockEvent<'_>) -> Flow {
    let click = event.click;
    let Some(stack) = click.stack else {
        return Flow::Continue;
    };
    if click.hand != Hand::Main {
        return Flow::Continue;
    }
    let (id, state, player) = match host {
        HostSide::Server(h) => (
            stack.item().name(&**h),
            h.level(click.dimension.key().id())
                .and_then(|l| l.block_state(&**h, click.pos)),
            h.player(click.player)
                .map(|p| (p.may_build(&**h) == Some(true), sneaking(&**h, p))),
        ),
        HostSide::Client(h) => (
            stack.item().name(&**h),
            h.level(h.dimension())
                .and_then(|l| l.block_state(&**h, click.pos)),
            h.local_player()
                .map(|p| (p.may_build(&**h) == Some(true), sneaking(&**h, p))),
        ),
    };
    if id != ITEM || player != Some((true, false)) {
        return Flow::Continue;
    }
    // `AbstractChassisBlock.getGlueableSide`: the face clicked takes glue as a slime ball.
    if state.is_some_and(|s| glueable_chassis_face(&*host, s, click.hit.face)) {
        return Flow::Continue;
    }
    *event.use_block = TriState::False;
    // The server takes the click here, ahead of the block's value boxes and filter slots.
    let HostSide::Server(h) = host else {
        return Flow::Continue;
    };
    match click_at(&mut **h, click.player, click.pos) {
        Some(answer) => {
            *event.cancellation_result = answer;
            Flow::Cancel
        }
        None => Flow::Continue,
    }
}

/// Whether `face` of the chassis state `s` is one it may be glued on (`getGlueableSide`): a
/// linear chassis' ends along its axis, a radial chassis' sides across it.
fn glueable_chassis_face(host: &HostSide, s: mod_api::BlockState, face: [i32; 3]) -> bool {
    use create_engine::Block;
    let is = |b: Block| s.block() == crate::glue::states::key(b);
    let linear = is(Block::LinearChassis) || is(Block::SecondaryLinearChassis);
    if !linear && !is(Block::RadialChassis) {
        return false;
    }
    let read = |h: &dyn mod_api::Host| {
        s.property(&mod_api::HostBlockStates::block_states(h), "axis")
            .ok()
            .flatten()
            .map(str::to_owned)
    };
    let axis = match host {
        HostSide::Server(h) => read(&**h),
        HostSide::Client(h) => read(&**h),
    };
    let i = match axis.as_deref() {
        Some("x") => 0,
        Some("z") => 2,
        _ => 1,
    };
    let along = face[i] != 0;
    if linear { along } else { !along }
}

fn is_glue(host: &dyn ServerHost, player: PlayerRef) -> bool {
    held(host, player, Hand::Main).is_some_and(|s| s.item().name(host) == ITEM)
}

fn use_on(host: &mut dyn ServerHost, event: &UseOnEvent<'_>) -> Option<InteractionResult> {
    click_at(host, event.player, event.pos)
}

fn click_at(host: &mut dyn ServerHost, id: u128, pos: [i32; 3]) -> Option<InteractionResult> {
    let player = host.player(id)?;
    if player.may_build(&*host) != Some(true) {
        return None;
    }
    let first = FIRST.with_borrow(|f| f.get(&id).copied());
    if sneaking(&*host, player) {
        // `discard`: sneaking forgets the first corner.
        first?;
        FIRST.with_borrow_mut(|f| f.remove(&id));
        action_bar(host, player, "create.super_glue.abort");
        let dimension = player.dimension(&*host)?.to_string();
        if let Some(mut lv) = ServerLevel::new(host, &dimension) {
            send_glue(&mut lv, player);
        }
        return Some(InteractionResult::Success);
    }
    let dimension = player.dimension(&*host)?.to_string();
    let mut lv = ServerLevel::new(host, &dimension)?;
    let dim = lv.dim;
    // `onMouseInput` with a first corner: the client's cluster (`searchGlueGroup` with the glue
    // already there) must reach the block clicked.
    let second = first.map(|from| {
        let group = glue::search_group(
            from,
            pos,
            true,
            |p| found(&lv, p),
            |p| super::actors::sticky_sides(&lv, p),
            |p, d| with_world(|w| glue::is_glued(&w.dims[dim].glue, p, d)).unwrap_or(false),
        );
        (from, glue::second_click(from, pos, group.as_ref()))
    });
    let from = match second {
        None | Some((_, glue::SecondClick::FirstAgain)) => {
            // `firstPos = hoveredPos`.
            FIRST.with_borrow_mut(|f| f.insert(id, pos));
            action_bar(lv.host, player, "create.super_glue.first_pos");
            slime_sound(&mut lv, pos, 0.85);
            send_glue(&mut lv, player);
            return Some(InteractionResult::Success);
        }
        Some((_, glue::SecondClick::TooFar)) => {
            action_bar(lv.host, player, "create.super_glue.too_far");
            return Some(InteractionResult::Success);
        }
        Some((_, glue::SecondClick::CannotReach)) => {
            action_bar(lv.host, player, "create.super_glue.cannot_reach");
            return Some(InteractionResult::Success);
        }
        Some((from, glue::SecondClick::Confirm)) => from,
    };
    // `confirm`: the selection is sent and forgotten, whatever the server makes of it.
    FIRST.with_borrow_mut(|f| f.remove(&id));
    slime_sound(&mut lv, pos, 0.95);
    action_bar(lv.host, player, "create.super_glue.success");
    // `SuperGlueSelectionPacket.handle`: the server's own search, without the glue there.
    let placed = glue::place(
        from,
        pos,
        |p| found(&lv, p),
        |p| super::actors::sticky_sides(&lv, p),
    );
    if let Ok(g) = placed {
        with_world(|w| w.dims[dim].glue.push(g));
        let (cx, cz) = g.column();
        mark_unsaved(dim, [cx, cz]);
        if !creative(&*lv.host, player)
            && let Some(living) = lv.host.living(player.entity())
        {
            living.hurt_and_break(lv.host, EquipmentSlot::MainHand, 1);
        }
        glue_particles(&mut lv, &g);
        crate::glue::advancements::award(lv.host, player, "super_glue");
    }
    send_glue(&mut lv, player);
    Some(InteractionResult::Success)
}

/// `ParticleTypes.ITEM_SLIME`, sent along a glue area's edges (`SuperGlueEntity.spawnParticles`).
fn glue_particles(lv: &mut ServerLevel<'_>, g: &glue::Glue) {
    for at in glue::edge_points(g) {
        super::bearings::slime_particle(lv, at);
    }
}

/// `AllSoundEvents.SLIME_ADDED` and the item frame's click, as the selection plays them.
fn slime_sound(lv: &mut ServerLevel<'_>, pos: [i32; 3], pitch: f32) {
    for (sound, volume, pitch) in [
        ("create:slime_added", 0.5, pitch),
        ("minecraft:entity.item_frame.add_item", 0.75, 1.0),
    ] {
        lv.level.play_sound(
            &mut *lv.host,
            sound,
            center(pos),
            SoundSource::Blocks,
            volume,
            pitch,
        );
    }
}

/// `SuperGlueSelectionHandler.onMouseInput(attack)` and `SuperGlueRemovalPacket`: a left click
/// with the glue in hand (no first corner picked) takes off the glue area the look enters
/// nearest the eye, within the player's block reach and one (`getTraceTarget`), of those near
/// them (`inflate(32, 16, 32)`); an area the eye is inside is not picked. The sound plays at the
/// block where the look enters it.
pub(super) fn left_click(host: &mut dyn ServerHost, _event: &LeftClickBlockEvent) -> bool {
    let Some(player) = host.players().acting_player() else {
        return false;
    };
    remove_looked_at(host, player)
}

/// The glue area `player`'s look picks comes off (`SuperGlueRemovalPacket.handle`, the area
/// picked as the client picks it), when they hold the glue with no first corner and may build.
pub(super) fn remove_looked_at(host: &mut dyn ServerHost, player: PlayerRef) -> bool {
    if !is_glue(&*host, player) || player.may_build(&*host) != Some(true) {
        return false;
    }
    // `selected` is only looked for while no first corner is picked.
    if FIRST.with_borrow(|f| f.contains_key(&player.uuid())) {
        return false;
    }
    let entity = player.entity();
    let (Some(eye), Some(look), Some(at)) = (
        entity.eye_position(&*host),
        entity.view_vector(&*host),
        entity.position(&*host),
    ) else {
        return false;
    };
    let range = host
        .living(entity)
        .and_then(|l| l.attribute_value(&*host, "minecraft:block_interaction_range"))
        .unwrap_or(4.5)
        + 1.0;
    let target = [0, 1, 2].map(|i| eye[i] + look[i] * range);
    let Some(dimension) = player.dimension(&*host).map(|d| d.to_string()) else {
        return false;
    };
    let Some(mut lv) = ServerLevel::new(host, &dimension) else {
        return false;
    };
    let dim = lv.dim;
    let removed = with_world(|w| {
        let areas = &mut w.dims[dim].glue;
        let near: Vec<usize> = (0..areas.len()).filter(|&i| near(&areas[i], at)).collect();
        let boxes: Vec<glue::Glue> = near.iter().map(|&i| areas[i]).collect();
        let (k, clip) = glue::pick(&boxes, eye, target)?;
        Some((areas.remove(near[k]), clip))
    })
    .flatten();
    let Some((g, clip)) = removed else {
        return false;
    };
    let (cx, cz) = g.column();
    mark_unsaved(dim, [cx, cz]);
    // `SLIME_ADDED.play(level, null, soundSource, 0.5, 0.5)`, `BlockPos.containing(clip)`.
    let source = clip.map(|v| v.floor() as i32);
    super::play_entry(&mut lv, super::SLIME_ADDED, source, 0.5, 0.5);
    glue_particles(&mut lv, &g);
    send_glue(&mut lv, player);
    true
}

/// `getEntitiesOfClass(SuperGlueEntity.class, player.getBoundingBox().inflate(32, 16, 32))`,
/// from the player's feet.
fn near(g: &glue::Glue, at: [f64; 3]) -> bool {
    let reach = [32.0, 16.0, 32.0];
    (0..3).all(|i| {
        f64::from(g.max[i]) + 1.0 >= at[i] - reach[i] && f64::from(g.min[i]) <= at[i] + reach[i]
    })
}

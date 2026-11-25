//! The sliding doors' block entity (`SlidingDoorBlockEntity`) as its clients draw it
//! (`SlidingDoorRenderer`). A door that is not `visible` draws nothing of its own: its
//! blockstate (`assets/create/blockstates/<door>.json`, written by `create-data compile`) has
//! cases for the `visible` states only, as `SlidingDoorBlock.getRenderShape` is
//! `ENTITYBLOCK_ANIMATED` then. The lower half's block entity draws both halves slid aside, or a
//! folding door's two leaves folded, as far as its panel has got, and for two ticks more once
//! the door shows again (`bridgeTicks`), while the chunk takes the block's own model back. The
//! panel's animation is each client's own (`LerpedFloat.linear`, 0.15 a tick); the server's
//! (`blocks`) only decides when the door shows its block again. A door riding a moving
//! structure is drawn by the structure the same way ([`draw_riding`]).

use create_engine::door::{FoldStep, Panel, fold_pose, slide_offset};
use create_engine::kinetics::Dir;
use mod_api::{
    BlockEntityDrawContext, BlockEntityPart, BlockEntityRenderBoundingBoxEvent, BlockKey,
    BlockPart, ClientHost, HostBlockStates, Registrar,
};
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::OnceLock;

/// The sliding doors' block entity type.
pub const TYPE: &str = "create:sliding_door";

/// The doors, and the folding ones' leaves (`AllPartialModels.FOLDING_DOORS`: left, right).
const DOORS: [(&str, Option<[&str; 2]>); 5] = [
    (
        "create:andesite_door",
        Some([
            "create:part_andesite_door_fold_left",
            "create:part_andesite_door_fold_right",
        ]),
    ),
    ("create:brass_door", None),
    (
        "create:copper_door",
        Some([
            "create:part_copper_door_fold_left",
            "create:part_copper_door_fold_right",
        ]),
    ),
    ("create:train_door", None),
    ("create:framed_glass_door", None),
];

/// A folding door's two leaves (`AllPartialModels.FOLDING_DOORS`: left, right), by the door's
/// id; None for a door that slides.
pub(crate) fn fold_leaves(door: &str) -> Option<[&'static str; 2]> {
    DOORS.iter().find(|(d, _)| *d == door).and_then(|(_, l)| *l)
}

/// A door's block key and its leaves' keys.
type Keys = (BlockKey, Option<[BlockKey; 2]>);

/// Each door's block key and its leaves' keys, once registered.
static KEYS: OnceLock<Vec<Keys>> = OnceLock::new();

pub(super) fn register(r: &mut dyn Registrar) {
    let keys = DOORS
        .iter()
        .map(|(door, leaves)| {
            (
                r.block_key(door),
                leaves.map(|[l, rt]| [r.block_key(l), r.block_key(rt)]),
            )
        })
        .collect();
    let _ = KEYS.set(keys);
    let blocks: Vec<&str> = DOORS.iter().map(|(d, _)| *d).collect();
    let kind = r.block_entity_type(TYPE).blocks(&blocks).build();
    super::super::entity_data::attach(r, &kind);
    if let Err(e) = r.block_entity_part(&kind, DoorEntity) {
        crate::create_log!("[create] the sliding door block entity's part was refused: {e:?}");
    }
}

std::thread_local! {
    static PANELS: RefCell<HashMap<[i32; 3], Panel>> = RefCell::new(HashMap::new());
    /// The panels of doors riding moving structures, by the structure's anchor and the door's
    /// offset in it.
    static RIDING: RefCell<HashMap<([i32; 3], [i32; 3]), Panel>> = RefCell::new(HashMap::new());
}

struct DoorEntity;

#[mod_api::horizon_part]
impl BlockPart for DoorEntity {}

#[mod_api::horizon_part]
impl BlockEntityPart for DoorEntity {
    /// `createRenderBoundingBox().inflate(1)`: the panel reaches into the next block.
    fn block_entity_render_bounding_box(
        &self,
        _host: &dyn ClientHost,
        event: &mut BlockEntityRenderBoundingBoxEvent<'_>,
    ) {
        *event.bounds = [-1.0, -1.0, -1.0, 2.0, 3.0, 2.0];
    }

    fn block_entity_submit(
        &self,
        host: &dyn ClientHost,
        event: &mut mod_api::BlockEntitySubmitEvent<'_>,
    ) {
        let event = &mut crate::glue::Submit::of(event);
        let facts = host.block_states();
        let prop = |k: &str| {
            event
                .state
                .property(&facts, k)
                .ok()
                .flatten()
                .map(str::to_owned)
        };
        let draw = &mut *event.draw;
        if prop("half").as_deref() != Some("lower") {
            draw.keep();
            return;
        }
        let open = prop("open").as_deref() == Some("true");
        let visible = prop("visible").as_deref() != Some("false");
        let now = draw.game_time() as f64 + f64::from(draw.partial());
        let target = if open { 1.0 } else { 0.0 };
        let (value, special) = PANELS.with_borrow_mut(|panels| {
            let panel = panels
                .entry(event.pos)
                .or_insert_with(|| Panel::new(open, now));
            panel.follow(open, now);
            (panel.value(now), panel.special(visible, now))
        });
        let settled = value == target;
        if !special {
            // The block draws itself.
            if settled {
                draw.keep();
            }
            return;
        }
        let Some((key, leaves)) = KEYS.get().and_then(|k| {
            DOORS
                .iter()
                .position(|(d, _)| *d == event.block)
                .map(|i| k[i])
        }) else {
            return;
        };
        let facing = prop("facing")
            .as_deref()
            .and_then(super::dir_named)
            .unwrap_or(Dir::North);
        let hinge = prop("hinge").unwrap_or_else(|| "left".into());
        draw_panel(draw, key, leaves, facing, &hinge, value);
        // Kept once still and hidden; while its bridge runs it is drawn again each frame, so it
        // stops when the bridge does.
        if settled && !visible {
            draw.keep();
        }
    }
}

/// What `SlidingDoorRenderer` draws of a door at `value` (0 closed, 1 open), under the pose at
/// its lower half's corner: both halves closed (`setValue(OPEN, false)`) slid aside, or a
/// folding door's two leaves folded.
fn draw_panel(
    draw: &mut dyn BlockEntityDrawContext,
    key: BlockKey,
    leaves: Option<[BlockKey; 2]>,
    facing: Dir,
    hinge: &str,
    value: f32,
) {
    let hinge_right = hinge == "right";
    match leaves {
        Some(leaves) => {
            for left in [true, false] {
                let (model_left, steps) = fold_pose(facing, hinge_right, left, value);
                draw.push();
                for step in steps {
                    match step {
                        FoldStep::Translate(by) => draw.translate(by),
                        FoldStep::TurnY { degrees, pivot } => {
                            draw.rotate_about([0.0, 1.0, 0.0], degrees, pivot)
                        }
                    }
                }
                let leaf = if model_left { leaves[0] } else { leaves[1] };
                draw.block(leaf, &[("facing", "north")]);
                draw.pop();
            }
        }
        None => {
            let offset = slide_offset(facing, hinge_right, value);
            let facing = super::super::kinetics::dir_name(facing);
            for (half, y) in [("lower", 0.0), ("upper", 1.0 - 1.0 / 512.0)] {
                draw.push();
                draw.translate([offset[0], offset[1] + y, offset[2]]);
                draw_closed(draw, key, facing, half, hinge);
                draw.pop();
            }
        }
    }
}

/// The door's block, closed (`setValue(OPEN, false)`), as one half. Drawn `visible`, the only
/// states its blockstate has a model for.
fn draw_closed(
    draw: &mut dyn BlockEntityDrawContext,
    key: BlockKey,
    facing: &str,
    half: &str,
    hinge: &str,
) {
    draw.block(
        key,
        &[
            ("facing", facing),
            ("half", half),
            ("hinge", hinge),
            ("open", "false"),
            ("powered", "false"),
            ("visible", "true"),
        ],
    );
}

/// A sliding door riding a moving structure, drawn by the structure under its pose at the
/// block's corner (`renderBlockEntities` with `SlidingDoorRenderer`). A structure takes its
/// doors hidden (`Contraption.capture`): the lower half then draws both halves slid as far as
/// its panel has got, the panel chasing the door's `open` at 0.15 a tick on this client
/// (`SlidingDoorMovementBehaviour.tick`). `anchor` and `local` (the structure's anchor and the
/// block's offset in it) key the panel. The upper half has no block entity (`newBlockEntity`),
/// and a door still shown (taken before structures hid them) draws itself.
pub(crate) fn draw_riding(
    draw: &mut dyn BlockEntityDrawContext,
    anchor: [i32; 3],
    local: [i32; 3],
    block: BlockKey,
    props: &[(&str, &str)],
) {
    let prop = |k: &str| props.iter().find(|(n, _)| *n == k).map(|(_, v)| *v);
    if prop("half") != Some("lower") || prop("visible") != Some("false") {
        return;
    }
    let Some((key, leaves)) = KEYS
        .get()
        .and_then(|k| k.iter().find(|(d, _)| *d == block).copied())
    else {
        return;
    };
    let open = prop("open") == Some("true");
    let now = draw.game_time() as f64 + f64::from(draw.partial());
    let value = RIDING.with_borrow_mut(|panels| {
        let panel = panels
            .entry((anchor, local))
            .or_insert_with(|| Panel::new(open, now));
        panel.follow(open, now);
        panel.value(now)
    });
    let facing = prop("facing")
        .and_then(super::dir_named)
        .unwrap_or(Dir::North);
    draw_panel(
        draw,
        key,
        leaves,
        facing,
        prop("hinge").unwrap_or("left"),
        value,
    );
}

/// Forgets the panels when the client leaves its level.
pub(super) fn forget() {
    PANELS.with_borrow_mut(HashMap::clear);
    RIDING.with_borrow_mut(HashMap::clear);
}

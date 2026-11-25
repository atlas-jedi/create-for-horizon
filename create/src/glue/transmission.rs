//! Shafts and cogs in a player's hands: casings put on them (`EncasableBlock.tryEncase`) and
//! taken off with the wrench (`onSneakWrenched`), an encased cog's shaft ends toggled with the
//! wrench (`EncasedCogwheelBlock.onWrenched`), a metal girder around a shaft
//! (`ShaftBlock.useItemOn`), and the placement helpers that put a shaft or cog clicked on a
//! shaft or cog next to it: the shaft line extended (`ShaftBlock.PlacementHelper`) and cogs
//! meshing beside or diagonally (`CogwheelBlockItem`'s `SmallCogHelper`, `LargeCogHelper`,
//! `DiagonalCogHelper`). The helpers' ghost preview is client drawing Create does every frame;
//! it is left out. Also the wrench on chain drives (`ChainDriveBlock`'s turn, about the chain
//! in one, and `updateAfterWrenched`) and on sequenced gearshifts (`onWrenched`), and a
//! horizontal gearbox picked as the
//! vertical gearbox (`getCloneItemStack`).
//!
//! A casing taken off shows its break particles and sound as `levelEvent(2001)` does (rebuilt
//! from particles and the sound, `ServerLevel::destroy_effect`). Left out for want of API:
//! connected casing textures (`CTModel`), see `API_GAPS.md`.

use super::items::{creative, sneaking};
use super::server::{ServerLevel, update_all};
use super::states;
use super::world::with_world;
use crate::glue::names::Named as _;
use create_engine::Block;
use create_engine::kinetics::{Axis, Dir};
use create_engine::place;
use mod_api::{
    BlockClick, BlockState, Hand, HostBlockStates, InteractionResult, ItemStack,
    OnItemUseFirstEvent, PlayerRef, Registrar, ServerHost, SoundSource, UseItemOnEvent,
};

/// Attaches the clicks to shafts, cogs and the speed controller, and
/// `CogwheelBlockItem.onItemUseFirst` to both cog items.
pub fn register(r: &mut dyn Registrar) {
    for block in [
        Block::Shaft,
        Block::PoweredShaft,
        Block::Cogwheel,
        Block::LargeCogwheel,
        Block::RotationSpeedController,
    ] {
        r.block_part(states::key(block), Clicked);
    }
    let cog = r.item_key("create:cogwheel");
    r.item_part(cog, CogItem(false));
    let large = r.item_key("create:large_cogwheel");
    r.item_part(large, CogItem(true));
    r.block_part(states::key(Block::Gearbox), GearboxPick);
}

/// `GearboxBlock.getCloneItemStack`: a gearbox on a horizontal axis is picked as the vertical
/// gearbox. (Its drops are the loot table's, which `create-data prepare` gives the same rule.)
struct GearboxPick;

#[mod_api::horizon_part]
impl mod_api::BlockPart for GearboxPick {
    fn clone_item_stack(
        &self,
        host: &dyn mod_api::Host,
        event: &mut mod_api::CloneItemStackEvent<'_>,
    ) -> Option<ItemStack> {
        let axis = with_world(|w| {
            w.table
                .get(states::id(event.state)?)
                .and_then(|(_, i)| i.axis)
        })
        .flatten()?;
        if axis == Axis::Y {
            return None;
        }
        let key = host.item_key(&mod_api::Identifier::parse("create:vertical_gearbox").ok()?)?;
        ItemStack::new(key, 1)
    }
}

fn axis_name(a: Axis) -> &'static str {
    match a {
        Axis::X => "x",
        Axis::Y => "y",
        Axis::Z => "z",
    }
}

fn add(p: [i32; 3], d: Dir) -> [i32; 3] {
    let o = d.offset();
    [p[0] + o[0], p[1] + o[1], p[2] + o[2]]
}

/// The default state of `block` with `props` set.
fn state_of(host: &dyn mod_api::Host, block: Block, props: &[(&str, &str)]) -> Option<BlockState> {
    let facts = host.block_states();
    props.iter().try_fold(
        host.block_state_default(states::key(block))?,
        |s, (k, v)| s.with(&facts, k, v).ok(),
    )
}

impl ServerLevel<'_> {
    /// `SoundType.getPlaceSound()` of the state at `pos`, at `(volume + 1) / 2` and `pitch * 0.8`
    /// (`EncasableBlock.playEncaseSound`, `PlacementOffset.placeInWorld`).
    pub(crate) fn place_sound(&mut self, pos: [i32; 3]) {
        let Some(state) = self.block_state(pos) else {
            return;
        };
        let Some(sound) = self.host.block_states().sound(state) else {
            return;
        };
        let at = [0, 1, 2].map(|i| f64::from(pos[i]) + 0.5);
        let _ = self.level.play_sound(
            &mut *self.host,
            &sound.place,
            at,
            SoundSource::Blocks,
            (sound.volume + 1.0) / 2.0,
            sound.pitch * 0.8,
        );
    }

    /// Whether `pos` can be replaced (`BlockState.canBeReplaced`).
    pub(crate) fn free(&self, pos: [i32; 3]) -> bool {
        let facts = self.host.block_states();
        self.block_state(pos)
            .is_some_and(|s| facts.replaceable(s) == Some(true))
    }
}

/// One of the held stack used up, outside creative.
fn use_one(host: &mut dyn ServerHost, player: PlayerRef, hand: Hand, stack: &ItemStack) {
    if creative(&*host, player) {
        return;
    }
    if let Some(living) = host.living(player.entity()) {
        let _ = living.set_hand_item(host, hand, stack.clone().shrink(1));
    }
}

/// `CogwheelBlockItem.onItemUseFirst`: a cog item clicked on any cog block places beside or
/// diagonally before the block's own `useItemOn` runs.
struct CogItem(bool);

#[mod_api::horizon_part]
impl mod_api::ItemPart for CogItem {
    fn on_item_use_first(
        &self,
        host: &mut dyn ServerHost,
        event: &mut OnItemUseFirstEvent<'_>,
    ) -> Option<InteractionResult> {
        let click = event.click;
        let player = host.players().acting_player()?;
        // `if (player.isShiftKeyDown() || !player.mayBuild()) return PASS`.
        if sneaking(&*host, player) || player.may_build(&*host) != Some(true) {
            return None;
        }
        let dimension = click.dimension.key().id().to_owned();
        let mut lv = ServerLevel::new(host, &dimension)?;
        cog_helper(&mut lv, player, click, event.stack, self.0, click.pos)
    }
}

struct Clicked;

#[mod_api::horizon_part]
impl mod_api::BlockPart for Clicked {
    fn use_item_on(
        &self,
        host: &mut dyn ServerHost,
        event: &mut UseItemOnEvent<'_>,
    ) -> Option<InteractionResult> {
        let click = event.click;
        let stack = click.stack?;
        let item = stack.item().name(&*host);
        let player = host.player(click.player)?;
        // `if (player.isShiftKeyDown() || !player.mayBuild()) return PASS`.
        if sneaking(&*host, player) || player.may_build(&*host) != Some(true) {
            return None;
        }
        let dimension = click.dimension.key().id().to_owned();
        let mut lv = ServerLevel::new(host, &dimension)?;
        let (block, info) =
            with_world(|w| w.table.get(states::id(event.state)?).map(|(b, i)| (b, *i)))
                .flatten()?;
        let pos = click.pos;
        match block {
            // ShaftBlock.useItemOn: casing, girder, then the shaft line extended.
            Block::Shaft => {
                if let Some(r) = encase(&mut lv, block, info.axis?, pos, &item) {
                    return Some(r);
                }
                let axis = info.axis?;
                if item == "create:metal_girder" && axis != Axis::Y {
                    let water = if info.waterlogged { "true" } else { "false" };
                    let next = state_of(
                        &*lv.host,
                        Block::MetalGirderEncasedShaft,
                        &[("axis", axis_name(axis)), ("waterlogged", water)],
                    )?;
                    super::items::wrench::switch_state(&mut lv, pos, next);
                    use_one(&mut *lv.host, player, click.hand, stack);
                    return Some(InteractionResult::Success);
                }
                pole(&mut lv, player, click, stack, &item, axis)
            }
            Block::PoweredShaft => pole(&mut lv, player, click, stack, &item, info.axis?),
            // CogWheelBlock.useItemOn: casing.
            Block::Cogwheel | Block::LargeCogwheel => {
                encase(&mut lv, block, info.axis?, pos, &item)
            }
            // SpeedControllerBlock.useItemOn (its `PlacementHelper`): a large cog clicked on any
            // face of the controller goes above it, across its axis.
            Block::RotationSpeedController if item == "create:large_cogwheel" => {
                let (at, axis) = controller_target(&lv, pos, info.axis?)?;
                Some(place_helped(
                    &mut lv,
                    player,
                    click,
                    stack,
                    Block::LargeCogwheel,
                    at,
                    axis,
                ))
            }
            _ => None,
        }
    }
}

/// `SpeedControllerBlock.PlacementHelper.getOffset`: the large cog's place above the controller
/// at `pos` on its horizontal `axis` and the cog's axis across it, when that place can be
/// replaced and a large cog may stand there (`isValidCogwheelPosition`).
fn controller_target(
    lv: &ServerLevel<'_>,
    pos: [i32; 3],
    axis: Axis,
) -> Option<([i32; 3], Axis)> {
    let above = add(pos, Dir::Up);
    if !lv.free(above) {
        return None;
    }
    let across = if axis == Axis::X { Axis::Z } else { Axis::X };
    let dim = lv.dim;
    with_world(|w| w.valid_cog_position(dim, above, true, across))
        .unwrap_or(false)
        .then_some((above, across))
}

/// The cog helpers: where a cog item clicked on the cog at `pos` goes, placed there.
fn cog_helper(
    lv: &mut ServerLevel<'_>,
    player: PlayerRef,
    click: &BlockClick<'_>,
    stack: &ItemStack,
    large: bool,
    pos: [i32; 3],
) -> Option<InteractionResult> {
    // `PlacementOffset.fail()`: the click goes on as usual.
    let (at, new_axis) = cog_target(lv, large, pos, click.hit.point)?;
    let block = if large {
        Block::LargeCogwheel
    } else {
        Block::Cogwheel
    };
    Some(place_helped(lv, player, click, stack, block, at, new_axis))
}

/// Where a cog item (`large` or small) clicked at `point` on the cog at `pos` goes, and its axis.
fn cog_target(
    lv: &ServerLevel<'_>,
    large: bool,
    pos: [i32; 3],
    point: [f64; 3],
) -> Option<([i32; 3], Axis)> {
    let dim = lv.dim;
    let (cog_large, axis) = with_world(|w| w.cog_at(dim, pos)).flatten()?;
    let local = [0, 1, 2].map(|i| point[i] - f64::from(pos[i]));
    with_world(|w| {
        place::cog_helper(
            large,
            pos,
            place::ClickedCog {
                large: cog_large,
                axis,
            },
            local,
            |p, l, a| w.valid_cog_position(dim, p, l, a),
            |p| lv.free(p),
        )
    })
    .flatten()
}

/// Where a shaft or cog item clicked at `point` on the shaft along `axis` at `pos` goes.
fn pole_target(
    lv: &ServerLevel<'_>,
    pos: [i32; 3],
    point: [f64; 3],
    axis: Axis,
) -> Option<[i32; 3]> {
    let dim = lv.dim;
    let hit = [0, 1, 2].map(|i| point[i] - f64::from(pos[i]) - 0.5);
    with_world(|w| {
        place::pole_helper(
            pos,
            axis,
            hit,
            // `PoleHelper.matchesAxis`: an `AbstractSimpleShaftBlock` (shaft, cog, large cog) or
            // a powered shaft along the axis; the line runs on through cogs.
            |p| {
                w.state_at(dim, p)
                    .and_then(|s| w.table.get(s))
                    .is_some_and(|(b, i)| {
                        matches!(
                            b,
                            Block::Shaft
                                | Block::PoweredShaft
                                | Block::Cogwheel
                                | Block::LargeCogwheel
                        ) && i.axis == Some(axis)
                    })
            },
            |p| lv.free(p),
        )
    })
    .flatten()
}

/// The shaft and cog helpers' target (`PlacementClient`'s indicator): where `item` clicked at
/// `point` on the block at `pos` would go, as `use_item_on` places it, and the axis it takes
/// (the offset's transform, for its ghost).
pub(crate) fn helper_target(
    lv: &ServerLevel<'_>,
    pos: [i32; 3],
    point: [f64; 3],
    item: &str,
) -> Option<crate::glue::interaction::HelperTarget> {
    let state = lv.block_state(pos)?;
    let (block, info) =
        with_world(|w| w.table.get(states::id(state)?).map(|(b, i)| (b, *i))).flatten()?;
    let large = match item {
        "create:cogwheel" => Some(false),
        "create:large_cogwheel" => Some(true),
        "create:shaft" => None,
        _ => return None,
    };
    let axis = |a: Axis| vec![("axis", axis_name(a).to_owned())];
    if let Some(large) = large
        && let Some((at, new_axis)) = cog_target(lv, large, pos, point)
    {
        return Some((at, axis(new_axis)));
    }
    match block {
        Block::Shaft | Block::PoweredShaft => {
            let along = info.axis?;
            pole_target(lv, pos, point, along).map(|at| (at, axis(along)))
        }
        Block::RotationSpeedController if large == Some(true) => {
            controller_target(lv, pos, info.axis?).map(|(at, a)| (at, axis(a)))
        }
        _ => None,
    }
}

/// `ShaftBlock.PlacementHelper`: a shaft or cog item on a shaft along `axis` goes at the free end
/// of its line.
fn pole(
    lv: &mut ServerLevel<'_>,
    player: PlayerRef,
    click: &BlockClick<'_>,
    stack: &ItemStack,
    item: &str,
    axis: Axis,
) -> Option<InteractionResult> {
    // `getItemPredicate`: a shaft, cog or large cog (`AbstractSimpleShaftBlock`).
    let block = match item {
        "create:shaft" => Block::Shaft,
        "create:cogwheel" => Block::Cogwheel,
        "create:large_cogwheel" => Block::LargeCogwheel,
        _ => return None,
    };
    let at = pole_target(lv, click.pos, click.hit.point, axis)?;
    Some(place_helped(lv, player, click, stack, block, at, axis))
}

/// `PlacementOffset.placeInWorld`: the held block at `at` on `axis` (waterlogged in a water
/// source), heard by the subsystems as a player's placement (`EntityPlaceEvent` and
/// `setPlacedBy`: the symmetry wand, the block's owner, its rotation indicators, the Shifting
/// Gears advancement), its place sound, one used up outside creative.
fn place_helped(
    lv: &mut ServerLevel<'_>,
    player: PlayerRef,
    click: &BlockClick<'_>,
    stack: &ItemStack,
    block: Block,
    at: [i32; 3],
    axis: Axis,
) -> InteractionResult {
    // The pole helper transforms the shaft before writing the placement state.
    let block = if block == Block::Shaft {
        with_world(|w| {
            create_engine::steam::shaft_for_placement(at, axis, |p| w.engine_at(lv.dim, p))
        })
        .unwrap_or(block)
    } else {
        block
    };
    // `getFluidState(pos).getType() == Fluids.WATER`: a source, not flowing water.
    let water = lv
        .block_state(at)
        .and_then(|s| lv.host.block_state_string(s))
        .is_some_and(|t| t == "minecraft:water[level=0]" || t == "minecraft:water");
    let props = [
        ("axis", axis_name(axis)),
        ("waterlogged", if water { "true" } else { "false" }),
    ];
    let Some(state) = state_of(&*lv.host, block, &props) else {
        return InteractionResult::Pass;
    };
    // `BlockEvent.EntityPlaceEvent` and `setPlacedBy`, as the deployer's placement is heard; a
    // subsystem that cancels it keeps the block from being placed.
    let mut placed = super::subsystem::Placed {
        placer: Some(player.entity()),
        pos: mod_api::BlockPos::new(at[0], at[1], at[2]),
        state,
        cancelled: false,
    };
    for s in super::SUBSYSTEMS {
        if let mod_api::Flow::Cancel = s.entity_place(&mut *lv.host, &mut placed) {
            return InteractionResult::Fail;
        }
    }
    lv.set_state(at, state, update_all());
    lv.place_sound(at);
    use_one(&mut *lv.host, player, click.hand, stack);
    InteractionResult::Success
}

/// `EncasableBlock.tryEncase`: a casing on a shaft or cog makes it the encased one of that
/// casing (an encased cog keeps the shaft ends that meet a shaft); the casing is not used up.
fn encase(
    lv: &mut ServerLevel<'_>,
    block: Block,
    axis: Axis,
    pos: [i32; 3],
    item: &str,
) -> Option<InteractionResult> {
    let brass = match item {
        "create:andesite_casing" => false,
        "create:brass_casing" => true,
        _ => return None,
    };
    let encased = match (block, brass) {
        (Block::Shaft, false) => Block::AndesiteEncasedShaft,
        (Block::Shaft, true) => Block::BrassEncasedShaft,
        (Block::Cogwheel, false) => Block::AndesiteEncasedCogwheel,
        (Block::Cogwheel, true) => Block::BrassEncasedCogwheel,
        (Block::LargeCogwheel, false) => Block::AndesiteEncasedLargeCogwheel,
        (Block::LargeCogwheel, true) => Block::BrassEncasedLargeCogwheel,
        _ => return None,
    };
    let dim = lv.dim;
    let mut props = vec![("axis", axis_name(axis))];
    if block != Block::Shaft {
        // EncasedCogwheelBlock.handleEncasing: a shaft end toward each neighbour with a shaft
        // facing back.
        for (positive, key) in [(true, "top_shaft"), (false, "bottom_shaft")] {
            let d = Dir::from_axis(axis, positive);
            let shaft = with_world(|w| w.has_shaft_towards(dim, add(pos, d), d.opposite()));
            props.push((key, if shaft == Some(true) { "true" } else { "false" }));
        }
    }
    let next = state_of(&*lv.host, encased, &props)?;
    super::items::wrench::switch_state(lv, pos, next);
    lv.place_sound(pos);
    Some(InteractionResult::Success)
}

/// The wrench on the blocks this module knows: true when it took the use.
pub fn wrench_used(
    lv: &mut ServerLevel<'_>,
    pos: [i32; 3],
    block: Block,
    clicked: Dir,
    sneaking: bool,
) -> bool {
    let Some(state) = lv.block_state(pos) else {
        return false;
    };
    let Some((_, info)) =
        with_world(|w| w.table.get(states::id(state)?).map(|(b, i)| (b, *i))).flatten()
    else {
        return false;
    };
    let Some(axis) = info.axis else {
        return false;
    };
    // IWrenchable.onWrenched on a chain drive. ChainDriveBlock.getRotatedBlockState: a drive in
    // a chain turns about the chain's axis, one alone about the clicked face's, as any axis
    // block. ChainDriveBlock.updateAfterWrenched: it starts over from its default state on the
    // new axis, shaped by the drives beside it (a gearshift's `powered` read again); the
    // drives beside it are told when it is set.
    if matches!(
        block,
        Block::EncasedChainDrive | Block::AdjustableChainGearshift
    ) && !sneaking
    {
        let about = match info.chain.unwrap_or_default() {
            create_engine::chain::Part::None => clicked.axis(),
            _ => create_engine::chain::connection_axis(axis, info.along_first),
        };
        let turned = create_engine::wrench::clockwise(Dir::from_axis(axis, true), about).axis();
        let Some(shaped) = with_world(|w| w.chain_after_wrenched(lv.dim, pos, turned)) else {
            return true;
        };
        let flag = |b: bool| if b { "true" } else { "false" };
        let facts = lv.host.block_states();
        let mut next = state
            .with(&facts, "axis", axis_name(turned))
            .and_then(|s| s.with(&facts, "part", shaped.part.name()))
            .and_then(|s| s.with(&facts, "axis_along_first", flag(shaped.along_first)));
        if block == Block::AdjustableChainGearshift {
            let powered = lv.has_neighbor_signal(pos);
            next = next.and_then(|s| s.with(&facts, "powered", flag(powered)));
        }
        if let Ok(next) = next {
            super::items::wrench::switch_state(lv, pos, next);
        }
        if lv.block_state(pos) != Some(state) {
            super::items::wrench::rotate_sound(lv, pos);
        }
        return true;
    }
    let encased_cog = matches!(
        block,
        Block::AndesiteEncasedCogwheel
            | Block::BrassEncasedCogwheel
            | Block::AndesiteEncasedLargeCogwheel
            | Block::BrassEncasedLargeCogwheel
    );
    let bare = match block {
        Block::AndesiteEncasedShaft | Block::BrassEncasedShaft => Block::Shaft,
        Block::AndesiteEncasedCogwheel | Block::BrassEncasedCogwheel => Block::Cogwheel,
        Block::AndesiteEncasedLargeCogwheel | Block::BrassEncasedLargeCogwheel => {
            Block::LargeCogwheel
        }
        // SequencedGearshiftBlock.onWrenched: clicked on a side across its axis, it stands up or
        // lies down; its horizontal axis turns only when clicked on top or bottom. Sneaking, it
        // is picked up as any block (`onSneakWrenched` is not its own).
        Block::SequencedGearshift if clicked.axis() != Axis::Y && !sneaking => {
            if axis == clicked.axis() {
                return true;
            }
            let v = if info.vertical { "false" } else { "true" };
            let facts = lv.host.block_states();
            if let Ok(next) = state.with(&facts, "vertical", v) {
                super::items::wrench::switch_state(lv, pos, next);
                super::items::wrench::rotate_sound(lv, pos);
            }
            return true;
        }
        _ => return false,
    };
    if sneaking {
        // EncasedShaftBlock / EncasedCogwheelBlock.onSneakWrenched: the casing comes off with
        // the encased block's break particles, nothing drops.
        lv.destroy_effect(pos, state);
        let water = if info.waterlogged { "true" } else { "false" };
        if let Some(next) = state_of(
            &*lv.host,
            bare,
            &[("axis", axis_name(axis)), ("waterlogged", water)],
        ) {
            super::items::wrench::switch_state(lv, pos, next);
        }
        return true;
    }
    if !encased_cog {
        return false;
    }
    let facts = lv.host.block_states();
    if clicked.axis() == axis {
        // EncasedCogwheelBlock.onWrenched on an end: that end's shaft comes or goes.
        let (key, on) = if clicked == Dir::from_axis(axis, true) {
            ("top_shaft", info.top_shaft)
        } else {
            ("bottom_shaft", info.bottom_shaft)
        };
        if let Ok(next) = state.with(&facts, key, if on { "false" } else { "true" }) {
            super::items::wrench::switch_state(lv, pos, next);
            super::items::wrench::rotate_sound(lv, pos);
        }
        return true;
    }
    // getRotatedBlockState: the axis turns, and the shaft ends swap when the turn flips which
    // end is positive (`swapShaftsForRotation`, a clockwise quarter turn).
    let about = clicked.axis();
    let turned = create_engine::wrench::clockwise(Dir::from_axis(axis, true), about);
    let swap = matches!(
        (about, axis),
        (Axis::X, Axis::Y) | (Axis::Y, Axis::Z) | (Axis::Z, Axis::X)
    );
    let (top, bottom) = if swap {
        (info.bottom_shaft, info.top_shaft)
    } else {
        (info.top_shaft, info.bottom_shaft)
    };
    let flag = |b: bool| if b { "true" } else { "false" };
    let next = state
        .with(&facts, "axis", axis_name(turned.axis()))
        .and_then(|s| s.with(&facts, "top_shaft", flag(top)))
        .and_then(|s| s.with(&facts, "bottom_shaft", flag(bottom)));
    if let Ok(next) = next
        && next != state
    {
        super::items::wrench::switch_state(lv, pos, next);
        super::items::wrench::rotate_sound(lv, pos);
    }
    true
}

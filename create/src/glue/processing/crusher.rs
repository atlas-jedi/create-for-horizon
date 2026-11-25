//! Crushing wheels (`CrushingWheelBlock`) and the controller between two of them
//! (`CrushingWheelControllerBlockEntity`): the item it draws in, crushes and lets out.

use super::{recipes, stacks};
use crate::glue::kinetics::dir_name;
use crate::glue::logistics::{insert_into_from, random_angle, upright};
use crate::glue::names::Named as _;
use crate::glue::owned::LevelLists as _;
use crate::glue::server::{ServerLevel, update_all};
use crate::glue::states;
use crate::glue::world::{level_index, try_with_world, with_world};
use create_engine::kinetics::{Axis, Dir};
use create_engine::{Block, belt, crusher, processing};
use mod_api::{
    AttachmentCodec, AttachmentError, AttachmentType, BlockPart, BlockState, Compound,
    EntityInsideEvent, EntityJoinLevelEvent, EntityRef, EntityWrite, Holder, HostBlockStates as _,
    InsideEffect, LevelRandomHost, Registrar, ServerHost, Tag,
};
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::OnceLock;

/// Crushing wheels: the controllers between wheels turning against each other
/// (`CrushingWheelBlock.updateControllers`, after a wheel is placed, broken or changes speed),
/// then the turning controllers' `tick`.
pub fn tick_crushers(lv: &mut ServerLevel<'_>) {
    let dim = lv.dim;
    let mut checks =
        with_world(|w| std::mem::take(&mut w.dims[dim].crusher_checks)).unwrap_or_default();
    checks.sort();
    checks.dedup();
    for pos in checks {
        fix_controllers(lv, pos);
    }
    let running = with_world(|w| w.dims[dim].running_crushers.clone()).unwrap_or_default();
    if running.is_empty() || !recipes::ready() {
        return;
    }
    for pos in running {
        tick_crusher(lv, pos);
    }
}

/// `BypassCrushingWheel` in an item's persistent data: the controller that let it out, which
/// neither catches it again nor stops it.
static BYPASS: AttachmentType<Option<[i32; 3]>> = AttachmentType {
    namespace: "create",
    name: "bypass_crushing_wheel",
    default: || None,
    codec: Some(AttachmentCodec {
        encode: |pos| {
            Ok(Compound::from([(
                "Pos".into(),
                Tag::IntArray(pos.map_or_else(Vec::new, |p| p.to_vec())),
            )]))
        },
        decode: |saved| match saved.get(&"Pos".into()) {
            Some(Tag::IntArray(p)) => Ok(p.as_slice().try_into().ok()),
            _ => Err(AttachmentError::Decode),
        },
    }),
    copy_on_death: false,
    sync: false,
};

thread_local! {
    /// [`BYPASS`] of the loaded items that have one, by entity id, for the collision rule.
    static BYPASSING: RefCell<HashMap<u32, [i32; 3]>> = RefCell::new(HashMap::new());
}

static VALID_FACINGS: OnceLock<HashMap<BlockState, Dir>> = OnceLock::new();

pub(super) fn register(r: &mut dyn Registrar) {
    // Horizon: attachment types need no declaration at registration.
    r.block_part(states::key(Block::CrushingWheelController), Passage);
}

pub(super) fn world_opened(host: &dyn mod_api::Host) {
    if VALID_FACINGS.get().is_some() {
        return;
    }
    let key = states::key(Block::CrushingWheelController);
    let Some(range) = host.block_states().states_of(key) else {
        return;
    };
    let facings = range
        .iter()
        .filter_map(|state| {
            let text = host.block_state_string(state)?;
            let (_, props) = states::split(&text);
            if !props.contains(&("valid", "true")) {
                return None;
            }
            Dir::ALL
                .into_iter()
                .find(|&dir| props.contains(&("facing", dir_name(dir))))
                .map(|dir| (state, dir))
        })
        .collect();
    let _ = VALID_FACINGS.set(facings);
}

pub(super) fn clear() {
    BYPASSING.with_borrow_mut(HashMap::clear);
}

fn bypassing(entity: u32) -> Option<[i32; 3]> {
    BYPASSING.with_borrow(|m| m.get(&entity).copied())
}

fn bypass(host: &mut dyn ServerHost, entity: u32, controller: [i32; 3]) {
    if let Err(e) = BYPASS.set(host, Holder::Entity(entity), Some(controller)) {
        crate::create_log!("[create] tagging a crushed item: {e:?}");
    }
    BYPASSING.with_borrow_mut(|m| m.insert(entity, controller));
}

/// An item back with its chunk keeps its tag.
pub(super) fn item_joined(host: &mut dyn ServerHost, event: &EntityJoinLevelEvent<'_>) {
    if event.loaded_from_disk
        && event.kind == "minecraft:item"
        && let Ok(Some(Some(controller))) = BYPASS.get_existing(host, Holder::Entity(event.entity))
    {
        BYPASSING.with_borrow_mut(|m| m.insert(event.entity, controller));
    }
}

pub(super) fn item_left(entity: u32) {
    BYPASSING.with_borrow_mut(|m| m.remove(&entity));
}

struct Passage;

#[mod_api::horizon_part]
impl BlockPart for Passage {
    /// `CrushingWheelControllerBlock.getCollisionShape`. Horizon: no collision hook, so the
    /// controller keeps its collision for items (its data's) against the processing entity and
    /// the items it let out: `tick_entity` takes an item on that shelf (`crusher::on_shelf`) and
    /// the results start clear of it (`crusher::output_clear`).
    #[cfg(any())] // Horizon: API missing, see HORIZON_GAPS.md
    fn collision_shape(
        &self,
        _context: &dyn mod_api::LoopContext,
        event: &mut mod_api::CollisionShapeEvent<'_>,
    ) {
        let Some(facing) = VALID_FACINGS.get().and_then(|f| f.get(&event.state)) else {
            return;
        };
        let Some(entity) = event.query.entity() else {
            return;
        };
        let pos = [event.pos.x, event.pos.y, event.pos.z];
        if bypassing(entity) == Some(pos) && *facing != Dir::Up {
            *event.shape = Some(Vec::new());
            return;
        }
        if try_with_world(|w| {
            w.dims.iter().any(|dim| {
                dim.crushers
                    .get(&pos)
                    .is_some_and(|(c, _)| c.entity == Some(entity))
            })
        }) == Some(true)
        {
            *event.shape = Some(Vec::new());
        }
    }

    /// `CrushingWheelControllerBlock.entityInside`.
    fn entity_inside(&self, _level: &mut dyn LevelRandomHost, event: &mut EntityInsideEvent<'_>) {
        let Some(&facing) = VALID_FACINGS.get().and_then(|f| f.get(&event.state)) else {
            return;
        };
        let Some(dim) = level_index(event.dimension) else {
            return;
        };
        let pos = [event.pos.x, event.pos.y, event.pos.z];
        if try_with_world(|w| {
            w.dims[dim]
                .crushers
                .get(&pos)
                .is_some_and(|(c, _)| c.entity == Some(event.entity as u32))
        }) == Some(true)
        {
            event.effects.run_before(
                InsideEffect::Freeze,
                EntityWrite::StuckIn(crusher::stuck_multiplier(facing)),
            );
        }
    }
}

/// `updateControllers` for the wheel (or the place a wheel left) at `pos`.
fn fix_controllers(lv: &mut ServerLevel<'_>, pos: [i32; 3]) {
    let dim = lv.dim;
    let wheel = lv
        .info(pos)
        .filter(|(b, _)| *b == Block::CrushingWheel)
        .map(|(_, i)| i.axis);
    for side in Dir::ALL {
        let cpos = super::step(pos, side, 1);
        let existing = lv
            .info(cpos)
            .is_some_and(|(b, _)| b == Block::CrushingWheelController);
        let want = wheel.and_then(|axis| {
            let axis = axis?;
            let speed = lv.speed(pos);
            let opos = super::step(pos, side, 2);
            let other = lv
                .info(opos)
                .filter(|(b, _)| *b == Block::CrushingWheel)
                .and_then(|(_, i)| i.axis)
                .map(|a| (a, lv.speed(opos)));
            crusher::controller(axis, side, speed, other).map(|c| (c, speed))
        });
        match want {
            // A wheel that went (`onRemove`) or no partner: no controller.
            None => {
                if existing && (wheel.is_none() || wheel.flatten() != Some(side.axis())) {
                    let _ = lv.level.remove_block(&mut *lv.host, cpos, false);
                }
            }
            Some(((valid, facing), speed)) => {
                if !existing && !lv.replaceable(cpos) {
                    continue;
                }
                let Some(state) = lv.host.block_state_id(&format!(
                    "create:crushing_wheel_controller[facing={},valid={valid}]",
                    dir_name(facing)
                )) else {
                    continue;
                };
                if lv.block_state(cpos) != Some(state) {
                    lv.set_state(cpos, state, update_all());
                }
                let crushing = if valid {
                    crate::glue::advancements::crushing(lv, pos, side);
                    crusher::crushing_speed(speed)
                } else {
                    0.0
                };
                with_world(|w| w.set_crusher_speed(dim, cpos, crushing));
            }
        }
    }
}

/// `CrushingWheelControllerBlockEntity.tick` (and the controller block's `entityInside`).
fn tick_crusher(lv: &mut ServerLevel<'_>, pos: [i32; 3]) {
    let dim = lv.dim;
    let Some(facing) = lv.facing_of(pos, Block::CrushingWheelController) else {
        return;
    };
    let Some((mut c, speed)) = with_world(|w| w.dims[dim].crushers.get(&pos).cloned()).flatten()
    else {
        return;
    };
    if speed == 0.0 {
        // `crushingspeed == 0`: it neither works nor sounds (`tickAudio`).
        super::ambient::show_input(lv, pos, super::ambient::CRUSH_KEY, None);
        return;
    }
    let before = c.slots.clone();
    if let Some(id) = c.entity {
        tick_entity(lv, pos, facing, speed, &mut c, id);
    } else if !c.occupied() {
        c.entity = crushable_in(lv, pos);
    } else {
        match crusher::tick_inventory(&mut c, speed) {
            crusher::Step::Wait => {}
            crusher::Step::Apply => {
                let host = &*lv.host;
                let max = |i: &str| stacks::limit(host, i);
                let mut rng = processing::Rng::new(lv.seed_at(pos));
                let item = c.slots[0].0.clone();
                // The input's crafting remainder comes out with the results.
                let left = stacks::remainder(host, &item);
                recipes::with(|r| {
                    let recipe = crusher::recipe(&r.crushing, &r.milling, &item);
                    crusher::apply(&mut c, recipe, max, &mut rng, left.as_deref());
                });
            }
            crusher::Step::Output => {
                // Facing anywhere but up: a belt or depot below (and on, past a sideways
                // facing) takes the results from that side; what it doesn't take waits.
                let side = if facing.axis() == Axis::Y {
                    [0, 0, 0]
                } else {
                    facing.offset()
                };
                let next = [pos[0] + side[0], pos[1] - 1, pos[2] + side[2]];
                if facing != Dir::Up && crate::glue::logistics::direct_input_at(lv, next) {
                    // `canInsertFromSide`: what it takes nothing from this side waits.
                    if !crate::glue::logistics::direct_input_accepts(lv, next, facing) {
                        return;
                    }
                    let mut left = Vec::new();
                    for (item, count) in std::mem::take(&mut c.slots) {
                        let up = upright(&*lv.host, &item);
                        let one = belt::Item::new(&item, count, random_angle(lv, pos), up);
                        let n = insert_into_from(lv, next, facing, &one, false).unwrap_or(count);
                        if n > 0 {
                            left.push((item, n));
                        }
                    }
                    c.slots = left;
                } else {
                    // Results thrown out past the facing side (`outPos`, `outSpeed`), tagged
                    // `BypassCrushingWheel`. Horizon: started clear of the controller's collision
                    // for items, which can't be emptied for them.
                    let (_, v) = crusher::output(pos, facing);
                    let at = crusher::output_clear(pos, facing);
                    for (item, count) in crusher::ejected(&mut c) {
                        let Some(stack) = stacks::stack(&*lv.host, &item, u32::from(count)) else {
                            continue;
                        };
                        if let Ok(Some(entity)) = lv.level.spawn_item_with_motion(
                            &mut *lv.host,
                            at[0],
                            at[1],
                            at[2],
                            stack,
                            v,
                            0,
                        ) {
                            bypass(&mut *lv.host, entity.id(), pos);
                        }
                    }
                }
            }
        }
    }
    with_world(|w| {
        if let Some(e) = w.dims[dim].crushers.get_mut(&pos) {
            e.0 = c.clone();
        }
        if c.slots != before {
            w.mark_unsaved(dim, pos);
        }
    });
    // What its clients crush (`tickAudio`: an entity drawn in, or an input not yet crushed;
    // `spawnParticles` of that input), for their crumbs and noise (`ambient`).
    let shown = if c.entity.is_some() {
        Some((String::new(), false))
    } else if !c.applied {
        c.slots.first().map(|(item, _)| (item.clone(), false))
    } else {
        None
    };
    super::ambient::show_input(lv, pos, super::ambient::CRUSH_KEY, shown);
}

/// The controller's inventory taking `count` of `item` from a belt, a saw or another crusher
/// (`ProcessingInventory.insertItem` through its handler): only while it holds nothing and
/// draws in no entity. Whether it took them.
pub fn crusher_put(lv: &mut ServerLevel<'_>, pos: [i32; 3], item: &str, count: u8) -> bool {
    let dim = lv.dim;
    let time =
        recipes::with(|r| crusher::recipe(&r.crushing, &r.milling, item).map(|x| x.time)).flatten();
    with_world(|w| {
        let (c, _) = w.dims[dim].crushers.get_mut(&pos)?;
        if c.occupied() {
            return None;
        }
        crusher::intake(c, item, count, time);
        w.mark_unsaved(dim, pos);
        Some(())
    })
    .flatten()
    .is_some()
}

/// The controller's `ITEMS` handler (`CrushingWheelControllerBlockEntity.registerCapabilities`:
/// its `ProcessingInventory` on every side): a stack into slot 0 while it holds nothing
/// (`crusher::insert`), nothing out (`extractItem` gives nothing). The inventory lives in the
/// engine, so an insert the host's transaction later aborts cannot be taken back, as with a
/// belt segment's handler.
pub struct CrusherItems {
    pub dim: usize,
    pub pos: [i32; 3],
}

impl CrusherItems {
    fn slot(&self, index: usize) -> Option<(String, u8)> {
        with_world(|w| {
            let (c, _) = w.dims[self.dim].crushers.get(&self.pos)?;
            // Results sit after the input's slot 0 in Create's inventory.
            let i = if c.applied {
                index.checked_sub(1)?
            } else {
                index
            };
            c.slots.get(i).cloned()
        })
        .flatten()
    }
}

impl mod_api::ResourceHandler<mod_api::ItemResource> for CrusherItems {
    fn size(&self, _host: &mut dyn mod_api::ServerHost) -> usize {
        32
    }
    fn resource(
        &self,
        host: &mut dyn mod_api::ServerHost,
        index: usize,
    ) -> Option<mod_api::ItemResource> {
        let (item, n) = self.slot(index)?;
        stacks::stack(&*host, &item, u32::from(n)).map(|s| mod_api::ItemResource::of(&s))
    }
    fn amount(&self, _host: &mut dyn mod_api::ServerHost, index: usize) -> u32 {
        self.slot(index).map_or(0, |(_, n)| u32::from(n))
    }
    fn capacity(
        &self,
        host: &mut dyn mod_api::ServerHost,
        _index: usize,
        resource: &mod_api::ItemResource,
    ) -> u32 {
        let resource = Some(resource);
        resource.map_or(64, |r| {
            u32::from(stacks::limit(&*host, &stacks::id(&*host, r.as_stack())))
        })
    }
    fn is_valid(
        &self,
        _host: &mut dyn mod_api::ServerHost,
        index: usize,
        _resource: &mod_api::ItemResource,
    ) -> bool {
        index == 0
            && with_world(|w| {
                w.dims[self.dim]
                    .crushers
                    .get(&self.pos)
                    .is_some_and(|(c, _)| !c.occupied())
            })
            .unwrap_or(false)
    }
    fn insert(
        &self,
        host: &mut dyn mod_api::ServerHost,
        _tx: &mod_api::Transaction,
        index: usize,
        resource: &mod_api::ItemResource,
        amount: u32,
    ) -> u32 {
        let stack = resource.as_stack();
        // The engine keeps items by id: a stack with components stays out.
        if stack.components().is_some() || stack.damage() != 0 {
            return 0;
        }
        let item = stacks::id(&*host, stack);
        let max = stacks::limit(&*host, &item);
        let time =
            recipes::with(|r| crusher::recipe(&r.crushing, &r.milling, &item).map(|x| x.time))
                .flatten();
        let count = amount.min(255) as u8;
        let (dim, pos) = (self.dim, self.pos);
        with_world(|w| {
            let (c, _) = w.dims[dim].crushers.get_mut(&pos)?;
            let n = crusher::insert(c, index, &item, count, max, time);
            if n > 0 {
                w.mark_unsaved(dim, pos);
            }
            Some(u32::from(n))
        })
        .flatten()
        .unwrap_or(0)
    }
    fn extract(
        &self,
        _host: &mut dyn mod_api::ServerHost,
        _tx: &mod_api::Transaction,
        _index: usize,
        _resource: &mod_api::ItemResource,
        _amount: u32,
    ) -> u32 {
        0
    }
}

/// `CrushingWheelControllerBlockEntity.CRUSHER_DAMAGE` (`crushingDamage`, 4 by default).
const CRUSHING_DAMAGE: f32 = 4.0;

/// The damage the wheels deal: Create's own `create:crush`.
const CRUSH: &str = "create:crush";

/// `CrushingWheelControllerBlock.checkEntityForProcessing`.
fn crushable_in(lv: &mut ServerLevel<'_>, pos: [i32; 3]) -> Option<u32> {
    let lo = pos.map(f64::from);
    let hi = lo.map(|v| v + 1.0);
    let peaceful = lv.level.difficulty(&*lv.host) == Some("peaceful");
    lv.level
        .entities_in(&*lv.host, lo, hi)
        .unwrap_or_default()
        .into_iter()
        .find(|&entity| {
            if bypassing(entity.id()) == Some(pos)
                || (lv.host.item_entity(entity).is_none()
                    && lv
                        .host
                        .living(entity)
                        .is_none_or(|living| living.alive(&*lv.host) != Some(true)))
            {
                return false;
            }
            if entity.kind(&*lv.host).map(|k| k.name(&*lv.host)).as_deref()
                == Some("minecraft:player")
            {
                return !peaceful
                    && !entity
                        .uuid(&mut *lv.host)
                        .and_then(|u| lv.host.player(u))
                        .and_then(|p| p.is_creative(&*lv.host))
                        .unwrap_or(false);
            }
            true
        })
        .map(|e| e.id())
}

/// `CrushingWheelControllerBlockEntity.tick`.
fn tick_entity(
    lv: &mut ServerLevel<'_>,
    pos: [i32; 3],
    facing: Dir,
    speed: f32,
    c: &mut crusher::Crusher,
    id: u32,
) {
    let Some(entity) = lv.host.entity(id).filter(|&entity| {
        entity.level(&*lv.host) == Some(lv.level)
            && entity
                .bounding_box(&*lv.host)
                .is_some_and(|bounds| crusher::overlaps(pos, 0.5, bounds))
            && lv
                .host
                .living(entity)
                .is_none_or(|l| l.alive(&*lv.host) == Some(true))
    }) else {
        c.entity = None;
        return;
    };
    let Some(at) = entity.position(&*lv.host) else {
        c.entity = None;
        return;
    };
    let sneaking = entity.is_shift_key_down(&*lv.host) == Some(true);
    let (motion, taken) = crusher::hold(pos, facing, speed, at, sneaking);
    entity.set_velocity(&mut *lv.host, motion);
    if let Some(item) = lv.host.item_entity(entity) {
        let _ = item.set_pickup_delay(&mut *lv.host, 20);
        // Horizon: also where it rests on the controller's collision for items, which can't be
        // emptied for it (`collision_shape`).
        let taken = taken || crusher::on_shelf(pos, facing, at);
        if taken && let Some(stack) = item.stack(&*lv.host) {
            let item = stacks::id(&*lv.host, &stack);
            let count = stack.count().min(255) as u8;
            let time =
                recipes::with(|r| crusher::recipe(&r.crushing, &r.milling, &item).map(|x| x.time))
                    .flatten();
            crusher::intake(c, &item, count, time);
            entity.discard(&mut *lv.host);
        }
        return;
    }
    crush_entity(lv, pos, facing, entity);
}

fn crush_entity(lv: &mut ServerLevel<'_>, pos: [i32; 3], facing: Dir, entity: EntityRef) {
    let (out, _) = crusher::output(pos, facing);
    let o = facing.offset();
    let out = [0, 1, 2].map(|i| out[i] + 0.5 * f64::from(o[i]));
    let living = lv.host.living(entity);
    if living
        .and_then(|l| l.health(&*lv.host))
        .is_some_and(|h| h - CRUSHING_DAMAGE <= 0.0)
    {
        entity.teleport_to(&mut *lv.host, out, None);
    }
    let _ = entity.hurt(&mut *lv.host, CRUSH, CRUSHING_DAMAGE, None, None);
    if living.is_some_and(|l| l.alive(&*lv.host) != Some(true)) {
        entity.teleport_to(&mut *lv.host, out, None);
    }
}

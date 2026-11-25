//! Item entities that join the world, as the glue spawns them through the host
//! (glue/logistics/items.rs `drop_item`, and the host's loot for `destroyBlock(pos, true)`).
//! The harness discards every item entity at the end of each server tick
//! (`CaseIsolation.disposeUnobservedDrops`). One a block entity spawned joined after the
//! level's entity pass and is observed where and how it was put; one an input put before the
//! level tick flew a tick first, which the driver does not follow.
use super::*;
use crate::transport::{DroppedItem, Placed, SPREAD_MIDDLE, Spreader, Stack};
use create_engine::{Block, belt};

pub(super) struct ItemEntity {
    stack: Stack,
    at: [f64; 3],
    motion: [f64; 3],
    placed: Placed,
}

impl Run {
    /// `count` of `item` at `at`, moving `motion` blocks a tick.
    pub(super) fn spawn_item(&mut self, at: [f64; 3], motion: [f64; 3], item: &str, count: u8) {
        let placed = if self.entity_pass_done {
            Placed::Exact
        } else {
            Placed::Flown
        };
        self.join(at, motion, item, count, placed);
    }

    /// `Block.popResource` at `p`.
    pub(super) fn pop_resource(&mut self, p: Pos, item: &str, count: u8) {
        self.spread(p, Spreader::PopResource, item, count);
    }

    /// `Containers.dropItemStack` at `at`.
    pub(super) fn drop_item_stack(&mut self, at: [f64; 3], item: &str, count: u8) {
        self.spread(
            at.map(|v| v.floor() as i32),
            Spreader::DropItemStack,
            item,
            count,
        );
    }

    fn spread(&mut self, p: Pos, by: Spreader, item: &str, count: u8) {
        let at = std::array::from_fn(|i| f64::from(p[i]) + SPREAD_MIDDLE[i]);
        self.join(at, [0.0; 3], item, count, Placed::Spread(by));
    }

    fn join(&mut self, at: [f64; 3], motion: [f64; 3], item: &str, count: u8, placed: Placed) {
        if count == 0 || item == "minecraft:air" {
            return;
        }
        self.item_entities.push(ItemEntity {
            stack: Stack {
                item: item.into(),
                count,
            },
            at,
            motion,
            placed,
        });
    }

    /// The glue's `nudge_entities` (`EjectorBlockEntity.nudgeEntities`): items whose box (a
    /// quarter block each way) meets the ejector's block, inset a pixel at the sides, go up 2
    /// pixels.
    pub(super) fn nudge_items(&mut self, p: Pos) {
        let inset = 1.0 / 16.0;
        let lo = [
            f64::from(p[0]) + inset,
            f64::from(p[1]),
            f64::from(p[2]) + inset,
        ];
        let hi = [
            f64::from(p[0] + 1) - inset,
            f64::from(p[1] + 1),
            f64::from(p[2] + 1) - inset,
        ];
        for e in &mut self.item_entities {
            let a = e.at;
            let meets = a[0] - 0.125 < hi[0]
                && a[0] + 0.125 > lo[0]
                && a[1] < hi[1]
                && a[1] + 0.25 > lo[1]
                && a[2] - 0.125 < hi[2]
                && a[2] + 0.125 > lo[2];
            if meets {
                e.at[1] += 0.125;
            }
        }
    }

    /// The end of the server tick: the harness discards the item entities.
    pub(super) fn dispose_item_entities(&mut self) {
        let tick = self.now;
        for e in std::mem::take(&mut self.item_entities) {
            self.drops.push(DroppedItem {
                tick,
                stack: e.stack,
                pos: e.at,
                motion: e.motion,
                placed: Some(e.placed),
            });
        }
        self.entity_pass_done = false;
    }

    /// `destroyBlock(pos, true)`, by the actor or not: the block's loot, then air. The actor
    /// drops nothing in creative mode, nor with its empty hand from a block that needs a tool.
    pub(super) fn break_block(&mut self, p: Pos, by_player: bool) {
        let state = self.at(p);
        if state == AIR {
            return;
        }
        let name = self.name(state);
        let kept = by_player && (self.creative || crate::loot::requires_tool(&name) == Some(true));
        let loot = if kept {
            Some(Vec::new())
        } else {
            crate::loot::drops(&name)
        };
        match loot {
            Some(drops) => {
                // `BeltBlock.getDrops`: a pulley's shaft too. The port's belt loot table rolls
                // it (a shaft pool for every part but the middle); a checkout whose table
                // lacks that pool gets it here.
                let shaft_rolled = drops.iter().any(|(item, _)| item == "create:shaft");
                for (item, count) in drops {
                    self.pop_resource(p, &item, count);
                }
                if !kept
                    && !shaft_rolled
                    && let Some((Block::Belt, i)) = self.w.table.get(state)
                    && i.belt.is_some_and(|s| s.part != belt::Part::Middle)
                {
                    for (item, count) in crate::loot::drops("create:shaft").unwrap_or_default() {
                        self.pop_resource(p, &item, count);
                    }
                }
            }
            None => self
                .notes
                .push(format!("adapter gap: loot of {name} at {p:?}")),
        }
        self.set(p, AIR);
    }

    /// What block entities the engine removed drop (`destroy`): a belt's items thrown off it,
    /// a depot's merging stacks and an ejector's stacks in flight.
    pub(super) fn flush_block_drops(&mut self) {
        let d = &mut self.w.dims[0];
        let belts = std::mem::take(&mut d.belt_drops);
        let depots = std::mem::take(&mut d.depot_drops);
        let ejectors = std::mem::take(&mut d.ejector_drops);
        // `BeltInventory.eject` at the speed the belt had (glue `break_pass`).
        for (c, seg, length, item, rpm) in belts {
            let (at, motion) = belt::eject(c, &seg, length, rpm, item.position);
            self.spawn_item(at, motion, &item.item, item.count);
        }
        for (p, item, count) in depots {
            self.pop_resource(p, &item, count);
        }
        for (at, motion, item, count) in ejectors {
            self.spawn_item(at, motion, &item, count);
        }
    }
}

//! Create's item drain (`ItemDrainBlockEntity`, `GenericItemEmptying`): a stack put on it
//! travels across it the way it came in, an eighth of a block a tick; an item that holds a fluid
//! stops in the middle, is emptied into the 1,500 mB tank over a 20-tick cycle (waiting while
//! the tank has no room) and goes on as what is left of it; at the far side the stack is handed
//! on (`DirectBeltInputBehaviour`) or thrown off.

use crate::kinetics::Dir;
use crate::spout::FILLING_TIME;
use crate::tank::{BUCKET, Fluid};

/// `SmartFluidTankBehaviour.single(this, 1500)`.
pub const CAPACITY: u32 = 1500;

/// `itemMovementPerTick`.
pub const SPEED: f32 = 1.0 / 8.0;

/// The stack on a drain (`heldItem`, a `TransportedItemStack`).
#[derive(Clone, Debug, PartialEq)]
pub struct Held {
    pub item: String,
    pub count: u8,
    /// How far across it is (`beltPosition`), 0 to 1, and where it was a tick ago.
    pub position: f32,
    pub prev: f32,
    /// The way it travels (`insertedFrom`): it leaves by that side.
    pub from: Dir,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Drain {
    pub held: Option<Held>,
    /// `processingTicks`: counting down while an item is emptied.
    pub processing: i32,
    pub fluid: Option<(Fluid, u32)>,
}

/// What emptying an item gives (`GenericItemEmptying`): a fluid and how much, and what is left
/// of the item.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Emptying {
    pub fluid: Fluid,
    pub amount: u32,
    pub leftover: Option<String>,
}

/// `GenericItemEmptying.emptyItem`: a potion's quarter bucket leaving its glass bottle
/// (`PotionFluidHandler.emptyPotion`), a `create:emptying` recipe for the item (`recipes`, by
/// item), else a filled bucket's worth leaving the bucket.
pub fn contents(item: &str, recipes: &[(String, Emptying)]) -> Option<Emptying> {
    if let Some(fluid) = crate::tank::potion_fluid(item) {
        return Some(Emptying {
            fluid,
            amount: crate::spout::BOTTLE,
            leftover: Some("minecraft:glass_bottle".into()),
        });
    }
    if let Some((_, e)) = recipes.iter().find(|(i, _)| i == item) {
        return Some(e.clone());
    }
    Fluid::of_bucket(item).map(|fluid| Emptying {
        fluid,
        amount: BUCKET,
        leftover: Some("minecraft:bucket".into()),
    })
}

/// `ItemDrainBlock.updateEntityAfterFallOn`: an item landing on the drain goes in travelling
/// the way it moved (`Direction.getNearest` of its horizontal motion, normalised), north when it
/// fell straight down (`getNearest` of no motion: `Vec3.normalize` gives zero under 1e-5).
pub fn landing_side(motion: [f64; 3]) -> Dir {
    let len = (motion[0] * motion[0] + motion[2] * motion[2]).sqrt();
    if len < 1.0e-5 {
        return Dir::North;
    }
    let (x, z) = ((motion[0] / len) as f32, (motion[2] / len) as f32);
    let mut best = (Dir::North, f32::MIN_POSITIVE);
    for d in Dir::ALL {
        let o = d.offset();
        let dot = x * o[0] as f32 + z * o[2] as f32;
        if dot > best.1 {
            best = (d, dot);
        }
    }
    best.0
}

/// `tryInsertingFromSide`: `count` of `item` coming in travelling toward `side`. Only while the
/// drain holds nothing; of a stack that can be emptied it takes one. How many it took.
pub fn insert(
    d: &mut Drain,
    item: &str,
    count: u8,
    side: Dir,
    recipes: &[(String, Emptying)],
) -> u8 {
    if d.held.is_some() || count == 0 {
        return 0;
    }
    let taken = if count > 1 && contents(item, recipes).is_some() {
        1
    } else {
        count
    };
    let position = if side.axis() == crate::kinetics::Axis::Y {
        0.5
    } else {
        0.0
    };
    d.held = Some(Held {
        item: item.to_owned(),
        count: taken,
        position,
        prev: position,
        from: side,
    });
    taken
}

/// What a tick did that the world must know of.
#[derive(Clone, Debug, PartialEq)]
pub enum Tick {
    /// Nothing to tell (it moved, or waits).
    Idle,
    /// The held item was emptied into the tank (`notifyUpdate`).
    Emptied,
    /// The stack reached the far side: hand it on that way, or throw it off.
    Out(Dir),
}

/// `tick` and `continueProcessing` on the server.
pub fn tick(d: &mut Drain, recipes: &[(String, Emptying)]) -> Tick {
    let Some(held) = d.held.as_mut() else {
        d.processing = 0;
        return Tick::Idle;
    };
    if d.processing > 0 {
        held.prev = 0.5;
        d.processing -= 1;
        if d.processing < 5 {
            return Tick::Idle;
        }
        let Some(e) = contents(&held.item, recipes) else {
            d.processing = 0;
            return Tick::Idle;
        };
        let room = d
            .fluid
            .is_none_or(|(f, n)| f == e.fluid && n + e.amount <= CAPACITY);
        if d.processing > 5 {
            if !room {
                d.processing = FILLING_TIME;
            }
            return Tick::Idle;
        }
        // continueProcessing only simulates capacity while processingTicks > 5.
        // At 5 it consumes the item and executes a (possibly partial) tank fill.
        match d.fluid {
            None => d.fluid = Some((e.fluid, e.amount.min(CAPACITY))),
            Some((fluid, amount)) if fluid == e.fluid => {
                d.fluid = Some((fluid, amount.saturating_add(e.amount).min(CAPACITY)));
            }
            Some(_) => {}
        }
        match e.leftover {
            Some(left) => held.item = left,
            None => d.held = None,
        }
        return Tick::Emptied;
    }
    held.prev = held.position;
    held.position += SPEED;
    if held.position > 1.0 {
        held.position = 1.0;
        return Tick::Out(held.from);
    }
    if held.prev < 0.5 && held.position >= 0.5 {
        if contents(&held.item, recipes).is_none() {
            return Tick::Idle;
        }
        held.position = 0.5;
        d.processing = FILLING_TIME;
    }
    Tick::Idle
}

/// Drained by a pump: up to `amount`.
pub fn drain(d: &mut Drain, amount: u32) -> Option<(Fluid, u32)> {
    let (fluid, have) = d.fluid?;
    let out = amount.min(have);
    d.fluid = (have > out).then_some((fluid, have - out));
    (out > 0).then_some((fluid, out))
}

/// Whether the drain's stack moves this tick (clients draw it each frame).
pub fn moving(d: &Drain) -> bool {
    d.held
        .as_ref()
        .is_some_and(|h| h.position != h.prev || d.processing > 0)
}

/// `Block.isFaceFull(shape, face)` for a shape given as block-local boxes (`[min x, y, z, max
/// x, y, z]`): the slice of the shape just inside its `face` side (`VoxelShape.getFaceShape`)
/// covers the whole face. With `BlockHelper.hasBlockSolidSide` (leaves aside), what makes an
/// item drain hold a stack at its edge instead of throwing it off.
pub fn face_full(boxes: &[[f64; 6]], face: Dir) -> bool {
    let a = match face.axis() {
        crate::kinetics::Axis::X => 0,
        crate::kinetics::Axis::Y => 1,
        crate::kinetics::Axis::Z => 2,
    };
    let (u, v) = match a {
        0 => (1, 2),
        1 => (0, 2),
        _ => (0, 1),
    };
    let at = if face.positive() { 1.0 - 1.0e-7 } else { 1.0e-7 };
    let rects: Vec<[f64; 4]> = boxes
        .iter()
        .filter(|b| b[a] <= at && at < b[a + 3])
        .map(|b| {
            [
                b[u].max(0.0),
                b[v].max(0.0),
                b[u + 3].min(1.0),
                b[v + 3].min(1.0),
            ]
        })
        .collect();
    let mut us = vec![0.0, 1.0];
    let mut vs = vec![0.0, 1.0];
    for r in &rects {
        us.extend([r[0], r[2]].into_iter().filter(|c| (0.0..=1.0).contains(c)));
        vs.extend([r[1], r[3]].into_iter().filter(|c| (0.0..=1.0).contains(c)));
    }
    us.sort_by(f64::total_cmp);
    vs.sort_by(f64::total_cmp);
    us.dedup();
    vs.dedup();
    // Every cell of the grid the rectangles' edges make is covered by one of them.
    us.windows(2).all(|du| {
        vs.windows(2).all(|dv| {
            let (cu, cv) = ((du[0] + du[1]) / 2.0, (dv[0] + dv[1]) / 2.0);
            du[1] - du[0] < 1.0e-9
                || dv[1] - dv[0] < 1.0e-9
                || rects
                    .iter()
                    .any(|r| r[0] <= cu && cu <= r[2] && r[1] <= cv && cv <= r[3])
        })
    })
}

/// `ItemDrainRenderer.renderFluid`'s stream from the emptied item into the basin at
/// `processingTicks - partialTicks` = `pt`: a column from 1/4 to the top, thickest halfway
/// through the cycle (`radius = (2 * progress - 1)^2 - 1`, the box `inflate(radius / 32)`).
/// Its corners, block-local, or None while it has no width.
pub fn stream(pt: f32) -> Option<([f32; 3], [f32; 3])> {
    let progress = (1.0 - (pt - 5.0) / 10.0).clamp(0.0, 1.0);
    let radius = (2.0 * progress - 1.0).powi(2) - 1.0;
    let k = -radius / 32.0;
    (k > 0.0).then_some(([0.5 - k, 0.25 + k, 0.5 - k], [0.5 + k, 1.0 - k, 0.5 + k]))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `BlockHelper.hasBlockSolidSide`: a full block or a top slab's side is full; a bottom
    /// slab's side, a chest's (inset a pixel) and nothing at all are not.
    #[test]
    fn only_a_full_face_holds_the_stack() {
        let cube = [[0.0, 0.0, 0.0, 1.0, 1.0, 1.0]];
        assert!(face_full(&cube, Dir::West) && face_full(&cube, Dir::Up));
        let bottom_slab = [[0.0, 0.0, 0.0, 1.0, 0.5, 1.0]];
        assert!(!face_full(&bottom_slab, Dir::West));
        assert!(face_full(&bottom_slab, Dir::Down));
        assert!(!face_full(&bottom_slab, Dir::Up));
        let chest = [[1.0 / 16.0, 0.0, 1.0 / 16.0, 15.0 / 16.0, 14.0 / 16.0, 15.0 / 16.0]];
        assert!(!face_full(&chest, Dir::North));
        assert!(!face_full(&[], Dir::East));
        // Stairs facing east: the bottom half and the east half; its east side is full, its
        // west side only below.
        let stairs = [
            [0.0, 0.0, 0.0, 1.0, 0.5, 1.0],
            [0.5, 0.5, 0.0, 1.0, 1.0, 1.0],
        ];
        assert!(face_full(&stairs, Dir::East));
        assert!(!face_full(&stairs, Dir::West));
        // Two halves side by side make a full face.
        let halves = [
            [0.0, 0.0, 0.0, 0.5, 1.0, 1.0],
            [0.5, 0.0, 0.0, 1.0, 1.0, 1.0],
        ];
        assert!(face_full(&halves, Dir::North));
    }

    #[test]
    fn the_stream_pours_between_the_cycle_s_15th_and_5th_last_ticks() {
        assert_eq!(stream(20.0), None);
        assert_eq!(stream(15.0), None);
        assert_eq!(stream(5.0), None);
        assert_eq!(stream(0.0), None);
        let (min, max) = stream(10.0).unwrap();
        let k = 1.0 / 32.0;
        assert_eq!((min, max), ([0.5 - k, 0.25 + k, 0.5 - k], [0.5 + k, 1.0 - k, 0.5 + k]));
        let (a, _) = stream(12.5).unwrap();
        assert!(a[0] > 0.5 - k && a[0] < 0.5);
    }

    #[test]
    fn a_thrown_item_goes_in_the_way_it_moved() {
        assert_eq!(landing_side([0.0, -0.3, 0.0]), Dir::North, "fell straight");
        assert_eq!(landing_side([0.2, -0.3, 0.05]), Dir::East);
        assert_eq!(landing_side([-0.01, 0.0, 0.0]), Dir::West);
        assert_eq!(landing_side([0.05, 0.0, 0.2]), Dir::South);
        assert_eq!(landing_side([0.0, 0.0, -0.2]), Dir::North);
        // A diagonal tie: the first of `Direction.values()` wins (north before east).
        assert_eq!(landing_side([0.1, 0.0, -0.1]), Dir::North);
    }

    #[test]
    fn a_handler_insert_simulated_then_committed_takes_it_once() {
        // `ItemDrainItemHandler.insertItem` from the east face: `insertedFrom` is west, the
        // stack starts at the edge. A simulated insert (the glue's `insert_into_drain_sim` on a
        // copy) leaves the drain as it was; the committed one takes it; then it holds a stack
        // and takes no more (`!getHeldItemStack().isEmpty()`).
        let mut d = Drain::default();
        let mut copy = d.clone();
        assert_eq!(insert(&mut copy, "minecraft:cobblestone", 32, Dir::West, &[]), 32);
        assert_eq!(d, Drain::default());
        assert_eq!(insert(&mut d, "minecraft:cobblestone", 32, Dir::West, &[]), 32);
        let held = d.held.clone().unwrap();
        assert_eq!((held.position, held.from, held.count), (0.0, Dir::West, 32));
        assert_eq!(insert(&mut d, "minecraft:cobblestone", 32, Dir::West, &[]), 0);
        // Nothing to empty: across in 8 ticks, out by the west side.
        assert_eq!(run(&mut d, &[], 9), vec![Tick::Out(Dir::West)]);
        // Of a stack of buckets it takes one (`canItemBeEmptied`).
        let mut d = Drain::default();
        assert_eq!(insert(&mut d, "minecraft:water_bucket", 3, Dir::South, &[]), 1);
    }

    fn run(d: &mut Drain, recipes: &[(String, Emptying)], ticks: usize) -> Vec<Tick> {
        (0..ticks)
            .map(|_| tick(d, recipes))
            .filter(|t| *t != Tick::Idle)
            .collect()
    }

    #[test]
    fn drain_and_pump_turns_match_java_capacity_boundary() {
        // Native seed 147: the drain precedes the pump. First emptying is at 79;
        // first transfer at 101, capacity admission at 133, second emptying at 147.
        use crate::fluids::{
            Cell,
            runtime::{Face, Host, Transport},
            transport::Source,
        };
        struct World {
            drain: Drain,
            tank: u32,
            clock: i64,
            kept: u32,
        }
        impl Host for World {
            fn cell(&self, p: [i32; 3], _: Dir) -> Cell {
                match p {
                    [0, 0, 0] | [4, 0, 0] => Cell::Tank,
                    [1, 0, 0] | [3, 0, 0] => Cell::Pipe(Dir::East.bit() | Dir::West.bit()),
                    [2, 0, 0] => Cell::Pump(Dir::East),
                    _ => Cell::Solid,
                }
            }
            fn speed(&self, _: [i32; 3]) -> f32 {
                32.
            }
            fn time(&self) -> i64 {
                self.clock
            }
            fn tank_fluids(&self, f: Face) -> Vec<Fluid> {
                if f.connected() == [0, 0, 0] {
                    self.drain.fluid.map(|(f, _)| vec![f]).unwrap_or_default()
                } else {
                    Vec::new()
                }
            }
            fn offer(
                &mut self,
                f: Face,
                _: &mut Source,
                n: u32,
                only: Option<Fluid>,
            ) -> Option<(Fluid, u32)> {
                if f.connected() != [0, 0, 0] {
                    return None;
                }
                self.drain
                    .fluid
                    .filter(|(f, _)| only.is_none_or(|wanted| wanted == *f))
                    .map(|(f, k)| (f, k.min(n)))
            }
            fn drain(&mut self, f: Face, _: &mut Source, _: Fluid, n: u32) -> u32 {
                if f.connected() == [0, 0, 0] {
                    super::drain(&mut self.drain, n).map_or(0, |(_, n)| n)
                } else {
                    0
                }
            }
            fn fill(&mut self, f: Face, _: &mut Source, _: Fluid, n: u32, execute: bool) -> u32 {
                if f.connected() != [4, 0, 0] {
                    return 0;
                }
                let n = n.min(8000 - self.tank);
                if execute {
                    self.tank += n;
                }
                n
            }
            fn keep_alive(&mut self, _: Face) {
                self.kept += 1;
            }
        }
        let mut world = World {
            drain: Drain::default(),
            tank: 0,
            clock: 0,
            kept: 0,
        };
        let mut transport = Transport::default();
        for x in 1..=3 {
            transport.register(
                [x, 0, 0],
                world.cell([x, 0, 0], Dir::Down),
                world.speed([x, 0, 0]),
            );
        }
        for clock in 0..225 {
            if clock == 60 || clock == 110 {
                assert_eq!(
                    insert(
                        &mut world.drain,
                        "minecraft:water_bucket",
                        1,
                        Dir::North,
                        &[]
                    ),
                    1
                );
            }
            if let Tick::Out(_) = tick(&mut world.drain, &[]) {
                world.drain.held = None;
            }
            world.clock = clock;
            transport.prepare(&world);
            for x in 1..=3 {
                transport.tick_pipe(&mut world, [x, 0, 0]);
            }
            let tank = world.tank;
            let amount = world.drain.fluid.map_or(0, |(_, n)| n);
            let completed = clock + 1;
            match completed {
                79 | 100 => assert_eq!((amount, tank), (1000, 0)),
                101 => assert_eq!((amount, tank), (984, 16)),
                132 => assert_eq!((amount, tank, world.drain.processing), (488, 512, 20)),
                133 => assert_eq!((amount, tank, world.drain.processing), (472, 528, 19)),
                147 => {
                    assert_eq!((amount, tank, world.drain.processing), (1248, 752, 5));
                    assert_eq!(world.drain.held.as_ref().unwrap().item, "minecraft:bucket");
                }
                _ => {}
            }
        }
    }

    #[test]
    fn final_emptying_turn_consumes_item_even_if_capacity_changed() {
        let mut d = Drain::default();
        insert(&mut d, "minecraft:water_bucket", 1, Dir::North, &[]);
        d.processing = 6;
        d.fluid = Some((Fluid::Water, 1490));
        assert_eq!(tick(&mut d, &[]), Tick::Emptied);
        assert_eq!(d.processing, 5);
        assert_eq!(d.fluid, Some((Fluid::Water, 1500)));
        assert_eq!(d.held.unwrap().item, "minecraft:bucket");
    }

    #[test]
    fn a_bucket_crosses_is_emptied_in_the_middle_and_leaves() {
        let mut d = Drain::default();
        assert_eq!(
            insert(&mut d, "minecraft:water_bucket", 2, Dir::East, &[]),
            1
        );
        assert_eq!(insert(&mut d, "minecraft:stone", 1, Dir::East, &[]), 0);
        // Four ticks to the middle, the 20-tick cycle emptying at its 5th-last.
        let events = run(&mut d, &[], 4 + 15);
        assert_eq!(events, vec![Tick::Emptied]);
        assert_eq!(d.fluid, Some((Fluid::Water, 1000)));
        assert_eq!(d.held.as_ref().unwrap().item, "minecraft:bucket");
        assert_eq!(d.held.as_ref().unwrap().position, 0.5);
        // The cycle's end, then four more ticks to the far side.
        let events = run(&mut d, &[], 5 + 5);
        assert_eq!(events, vec![Tick::Out(Dir::East)]);
    }

    #[test]
    fn it_waits_for_room_and_passes_other_items() {
        let mut d = Drain {
            fluid: Some((Fluid::Water, 1000)),
            ..Drain::default()
        };
        insert(&mut d, "minecraft:water_bucket", 1, Dir::North, &[]);
        assert!(run(&mut d, &[], 200).is_empty(), "no room: it waits");
        assert_eq!(drain(&mut d, 600), Some((Fluid::Water, 600)));
        assert_eq!(run(&mut d, &[], 40)[0], Tick::Emptied);
        let mut d = Drain::default();
        insert(&mut d, "minecraft:stone", 5, Dir::West, &[]);
        assert_eq!(d.held.as_ref().unwrap().count, 5);
        assert_eq!(run(&mut d, &[], 9), vec![Tick::Out(Dir::West)]);
    }

    #[test]
    fn honey_bottles_empty_by_their_recipe() {
        // data/create/recipe/emptying/honey_bottle.json
        let recipes = vec![(
            "minecraft:honey_bottle".to_owned(),
            Emptying {
                fluid: Fluid::Honey,
                amount: 250,
                leftover: Some("minecraft:glass_bottle".into()),
            },
        )];
        let mut d = Drain::default();
        assert_eq!(
            insert(&mut d, "minecraft:honey_bottle", 7, Dir::South, &recipes),
            1
        );
        assert_eq!(run(&mut d, &recipes, 30)[0], Tick::Emptied);
        assert_eq!(d.fluid, Some((Fluid::Honey, 250)));
        assert_eq!(d.held.as_ref().unwrap().item, "minecraft:glass_bottle");
    }
}

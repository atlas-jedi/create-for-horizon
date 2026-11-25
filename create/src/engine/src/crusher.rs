//! Create's crushing wheels (`CrushingWheelBlock`, `CrushingWheelControllerBlock`,
//! `CrushingWheelControllerBlockEntity`): two wheels turning against each other two blocks apart
//! keep a controller between them, which catches an item falling in, holds it while it sinks, takes
//! it in and crushes it (Create's crushing recipes, else milling), then lets the results out.

use crate::kinetics::{Axis, Dir};
use crate::processing::{Book, Rng, roll};

/// `updateControllers` from the wheel at `pos`'s side `side`: what the controller between it and
/// the wheel two blocks away should be. None: no controller (no wheel there, or one on another
/// axis); `Some((false, Down))`: one that doesn't crush (the wheels don't turn against each other).
pub fn controller(
    axis: Axis,
    side: Dir,
    speed: f32,
    other: Option<(Axis, f32)>,
) -> Option<(bool, Dir)> {
    if side.axis() == axis {
        return None;
    }
    let (other_axis, other_speed) = other?;
    if other_axis != axis {
        return None;
    }
    if (speed > 0.0) == (other_speed > 0.0) || speed == 0.0 || other_speed == 0.0 {
        return Some((false, Dir::Down));
    }
    // The wheel's axis crossed with the side's, signed by the wheel's turn and the side.
    let unit = |a: Axis| match a {
        Axis::X => [1, 0, 0],
        Axis::Y => [0, 1, 0],
        Axis::Z => [0, 0, 1],
    };
    let (w, s) = (unit(axis), unit(side.axis()));
    let cross = [
        w[1] * s[2] - w[2] * s[1],
        w[2] * s[0] - w[0] * s[2],
        w[0] * s[1] - w[1] * s[0],
    ];
    let sign = speed.signum() as i32 * if side.positive() { 1 } else { -1 };
    let v = cross.map(|c| c * sign);
    Some((true, Dir::from_offset(v).unwrap_or(Dir::Down)))
}

/// `updateSpeed`: the controller crushes at a wheel's speed / 50.
pub fn crushing_speed(wheel_speed: f32) -> f32 {
    (wheel_speed / 50.0).abs()
}

/// A controller's state: the item entity it holds, else what it took in and made.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Crusher {
    pub entity: Option<u32>,
    /// Slot 0 (the input), then up to 31 results.
    pub slots: Vec<(String, u8)>,
    pub remaining: f32,
    pub applied: bool,
}

impl Crusher {
    /// `isOccupied`.
    pub fn occupied(&self) -> bool {
        self.entity.is_some() || !self.slots.is_empty()
    }
}

/// `findRecipe`: crushing, else milling.
pub fn recipe<'a>(
    crushing: &'a Book,
    milling: &'a Book,
    item: &str,
) -> Option<&'a crate::processing::Recipe> {
    crushing
        .find_single(item)
        .map(|i| crushing.get(i))
        .or_else(|| milling.find_single(item).map(|i| milling.get(i)))
}

/// `intakeItem` + `itemInserted`: the stack goes into slot 0; its recipe's time, else 100.
pub fn intake(c: &mut Crusher, item: &str, count: u8, time: Option<i32>) {
    c.slots = vec![(item.to_owned(), count)];
    c.remaining = time.map_or(100.0, |t| t as f32);
    c.applied = false;
}

/// `ProcessingInventory.insertItem` through the controller's item handler: `isItemValid` takes
/// only slot 0 of an inventory holding nothing, and the controller's override only while it
/// holds no entity; up to a stack (`max`), then `itemInserted`. How many it took.
pub fn insert(
    c: &mut Crusher,
    slot: usize,
    item: &str,
    count: u8,
    max: u8,
    time: Option<i32>,
) -> u8 {
    if slot != 0 || c.occupied() {
        return 0;
    }
    let n = count.min(max);
    if n > 0 {
        intake(c, item, n, time);
    }
    n
}

/// What a tick of a controller without an entity does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Wait,
    /// Apply the recipe now (`remainingTime < 20`).
    Apply,
    /// Let the results out.
    Output,
}

/// The inventory's part of `tick`: the time runs down faster at speed, slower for big stacks
/// until the recipe is applied (by log2 of the count).
pub fn tick_inventory(c: &mut Crusher, crushing_speed: f32) -> Step {
    if c.slots.is_empty() || crushing_speed == 0.0 {
        return Step::Wait;
    }
    let speed = crushing_speed * 4.0;
    let count = c.slots.first().map_or(1, |s| s.1);
    let divisor = if c.applied {
        1.0
    } else {
        (f32::from(count)).ln() / 2f32.ln()
    };
    c.remaining -= (speed / divisor).clamp(0.25, 20.0);
    if c.remaining < 20.0 && !c.applied {
        c.applied = true;
        return Step::Apply;
    }
    if c.remaining > 0.0 {
        return Step::Wait;
    }
    c.remaining = 0.0;
    Step::Output
}

/// `applyRecipe`: each item of the input rolls the recipe's results, merged into stacks of at most
/// `max`, then the input's crafting remainder (`remainder`, once); no recipe and the input is
/// gone. Either way the inventory is cleared first (`ProcessingInventory.clear`), which zeroes
/// its time: the results leave on the next tick.
pub fn apply(
    c: &mut Crusher,
    recipe: Option<&crate::processing::Recipe>,
    max: impl Fn(&str) -> u8,
    rng: &mut Rng,
    remainder: Option<&str>,
) {
    let Some((_, count)) = c.slots.first().cloned() else {
        return;
    };
    c.remaining = 0.0;
    let mut list: Vec<(String, u8)> = Vec::new();
    if let Some(r) = recipe {
        for _ in 0..count {
            for (item, n) in roll(r, rng) {
                crate::processing::add_to_list(&mut list, item, n, &max);
            }
        }
        if let Some(r) = remainder {
            crate::processing::add_to_list(&mut list, r.to_owned(), 1, &max);
        }
    }
    list.truncate(31);
    c.slots = list;
}

/// The results thrown out as items (`inventory.clear()`): nothing held, no time, not applied.
pub fn ejected(c: &mut Crusher) -> Vec<(String, u8)> {
    c.remaining = 0.0;
    c.applied = false;
    std::mem::take(&mut c.slots)
}

/// `CrushingWheelControllerBlockEntity.tick`.
pub fn overlaps(controller: [i32; 3], inflate: f64, [min, max]: [[f64; 3]; 2]) -> bool {
    (0..3).all(|a| {
        let base = f64::from(controller[a]);
        min[a] < base + 1.0 + inflate && max[a] > base - inflate
    })
}

/// `CrushingWheelControllerBlock.entityInside`.
pub fn stuck_multiplier(facing: Dir) -> [f64; 3] {
    let mut multiplier = [0.25; 3];
    multiplier[axis_index(facing)] = f64::from(0.05_f32);
    multiplier
}

fn axis_index(facing: Dir) -> usize {
    match facing.axis() {
        Axis::X => 0,
        Axis::Y => 1,
        Axis::Z => 2,
    }
}

/// `CrushingWheelControllerBlockEntity.tick`.
pub fn hold(
    controller: [i32; 3],
    facing: Dir,
    crushing_speed: f32,
    at: [f64; 3],
    sneaking: bool,
) -> ([f64; 3], bool) {
    let centre = controller.map(|c| f64::from(c) + 0.5);
    let offset = if facing.positive() { 1.0 } else { -1.0 };
    let movement = f64::from((-(crushing_speed * 4.0) / 4.0).max(-0.5)) * -offset;
    let mut motion = [0.0; 3];
    if !sneaking {
        motion[0] = (f64::from(controller[0] as f32 + 0.5) - at[0]) / 2.0;
        motion[2] = (f64::from(controller[2] as f32 + 0.5) - at[2]) / 2.0;
    }
    let a = axis_index(facing);
    motion[a] = movement;
    let taken = at[a] * -offset < (centre[a] - 0.25) * -offset;
    (motion, taken)
}

/// Where (the dropped item's feet) and how fast the results leave (`outPos`, `outSpeed`): 0.55
/// past the middle along the facing, as a float.
pub fn output(controller: [i32; 3], facing: Dir) -> ([f64; 3], [f64; 3]) {
    let o = facing.offset();
    let pos = [0, 1, 2].map(|a| f64::from(controller[a]) + 0.5 + f64::from(0.55f32 * o[a] as f32));
    let speed = match facing {
        Dir::Up => [0.0, 0.5, 0.0],
        Dir::Down => [0.0; 3],
        _ => o.map(|v| 0.25 * f64::from(v)),
    };
    (pos, speed)
}

/// For a host whose controller collision can't be emptied for one entity
/// (`CrushingWheelControllerBlock.getCollisionShape` answers `Shapes.empty()` to the processing
/// entity and to an item tagged `BypassCrushingWheel`): whether a dropped item at `at` (its feet)
/// rests against the controller's collision for items (`CRUSHING_WHEEL_CONTROLLER_COLLISION`, 13
/// px from the facing side). It can't be drawn through it, so it is taken there.
pub fn on_shelf(controller: [i32; 3], facing: Dir, at: [f64; 3]) -> bool {
    const DEPTH: f64 = 13.0 / 16.0;
    const EPS: f64 = 0.05;
    let a = axis_index(facing);
    // The item's extent along the axis (0.25 square, feet at `at`).
    let (lo, hi) = if a == 1 {
        (at[1], at[1] + 0.25)
    } else {
        (at[a] - 0.125, at[a] + 0.125)
    };
    let base = f64::from(controller[a]);
    if facing.positive() {
        hi >= base + 1.0 - DEPTH - EPS
    } else {
        lo <= base + DEPTH + EPS
    }
}

/// For the same host: where the results start instead of [`output`]'s 0.55 past the middle, clear
/// of the controller's collision for items, or it would push them back in (Create's results
/// ignore that collision through `BypassCrushingWheel`). Along the facing 0.55 up or 0.68
/// sideways past the middle; facing down, their whole height (a quarter) below the controller.
pub fn output_clear(controller: [i32; 3], facing: Dir) -> [f64; 3] {
    let o = facing.offset();
    let mut pos = [0, 1, 2].map(|a| {
        let past = if a == 1 { 0.55 } else { 0.68 };
        f64::from(controller[a]) + 0.5 + past * f64::from(o[a])
    });
    if facing == Dir::Down {
        pos[1] = f64::from(controller[1]) - 0.3;
    }
    pos
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::processing::{Output, Recipe};

    #[test]
    fn an_item_resting_on_a_downward_controller_is_on_its_shelf() {
        let c = [0, 64, 0];
        // Resting on the 13 px collision for items.
        assert!(on_shelf(c, Dir::Down, [0.5, 64.8125, 0.5]));
        // Still falling through the top of the cell.
        assert!(!on_shelf(c, Dir::Down, [0.5, 64.95, 0.5]));
    }

    #[test]
    fn results_start_clear_of_the_controller_s_collision_for_items() {
        let near = |a: [f64; 3], b: [f64; 3]| (0..3).all(|i| (a[i] - b[i]).abs() < 1e-9);
        assert!(near(output_clear([0, 0, 0], Dir::Down), [0.5, -0.3, 0.5]));
        assert!(near(output_clear([0, 0, 0], Dir::East), [1.18, 0.5, 0.5]));
        assert!(near(output_clear([0, 0, 0], Dir::Up), [0.5, 1.05, 0.5]));
    }

    #[test]
    fn downward_intake_waits_until_the_item_crosses_the_threshold() {
        let pos = [0, 64, 0];
        let (motion, taken) = hold(pos, Dir::Down, 0.32, [0.75, 64.8125, 0.25], false);
        assert_eq!(motion, [-0.125, -f64::from(0.32_f32), 0.125]);
        assert!(!taken);
        assert!(!hold(pos, Dir::Down, 0.32, [0.5, 64.25, 0.5], false).1);
        assert!(hold(pos, Dir::Down, 0.32, [0.5, 64.24, 0.5], false).1);
    }

    #[test]
    fn intake_leaves_the_entity_for_the_next_ticks_loss_check() {
        let mut c = Crusher {
            entity: Some(42),
            ..Crusher::default()
        };
        intake(&mut c, "minecraft:iron_ore", 2, Some(250));
        assert_eq!(c.entity, Some(42));
        assert_eq!(c.slots, vec![("minecraft:iron_ore".into(), 2)]);
        assert_eq!(c.remaining, 250.0);
    }

    #[test]
    fn held_entity_is_lost_when_its_box_stops_overlapping() {
        let pos = [0, 64, 0];
        assert!(overlaps(pos, 0.5, [[0.25, 63.25, 0.25], [0.5, 63.51, 0.5]]));
        assert!(!overlaps(pos, 0.5, [[0.25, 63.25, 0.25], [0.5, 63.5, 0.5]]));
    }

    #[test]
    fn wheels_turning_against_each_other_crush_downward() {
        // Two z-axis wheels at x 0 and x 2 (Create: z × x = +y, signed by the turn and the side).
        assert_eq!(
            controller(Axis::Z, Dir::East, -16.0, Some((Axis::Z, 16.0))),
            Some((true, Dir::Down))
        );
        // Seen from the other wheel, the same direction.
        assert_eq!(
            controller(Axis::Z, Dir::West, 16.0, Some((Axis::Z, -16.0))),
            Some((true, Dir::Down))
        );
        // Turned the other way, they throw items up.
        assert_eq!(
            controller(Axis::Z, Dir::East, 16.0, Some((Axis::Z, -16.0))),
            Some((true, Dir::Up))
        );
        assert_eq!(
            controller(Axis::Z, Dir::East, 16.0, Some((Axis::Z, 16.0))),
            Some((false, Dir::Down))
        );
        assert_eq!(
            controller(Axis::Z, Dir::East, 16.0, Some((Axis::X, -16.0))),
            None
        );
        assert_eq!(
            controller(Axis::Z, Dir::South, 16.0, Some((Axis::Z, -16.0))),
            None
        );
    }

    #[test]
    fn its_handler_takes_a_stack_into_an_empty_controller_only() {
        // Seen in Create: 4 cobblestone into slot 0 of an empty controller at 64 rpm
        // (crushing speed 1.28) are all taken, with milling's 250 ticks; more cobblestone, gravel
        // or andesite into slot 0 or 1 of the busy one are refused; 3 dirt into slot 2 of the
        // empty one are refused.
        let mut c = Crusher::default();
        assert_eq!(insert(&mut c, 2, "minecraft:dirt", 3, 64, None), 0);
        assert!(!c.occupied());
        assert_eq!(
            insert(&mut c, 0, "minecraft:cobblestone", 4, 64, Some(250)),
            4
        );
        assert_eq!(c.slots, vec![("minecraft:cobblestone".to_owned(), 4)]);
        assert_eq!((c.remaining, c.applied), (250.0, false));
        assert_eq!(
            insert(&mut c, 0, "minecraft:cobblestone", 4, 64, Some(250)),
            0
        );
        assert_eq!(insert(&mut c, 0, "minecraft:gravel", 2, 64, Some(250)), 0);
        assert_eq!(insert(&mut c, 1, "minecraft:andesite", 2, 64, Some(200)), 0);
        assert_eq!(tick_inventory(&mut c, crushing_speed(64.0)), Step::Wait);
        assert!((c.remaining - 247.44).abs() < 1e-3);
        let mut big = Crusher::default();
        assert_eq!(
            insert(&mut big, 0, "minecraft:ender_pearl", 20, 16, None),
            16
        );
        assert_eq!(big.remaining, 100.0);
    }

    #[test]
    fn a_stack_is_crushed_then_let_out() {
        let ore = Recipe {
            id: "create:crushing/iron_ore".into(),
            ingredients: vec![vec!["minecraft:iron_ore".into()]],
            results: vec![Output {
                item: "create:crushed_raw_iron".into(),
                count: 1,
                chance: 1.0,
            }],
            time: 250,
        };
        let mut c = Crusher::default();
        intake(&mut c, "minecraft:iron_ore", 2, Some(250));
        // 64 rpm: crushing speed 1.28, 5.12 a tick over log2(2) = 1.
        let mut ticks = 0;
        while tick_inventory(&mut c, crushing_speed(64.0)) != Step::Apply {
            ticks += 1;
        }
        assert_eq!(ticks, 44, "250 - 45 × 5.12 < 20");
        apply(&mut c, Some(&ore), |_| 64, &mut Rng::new(1), None);
        assert_eq!(c.slots, vec![("create:crushed_raw_iron".to_owned(), 2)]);
        while tick_inventory(&mut c, crushing_speed(64.0)) != Step::Output {}
        // A single item divides by log2(1) = 0: the fastest, 20 a tick.
        let mut one = Crusher::default();
        intake(&mut one, "minecraft:stone", 1, None);
        tick_inventory(&mut one, 0.32);
        assert_eq!(one.remaining, 80.0);
    }

    #[test]
    fn results_leave_the_tick_after_they_are_made() {
        // Create: 2 obsidian (500 ticks) at 128 rpm (crushing speed 2.56, 10.24 a tick over
        // log2(2)) are crushed on the 47th tick and thrown out on the 48th, which leaves the
        // controller unapplied and without time.
        let obsidian = Recipe {
            id: "create:crushing/obsidian".into(),
            ingredients: vec![vec!["minecraft:obsidian".into()]],
            results: vec![Output {
                item: "create:powdered_obsidian".into(),
                count: 1,
                chance: 1.0,
            }],
            time: 500,
        };
        let mut c = Crusher::default();
        intake(&mut c, "minecraft:obsidian", 2, Some(500));
        let speed = crushing_speed(128.0);
        let mut ticks = 1;
        while tick_inventory(&mut c, speed) != Step::Apply {
            ticks += 1;
        }
        assert_eq!(ticks, 47);
        apply(&mut c, Some(&obsidian), |_| 64, &mut Rng::new(1), None);
        assert_eq!((c.remaining, c.applied), (0.0, true));
        assert_eq!(tick_inventory(&mut c, speed), Step::Output);
        assert_eq!(
            ejected(&mut c),
            vec![("create:powdered_obsidian".to_owned(), 2)]
        );
        assert_eq!((c.remaining, c.applied, c.occupied()), (0.0, false, false));
        // Without a recipe (dirt) the input is gone and the time with it.
        let mut dirt = Crusher::default();
        intake(&mut dirt, "minecraft:dirt", 12, None);
        while tick_inventory(&mut dirt, 0.32) != Step::Apply {}
        apply(&mut dirt, None, |_| 64, &mut Rng::new(1), None);
        assert_eq!(
            (dirt.remaining, dirt.applied, dirt.occupied()),
            (0.0, true, false)
        );
    }

    #[test]
    fn a_falling_item_is_held_and_taken() {
        let (v, taken) = hold([0, 0, 0], Dir::Down, 1.28, [0.7, 0.6, 0.5], false);
        assert_eq!(v, [-0.09999999999999998, -0.5, 0.0]);
        assert!(!taken);
        assert!(hold([0, 0, 0], Dir::Down, 1.28, [0.5, 0.2, 0.5], false).1);
        assert_eq!(
            output([0, 0, 0], Dir::Down),
            ([0.5, 0.5 - f64::from(0.55f32), 0.5], [0.0; 3])
        );
        assert_eq!(
            output([0, 0, 0], Dir::East),
            ([0.5 + f64::from(0.55f32), 0.5, 0.5], [0.25, 0.0, 0.0])
        );
    }
}

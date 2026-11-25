//! Create's mechanical saw facing up (`SawBlockEntity` processing): an item dropped on it is
//! cut by its cutting recipes (taking turns when several fit), then thrown off the way it turns.
//! A package is opened instead, its contents thrown off.

use crate::processing::{Recipe, Rng, roll};
use crate::sequenced::{Advanced, Assembly, Progress};

/// The result slots after the input's (`ProcessingInventory`'s 32 slots, slot 0 the input's).
pub const RESULTS: usize = 31;

/// The saw's inventory (`ProcessingInventory`): the input, then up to 31 results. Items are
/// named as the glue names stacks: an id, or an id with its components for a stack with some.
#[derive(Clone, Debug, PartialEq)]
pub struct Saw {
    pub slots: Vec<(String, u8)>,
    pub remaining: f32,
    pub applied: bool,
    pub index: usize,
    /// The sequenced assembly progress its items carry (`SequencedAssembly`), the input's
    /// before its step, the results' after.
    pub progress: Option<Progress>,
}

impl Default for Saw {
    fn default() -> Saw {
        Saw {
            slots: Vec::new(),
            remaining: -1.0,
            applied: false,
            index: 0,
            progress: None,
        }
    }
}

/// What a tick asks of the glue.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Wait,
    /// Start on the input with this many recipes fitting it (`start`).
    Start,
    /// Apply the current recipe now (`applyRecipe`).
    Apply,
    /// Let the results out (eject).
    Out,
}

impl Saw {
    /// `start`: with recipes, the next of them in turn, its time (`processing_time`) per 5 items;
    /// without, 10 ticks before the input goes on unchanged.
    pub fn start(&mut self, recipes: &[&Recipe]) {
        let count = self.slots.first().map_or(1, |s| i32::from(s.1));
        self.applied = false;
        if recipes.is_empty() {
            self.remaining = 10.0;
            return;
        }
        self.index += 1;
        if self.index >= recipes.len() {
            self.index = 0;
        }
        let time = recipes[self.index].time;
        self.remaining = (time * (count / 5).max(1)) as f32;
    }

    /// `ProcessingInventory.insertItem` into slot 0 of a saw holding nothing (`isItemValid`; a
    /// slot limit of one without `bulkCutting`), then its callback `start` on the `recipes` that
    /// fit, when the saw faces up (`canProcess`). How many it took. The item's sequenced
    /// progress is the caller's to set.
    pub fn insert(&mut self, slot: usize, item: &str, recipes: &[&Recipe], up: bool) -> u8 {
        if slot != 0 || !self.slots.is_empty() {
            return 0;
        }
        self.slots = vec![(item.to_owned(), 1)];
        self.applied = false;
        if up {
            self.start(recipes);
        }
        1
    }

    /// `tick` at `speed` rpm.
    pub fn tick(&mut self, speed: f32) -> Step {
        if speed == 0.0 {
            return Step::Wait;
        }
        if self.remaining == -1.0 {
            return if !self.slots.is_empty() && !self.applied {
                Step::Start
            } else {
                Step::Wait
            };
        }
        self.remaining -= (speed.abs() / 24.0).clamp(1.0, 128.0);
        if self.remaining < 5.0 && !self.applied {
            self.applied = true;
            self.remaining = 20.0;
            return Step::Apply;
        }
        if self.remaining > 0.0 {
            return Step::Wait;
        }
        self.remaining = 0.0;
        Step::Out
    }

    /// `applyRecipe` with the recipe in turn: each item rolls its results, merged into stacks of
    /// at most `max`. No recipe: the input stays as it is.
    /// `remainder`: the input's crafting remainder (a bucket's), given back with each roll.
    pub fn apply(
        &mut self,
        recipe: Option<&Recipe>,
        max: impl Fn(&str) -> u8,
        rng: &mut Rng,
        remainder: Option<&str>,
    ) {
        let (Some(recipe), Some((_, count))) = (recipe, self.slots.first().cloned()) else {
            return;
        };
        let mut list: Vec<(String, u8)> = Vec::new();
        for _ in 0..count {
            for (item, n) in roll(recipe, rng) {
                crate::processing::add_to_list(&mut list, item, n, &max);
            }
            if let Some(r) = remainder {
                crate::processing::add_to_list(&mut list, r.to_owned(), 1, &max);
            }
        }
        list.truncate(RESULTS);
        self.slots = list;
        self.progress = None;
    }

    /// `applyRecipe` for a package (`PackageItem.isPackage`, ahead of any recipe): the package
    /// goes (`inventory.clear()`) and its `contents` take the result slots, merged into stacks
    /// of at most `max` (`ItemHelper.addToList`). An empty package leaves nothing.
    pub fn unpack(&mut self, contents: &[(String, u8)], max: impl Fn(&str) -> u8) {
        let mut list: Vec<(String, u8)> = Vec::new();
        for (item, n) in contents {
            crate::processing::add_to_list(&mut list, item.clone(), *n, &max);
        }
        list.truncate(RESULTS);
        self.slots = list;
        self.progress = None;
    }

    /// After ejecting: empty, waiting (`remainingTime = -1`).
    pub fn clear(&mut self) {
        self.slots.clear();
        self.remaining = -1.0;
        self.progress = None;
    }

    /// `applyRecipe` with a sequenced assembly's cutting step (`enforceNextResult`): each of the
    /// input `advance`s, to the transitional item a step on or a result of the pool.
    pub fn apply_assembly(&mut self, a: &Assembly, rng: &mut Rng) {
        let Some((_, count)) = self.slots.first().cloned() else {
            return;
        };
        let before = self.progress.take();
        let mut list: Vec<(String, u8)> = Vec::new();
        for _ in 0..count {
            let (item, n) = match a.advance(before.as_ref(), rng.next_float()) {
                Advanced::Next { item, progress, .. } => {
                    self.progress = Some(progress);
                    (item, 1)
                }
                Advanced::Done { item, count } => (item, count),
                Advanced::Nothing => continue,
            };
            match list.iter_mut().find(|(i, _)| *i == item) {
                Some((_, m)) => *m = m.saturating_add(n),
                None => list.push((item, n)),
            }
        }
        self.slots = list;
        self.slots.truncate(RESULTS);
    }
}

/// `getItemMovementVec`: along x unless `axis_along_first`, the sign of the speed.
pub fn movement(along_first: bool, speed: f32) -> [i32; 3] {
    let s = if speed < 0.0 { -1 } else { 1 };
    if !along_first { [s, 0, 0] } else { [0, 0, -s] }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::processing::Output;

    #[test]
    fn a_log_is_cut_into_planks_and_thrown_off() {
        let log = Recipe {
            id: "create:cutting/oak_log".into(),
            ingredients: vec![vec!["minecraft:oak_log".into()]],
            results: vec![Output {
                item: "minecraft:stripped_oak_log".into(),
                count: 1,
                chance: 1.0,
            }],
            time: 50,
        };
        let mut s = Saw {
            slots: vec![("minecraft:oak_log".into(), 2)],
            ..Saw::default()
        };
        assert_eq!(s.tick(32.0), Step::Start);
        s.start(&[&log]);
        assert_eq!(s.remaining, 50.0);
        let mut ticks = 0;
        while s.tick(32.0) != Step::Apply {
            ticks += 1;
        }
        // 32 rpm: 1.33 a tick, 50 → under 5 after 34 ticks.
        assert_eq!(ticks, 33);
        s.apply(Some(&log), |_| 64, &mut Rng::new(1), None);
        assert_eq!(s.slots, vec![("minecraft:stripped_oak_log".to_owned(), 2)]);
        let mut out = 0;
        while s.tick(32.0) != Step::Out {
            out += 1;
        }
        assert_eq!(out, 15);
        assert_eq!(movement(false, -8.0), [-1, 0, 0]);
        // An input with a crafting remainder gives it back with each roll.
        let mut s = Saw {
            slots: vec![("minecraft:oak_log".into(), 2)],
            ..Saw::default()
        };
        s.apply(
            Some(&log),
            |_| 64,
            &mut Rng::new(1),
            Some("minecraft:bucket"),
        );
        assert_eq!(
            s.slots,
            vec![
                ("minecraft:stripped_oak_log".to_owned(), 2),
                ("minecraft:bucket".to_owned(), 2)
            ]
        );
    }

    #[test]
    fn an_item_put_in_starts_its_cut_at_once() {
        // Create: a stripped oak log dropped on an upward saw at 48 rpm has 50 left at once and
        // 48 after the next tick; it is cut on the 23rd tick and thrown off on the 33rd.
        let planks = Recipe {
            id: "create:cutting/runtime_generated/compat/minecraft/stripped_oak_log_to_oak_planks"
                .into(),
            ingredients: vec![vec!["minecraft:stripped_oak_log".into()]],
            results: vec![Output {
                item: "minecraft:oak_planks".into(),
                count: 6,
                chance: 1.0,
            }],
            time: 50,
        };
        let mut s = Saw::default();
        assert_eq!(
            s.insert(0, "minecraft:stripped_oak_log", &[&planks], true),
            1
        );
        assert_eq!((s.remaining, s.index), (50.0, 0));
        assert_eq!(s.insert(0, "minecraft:dirt", &[], true), 0);
        let mut ticks = 1;
        while s.tick(48.0) != Step::Apply {
            ticks += 1;
        }
        assert_eq!(ticks, 23);
        s.apply(Some(&planks), |_| 64, &mut Rng::new(1), None);
        while s.tick(48.0) != Step::Out {
            ticks += 1;
        }
        assert_eq!(ticks + 1, 33);
        // Not facing up, it holds the item without a cut; no recipe, 10 ticks.
        let mut side = Saw::default();
        assert_eq!(side.insert(0, "minecraft:oak_log", &[&planks], false), 1);
        assert_eq!(side.remaining, -1.0);
        let mut none = Saw::default();
        assert_eq!(none.insert(2, "minecraft:dirt", &[], true), 0);
        none.insert(0, "minecraft:dirt", &[], true);
        assert_eq!(none.remaining, 10.0);
    }

    #[test]
    fn a_package_is_opened_into_its_contents() {
        // Create: no cutting recipe takes a package, so it waits 10 ticks (`start`), then
        // `applyRecipe` opens it instead of cutting: its stacks, merged, are the results.
        let package = "create:cardboard_package_12x12#0a";
        let mut s = Saw::default();
        assert_eq!(s.insert(0, package, &[], true), 1);
        assert_eq!(s.remaining, 10.0);
        let mut ticks = 1;
        while s.tick(32.0) != Step::Apply {
            ticks += 1;
        }
        assert_eq!(ticks, 4, "10 at 1.33 a tick: under 5 on the 4th tick");
        let contents = [
            ("minecraft:cobblestone".to_owned(), 40),
            ("minecraft:cobblestone".to_owned(), 40),
            ("minecraft:ender_pearl".to_owned(), 10),
            ("minecraft:ender_pearl".to_owned(), 10),
            ("minecraft:stone#0b".to_owned(), 3),
        ];
        let max = |i: &str| if i == "minecraft:ender_pearl" { 16 } else { 64 };
        s.unpack(&contents, max);
        assert_eq!(
            s.slots,
            vec![
                ("minecraft:cobblestone".to_owned(), 64),
                ("minecraft:cobblestone".to_owned(), 16),
                ("minecraft:ender_pearl".to_owned(), 16),
                ("minecraft:ender_pearl".to_owned(), 4),
                ("minecraft:stone#0b".to_owned(), 3),
            ]
        );
        assert!(s.applied);
        // Thrown off as any cut is.
        while s.tick(32.0) != Step::Out {}
        // An empty package leaves nothing.
        let mut empty = Saw {
            slots: vec![(package.to_owned(), 1)],
            ..Saw::default()
        };
        empty.unpack(&[], |_| 64);
        assert!(empty.slots.is_empty());
    }

    #[test]
    fn a_cutting_step_advances_the_assembly() {
        use crate::sequenced::{Outcome, Step as S};
        let step = |kind: &str| S {
            kind: kind.into(),
            with: None,
            fluid: None,
            keep_held: false,
            time: 50,
        };
        let a = Assembly {
            id: "x:cut".into(),
            ingredient: vec!["minecraft:oak_planks".into()],
            transitional: "x:incomplete".into(),
            steps: vec![step("create:cutting"), step("create:cutting")],
            loops: 1,
            results: vec![Outcome {
                item: "x:done".into(),
                count: 2,
                weight: 1.0,
            }],
        };
        let mut rng = Rng::new(1);
        let mut saw = Saw {
            slots: vec![("minecraft:oak_planks".into(), 1)],
            ..Saw::default()
        };
        saw.apply_assembly(&a, &mut rng);
        assert_eq!(saw.slots, vec![("x:incomplete".to_string(), 1)]);
        assert_eq!(saw.progress.as_ref().map(|p| p.step), Some(1));
        saw.apply_assembly(&a, &mut rng);
        assert_eq!(saw.slots, vec![("x:done".to_string(), 2)]);
        assert_eq!(saw.progress, None);
    }
}

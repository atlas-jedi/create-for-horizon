//! Create's processing recipes (`ProcessingRecipe`, `ProcessingOutput`) as typed tables, and the
//! machines that run them. Recipes are read once per world load or `/reload` into a `Book`
//! indexed by input item; nothing reads recipe JSON during a tick.

use std::collections::HashMap;

/// `ProcessingOutput`: an item, a count, and the chance each of the count is made.
#[derive(Clone, Debug, PartialEq)]
pub struct Output {
    pub item: String,
    pub count: u8,
    pub chance: f32,
}

/// One processing recipe: its ingredients (each a list of items any of which fits), results
/// and `processing_time` (0 when the recipe has none, as Create reads it).
#[derive(Clone, Debug, PartialEq)]
pub struct Recipe {
    pub id: String,
    pub ingredients: Vec<Vec<String>>,
    pub results: Vec<Output>,
    pub time: i32,
}

/// The recipes of one type, indexed by the items their first ingredient takes.
#[derive(Clone, Debug, Default)]
pub struct Book {
    pub recipes: Vec<Recipe>,
    by_item: HashMap<String, Vec<u32>>,
}

impl Book {
    pub fn new(recipes: Vec<Recipe>) -> Book {
        let mut by_item: HashMap<String, Vec<u32>> = HashMap::new();
        for (i, r) in recipes.iter().enumerate() {
            if let Some(first) = r.ingredients.first() {
                for item in first {
                    by_item.entry(item.clone()).or_default().push(i as u32);
                }
            }
        }
        Book { recipes, by_item }
    }

    /// The first recipe (in data order) a single input item fits: one hash lookup.
    pub fn find_single(&self, item: &str) -> Option<u32> {
        self.by_item
            .get(item)?
            .iter()
            .copied()
            .find(|&i| self.recipes[i as usize].ingredients.len() == 1)
    }

    /// Every single-ingredient recipe the item fits, in data order (`firstIngredientMatches`).
    pub fn matching(&self, item: &str) -> Vec<u32> {
        self.by_item.get(item).map_or_else(Vec::new, |v| {
            v.iter()
                .copied()
                .filter(|&i| self.recipes[i as usize].ingredients.len() == 1)
                .collect()
        })
    }

    pub fn get(&self, id: u32) -> &Recipe {
        &self.recipes[id as usize]
    }

    pub fn is_empty(&self) -> bool {
        self.recipes.is_empty()
    }
}

/// A small random source for result rolls (xorshift64*). Create rolls with the level's random;
/// the odds are the same, the sequence is not.
#[derive(Clone, Debug)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Rng {
        Rng(seed | 1)
    }
    pub fn next_float(&mut self) -> f32 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        let v = self.0.wrapping_mul(0x2545_F491_4F6C_DD1D);
        (v >> 40) as f32 / (1u64 << 24) as f32
    }
}

/// `ProcessingRecipe.rollResults`: each output's count, less one for each roll above its chance.
pub fn roll(recipe: &Recipe, rng: &mut Rng) -> Vec<(String, u8)> {
    recipe
        .results
        .iter()
        .filter_map(|o| {
            let count = if o.chance < 1.0 {
                (0..o.count)
                    .filter(|_| rng.next_float() <= o.chance)
                    .count() as u8
            } else {
                o.count
            };
            (count > 0).then(|| (o.item.clone(), count))
        })
        .collect()
}

/// A slot: an item and its count, with the item's stack limit.
pub type Slot = Option<(String, u8)>;

/// `ProcessingInventory`'s slots (`super(32)`).
pub const INVENTORY_SLOTS: usize = 32;

/// A crushing wheel controller's or saw's `ProcessingInventory` slot by slot, as
/// `ItemHelper.dropContents` walks it: the input in slot 0, the results from slot 1 once applied.
pub fn inventory_slots(slots: Vec<(String, u8)>, applied: bool) -> Vec<Slot> {
    let mut out = vec![None; INVENTORY_SLOTS];
    for (slot, stack) in out.iter_mut().skip(usize::from(applied)).zip(slots) {
        *slot = Some(stack);
    }
    out
}

/// `ItemHandlerHelper.insertItemStacked`: onto stacks of the same item first, then into empty
/// slots. Returns what did not fit (Create's machines throw it away).
pub fn insert_stacked(slots: &mut [Slot], item: &str, mut count: u8, max: u8) -> u8 {
    for (it, n) in slots.iter_mut().flatten() {
        if count == 0 {
            break;
        }
        if it == item && *n < max {
            let moved = count.min(max - *n);
            *n += moved;
            count -= moved;
        }
    }
    for slot in slots.iter_mut() {
        if count == 0 {
            break;
        }
        if slot.is_none() {
            let moved = count.min(max);
            *slot = Some((item.to_owned(), moved));
            count -= moved;
        }
    }
    count
}

/// `MillstoneBlockEntity`'s state: its timer and the recipe it last found.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Millstone {
    pub timer: i32,
    pub recipe: Option<u32>,
}

/// `MillstoneBlockEntity.getProcessingSpeed`.
pub fn processing_speed(speed: f32) -> i32 {
    ((speed / 16.0).abs() as i32).clamp(1, 512)
}

/// One tick of `MillstoneBlockEntity.tick` on the server: returns the recipe to process now
/// (`process()`), with its input taken and its results rolled by the caller. `input` is the
/// input slot's item; `outputs_full` whether any output slot is at its limit.
pub fn tick_millstone(
    m: &mut Millstone,
    speed: f32,
    input: Option<&str>,
    outputs_full: bool,
    book: &Book,
) -> Option<u32> {
    if speed == 0.0 || outputs_full {
        return None;
    }
    if m.timer > 0 {
        m.timer -= processing_speed(speed);
        if m.timer <= 0 {
            // process(): the last recipe, or the one the input fits now.
            let item = input?;
            let fits = |r: u32| book.get(r).ingredients[0].iter().any(|i| i == item);
            return match m.recipe.filter(|&r| fits(r)) {
                Some(r) => Some(r),
                None => {
                    m.recipe = book.find_single(item);
                    m.recipe
                }
            };
        }
        return None;
    }
    let item = input?;
    let fits = m
        .recipe
        .is_some_and(|r| book.get(r).ingredients[0].iter().any(|i| i == item));
    if !fits {
        match book.find_single(item) {
            None => m.timer = 100,
            Some(r) => {
                m.recipe = Some(r);
                m.timer = book.get(r).time;
            }
        }
        return None;
    }
    m.timer = book.get(m.recipe.unwrap()).time;
    None
}

/// `ItemHelper.addToList`: `n` of `item` onto the stacks of it in `list` (up to `max` each),
/// the rest as a new stack.
pub fn add_to_list(
    list: &mut Vec<(String, u8)>,
    item: String,
    mut n: u8,
    max: &impl Fn(&str) -> u8,
) {
    for (have, m) in list.iter_mut() {
        if n > 0 && *have == item {
            let moved = n.min(max(have).saturating_sub(*m));
            *m += moved;
            n -= moved;
        }
    }
    if n > 0 {
        list.push((item, n));
    }
}

/// `MillstoneBlockEntity.tickAudio` and `CrushingWheelControllerBlockEntity.tickAudio`: the pitch
/// their ambient noise asks for, `|speed| / 256 + .45` held within .85 and 1 (a crushing
/// wheel controller's speed is a wheel's / 50, so it always asks for .85).
pub fn machine_pitch(speed: f32) -> f32 {
    (speed.abs() / 256.0 + 0.45).clamp(0.85, 1.0)
}

/// `SoundScapes.getGroupFromPitch`: the band an ambient sound's pitch falls in, very low (0) to
/// very high (4); sources of one group and band share one sound.
pub fn pitch_group(pitch: f32) -> u8 {
    match pitch {
        p if p < 0.70 => 0,
        p if p < 0.90 => 1,
        p if p < 1.10 => 2,
        p if p < 1.30 => 3,
        _ => 4,
    }
}

/// `SoundScapes.MAX_AMBIENT_SOURCE_DISTANCE`: sources farther from the camera are not heard.
pub const AMBIENT_DISTANCE: f64 = 16.0;

/// `SoundScape.getVolume`: `count` sources sharing a sound heard `distance` blocks from where
/// they stand on average: their count over 150, held within .025 and Create's default
/// `ambientVolumeCap` (.1), times 2 fading to 0 at 16 blocks (`Mth.lerp`, not held, so a sound
/// farther away asks for no volume).
pub fn scape_volume(count: usize, distance: f64) -> f32 {
    let near = 2.0 - 2.0 * distance / AMBIENT_DISTANCE;
    (count as f32 / 150.0).clamp(0.025, 0.1) * near as f32
}

/// How many times a tick on average the game calls a block's `animateTick` (`ClientLevel
/// .animateTick`: 667 tries a tick, each picking one block `nextInt(16) - nextInt(16)` and one
/// `nextInt(32) - nextInt(32)` away along each axis from the player's block) for a block
/// `offset` away. Used for the client effects of a block that has a client tick but whose
/// `animateTick` the game can't call here.
pub fn animate_tick_chance(offset: [i32; 3]) -> f64 {
    // `nextInt(r) - nextInt(r)` is `d` with odds (r - |d|) / r².
    let odds = |r: i32| {
        offset
            .iter()
            .map(|&d| f64::from((r - d.abs()).max(0)) / f64::from(r * r))
            .product::<f64>()
    };
    667.0 * (odds(16) + odds(32))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ambient_noise_follows_create_s_soundscapes() {
        assert_eq!(machine_pitch(0.0), 0.85);
        assert_eq!(machine_pitch(5.12), 0.85, "a controller crushing at 256 rpm");
        assert_eq!(machine_pitch(-256.0), 1.0);
        assert_eq!(pitch_group(0.85), 1);
        assert_eq!(pitch_group(1.0), 2);
        assert_eq!(pitch_group(0.5), 0);
        // One source next to the camera: the least volume, doubled; none past 16 blocks.
        assert_eq!(scape_volume(1, 0.0), 0.05);
        assert_eq!(scape_volume(300, 8.0), 0.1);
        assert!(scape_volume(1, 20.0) < 0.0);
    }

    #[test]
    fn blocks_near_the_player_are_animated_more_often() {
        let at = animate_tick_chance([0, 0, 0]);
        assert!((at - 667.0 * (1.0 / 4096.0 + 1.0 / 32768.0)).abs() < 1e-12);
        let near = animate_tick_chance([8, 0, 0]);
        assert!(near < at && near > 0.09, "{near}");
        // Only the wider tries reach 16 blocks and more; none reach 32.
        assert!(animate_tick_chance([20, 0, 0]) < 0.02);
        assert_eq!(animate_tick_chance([32, 0, 0]), 0.0);
    }

    #[test]
    fn an_applied_inventory_keeps_its_results_after_the_input_slot() {
        let gravel = ("minecraft:gravel".to_owned(), 6);
        let before = inventory_slots(vec![gravel.clone()], false);
        assert_eq!(before.len(), INVENTORY_SLOTS);
        assert_eq!(before[0], Some(gravel.clone()));
        let after = inventory_slots(vec![gravel.clone()], true);
        assert_eq!((&after[0], &after[1]), (&None, &Some(gravel)));
        assert!(after[2..].iter().all(Option::is_none));
    }

    fn wheat_book() -> Book {
        // data/create/recipe/milling/wheat.json
        Book::new(vec![Recipe {
            id: "create:milling/wheat".into(),
            ingredients: vec![vec!["minecraft:wheat".into()]],
            results: vec![
                Output {
                    item: "create:wheat_flour".into(),
                    count: 1,
                    chance: 1.0,
                },
                Output {
                    item: "create:wheat_flour".into(),
                    count: 2,
                    chance: 0.25,
                },
                Output {
                    item: "minecraft:wheat_seeds".into(),
                    count: 1,
                    chance: 0.25,
                },
            ],
            time: 150,
        }])
    }

    #[test]
    fn a_millstone_mills_wheat_in_150_ticks_of_work() {
        let book = wheat_book();
        let mut m = Millstone::default();
        // First tick with input: it finds the recipe and sets its timer.
        assert_eq!(
            tick_millstone(&mut m, 32.0, Some("minecraft:wheat"), false, &book),
            None
        );
        assert_eq!(m.timer, 150);
        // 32 RPM works 2 a tick: 75 more ticks.
        let mut ticks = 0;
        let done = loop {
            ticks += 1;
            if let Some(r) = tick_millstone(&mut m, 32.0, Some("minecraft:wheat"), false, &book) {
                break r;
            }
        };
        assert_eq!((ticks, done), (75, 0));
        // Stopped, or with a full output, nothing happens.
        let before = m;
        assert_eq!(
            tick_millstone(&mut m, 0.0, Some("minecraft:wheat"), false, &book),
            None
        );
        assert_eq!(
            tick_millstone(&mut m, 16.0, Some("minecraft:wheat"), true, &book),
            None
        );
        assert_eq!(m, before);
        // An item no recipe takes waits 100.
        let mut n = Millstone::default();
        tick_millstone(&mut n, 16.0, Some("minecraft:dirt"), false, &book);
        assert_eq!(n.timer, 100);
    }

    #[test]
    fn rolls_keep_create_s_odds() {
        let book = wheat_book();
        let mut rng = Rng::new(7);
        let mut flour = 0u32;
        let mut seeds = 0u32;
        for _ in 0..4000 {
            for (item, n) in roll(book.get(0), &mut rng) {
                match item.as_str() {
                    "create:wheat_flour" => flour += u32::from(n),
                    _ => seeds += u32::from(n),
                }
            }
        }
        // Expected: 1 + 2 * 0.25 = 1.5 flour, 0.25 seeds a wheat.
        assert!((5700..6300).contains(&flour), "{flour}");
        assert!((850..1150).contains(&seeds), "{seeds}");
    }

    #[test]
    fn stacked_insertion_fills_matching_stacks_first() {
        let mut slots: Vec<Slot> = vec![None, Some(("a".into(), 60)), None];
        assert_eq!(insert_stacked(&mut slots, "a", 10, 64), 0);
        assert_eq!(
            slots,
            vec![Some(("a".into(), 6)), Some(("a".into(), 64)), None]
        );
        assert_eq!(insert_stacked(&mut slots, "b", 200, 64), 200 - 64);
    }
}

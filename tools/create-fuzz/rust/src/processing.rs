//! Processing machines: the millstone, the crushing wheels and the upward saw (see
//! docs/processing-scope.md). Their observation, its comparison, the recipe subset both sides
//! run (`processing.json`, audited by the server) and the scenario families.
use crate::{
    actions::{Action, Face},
    generate::Rng,
    scenario::{Change, Placed, Pos, Scenario, Snapshot},
    transport::{Input, Stack},
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// One non-empty slot of a machine's inventory.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Slot {
    pub item: String,
    pub count: u8,
    pub slot: u8,
}

/// A machine's state (`ProcessingObservations.observe`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Observation {
    pub kind: String,
    /// `MillstoneBlockEntity.timer`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timer: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input: Option<Vec<Slot>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outputs: Option<Vec<Slot>>,
    /// `CrushingWheelControllerBlockEntity.crushingspeed` and whether it holds an entity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speed: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entity: Option<bool>,
    /// A `ProcessingInventory`: its slots, `remainingTime`, `recipeDuration`, `appliedRecipe`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slots: Option<Vec<Slot>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remaining: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub applied: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recipe_index: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filter: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_tanks: Option<Vec<Option<TankFluid>>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_tanks: Option<Vec<Option<TankFluid>>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub running: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub running_ticks: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub processing_ticks: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TankFluid {
    pub fluid: String,
    pub amount: i32,
}

#[derive(Debug, Deserialize)]
pub struct Manifest {
    pub inputs: BTreeMap<String, Vec<String>>,
    pub recipes: Vec<ManifestRecipe>,
}

#[derive(Debug, Deserialize)]
pub struct ManifestRecipe {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub input: String,
    pub time: i32,
    pub results: Vec<ManifestOutput>,
}

#[derive(Debug, Deserialize)]
pub struct ManifestOutput {
    pub item: String,
    pub count: u8,
    pub chance: f32,
}

pub fn contract() -> &'static Manifest {
    static M: std::sync::OnceLock<Manifest> = std::sync::OnceLock::new();
    M.get_or_init(|| {
        serde_json::from_str(include_str!("processing.json")).expect("processing.json")
    })
}

/// The recipes of `kind` as the engine's book, in the manifest's order.
pub fn book(kind: &str) -> create_engine::processing::Book {
    use create_engine::processing::{Book, Output, Recipe};
    Book::new(
        contract()
            .recipes
            .iter()
            .filter(|r| r.kind == kind)
            .map(|r| Recipe {
                id: r.id.clone(),
                ingredients: vec![vec![r.input.clone()]],
                results: r
                    .results
                    .iter()
                    .map(|o| Output {
                        item: o.item.clone(),
                        count: o.count,
                        chance: o.chance,
                    })
                    .collect(),
                time: r.time,
            })
            .collect(),
    )
}

/// What `level.random` decides in a case (`ProcessingRecipe.rollResults`): the results of the
/// rolled recipes whose input the case offered. An item a rolled recipe makes only by chance may
/// or may not be there (`maybe`); one it makes for sure is there in a count of chance (`counted`).
#[derive(Debug, Default)]
pub struct Rolls {
    counted: std::collections::BTreeSet<String>,
    maybe: std::collections::BTreeSet<String>,
}

impl Rolls {
    pub fn of(a: &Snapshot, b: &Snapshot) -> Option<Rolls> {
        let offered: std::collections::BTreeSet<&str> = a
            .item_outcomes
            .iter()
            .chain(&b.item_outcomes)
            .filter_map(|o| o.offered.as_ref().map(|s| s.item.as_str()))
            .collect();
        let active: Vec<&ManifestRecipe> = contract()
            .recipes
            .iter()
            .filter(|r| offered.contains(r.input.as_str()))
            .collect();
        let mut rolls = Rolls::default();
        for r in active
            .iter()
            .filter(|r| r.results.iter().any(|o| o.chance < 1.0))
        {
            for o in &r.results {
                rolls.counted.insert(o.item.clone());
            }
        }
        for r in &active {
            for o in &r.results {
                let sure = r
                    .results
                    .iter()
                    .any(|x| x.item == o.item && x.chance >= 1.0);
                if !sure && rolls.counted.contains(&o.item) {
                    rolls.maybe.insert(o.item.clone());
                }
            }
        }
        for m in &rolls.maybe {
            rolls.counted.remove(m);
        }
        (!rolls.counted.is_empty() || !rolls.maybe.is_empty()).then_some(rolls)
    }

    fn touches(&self, item: &str) -> bool {
        self.counted.contains(item) || self.maybe.contains(item)
    }

    /// Whether a rolled stack sits outside the kept input slot.
    fn holds_rolled(&self, v: &[Slot], keep_input: bool) -> bool {
        v.iter()
            .any(|x| self.touches(&x.item) && !(keep_input && x.slot == 0))
    }

    /// Stacks with what chance decides taken out: counts of `counted` items zeroed, `maybe` ones
    /// gone, duplicates merged.
    fn stacks(&self, v: &mut Vec<Stack>) {
        v.retain(|s| !self.maybe.contains(&s.item));
        for s in v.iter_mut() {
            if self.counted.contains(&s.item) {
                s.count = 0;
            }
        }
        v.sort_by(|a, b| (&a.item, a.count).cmp(&(&b.item, b.count)));
        v.dedup();
    }

    /// Slots with what chance decides taken out: the input's slot 0 kept (`keep_input`), the
    /// rest by stack, duplicates merged.
    fn unslot(&self, v: &mut Vec<Slot>, keep_input: bool) {
        let mut out: Vec<Slot> = v
            .iter()
            .filter(|x| keep_input && x.slot == 0)
            .cloned()
            .collect();
        let mut rest: Vec<Stack> = v
            .iter()
            .filter(|x| !(keep_input && x.slot == 0))
            .map(|x| Stack {
                item: x.item.clone(),
                count: if self.touches(&x.item) { 0 } else { x.count },
            })
            .filter(|x| !self.maybe.contains(&x.item))
            .collect();
        rest.sort_by(|a, b| (&a.item, a.count).cmp(&(&b.item, b.count)));
        rest.dedup();
        out.extend(rest.into_iter().map(|s| Slot {
            item: s.item,
            count: s.count,
            slot: u8::MAX,
        }));
        *v = out;
    }

    /// The slots rolls can sit in, and whether slot 0 is the input's, kept.
    fn slots(o: &Observation) -> Option<(&[Slot], bool)> {
        match o.kind.as_str() {
            "millstone" => Some((o.outputs.as_deref()?, false)),
            "crusher" | "saw" => Some((o.slots.as_deref()?, o.applied != Some(true))),
            _ => None,
        }
    }

    fn slots_mut(o: &mut Observation) -> Option<(&mut Vec<Slot>, bool)> {
        match o.kind.as_str() {
            "millstone" => Some((o.outputs.as_mut()?, false)),
            "crusher" | "saw" => Some((o.slots.as_mut()?, o.applied != Some(true))),
            _ => None,
        }
    }

    fn slots_together(&self, x: Option<&mut Observation>, y: Option<&mut Observation>) {
        let rolled = [x.as_deref(), y.as_deref()]
            .into_iter()
            .flatten()
            .any(|o| Self::slots(o).is_some_and(|(v, keep)| self.holds_rolled(v, keep)));
        if !rolled {
            return;
        }
        for o in [x, y].into_iter().flatten() {
            if let Some((v, keep_input)) = Self::slots_mut(o) {
                self.unslot(v, keep_input);
            }
        }
    }

    /// Both snapshots, and their checkpoints, with what chance decides taken out. A machine's
    /// slots are read by stack when either side holds a rolled one outside the input slot:
    /// chance decided which slots the rest sit in, whether or not a rolled stack is left.
    pub fn normalize(&self, a: &Snapshot, b: &Snapshot) -> (Snapshot, Snapshot) {
        let (mut a, mut b) = (a.clone(), b.clone());
        self.normalize_in(&mut a, Some(&mut b));
        (a, b)
    }

    fn normalize_in(&self, a: &mut Snapshot, mut b: Option<&mut Snapshot>) {
        self.by_stack(a);
        if let Some(b) = b.as_deref_mut() {
            self.by_stack(b);
        }
        for (key, x) in a.blocks.iter_mut() {
            let y = b
                .as_deref_mut()
                .and_then(|b| b.blocks.get_mut(key))
                .and_then(|y| y.processing.as_mut());
            self.slots_together(x.processing.as_mut(), y);
        }
        if let Some(b) = b.as_deref_mut() {
            for (key, y) in b.blocks.iter_mut() {
                if !a.blocks.contains_key(key) {
                    self.slots_together(None, y.processing.as_mut());
                }
            }
        }
        for (t, x) in a.checkpoints.iter_mut() {
            let y = b.as_deref_mut().and_then(|b| b.checkpoints.get_mut(t));
            self.normalize_in(x, y);
        }
        if let Some(b) = b {
            for (t, y) in b.checkpoints.iter_mut() {
                if !a.checkpoints.contains_key(t) {
                    self.normalize_in(y, None);
                }
            }
        }
    }

    fn by_stack(&self, s: &mut Snapshot) {
        for o in &mut s.item_outcomes {
            self.stacks(&mut o.taken);
        }
        if let Some(drops) = s.drops.as_mut() {
            drops.retain(|d| !self.maybe.contains(&d.stack.item));
            for d in drops.iter_mut() {
                if self.counted.contains(&d.stack.item) {
                    d.stack.count = 0;
                }
            }
            drops.dedup_by(|a, b| a == b);
        }
        for b in s.blocks.values_mut() {
            if let Some(t) = b.transport.as_mut() {
                let before = (t.items.len(), t.outputs.len());
                let touched = t.items.iter().any(|i| self.touches(&i.stack.item))
                    || t.outputs.iter().any(|o| self.touches(&o.item));
                t.items.retain(|i| !self.maybe.contains(&i.stack.item));
                for i in &mut t.items {
                    if self.counted.contains(&i.stack.item) {
                        i.stack.count = 0;
                    }
                }
                self.stacks(&mut t.outputs);
                if touched || before != (t.items.len(), t.outputs.len()) {
                    t.comparator = None;
                }
                b.payload = None;
            }
        }
    }
}

/// Whether a recipe that takes `input` rolls its results.
fn rolled(input: &str) -> bool {
    contract()
        .recipes
        .iter()
        .any(|r| r.input == input && r.results.iter().any(|o| o.chance < 1.0))
}

/// Equal machine states. An applied `ProcessingInventory` compares its stacks, not where they
/// sit: Create keeps results after slot 0 and an uncut input in it, the port keeps a list. An
/// empty saw's time is not compared: Create leaves it applied with none left after handing its
/// results to a belt or depot (`SawBlockEntity.tick`), the port forgets it, and every insert
/// sets it again (`insertItem` clears, `start`).
pub fn equal(java: Option<&Observation>, rust: Option<&Observation>) -> bool {
    let unslotted = |o: Option<&Observation>| {
        o.cloned().map(|mut o| {
            if o.kind == "saw" && o.slots.as_ref().is_some_and(Vec::is_empty) {
                o.remaining = None;
                o.duration = None;
                o.applied = None;
            }
            if o.applied == Some(true)
                && let Some(s) = o.slots.as_mut()
            {
                for x in s.iter_mut() {
                    x.slot = 0;
                }
                s.sort_by(|a, b| (&a.item, a.count).cmp(&(&b.item, b.count)));
            }
            o
        })
    };
    unslotted(java) == unslotted(rust)
}

fn placed(pos: Pos, state: &str, motor: Option<i32>) -> Placed {
    Placed {
        pos,
        state: state.into(),
        motor,
    }
}
fn act(tick: u32, pos: Pos, action: Action) -> Change {
    Change {
        tick,
        pos,
        action: Some(action),
        state: None,
        motor: None,
        turn: None,
    }
}
fn drop_on(item: &str, count: u8) -> Action {
    Action::Item {
        input: Input::DropOn {
            stack: Stack {
                item: item.into(),
                count,
            },
        },
    }
}
fn insert(side: Option<Face>, slot: u8, item: &str, count: u8) -> Action {
    Action::Capability {
        side,
        slot,
        insert: Some(Stack {
            item: item.into(),
            count,
        }),
        extract: None,
        simulate: false,
    }
}
fn extract(side: Option<Face>, slot: u8, count: u8) -> Action {
    Action::Capability {
        side,
        slot,
        insert: None,
        extract: Some(count),
        simulate: false,
    }
}
fn empty_use(face: Face) -> Action {
    Action::Item {
        input: Input::Use {
            stack: None,
            face,
            yaw: 0.,
            sneaking: false,
        },
    }
}
fn scenario(seed: u64, ticks: u32) -> Scenario {
    Scenario {
        version: 3,
        seed,
        ticks,
        day_time: 0,
        checkpoints: vec![],
        blocks: vec![],
        events: vec![],
    }
}
fn inputs(machine: &str) -> &'static [String] {
    &contract().inputs[machine]
}

const SPEEDS: [i32; 7] = [8, 16, 32, 64, 128, 256, -48];

/// A millstone output slot's limit (`ItemStackHandler.getSlotLimit`).
const OUTPUT_LIMIT: u8 = 64;

/// The most one `input` makes of any one result: a recipe's results of one item added up.
fn most(input: &str) -> u8 {
    let mut of_one: BTreeMap<&str, u8> = BTreeMap::new();
    for r in contract().recipes.iter().filter(|r| r.input == input) {
        for o in &r.results {
            *of_one.entry(o.item.as_str()).or_default() += o.count;
        }
    }
    of_one.into_values().max().unwrap_or(0)
}

/// The stack an action offers to a machine's input slot 0: dropped on it, or put there.
fn to_input(a: &Action) -> Option<&Stack> {
    match a {
        Action::Item {
            input: Input::DropOn { stack },
        }
        | Action::Capability {
            slot: 0,
            insert: Some(stack),
            ..
        } => Some(stack),
        _ => None,
    }
}

/// A millstone on a motor (`MillstoneBlockEntity`): stacks dropped on it and put into its
/// handler (slot 0 only takes what it can mill, outputs refuse), outputs taken through the
/// handler or by a player's empty hand (`MillstoneBlock.useItemOn`, the input when there are no
/// outputs), the motor stopped and restarted, the millstone broken with what it holds.
pub fn millstone(seed: u64) -> Scenario {
    let mut r = Rng::new(seed);
    let mut sc = scenario(seed, 420);
    let mill = [0, 8, 0];
    let rpm = *r.pick(&SPEEDS);
    sc.blocks.push(placed(mill, "create:millstone", None));
    sc.blocks.push(placed(
        [0, 7, 0],
        "create:creative_motor[facing=up]",
        Some(rpm),
    ));
    let mut tick = 10;
    // Which stack sits in an output slot is chance's once a rolled input has reached the input
    // slot, so none is drawn on after one. All a case's rolled inputs make less than a full
    // slot: a full one stops the millstone (`MillstoneBlockEntity.tick`, :92) when chance says.
    let mut reached = false;
    let mut room = OUTPUT_LIMIT - 1;
    for _ in 0..3 + r.below(4) {
        if tick + 40 > sc.ticks {
            break;
        }
        let item = r.pick(inputs("millstone")).as_str();
        let rolled = rolled(item);
        let mut count = 1 + r.below(if rolled { 6 } else { 20 }) as u8;
        if rolled {
            count = count.min(room / most(item));
            room -= count * most(item);
        }
        let a = match r.below(5) {
            0 | 1 => drop_on(item, count),
            2 => insert(None, 0, item, count),
            3 => insert(Some(Face::North), 1 + r.below(9) as u8, item, count),
            _ => insert(Some(Face::Up), 0, item, count),
        };
        if count > 0 {
            reached |= rolled && to_input(&a).is_some();
            sc.events.push(act(tick, mill, a));
        }
        sc.checkpoints.extend(tick..tick + 4);
        tick += 15 + r.below(60);
        match r.below(6) {
            0 => {
                let slot = 1 + r.below(9) as u8;
                if !reached {
                    sc.events.push(act(tick, mill, extract(None, slot, 64)));
                }
            }
            1 => sc.events.push(act(tick, mill, extract(None, 0, 64))),
            2 => sc.events.push(act(tick, mill, empty_use(Face::Up))),
            3 => sc.events.push(act(
                tick,
                [0, 7, 0],
                Action::SpeedSetting {
                    row: i32::from(r.chance(0.5)),
                    value: *r.pick(&[0, 4, 16, 96]),
                },
            )),
            _ => {}
        }
        tick += 1 + r.below(30);
    }
    if r.chance(0.3) {
        sc.events.push(Change {
            tick: tick.min(sc.ticks - 10),
            pos: mill,
            action: None,
            state: Some("minecraft:air".into()),
            motor: None,
            turn: None,
        });
    }
    sc.events.retain(|e| e.tick < sc.ticks);
    sc.checkpoints.retain(|t| *t < sc.ticks);
    sc.checkpoints.extend((60..sc.ticks).step_by(20));
    sc.checkpoints.sort_unstable();
    sc.checkpoints.dedup();
    sc
}

/// Two crushing wheels turning against each other and the controller between them
/// (`CrushingWheelBlock.updateControllers`, `CrushingWheelControllerBlockEntity`): along x or
/// z, horizontal wheels over a depot or nothing, both turned one way (no controller crushes)
/// or a wheel removed; stacks put into the controller's handler, crushed (crushing, else
/// milling, else lost) and let out onto the depot or as items.
pub fn crushing(seed: u64) -> Scenario {
    let mut r = Rng::new(seed);
    let mut sc = scenario(seed, 480);
    let rpm = *r.pick(&[16, 32, 64, 128, 256]);
    let along_x = r.chance(0.5);
    // Wheels on a vertical axis beside the controller would let it out sideways; the wheels'
    // axis here is horizontal, so the controller faces down (`updateControllers`).
    let (axis, a, b, motor_face) = if along_x {
        ("z", [-1, 8, 0], [1, 8, 0], "north")
    } else {
        ("x", [0, 8, -1], [0, 8, 1], "west")
    };
    let behind = |p: Pos| {
        if along_x {
            [p[0], p[1], p[2] + 1]
        } else {
            [p[0] + 1, p[1], p[2]]
        }
    };
    let wheel = format!("create:crushing_wheel[axis={axis}]");
    sc.blocks.push(placed(a, &wheel, None));
    sc.blocks.push(placed(b, &wheel, None));
    let same = r.chance(0.15);
    sc.blocks.push(placed(
        behind(a),
        &format!("create:creative_motor[facing={motor_face}]"),
        Some(rpm),
    ));
    sc.blocks.push(placed(
        behind(b),
        &format!("create:creative_motor[facing={motor_face}]"),
        Some(if same { rpm } else { -rpm }),
    ));
    let controller = [0, 8, 0];
    let depot = r.chance(0.6);
    if depot {
        sc.blocks
            .push(placed([0, 7, 0], "create:depot[waterlogged=false]", None));
    }
    // The depot takes one stack of the results; how many more stay in the controller, and so
    // whether it takes the next insert, is `level.random`'s.
    let mut rolled_left = false;
    let mut tick = 20;
    for _ in 0..2 + r.below(3) {
        if tick + 40 > sc.ticks {
            break;
        }
        let item = r.pick(inputs("crusher")).as_str();
        let count = 1 + r.below(if rolled(item) { 4 } else { 16 }) as u8;
        let slot = r.below(2) as u8;
        if !rolled_left {
            sc.events
                .push(act(tick, controller, insert(None, slot, item, count)));
            rolled_left = depot && slot == 0 && rolled(item);
        }
        sc.checkpoints.extend(tick..tick + 3);
        tick += 40 + r.below(120);
        if r.chance(0.2) {
            sc.events.push(act(tick, [0, 7, 0], extract(None, 0, 64)));
        }
    }
    if r.chance(0.25) {
        sc.events.push(Change {
            tick: tick.min(sc.ticks - 20),
            pos: b,
            action: None,
            state: Some("minecraft:air".into()),
            motor: None,
            turn: None,
        });
    }
    sc.events.retain(|e| e.tick < sc.ticks);
    sc.checkpoints.retain(|t| *t < sc.ticks);
    sc.checkpoints.extend((40..sc.ticks).step_by(10));
    sc.checkpoints.sort_unstable();
    sc.checkpoints.dedup();
    sc
}

/// A saw facing up (`SawBlockEntity` processing): an item dropped on it or put into its
/// handler (one at a time: `bulkCutting` off), cut by its cutting recipe (10 ticks and through
/// without one), and thrown the way it turns: onto a depot there, or out as items. Its speed
/// changes sign mid-cut, the saw stops, or is broken holding what it cut.
pub fn saw(seed: u64) -> Scenario {
    let mut r = Rng::new(seed);
    let mut sc = scenario(seed, 360);
    let saw = [0, 8, 0];
    let along_first = r.chance(0.5);
    sc.blocks.push(placed(
        saw,
        &format!("create:mechanical_saw[axis_along_first={along_first},facing=up,flipped=false]"),
        None,
    ));
    // Facing up, its shaft runs along x with `axis_along_first`, else along z
    // (`DirectionalAxisKineticBlock.getRotationAxis`).
    let (motor, facing) = if along_first {
        ([-1, 8, 0], "east")
    } else {
        ([0, 8, -1], "south")
    };
    let rpm = *r.pick(&[24, 48, 96, 256, -32, -128]);
    sc.blocks.push(placed(
        motor,
        &format!("create:creative_motor[facing={facing}]"),
        Some(rpm),
    ));
    let m = create_engine::saw::movement(along_first, rpm as f32);
    if r.chance(0.6) {
        sc.blocks.push(placed(
            [m[0], 8, m[2]],
            "create:depot[waterlogged=false]",
            None,
        ));
    }
    let mut tick = 10;
    for _ in 0..3 + r.below(4) {
        if tick + 40 > sc.ticks {
            break;
        }
        let item = r.pick(inputs("saw")).as_str();
        let a = if r.chance(0.6) {
            drop_on(item, 1 + r.below(8) as u8)
        } else {
            insert(Some(Face::Up), 0, item, 1 + r.below(8) as u8)
        };
        sc.events.push(act(tick, saw, a));
        sc.checkpoints.extend(tick..tick + 4);
        tick += 8 + r.below(50);
        if r.chance(0.2) {
            sc.events.push(act(
                tick,
                motor,
                Action::SpeedSetting {
                    row: i32::from(rpm < 0),
                    value: rpm.unsigned_abs() as i32,
                },
            ));
            tick += 1;
        }
    }
    if r.chance(0.2) {
        sc.events.push(Change {
            tick: tick.min(sc.ticks - 10),
            pos: saw,
            action: None,
            state: Some("minecraft:air".into()),
            motor: None,
            turn: None,
        });
    }
    sc.events.retain(|e| e.tick < sc.ticks);
    sc.checkpoints.retain(|t| *t < sc.ticks);
    sc.checkpoints.extend((30..sc.ticks).step_by(10));
    sc.checkpoints.sort_unstable();
    sc.checkpoints.dedup();
    sc
}

#[cfg(test)]
mod tests {
    use super::{Observation, Slot, Stack};
    use crate::observation::BlockSnap;
    use crate::scenario::Snapshot;
    use crate::transport::Outcome;

    #[test]
    fn no_insert_follows_a_rolled_one_over_a_depot() {
        let mut gated = 0;
        for seed in (221..20_000).step_by(256) {
            let sc = super::crushing(seed);
            if !sc
                .blocks
                .iter()
                .any(|b| b.state.starts_with("create:depot"))
            {
                continue;
            }
            let inserts: Vec<_> = sc
                .events
                .iter()
                .filter_map(|e| match &e.action {
                    Some(super::Action::Capability {
                        insert: Some(s),
                        slot,
                        ..
                    }) => Some((*slot, s.item.clone())),
                    _ => None,
                })
                .collect();
            if let Some(i) = inserts
                .iter()
                .position(|(slot, item)| *slot == 0 && super::rolled(item))
            {
                assert_eq!(i, inserts.len() - 1, "seed {seed}");
                gated += 1;
            }
        }
        assert!(gated > 0);
        let sc = super::crushing(5341);
        assert_eq!(sc.events.len(), 2);
    }

    fn stack(item: &str, count: u8) -> Stack {
        Stack {
            item: item.into(),
            count,
        }
    }
    fn slot(item: &str, count: u8, slot: u8) -> Slot {
        Slot {
            item: item.into(),
            count,
            slot,
        }
    }
    fn machine(outputs: Vec<Slot>) -> BlockSnap {
        BlockSnap {
            state: "create:millstone".into(),
            processing: Some(Observation {
                kind: "millstone".into(),
                timer: Some(88),
                input: Some(vec![slot("minecraft:andesite", 18, 0)]),
                outputs: Some(outputs),
                ..Observation::default()
            }),
            ..BlockSnap::default()
        }
    }
    fn milled(outputs: Vec<Slot>, bone_meal: Option<u8>) -> Snapshot {
        let mut s = Snapshot {
            item_outcomes: vec![
                Outcome {
                    pos: [0, 8, 0],
                    offered: Some(stack("minecraft:bone", 4)),
                    remainder: None,
                    taken: vec![],
                    handler: Some(true),
                },
                Outcome {
                    pos: [0, 8, 0],
                    offered: Some(stack("minecraft:andesite", 19)),
                    remainder: None,
                    taken: vec![],
                    handler: Some(true),
                },
            ],
            ..Snapshot::default()
        };
        if let Some(n) = bone_meal {
            s.item_outcomes.push(Outcome {
                pos: [0, 8, 0],
                offered: None,
                remainder: None,
                taken: vec![stack("minecraft:bone_meal", n)],
                handler: Some(true),
            });
        }
        s.blocks.insert("0,8,0".into(), machine(outputs));
        s
    }
    fn at_171(s: &Snapshot, outputs: Vec<Slot>) -> Snapshot {
        let mut s = s.clone();
        s.checkpoints.insert(
            171,
            Snapshot {
                blocks: [("0,8,0".to_string(), machine(outputs))].into(),
                ..Snapshot::default()
            },
        );
        s
    }
    #[test]
    fn a_result_a_chance_one_moved_compares_by_stack() {
        let dye = milled(
            vec![
                slot("minecraft:white_dye", 2, 1),
                slot("minecraft:cobblestone", 1, 2),
            ],
            Some(15),
        );
        let no_dye = milled(vec![slot("minecraft:cobblestone", 1, 1)], Some(14));
        assert!(!crate::compare::diff_exact(&dye, &no_dye).is_empty());
        assert!(crate::compare::diff(&dye, &no_dye).is_empty());
        assert!(crate::compare::diff(&no_dye, &dye).is_empty());
        let dye_rolling = at_171(
            &dye,
            vec![
                slot("minecraft:bone_meal", 15, 0),
                slot("minecraft:white_dye", 2, 1),
                slot("minecraft:cobblestone", 1, 2),
            ],
        );
        let no_dye_rolling = at_171(
            &no_dye,
            vec![
                slot("minecraft:bone_meal", 14, 0),
                slot("minecraft:cobblestone", 1, 1),
            ],
        );
        assert!(crate::compare::diff(&dye_rolling, &no_dye_rolling).is_empty());

        let elsewhere = milled(vec![slot("minecraft:cobblestone", 1, 2)], Some(14));
        assert!(!crate::compare::diff(&no_dye, &elsewhere).is_empty());
        let more_cobblestone = milled(
            vec![
                slot("minecraft:white_dye", 2, 1),
                slot("minecraft:cobblestone", 2, 2),
            ],
            Some(15),
        );
        assert!(!crate::compare::diff(&dye, &more_cobblestone).is_empty());
        let mut bone_meal_kept = dye.clone();
        bone_meal_kept.item_outcomes[2].taken.clear();
        assert!(!crate::compare::diff(&dye, &bone_meal_kept).is_empty());
    }

    #[test]
    fn no_output_slot_is_drawn_on_after_a_rolled_input() {
        let mut reached = 0;
        let mut drawn = 0;
        for seed in (205..20_000).step_by(256) {
            let mut rolled_yet = false;
            for e in super::millstone(seed).events {
                let Some(a) = &e.action else {
                    continue;
                };
                if let super::Action::Capability {
                    slot: 1..=9,
                    extract: Some(_),
                    ..
                } = a
                {
                    assert!(!rolled_yet, "seed {seed}, tick {}", e.tick);
                    drawn += 1;
                }
                rolled_yet |= super::to_input(a).is_some_and(|s| super::rolled(&s.item));
            }
            reached += usize::from(rolled_yet);
        }
        assert!(reached > 0 && drawn > 0);
    }

    #[test]
    fn a_case_s_rolled_inputs_cannot_fill_a_slot() {
        assert_eq!(super::most("minecraft:bone"), 6);
        let mut bones_717 = 0;
        for seed in (205..20_000).step_by(256) {
            let mut of_one = 0;
            for e in super::millstone(seed).events {
                if let Some(s) = e.action.as_ref().and_then(super::to_input)
                    && super::rolled(&s.item)
                {
                    of_one += u32::from(s.count) * u32::from(super::most(&s.item));
                    if seed == 717 && s.item == "minecraft:bone" {
                        bones_717 += s.count;
                    }
                }
            }
            assert!(
                of_one < u32::from(super::OUTPUT_LIMIT),
                "seed {seed}: {of_one}"
            );
        }
        assert_eq!(bones_717, 10);
    }
}

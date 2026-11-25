//! The processing machines, mirroring glue/processing/{millstone,crusher,saw}.rs: millstones
//! and crushing wheel controllers in the processing phase (`Processing::level_tick`, before
//! fluids and logistics), upward saws at their turn in the transport walk (`saw_turn`) and
//! their drops in the tools phase after it (`Tools::level_tick`). Their recipes are the audited subset (`processing.json`).
//! Item entities do not reach a controller: the harness discards them at the end of the tick
//! they join in, before an entity's `entityInside` could start it.
use super::*;
use crate::processing::{Observation, Slot};
use crate::transport::Stack;
use create_engine::kinetics::{Axis, Dir};
use create_engine::processing::{self as engine_processing, Book};
use create_engine::{Block, belt, crusher, saw};

pub(super) struct Books {
    milling: Book,
    crushing: Book,
    cutting: Book,
}

impl Books {
    pub(super) fn new() -> Books {
        Books {
            milling: crate::processing::book("milling"),
            crushing: crate::processing::book("crushing"),
            cutting: crate::processing::book("cutting"),
        }
    }
}

/// A millstone's slots: the input 0, then the nine outputs.
pub(super) type MillSlots = [Option<Stack>; 10];

fn limit(item: &str) -> u8 {
    Stack {
        item: item.into(),
        count: 1,
    }
    .limit()
}

fn step(p: Pos, d: Dir, n: i32) -> Pos {
    let o = d.offset();
    [p[0] + o[0] * n, p[1] + o[1] * n, p[2] + o[2] * n]
}

fn dir_name(d: Dir) -> &'static str {
    match d {
        Dir::Up => "up",
        Dir::Down => "down",
        Dir::North => "north",
        Dir::South => "south",
        Dir::West => "west",
        Dir::East => "east",
    }
}

impl Run {
    /// `ServerLevel::seed_at`: the seed of this tick's rolls at `pos`.
    fn seed_at(&self, pos: Pos) -> u64 {
        u64::from(self.now) ^ (pos[0] as u64).wrapping_mul(0x9E37_79B9) ^ (pos[2] as u64) << 20
    }

    fn mill_slots(&mut self, pos: Pos) -> &mut MillSlots {
        self.millstone_slots.entry(pos).or_default()
    }

    /// `tick_millstones`.
    pub(super) fn tick_millstones(&mut self) {
        let mut turning = self.w.turning_millstones(0);
        turning.sort_by_key(|(p, _)| *p);
        for (pos, speed) in turning {
            let Some(mut state) = self.w.dims[0].millstones.get(&pos).copied() else {
                continue;
            };
            let slots = self.mill_slots(pos).clone();
            let input = slots[0].as_ref().map(|s| s.item.clone());
            let mut outputs: Vec<engine_processing::Slot> = slots[1..]
                .iter()
                .map(|s| s.as_ref().map(|s| (s.item.clone(), s.count)))
                .collect();
            let outputs_full = outputs.iter().flatten().any(|(i, n)| *n >= limit(i));
            let done = engine_processing::tick_millstone(
                &mut state,
                speed,
                input.as_deref(),
                outputs_full,
                &self.books.milling,
            )
            .map(|r| self.books.milling.get(r).clone());
            if let Some(m) = self.w.dims[0].millstones.get_mut(&pos) {
                *m = state;
            }
            let Some(recipe) = done else {
                continue;
            };
            let mut rng = engine_processing::Rng::new(self.seed_at(pos));
            for (item, n) in engine_processing::roll(&recipe, &mut rng) {
                let max = limit(&item);
                engine_processing::insert_stacked(&mut outputs, &item, n, max);
            }
            let s = self.mill_slots(pos);
            s[0] = s[0].take().and_then(|mut x| {
                x.count -= 1;
                (x.count > 0).then_some(x)
            });
            for (i, o) in outputs.into_iter().enumerate() {
                s[i + 1] = o.map(|(item, count)| Stack { item, count });
            }
        }
    }

    /// `millstone_take`: a stack landing on (or handed to) a millstone goes into its input as far
    /// as it fits, when it has a milling recipe. The count left.
    pub(super) fn millstone_take(&mut self, pos: Pos, stack: &Stack) -> u8 {
        self.millstone_insert(pos, 0, stack, false)
    }

    /// The millstone's container through its handler (`MillstoneInventoryHandler`: slot 0 takes
    /// what `canProcess` mills, 1 to 9 give): `insertItem` into `slot`. The count left.
    fn millstone_insert(&mut self, pos: Pos, slot: u8, stack: &Stack, simulate: bool) -> u8 {
        if slot != 0 || self.books.milling.find_single(&stack.item).is_none() {
            return stack.count;
        }
        let max = limit(&stack.item);
        let s = self.mill_slots(pos);
        let mut input: Vec<engine_processing::Slot> =
            vec![s[0].as_ref().map(|x| (x.item.clone(), x.count))];
        let left = engine_processing::insert_stacked(&mut input, &stack.item, stack.count, max);
        if !simulate {
            s[0] = input[0].clone().map(|(item, count)| Stack { item, count });
        }
        left
    }

    fn millstone_extract(&mut self, pos: Pos, slot: u8, n: u8, simulate: bool) -> Option<Stack> {
        if !(1..10).contains(&slot) {
            return None;
        }
        let s = self.mill_slots(pos);
        let have = s[usize::from(slot)].clone()?;
        let k = n.min(have.count);
        if !simulate {
            s[usize::from(slot)] = (have.count > k).then(|| Stack {
                item: have.item.clone(),
                count: have.count - k,
            });
        }
        Some(Stack {
            item: have.item,
            count: k,
        })
    }

    /// `use_millstone` with an empty hand: the outputs, else the input, to the player.
    fn use_millstone(&mut self, pos: Pos) -> Vec<Stack> {
        let s = self.mill_slots(pos);
        let outputs: Vec<usize> = (1..10).filter(|&i| s[i].is_some()).collect();
        let taken = if outputs.is_empty() { vec![0] } else { outputs };
        taken.into_iter().filter_map(|i| s[i].take()).collect()
    }

    /// A millstone gone: the game spills its container slot by slot
    /// (`Containers.dropContents`), as Create's `destroy` does (`ItemHelper.dropContents`).
    pub(super) fn millstone_removed(&mut self, pos: Pos) {
        for s in self
            .millstone_slots
            .remove(&pos)
            .unwrap_or_default()
            .into_iter()
            .flatten()
        {
            self.drop_item_stack(pos.map(f64::from), &s.item, s.count);
        }
    }

    /// `tick_crushers`: the controllers wheels asked for, then the turning controllers.
    pub(super) fn tick_crushers(&mut self) {
        let mut checks = std::mem::take(&mut self.w.dims[0].crusher_checks);
        checks.sort();
        checks.dedup();
        for pos in checks {
            self.fix_controllers(pos);
        }
        let running = self.w.dims[0].running_crushers.clone();
        for pos in running {
            self.tick_crusher(pos);
        }
    }

    /// `fix_controllers` for the wheel (or the place a wheel left) at `pos`.
    fn fix_controllers(&mut self, pos: Pos) {
        let wheel = self
            .w
            .table
            .get(self.at(pos))
            .filter(|(b, _)| *b == Block::CrushingWheel)
            .map(|(_, i)| i.axis);
        for side in Dir::ALL {
            let cpos = step(pos, side, 1);
            let existing = self.w.block_at(0, cpos) == Some(Block::CrushingWheelController);
            let want = wheel.and_then(|axis| {
                let axis = axis?;
                let speed = self.w.speed(0, pos).unwrap_or(0.);
                let opos = step(pos, side, 2);
                let other = self
                    .w
                    .table
                    .get(self.at(opos))
                    .filter(|(b, _)| *b == Block::CrushingWheel)
                    .and_then(|(_, i)| i.axis)
                    .map(|a| (a, self.w.speed(0, opos).unwrap_or(0.)));
                crusher::controller(axis, side, speed, other).map(|c| (c, speed))
            });
            match want {
                None => {
                    if existing && (wheel.is_none() || wheel.flatten() != Some(side.axis())) {
                        self.set(cpos, AIR);
                    }
                }
                Some(((valid, facing), speed)) => {
                    if !existing && self.at(cpos) != AIR {
                        continue;
                    }
                    let id = self.id(&format!(
                        "create:crushing_wheel_controller[facing={},valid={valid}]",
                        dir_name(facing)
                    ));
                    if self.at(cpos) != id {
                        self.set(cpos, id);
                    }
                    let crushing = if valid {
                        crusher::crushing_speed(speed)
                    } else {
                        0.0
                    };
                    self.w.set_crusher_speed(0, cpos, crushing);
                }
            }
        }
    }

    /// `tick_crusher`, for a controller that holds no entity.
    fn tick_crusher(&mut self, pos: Pos) {
        let Some(facing) = self
            .w
            .table
            .get(self.at(pos))
            .filter(|(b, _)| *b == Block::CrushingWheelController)
            .and_then(|(_, i)| i.facing)
        else {
            return;
        };
        let Some((mut c, speed)) = self.w.dims[0].crushers.get(&pos).cloned() else {
            return;
        };
        if c.slots.is_empty() {
            return;
        }
        match crusher::tick_inventory(&mut c, speed) {
            crusher::Step::Wait => {}
            crusher::Step::Apply => {
                let mut rng = engine_processing::Rng::new(self.seed_at(pos));
                let item = c.slots[0].0.clone();
                let recipe = crusher::recipe(&self.books.crushing, &self.books.milling, &item);
                crusher::apply(&mut c, recipe, limit, &mut rng, None);
            }
            crusher::Step::Output => {
                let side = if facing.axis() == Axis::Y {
                    [0, 0, 0]
                } else {
                    facing.offset()
                };
                let next = [pos[0] + side[0], pos[1] - 1, pos[2] + side[2]];
                if facing != Dir::Up && self.direct_input_at(next) {
                    // The glue returns before it writes the controller back.
                    if !self.direct_input_accepts(next, facing) {
                        return;
                    }
                    let mut left = Vec::new();
                    for (item, count) in std::mem::take(&mut c.slots) {
                        let one = belt::Item::new(&item, count, 0, false);
                        let n = self.insert_transport(next, facing, &one).unwrap_or(count);
                        if n > 0 {
                            left.push((item, n));
                        }
                    }
                    c.slots = left;
                } else {
                    let (at, v) = crusher::output(pos, facing);
                    for (item, count) in crusher::ejected(&mut c) {
                        self.spawn_item(at, v, &item, count);
                    }
                }
            }
        }
        if let Some(e) = self.w.dims[0].crushers.get_mut(&pos) {
            e.0 = c;
        }
    }

    /// `saw_turn`, without belt funnels on top (`export_up`) or sequenced assemblies.
    pub(super) fn tick_saw(&mut self, pos: Pos) {
        let Some((along_first, facing)) = self
            .w
            .table
            .get(self.at(pos))
            .map(|(_, i)| (i.along_first, i.facing))
        else {
            return;
        };
        if facing != Some(Dir::Up) {
            return;
        }
        let speed = self.w.speed(0, pos).unwrap_or(0.);
        let Some(mut s) = self.w.dims[0].saws.get(&pos).cloned() else {
            return;
        };
        let item = s.slots.first().map(|x| x.0.clone()).unwrap_or_default();
        let step = s.tick(speed);
        // `saw_turn`: a saw whose turn did something has its column written.
        if step != saw::Step::Wait {
            self.w.mark_unsaved(0, pos);
        }
        match step {
            saw::Step::Wait => {}
            saw::Step::Start => {
                let list: Vec<&create_engine::processing::Recipe> = self
                    .books
                    .cutting
                    .matching(&item)
                    .into_iter()
                    .map(|i| self.books.cutting.get(i))
                    .collect();
                s.start(&list);
            }
            saw::Step::Apply => {
                let mut rng = engine_processing::Rng::new(self.seed_at(pos));
                let list: Vec<&create_engine::processing::Recipe> = self
                    .books
                    .cutting
                    .matching(&item)
                    .into_iter()
                    .map(|i| self.books.cutting.get(i))
                    .collect();
                let recipe = list.get(s.index).copied();
                s.apply(recipe, limit, &mut rng, None);
            }
            saw::Step::Out => {
                let m = saw::movement(along_first, speed);
                if self.at([pos[0], pos[1] + 1, pos[2]]) != AIR {
                    self.notes.push(format!(
                        "adapter gap: a saw's export into the block above it at {pos:?}"
                    ));
                }
                let next = [pos[0] + m[0], pos[1], pos[2] + m[2]];
                let facing = Dir::ALL
                    .into_iter()
                    .find(|d| d.offset() == m)
                    .filter(|_| self.direct_input_at(next));
                if let Some(facing) = facing {
                    if !self.direct_input_accepts(next, facing) {
                        return;
                    }
                    let mut left = Vec::new();
                    for (item, count) in std::mem::take(&mut s.slots) {
                        let one = belt::Item::new(&item, count, 0, false);
                        let n = self.insert_transport(next, facing, &one).unwrap_or(count);
                        if n > 0 {
                            left.push((item, n));
                        }
                    }
                    if left.is_empty() {
                        s.clear();
                        self.saw_index.insert(pos, s.index);
                    } else {
                        s.slots = left;
                    }
                    self.put_saw(pos, s);
                    return;
                }
                let at = [
                    f64::from(pos[0]) + 0.5 + f64::from(m[0]) * 0.5,
                    f64::from(pos[1]) + 1.0,
                    f64::from(pos[2]) + 0.5 + f64::from(m[2]) * 0.5,
                ];
                let v = [f64::from(m[0]) * 0.0625, 0.125, f64::from(m[2]) * 0.0625];
                for (item, count) in s.slots.clone() {
                    self.spawn_item(at, v, &item, count);
                }
                s.clear();
            }
        }
        if s.slots.is_empty() {
            self.saw_index.insert(pos, s.index);
        }
        if step == saw::Step::Start {
            self.saw_duration.insert(pos, s.remaining);
        }
        self.put_saw(pos, s);
    }

    fn put_saw(&mut self, pos: Pos, s: saw::Saw) {
        if s.slots.is_empty() {
            self.w.dims[0].saws.remove(&pos);
        } else {
            self.w.dims[0].saws.insert(pos, s);
        }
    }

    /// `saw_take`: one of a stack landing on a turning upward saw. The count left.
    pub(super) fn saw_take(&mut self, pos: Pos, stack: &Stack) -> u8 {
        let up = self.w.table.get(self.at(pos)).and_then(|(_, i)| i.facing) == Some(Dir::Up);
        if !up || self.w.speed(0, pos).unwrap_or(0.) == 0. {
            return stack.count;
        }
        if self.saw_put(pos, &stack.item) {
            stack.count - 1
        } else {
            stack.count
        }
    }

    /// `saw_put`: one item into the saw while it holds nothing, its cut started at once.
    fn saw_put(&mut self, pos: Pos, item: &str) -> bool {
        let mut s = self.w.dims[0].saws.get(&pos).cloned().unwrap_or_default();
        if !s.slots.is_empty() {
            return false;
        }
        s.index = self.saw_index.get(&pos).copied().unwrap_or(0);
        s.progress = None;
        let up = self.w.table.get(self.at(pos)).and_then(|(_, i)| i.facing) == Some(Dir::Up);
        let list: Vec<&create_engine::processing::Recipe> = if up {
            self.books
                .cutting
                .matching(item)
                .into_iter()
                .map(|i| self.books.cutting.get(i))
                .collect()
        } else {
            Vec::new()
        };
        s.insert(0, item, &list, up);
        if up {
            self.saw_duration.insert(pos, s.remaining);
        }
        self.w.dims[0].saws.insert(pos, s);
        self.w.mark_unsaved(0, pos);
        true
    }

    /// What a stack landing on the block at `p` does there, when it is a processing machine
    /// (`Landing`): the count left.
    pub(super) fn processing_landing(&mut self, p: Pos, stack: &Stack) -> Option<u8> {
        match self.w.block_at(0, p)? {
            Block::Millstone => Some(self.millstone_take(p, stack)),
            Block::MechanicalSaw => Some(self.saw_take(p, stack)),
            Block::CrushingWheelController => Some(stack.count),
            _ => None,
        }
    }

    /// An empty-hand use on a processing machine: what the player was given.
    pub(super) fn processing_use(&mut self, p: Pos) -> Option<Vec<Stack>> {
        match self.w.block_at(0, p)? {
            Block::Millstone => Some(self.use_millstone(p)),
            _ => None,
        }
    }

    /// The machines' item handlers: the millstone's container, the crushing wheel controller's
    /// `CrusherItems` and the saw's `SawItems` (none on its bottom).
    pub(super) fn processing_capability(
        &mut self,
        p: Pos,
        slot: u8,
        insert: Option<&Stack>,
        extract: Option<u8>,
        simulate: bool,
        outcome: &mut crate::transport::Outcome,
    ) -> bool {
        match self.w.block_at(0, p) {
            Some(Block::Millstone) => {
                outcome.handler = Some(true);
                if let Some(s) = insert {
                    let left = self.millstone_insert(p, slot, s, simulate);
                    outcome.remainder = (left > 0).then(|| Stack {
                        item: s.item.clone(),
                        count: left,
                    });
                } else if let Some(n) = extract
                    && let Some(t) = self.millstone_extract(p, slot, n, simulate)
                {
                    outcome.taken.push(t);
                }
                true
            }
            Some(Block::CrushingWheelController) => {
                outcome.handler = Some(true);
                if let Some(s) = insert {
                    let time = crusher::recipe(&self.books.crushing, &self.books.milling, &s.item)
                        .map(|r| r.time);
                    let taken = self.w.dims[0].crushers.get_mut(&p).map_or(0, |(c, _)| {
                        let mut c2 = c.clone();
                        let c = if simulate { &mut c2 } else { c };
                        crusher::insert(
                            c,
                            usize::from(slot),
                            &s.item,
                            s.count,
                            limit(&s.item),
                            time,
                        )
                    });
                    outcome.remainder = (taken < s.count).then(|| Stack {
                        item: s.item.clone(),
                        count: s.count - taken,
                    });
                }
                true
            }
            Some(Block::MechanicalSaw) => {
                outcome.handler = Some(self.capability_side != Some(Dir::Down));
                if outcome.handler == Some(true)
                    && let Some(s) = insert
                {
                    let empty = self.w.dims[0]
                        .saws
                        .get(&p)
                        .is_none_or(|x| x.slots.is_empty());
                    let took = slot == 0 && empty && (simulate || self.saw_put(p, &s.item));
                    outcome.remainder = (s.count > u8::from(took)).then(|| Stack {
                        item: s.item.clone(),
                        count: s.count - u8::from(took),
                    });
                }
                true
            }
            _ => false,
        }
    }

    /// `ProcessingObservations` for the machine at `p` (version 3 cases).
    pub(super) fn processing_observation(&self, p: Pos) -> Option<Observation> {
        let slot = |i: usize, (item, count): &(String, u8)| Slot {
            item: item.clone(),
            count: *count,
            slot: i as u8,
        };
        let stacks = |s: &[Option<Stack>], first: usize| -> Vec<Slot> {
            s.iter()
                .enumerate()
                .filter_map(|(i, x)| {
                    x.as_ref().map(|x| Slot {
                        item: x.item.clone(),
                        count: x.count,
                        slot: (i - first) as u8,
                    })
                })
                .collect()
        };
        // Results sit after the input's slot 0 in Create's `ProcessingInventory`.
        let inventory = |slots: &[(String, u8)], applied: bool| -> Vec<Slot> {
            let base = usize::from(applied);
            slots
                .iter()
                .enumerate()
                .map(|(i, s)| slot(i + base, s))
                .collect()
        };
        match self.w.block_at(0, p)? {
            Block::Millstone => {
                let empty: MillSlots = Default::default();
                let s = self.millstone_slots.get(&p).unwrap_or(&empty);
                Some(Observation {
                    kind: "millstone".into(),
                    timer: Some(self.w.dims[0].millstones.get(&p).map_or(0, |m| m.timer)),
                    input: Some(stacks(&s[..1], 0)),
                    outputs: Some(stacks(&s[1..], 0)),
                    ..Default::default()
                })
            }
            Block::CrushingWheelController => {
                let (c, speed) = self.w.dims[0].crushers.get(&p).cloned().unwrap_or_default();
                Some(Observation {
                    kind: "crusher".into(),
                    speed: Some(speed),
                    entity: Some(c.entity.is_some()),
                    slots: Some(inventory(&c.slots, c.applied)),
                    remaining: Some(c.remaining),
                    duration: Some(0.0),
                    applied: Some(c.applied),
                    ..Default::default()
                })
            }
            Block::MechanicalSaw => {
                let s = self.w.dims[0].saws.get(&p).cloned().unwrap_or_default();
                let duration = if s.applied {
                    20.0
                } else if s.remaining < 0.0 {
                    0.0
                } else {
                    self.saw_duration.get(&p).copied().unwrap_or(0.0)
                };
                let index = if s.slots.is_empty() {
                    self.saw_index.get(&p).copied().unwrap_or(s.index)
                } else {
                    s.index
                };
                Some(Observation {
                    kind: "saw".into(),
                    slots: Some(inventory(&s.slots, s.applied)),
                    remaining: Some(s.remaining),
                    duration: Some(duration),
                    applied: Some(s.applied),
                    recipe_index: Some(index as i32),
                    ..Default::default()
                })
            }
            Block::MechanicalPress => {
                let press = self.w.dims[0].presses.get(&p).copied().unwrap_or_default();
                Some(Observation {
                    kind: "press".into(),
                    running: Some(press.running),
                    running_ticks: Some(press.ticks),
                    mode: Some(
                        match press.mode {
                            create_engine::press::Mode::World => "world",
                            create_engine::press::Mode::Belt => "belt",
                            create_engine::press::Mode::Basin => "basin",
                        }
                        .into(),
                    ),
                    ..Default::default()
                })
            }
            Block::MechanicalMixer => {
                let m = self.w.dims[0].mixers.get(&p).copied().unwrap_or_default();
                Some(Observation {
                    kind: "mixer".into(),
                    running: Some(m.running),
                    running_ticks: Some(m.running_ticks),
                    processing_ticks: Some(m.processing_ticks),
                    ..Default::default()
                })
            }
            _ => None,
        }
    }
}

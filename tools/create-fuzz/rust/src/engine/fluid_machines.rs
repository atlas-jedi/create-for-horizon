//! The item drain/spout fluid phase and player boundaries from glue/fluids/machines.rs.
use super::*;
use crate::transport::{Input, Outcome, Stack};
use create_engine::{Block, belt, depot, drain, spout};
impl Run {
    pub(super) fn fluid_item_input(&mut self, p: Pos, input: &Input) -> Option<Outcome> {
        if self.w.block_at(0, p) != Some(Block::ItemDrain) {
            return None;
        }
        let offered = match input {
            Input::DropOn { stack } => Some(stack.clone()),
            Input::Use { stack, .. } => stack.clone(),
        };
        if matches!(
            input,
            Input::Use {
                stack: Some(_),
                sneaking: true,
                ..
            }
        ) {
            return Some(Outcome {
                pos: p,
                offered: offered.clone(),
                remainder: offered,
                taken: Vec::new(),
                handler: None,
            });
        }
        let mut remainder = offered.clone();
        let mut taken = Vec::new();
        match input {
            Input::DropOn { stack } => {
                // glue/fluids/machines.rs `drain_landed`: in the way the item entity moves
                // (`drain::landing_side`); the harness lands it with no motion, so north.
                let got = drain::insert(
                    self.w.dims[0].drains.get_mut(&p).unwrap(),
                    &stack.item,
                    stack.count,
                    drain::landing_side([0.0; 3]),
                    &crate::fluid_recipes::emptying(),
                );
                remainder = (got < stack.count).then(|| Stack {
                    item: stack.item.clone(),
                    count: stack.count - got,
                });
                if got > 0 {
                    self.mark_column(p);
                }
            }
            Input::Use { stack, .. } => {
                if let Some(stack) = stack
                    && let Some(empty) = crate::fluid_recipes::contents(&stack.item)
                {
                    let d = self.w.dims[0].drains.get_mut(&p).unwrap();
                    if d.fluid.is_none_or(|(f, n)| {
                        f == empty.fluid && n + empty.amount <= drain::CAPACITY
                    }) {
                        d.fluid = Some((empty.fluid, d.fluid.map_or(0, |(_, n)| n) + empty.amount));
                        self.mark_column(p);
                        self.w.dims[0].analog_checks.push(p);
                        if !self.creative {
                            if stack.count == 1 {
                                remainder = empty.leftover.map(|item| Stack { item, count: 1 });
                            } else {
                                remainder = Some(Stack {
                                    item: stack.item.clone(),
                                    count: stack.count - 1,
                                });
                                if let Some(item) = empty.leftover {
                                    taken.push(Stack { item, count: 1 });
                                }
                            }
                        }
                    }
                } else if let Some(held) = self.w.dims[0].drains.get_mut(&p).unwrap().held.take() {
                    taken.push(Stack {
                        item: held.item,
                        count: held.count,
                    });
                    self.mark_column(p);
                }
            }
        }
        Some(Outcome {
            pos: p,
            offered,
            remainder,
            taken,
            handler: None,
        })
    }
    pub(super) fn spout_on_depot(&mut self, depot: Pos) {
        let p = add(depot, [0, 2, 0]);
        let Some(mut s) = self.w.dims[0].spouts.get(&p).copied() else {
            return;
        };
        let recipes = crate::fluid_recipes::filling();
        let Some(mut h) = self.w.dims[0].depots.get(&depot).copied().flatten() else {
            return;
        };
        {
            let depot = add(p, [0, -2, 0]);
            let held = self.depot_stacks.get(&depot).cloned();
            let before = s;
            let result = held
                .as_ref()
                .map(|stack| spout::on_depot(&mut s, &mut h, &stack.item, &recipes));
            // glue/fluids/machines.rs: the lock, and a filled stack back at the centre
            // (`spout::on_depot`).
            if let Some(Some(held)) = self.w.dims[0].depots.get_mut(&depot) {
                *held = depot::Held {
                    locked: !matches!(result, Some(spout::OnBelt::Pass)),
                    ..h
                };
            }
            let filled = match result {
                Some(spout::OnBelt::Fill(out)) => Some(out),
                _ => None,
            };
            if let (Some(item), Some(mut stack)) = (filled, held) {
                if stack.count == 1 {
                    stack.item = item;
                    self.depot_stacks.insert(depot, stack);
                } else {
                    stack.count -= 1;
                    self.depot_stacks.insert(depot, stack);
                    let filled = Stack { item, count: 1 };
                    // glue/fluids/machines.rs: onto a stack of it with room, else an empty slot.
                    let outputs = self.depot_outputs.entry(depot).or_default();
                    if outputs.insert_stacked(&filled) > 0 {
                        self.notes.push(format!(
                            "adapter gap: native spout overflow item flight at {depot:?}"
                        ));
                    }
                }
                self.w.depot_changed(0, depot);
            }
            self.put_spout(p, s, before);
        }
    }
    pub(super) fn tick_drain(&mut self, p: Pos) {
        for p in [p] {
            let result = drain::tick(
                self.w.dims[0].drains.get_mut(&p).unwrap(),
                &crate::fluid_recipes::emptying(),
            );
            if result == drain::Tick::Emptied {
                self.w.dims[0].analog_checks.push(p);
                self.mark_column(p);
            }
            let drain::Tick::Out(side) = result else {
                continue;
            };
            let held = self.w.dims[0].drains[&p].held.clone().unwrap();
            let next = add(p, side.offset());
            let left = if let Some(d) = self.w.dims[0].drains.get_mut(&next) {
                let got = drain::insert(
                    d,
                    &held.item,
                    held.count,
                    side,
                    &crate::fluid_recipes::emptying(),
                );
                if got > 0 {
                    self.mark_column(next);
                }
                Some(held.count - got)
            } else {
                // The glue's `insert_into_from(.., false)`: no belt behind the hand-over, so no
                // smoother belt-to-belt offset (`BeltBlockEntity.tryInsertingFromSide`).
                self.handle_insertion(
                    next,
                    side,
                    &belt::Item::new(&held.item, held.count, 0, false),
                    false,
                    false,
                )
            };
            let left = match left {
                Some(left) => left,
                // No input there: thrown off unless the block there turns a full face to the
                // drain (`solid_side`, `BlockHelper.hasBlockSolidSide`; the host's full-face
                // support), else it waits.
                None if !self.sturdy(next, side.opposite()) => {
                    let o = side.offset().map(f64::from);
                    let at = [
                        f64::from(p[0]) + 0.5 + o[0] * 0.75,
                        f64::from(p[1]) + 0.5 + o[1] * 0.75 + 6.0 / 16.0,
                        f64::from(p[2]) + 0.5 + o[2] * 0.75,
                    ];
                    let motion = [o[0] / 8.0, o[1] / 8.0 + 1.0 / 8.0, o[2] / 8.0];
                    self.spawn_item(at, motion, &held.item, held.count);
                    0
                }
                None => continue,
            };
            if left != held.count {
                let d = self.w.dims[0].drains.get_mut(&p).unwrap();
                if left == 0 {
                    d.held = None;
                } else {
                    d.held.as_mut().unwrap().count = left;
                }
                self.mark_column(p);
            }
        }
    }
    pub(super) fn spout_on_belt(
        &mut self,
        p: Pos,
        item: &mut belt::Item,
        locked: bool,
    ) -> Option<belt::Process> {
        let above = add(p, [0, 2, 0]);
        let mut s = self.w.dims[0].spouts.get(&above).copied()?;
        let before = s;
        let result = spout::on_belt(&mut s, &item.item, &crate::fluid_recipes::filling(), locked);
        self.put_spout(above, s, before);
        Some(match result {
            spout::OnBelt::Pass => belt::Process::Pass,
            spout::OnBelt::Hold => belt::Process::Hold,
            spout::OnBelt::Fill(out) => {
                item.fan.clear();
                let mut filled = item.processing_copy();
                filled.item = out;
                filled.count = 1;
                filled.progress = None;
                belt::Process::Convert {
                    held: item.left_after_one(),
                    outputs: vec![filled],
                }
            }
        })
    }
    /// glue/fluids/machines.rs `tick_spout_blocks` (`SpoutBlockEntity.tick`'s block spouting),
    /// the last of the fluids' level tick, before the transport walk runs the spouts' timers: an
    /// idle spout holding fluid over a block it can fill (`spout::block_fill`) starts its 20-tick
    /// cycle, and at 6 (the walk takes it to 5 later this tick) fills the block if it still can,
    /// the fluid leaving its tank. The spouts whose cycle it started (`BLOCK_FILLS`,
    /// `spout_block_fills`) are the ones it fills for; a belt or depot cycle at 6 is skipped.
    pub(super) fn tick_spout_blocks(&mut self) {
        let mut due: Vec<(Pos, spout::Spout)> = self.w.dims[0]
            .spouts
            .iter()
            .filter(|(_, s)| s.fluid.is_some() && matches!(s.processing, -1 | 6))
            .map(|(p, s)| (*p, *s))
            .collect();
        due.sort_by_key(|(p, _)| *p);
        for (p, mut s) in due {
            let custom = self.spout_block_fills.remove(&p);
            if s.processing == 6 && !custom {
                continue;
            }
            let Some((fluid, have)) = s.fluid else {
                continue;
            };
            let below = add(p, [0, -2, 0]);
            let fill = self.block_fill_at(below, fluid, have);
            let before = s;
            if s.processing == -1 {
                if spout::start_block_fill(&mut s, fill) {
                    self.spout_block_fills.insert(p);
                    self.put_spout(p, s, before);
                }
                continue;
            }
            // `customProcess.fillBlock(.., false)`: the block may have changed since.
            let Some((amount, change)) = fill else {
                continue;
            };
            if self.apply_block_fill(below, change) {
                spout::finish_block_fill(&mut s, amount);
                self.put_spout(p, s, before);
            }
        }
    }
    /// The glue's `block_fill_at`: what spouting the block at `at` with `have` of `fluid` would
    /// take and do (`BlockSpoutingBehaviour.get(level, pos).fillBlock(.., simulate)`).
    fn block_fill_at(
        &self,
        at: Pos,
        fluid: create_engine::tank::Fluid,
        have: u32,
    ) -> Option<(u32, spout::BlockFill)> {
        let (name, props) = parse_state(&self.name(self.at(at)));
        let property = |k: &str| {
            props
                .iter()
                .find(|(p, _)| p == k)
                .and_then(|(_, v)| v.parse::<i32>().ok())
        };
        spout::block_fill(&name, property, fluid, have)
    }
    /// The glue's `apply_block_fill`: `level.setBlockAndUpdate(pos, ..)` with what the spout made
    /// of the block at `at` (a block's default state, or an integer property one higher). False,
    /// filling nothing, for a state the host catalogue lacks.
    fn apply_block_fill(&mut self, at: Pos, fill: spout::BlockFill) -> bool {
        let next = match fill {
            spout::BlockFill::Become(block) => match block {
                // `LayeredCauldronBlock`'s default `LEVEL` is 1.
                "minecraft:water_cauldron" => Some(format!("{block}[level=1]")),
                _ => Some(block.to_owned()),
            },
            spout::BlockFill::Raise(property) => {
                let current = self.name(self.at(at));
                let (_, props) = parse_state(&current);
                props
                    .iter()
                    .find(|(k, _)| k == property)
                    .and_then(|(_, v)| v.parse::<i32>().ok())
                    .map(|v| with_prop(&current, property, &(v + 1).to_string()))
            }
        };
        let Some(next) = next.filter(|s| crate::fluid_world::id(s).is_some()) else {
            self.notes.push(format!(
                "adapter gap: spouted block at {at:?} has no host state ({fill:?})"
            ));
            return false;
        };
        self.set_world_state(at, &next);
        true
    }
    /// glue/fluids/machines.rs `put_spout`: a fill started or done marks the column, a tank
    /// that changed is read by comparators.
    fn put_spout(&mut self, p: Pos, s: spout::Spout, before: spout::Spout) {
        self.w.dims[0].spouts.insert(p, s);
        if s != before {
            self.mark_column(p);
        }
        if s.fluid != before.fluid {
            self.w.dims[0].analog_checks.push(p);
        }
    }
    pub(super) fn fluid_storage(&self, p: Pos) -> Option<crate::fluid_machines::Storage> {
        let (fluid, capacity) = if let Some(s) = self.w.dims[0].spouts.get(&p) {
            (s.fluid, spout::CAPACITY)
        } else {
            let d = self.w.dims[0].drains.get(&p)?;
            (d.fluid, drain::CAPACITY)
        };
        Some(crate::fluid_machines::Storage {
            capacity,
            fluid: fluid.map(|(f, _)| f.key()),
            amount: fluid.map_or(0, |(_, n)| n),
        })
    }
    pub(super) fn fluid_machine(&self, p: Pos) -> Option<crate::fluid_machines::Machine> {
        if let Some(s) = self.w.dims[0].spouts.get(&p) {
            return Some(crate::fluid_machines::Machine {
                processing: s.processing,
                held: None,
            });
        }
        let d = self.w.dims[0].drains.get(&p)?;
        Some(crate::fluid_machines::Machine {
            processing: d.processing,
            held: d.held.as_ref().map(|h| crate::fluid_machines::Held {
                item: h.item.clone(),
                count: h.count,
                position: h.position,
                from: format!("{:?}", h.from).to_lowercase(),
            }),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use create_engine::{kinetics::Dir, tank::Fluid};

    #[test]
    fn depot_filling_matches_glue_step_and_preserves_all_outputs() {
        let mut sc = crate::fluid_inputs::generate(19);
        sc.blocks.clear();
        sc.events.clear();
        let mut run = Run::new(&sc);
        let p = [0, 6, 0];
        let depot = [0, 4, 0];
        run.set(p, run.id("create:spout"));
        run.set(depot, run.id("create:depot[waterlogged=false]"));
        run.depot_stacks.insert(
            depot,
            Stack {
                item: "minecraft:glass_bottle".into(),
                count: 4,
            },
        );
        let mut direct = spout::Spout::default();
        spout::fill(&mut direct, Fluid::Tea, 1000);
        run.w.dims[0].spouts.insert(p, direct);
        let mut held = create_engine::depot::Held::new(180, false);
        run.w.depot_put(0, depot, Some(held));
        let mut left = 4;
        for _ in 0..100 {
            let item = if left > 0 {
                "minecraft:glass_bottle"
            } else {
                "create:builders_tea"
            };
            spout::tick(&mut direct);
            if matches!(
                spout::on_depot(
                    &mut direct,
                    &mut held,
                    item,
                    &crate::fluid_recipes::filling()
                ),
                spout::OnBelt::Fill(_)
            ) {
                left -= 1;
            }
            run.tick_transport();
            assert_eq!(run.w.dims[0].spouts[&p], direct);
            assert_eq!(run.depot_stacks[&depot].count, left.max(1));
        }
        assert_eq!(direct.fluid, None);
        assert_eq!(run.depot_stacks[&depot].item, "create:builders_tea");
        assert_eq!(
            run.depot_outputs[&depot]
                .iter()
                .cloned()
                .collect::<Vec<_>>(),
            vec![Stack {
                item: "create:builders_tea".into(),
                count: 3
            }]
        );
    }

    #[test]
    fn landed_bucket_matches_glue_drain_cycle_without_native_peer() {
        let mut sc = crate::fluid_inputs::generate(19);
        sc.blocks.clear();
        sc.events.clear();
        let mut run = Run::new(&sc);
        let p = [0, 4, 0];
        run.set(p, run.id("create:item_drain"));
        let input = Input::DropOn {
            stack: Stack {
                item: "minecraft:water_bucket".into(),
                count: 1,
            },
        };
        assert!(run.fluid_item_input(p, &input).unwrap().remainder.is_none());
        let mut direct = drain::Drain::default();
        drain::insert(
            &mut direct,
            "minecraft:water_bucket",
            1,
            Dir::North,
            &crate::fluid_recipes::emptying(),
        );
        for _ in 0..23 {
            drain::tick(&mut direct, &crate::fluid_recipes::emptying());
            run.tick_drain(p);
            assert_eq!(run.w.dims[0].drains[&p], direct);
        }
        assert_eq!(direct.fluid, Some((Fluid::Water, 1000)));
        assert_eq!(direct.held.unwrap().item, "minecraft:bucket");
    }

    fn dry_run() -> Run {
        let mut sc = crate::fluid_inputs::generate(19);
        sc.blocks.clear();
        sc.events.clear();
        Run::new(&sc)
    }

    #[test]
    fn spout_fills_the_cauldron_two_below_on_the_tick_create_does() {
        // `SpoutBlockEntity.tick`: idle with fluid over a fillable block, the cycle starts
        // (20, then 19 the same tick); the fill lands as the timer reaches 5.
        let mut run = dry_run();
        let (p, below) = ([0, 6, 0], [0, 4, 0]);
        run.set(p, run.id("create:spout"));
        run.set(below, run.id("minecraft:cauldron"));
        let mut s = spout::Spout::default();
        spout::fill(&mut s, Fluid::Water, 1000);
        run.w.dims[0].spouts.insert(p, s);
        let mut fills = Vec::new();
        for tick in 0..80 {
            let shown = run.name(run.at(below));
            run.tick_spout_blocks();
            run.tick_transport();
            if run.name(run.at(below)) != shown {
                let s = run.w.dims[0].spouts[&p];
                assert_eq!(s.processing, 5, "tick {tick}");
                fills.push((tick, run.name(run.at(below)), s.fluid));
            }
        }
        assert_eq!(
            fills,
            vec![
                (
                    14,
                    "minecraft:water_cauldron[level=1]".into(),
                    Some((Fluid::Water, 750))
                ),
                (
                    35,
                    "minecraft:water_cauldron[level=2]".into(),
                    Some((Fluid::Water, 500))
                ),
                (
                    56,
                    "minecraft:water_cauldron[level=3]".into(),
                    Some((Fluid::Water, 250))
                ),
            ]
        );
        // Full: the spout idles with what is left.
        assert_eq!(run.w.dims[0].spouts[&p].processing, -1);
        // One run at a time (`Run::new` takes the driver's lock).
        drop(run);
        // An empty cauldron takes lava a whole bucket at a time, and nothing less.
        let mut run = dry_run();
        run.set(p, run.id("create:spout"));
        run.set(below, run.id("minecraft:cauldron"));
        let mut s = spout::Spout::default();
        spout::fill(&mut s, Fluid::Lava, 999);
        run.w.dims[0].spouts.insert(p, s);
        run.tick_spout_blocks();
        assert_eq!(run.w.dims[0].spouts[&p].processing, -1);
        spout::fill(run.w.dims[0].spouts.get_mut(&p).unwrap(), Fluid::Lava, 1);
        for _ in 0..15 {
            run.tick_spout_blocks();
            run.tick_transport();
        }
        assert_eq!(run.name(run.at(below)), "minecraft:lava_cauldron");
        assert_eq!(run.w.dims[0].spouts[&p].fluid, None);
        // A cycle the block spouting did not start (a belt's or depot's) fills no block at 6,
        // though a fillable one is there now (`customProcess` null).
        run.set(below, run.id("minecraft:cauldron"));
        let mut s = spout::Spout::default();
        spout::fill(&mut s, Fluid::Water, 1000);
        s.processing = 6;
        run.w.dims[0].spouts.insert(p, s);
        run.tick_spout_blocks();
        assert_eq!(run.name(run.at(below)), "minecraft:cauldron");
        assert_eq!(run.w.dims[0].spouts[&p], s);
    }

    #[test]
    fn drain_throws_its_stack_off_unless_a_full_face_holds_it() {
        // `ItemDrainBlockEntity.tick`: at the far side with nothing taking it, the stack flies
        // off unless `BlockHelper.hasBlockSolidSide` holds it.
        for (beside, thrown) in [
            (None, true),
            (Some("minecraft:stone_slab[type=bottom,waterlogged=false]"), true),
            (Some("minecraft:cobblestone"), false),
        ] {
            let mut run = dry_run();
            let p = [0, 4, 0];
            run.set(p, run.id("create:item_drain"));
            if let Some(state) = beside {
                run.set([1, 4, 0], run.id(state));
            }
            drain::insert(
                run.w.dims[0].drains.get_mut(&p).unwrap(),
                "minecraft:cobblestone",
                3,
                Dir::East,
                &crate::fluid_recipes::emptying(),
            );
            for _ in 0..12 {
                run.tick_drain(p);
            }
            run.dispose_item_entities();
            let held = run.w.dims[0].drains[&p].held.clone();
            assert_eq!(held.is_none(), thrown, "{beside:?}");
            if thrown {
                let d = &run.drops[0];
                assert_eq!(run.drops.len(), 1);
                assert_eq!(d.stack.item, "minecraft:cobblestone");
                assert_eq!(d.stack.count, 3);
                assert_eq!(d.pos, [1.25, 4.5 + 6.0 / 16.0, 0.5]);
                assert_eq!(d.motion, [1.0 / 8.0, 1.0 / 8.0, 0.0]);
            } else {
                assert!(run.drops.is_empty());
                assert_eq!(held.unwrap().count, 3);
            }
        }
    }
}

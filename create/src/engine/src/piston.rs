//! Create's mechanical piston (`MechanicalPistonBlockEntity`, `LinearActuatorBlockEntity`,
//! `PistonContraption`): its poles behind it set how far it reaches; turning, it takes its head,
//! poles and the structure in front off the world and slides them `speed / 512` blocks a tick,
//! then puts them back where they stop.

use crate::contraption::{Found, Refused};
use crate::kinetics::{Axis, Dir};
use std::collections::{HashSet, VecDeque};

/// `maxAllowedPistonPoles` (`kinetics.maxPistonPoles`).
pub const MAX_POLES: i32 = 64;

/// A block as the piston's pole search reads it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PoleCell {
    /// A piston extension pole along an axis.
    Pole(Axis),
    /// A piston head facing a direction.
    Head(Dir),
    Other,
}

/// What a piston takes when it starts: its head and poles (`poles`, offsets from the anchor),
/// how far it can go and where it is.
#[derive(Clone, Debug, PartialEq)]
pub struct Collected {
    /// `anchor`: the block in front of the head, from which the structure is searched.
    pub anchor: [i32; 3],
    /// The poles and head, offset from the anchor, and whether each is the head.
    pub poles: Vec<([i32; 3], bool)>,
    /// `extensionLength`, `initialExtensionProgress`.
    pub range: i32,
    pub progress: i32,
}

fn add(a: [i32; 3], d: Dir, n: i32) -> [i32; 3] {
    let o = d.offset();
    [a[0] + o[0] * n, a[1] + o[1] * n, a[2] + o[2] * n]
}

/// `PistonContraption.collectExtensions` for the piston at `pos` facing `facing`, `extended` when
/// its state is `EXTENDED`. Refused without poles (`noPistonPoles`) or with too many
/// (`tooManyPistonPoles`).
pub fn collect(
    pos: [i32; 3],
    facing: Dir,
    extended: bool,
    cell: impl Fn([i32; 3]) -> PoleCell,
) -> Result<Collected, Refused> {
    let mut poles: Vec<([i32; 3], bool)> = Vec::new();
    let mut front = 0;
    if extended {
        let mut at = pos;
        loop {
            let next = add(at, facing, 1);
            match cell(next) {
                PoleCell::Pole(a) if a == facing.axis() => {
                    at = next;
                    poles.push((at, false));
                    front += 1;
                }
                PoleCell::Head(f) if f == facing => {
                    poles.push((next, true));
                    front += 1;
                    break;
                }
                _ => break,
            }
            if front > MAX_POLES {
                return Err(Refused::TooManyPistonPoles);
            }
        }
    }
    // The head sits in the piston while retracted; a pole there when extended.
    poles.push((pos, front == 0));
    let mut back = 0;
    let mut end = pos;
    while let PoleCell::Pole(a) = cell(add(end, facing.opposite(), 1)) {
        if a != facing.axis() {
            break;
        }
        end = add(end, facing.opposite(), 1);
        poles.push((end, false));
        back += 1;
        if front + back > MAX_POLES {
            return Err(Refused::TooManyPistonPoles);
        }
    }
    let range = front + back;
    if range == 0 {
        return Err(Refused::NoPistonPoles);
    }
    let anchor = add(pos, facing, front + 1);
    let poles = poles
        .into_iter()
        .map(|(p, head)| {
            let rel = add(p, facing, -front);
            (
                [rel[0] - anchor[0], rel[1] - anchor[1], rel[2] - anchor[2]],
                head,
            )
        })
        .collect();
    Ok(Collected {
        anchor,
        poles,
        range,
        progress: front,
    })
}

/// `TranslatingContraption.createColliders`: the blocks of a structure (offsets) with no block
/// of it next to them going `dir`, the ones that meet the world first.
pub fn colliders(blocks: &[([i32; 3], u32)], dir: Dir) -> Vec<[i32; 3]> {
    let set: HashSet<[i32; 3]> = blocks.iter().map(|(p, _)| *p).collect();
    blocks
        .iter()
        .map(|(p, _)| *p)
        .filter(|p| !set.contains(&add(*p, dir, 1)))
        .collect()
}

/// `ContraptionCollider.collideBlocks` for a piston's structure (offsets from `anchor` at
/// offset 0, along `facing`) moved to offset `to` going `dir`: the world cells its leading
/// blocks enter (`BlockPos.containing(position)`, a block further on going a positive way),
/// each with its block's offset.
pub fn entered_cells(
    anchor: [i32; 3],
    facing: Dir,
    to: f32,
    dir: Dir,
    colliders: &[[i32; 3]],
) -> Vec<([i32; 3], [i32; 3])> {
    let f = facing.offset();
    let mut grid = [0, 1, 2].map(|i| (anchor[i] as f32 + f[i] as f32 * to).floor() as i32);
    if dir.positive() {
        grid = add(grid, dir, 1);
    }
    colliders
        .iter()
        .map(|l| ([grid[0] + l[0], grid[1] + l[1], grid[2] + l[2]], *l))
        .collect()
}

/// `LinearActuatorBlockEntity.getGridOffset`.
pub fn grid_offset(offset: f32, range: i32) -> f32 {
    ((offset + 0.5) as i32).clamp(0, range) as f32
}

/// `Contraption.searchMovedStructure` from `start` with a forced direction: blocks stuck to each
/// other (slime, honey) and the block ahead in `forced` move together; an unmovable block ahead
/// refuses the move. Offsets from `start`, and states.
pub fn gather(
    start: [i32; 3],
    forced: Dir,
    exclude: &[[i32; 3]],
    at: impl Fn([i32; 3]) -> Option<Found>,
) -> Result<Vec<([i32; 3], u32)>, Refused> {
    let frontier = if at(start).is_some_and(|f| f.brittle) {
        VecDeque::new()
    } else {
        VecDeque::from([start])
    };
    search(start, frontier, Some(forced), exclude, at)
}

/// `kinetics.maxChassisRange`: how far ahead an extending piston pushes a line of blocks.
pub const MAX_CHASSIS_RANGE: i32 = 16;

/// `PistonContraption.assemble` (`searchMovedStructure` from the anchor in front of the head,
/// forced along `facing` while it extends and back toward the piston while it retracts) with its
/// `addToInitialFrontier`: extending, the line of blocks ahead (up to `maxChassisDistance`)
/// until air, a brittle block but a carpet, a piston head facing back, or a face that pushes
/// nothing; retracting, a sticky piston's block in front of its head alone (none that can't
/// move, nor a push-only one), and a plain piston's nothing. Offsets from `start`.
pub fn gather_piston(
    start: [i32; 3],
    facing: Dir,
    retracting: bool,
    sticky: bool,
    exclude: &[[i32; 3]],
    at: impl Fn([i32; 3]) -> Option<Found>,
) -> Result<Vec<([i32; 3], u32)>, Refused> {
    let mut frontier = VecDeque::new();
    if retracting && !sticky {
        return Ok(Vec::new());
    }
    // The direction the structure is searched in (`retract ? orientation.getOpposite()`).
    let direction = if retracting { facing.opposite() } else { facing };
    for offset in 0..=MAX_CHASSIS_RANGE {
        if offset == 1 && retracting {
            break;
        }
        let p = add(start, facing, offset);
        let Some(f) = at(p) else {
            break;
        };
        // `isBrittle(state) && !(state.getBlock() instanceof WoolCarpetBlock)`: a carpet is the
        // one brittle block whose only face that pushes nothing is its top.
        if f.brittle && f.unsupportive != Dir::Up.bit() {
            break;
        }
        if f.head == Some(direction.opposite()) {
            break;
        }
        if !f.movable {
            if retracting {
                break;
            }
            return Err(Refused::Unmovable(p));
        }
        if retracting && f.push_only {
            break;
        }
        frontier.push_back(p);
        if crate::contraption::not_supportive(&f, facing) {
            break;
        }
    }
    search(start, frontier, Some(direction), exclude, at)
}

/// `Contraption.searchMovedStructure` from `frontier`, pushing ahead in `forced`.
fn search(
    start: [i32; 3],
    frontier: VecDeque<[i32; 3]>,
    forced: Option<Dir>,
    exclude: &[[i32; 3]],
    at: impl Fn([i32; 3]) -> Option<Found>,
) -> Result<Vec<([i32; 3], u32)>, Refused> {
    let skip = |p: [i32; 3]| exclude.contains(&p);
    let rules = crate::contraption::Rules {
        forced,
        skip: &skip,
        stabilizes: &crate::contraption::never,
    };
    Ok(crate::contraption::search(frontier, start, &rules, &at)?.blocks)
}

/// `MechanicalPistonBlockEntity.getMovementSpeed` on the server: blocks a tick, positive out,
/// held between the ends.
pub fn movement(speed: f32, facing: Dir, offset: f32, range: i32) -> f32 {
    let m = (speed / 512.0).clamp(-0.49, 0.49);
    let step = if facing.positive() { 1.0 } else { -1.0 };
    let modifier = step * if facing.axis() == Axis::Z { -1.0 } else { 1.0 };
    (m * -modifier).clamp(-offset, range as f32 - offset)
}

/// A block as a broken piston part's search reads it (`playerWillDestroy`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PistonPart {
    /// A mechanical or sticky mechanical piston facing a direction.
    Base(Dir),
    Head(Dir),
    Pole(Axis),
    Other,
}

/// What a player breaking a piston part takes with it: the blocks destroyed with it (dropping
/// outside creative), and the piston left retracted (none when the piston itself broke).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Teardown {
    pub destroy: Vec<[i32; 3]>,
    pub retract: Option<[i32; 3]>,
}

/// `playerWillDestroy` of the piston (`MechanicalPistonBlock`: its poles and head in front, up
/// to the head facing its way, and its poles behind), its head (`MechanicalPistonHeadBlock`: the
/// poles back to the piston facing its way, which retracts) and a pole
/// (`PistonExtensionPoleBlock`: both ways along its axis to a head and a piston facing the same
/// way, the poles between going and the piston retracting). Each search goes no further than
/// `maxPistonPoles`.
pub fn teardown(pos: [i32; 3], broken: PistonPart, at: impl Fn([i32; 3]) -> PistonPart) -> Teardown {
    let mut out = Teardown::default();
    match broken {
        PistonPart::Base(d) => {
            let ahead = (1..MAX_POLES).find_map(|n| match at(add(pos, d, n)) {
                PistonPart::Pole(a) if a == d.axis() => None,
                PistonPart::Head(f) if f == d => Some(Some(n)),
                _ => Some(None),
            });
            if let Some(Some(head)) = ahead {
                out.destroy.extend((1..=head).map(|n| add(pos, d, n)));
            }
            for n in 1..MAX_POLES {
                let p = add(pos, d.opposite(), n);
                match at(p) {
                    PistonPart::Pole(a) if a == d.axis() => out.destroy.push(p),
                    _ => break,
                }
            }
        }
        PistonPart::Head(d) => {
            let back = (1..MAX_POLES).find_map(|n| match at(add(pos, d.opposite(), n)) {
                PistonPart::Pole(a) if a == d.axis() => None,
                PistonPart::Base(f) if f == d => Some(Some(n)),
                _ => Some(None),
            });
            if let Some(Some(base)) = back {
                out.destroy
                    .extend((1..base).map(|n| add(pos, d.opposite(), n)));
                out.retract = Some(add(pos, d.opposite(), base));
            }
        }
        PistonPart::Pole(axis) => {
            let positive = Dir::ALL
                .into_iter()
                .find(|d| d.axis() == axis && d.positive())
                .unwrap_or(Dir::Up);
            let (mut base, mut head) = (None, None);
            for way in [positive, positive.opposite()] {
                for n in 1..MAX_POLES {
                    let p = add(pos, way, n);
                    match at(p) {
                        PistonPart::Pole(a) if a == axis => continue,
                        PistonPart::Base(f) if f.axis() == axis => base = Some((p, f, n, way)),
                        PistonPart::Head(f) if f.axis() == axis => head = Some((p, f, n, way)),
                        _ => {}
                    }
                    break;
                }
            }
            if let (Some((b, bf, bn, bway)), Some((_, hf, hn, hway))) = (base, head)
                && bf == hf
            {
                out.destroy.extend((1..=hn).map(|n| add(pos, hway, n)));
                out.destroy.extend((1..bn).map(|n| add(pos, bway, n)));
                out.retract = Some(b);
            }
        }
        PistonPart::Other => {}
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A piston at the origin facing east with `poles` poles and its head after them.
    fn line(poles: i32) -> impl Fn([i32; 3]) -> PistonPart {
        move |p: [i32; 3]| match (p[0], p[1], p[2]) {
            (0, 0, 0) => PistonPart::Base(Dir::East),
            (x, 0, 0) if (1..=poles).contains(&x) => PistonPart::Pole(Axis::X),
            (x, 0, 0) if x == poles + 1 => PistonPart::Head(Dir::East),
            (x, 0, 0) if (-2..0).contains(&x) => PistonPart::Pole(Axis::X),
            _ => PistonPart::Other,
        }
    }

    #[test]
    fn breaking_a_piston_takes_its_poles_and_head() {
        let t = teardown([0, 0, 0], PistonPart::Base(Dir::East), line(3));
        let mut d = t.destroy.clone();
        d.sort();
        // The poles and head in front, and the two poles behind it.
        let want: Vec<[i32; 3]> = [-2, -1, 1, 2, 3, 4].iter().map(|&x| [x, 0, 0]).collect();
        assert_eq!(d, want);
        assert_eq!(t.retract, None);
    }

    #[test]
    fn breaking_a_head_or_pole_retracts_the_piston() {
        let t = teardown([4, 0, 0], PistonPart::Head(Dir::East), line(3));
        assert_eq!(t.destroy, vec![[3, 0, 0], [2, 0, 0], [1, 0, 0]]);
        assert_eq!(t.retract, Some([0, 0, 0]));
        let t = teardown([2, 0, 0], PistonPart::Pole(Axis::X), line(3));
        let mut d = t.destroy.clone();
        d.sort();
        assert_eq!(d, vec![[1, 0, 0], [3, 0, 0], [4, 0, 0]]);
        assert_eq!(t.retract, Some([0, 0, 0]));
        // A loose pole takes nothing.
        let loose = |p: [i32; 3]| {
            if p == [1, 0, 0] {
                PistonPart::Pole(Axis::X)
            } else {
                PistonPart::Other
            }
        };
        assert_eq!(teardown([0, 0, 0], PistonPart::Pole(Axis::X), loose), Teardown::default());
    }

    #[test]
    fn a_moving_structure_meets_the_cells_its_leading_blocks_enter() {
        // Two blocks in a row along x and one above the first; moving east the second and
        // the one above lead, moving west the first and the one above.
        let blocks = vec![([0, 0, 0], 1), ([1, 0, 0], 1), ([0, 1, 0], 1)];
        let mut east = colliders(&blocks, Dir::East);
        east.sort();
        assert_eq!(east, vec![[0, 1, 0], [1, 0, 0]]);
        let mut west = colliders(&blocks, Dir::West);
        west.sort();
        assert_eq!(west, vec![[0, 0, 0], [0, 1, 0]]);
        // Anchor at x 10, 0.3 out east: going east it enters the cells a block further on.
        let cells = entered_cells([10, 0, 0], Dir::East, 0.3, Dir::East, &[[1, 0, 0]]);
        assert_eq!(cells, vec![([12, 0, 0], [1, 0, 0])]);
        // Going back west from 0.3 it is entering the cells at its floor.
        let cells = entered_cells([10, 0, 0], Dir::East, 0.3, Dir::West, &[[0, 0, 0]]);
        assert_eq!(cells, vec![([10, 0, 0], [0, 0, 0])]);
        assert_eq!(grid_offset(2.4, 4), 2.0);
        assert_eq!(grid_offset(3.6, 3), 3.0);
    }

    #[test]
    fn a_retracted_piston_takes_its_poles_behind_it() {
        // A piston at x 0 facing east, three poles behind it (x -1..-3).
        let cell = |p: [i32; 3]| {
            if (-3..=-1).contains(&p[0]) && p[1] == 0 && p[2] == 0 {
                PoleCell::Pole(Axis::X)
            } else {
                PoleCell::Other
            }
        };
        let c = collect([0, 0, 0], Dir::East, false, cell).unwrap();
        assert_eq!((c.anchor, c.range, c.progress), ([1, 0, 0], 3, 0));
        assert_eq!(c.poles[0], ([-1, 0, 0], true), "the head, in the piston");
        assert_eq!(c.poles.last(), Some(&([-4, 0, 0], false)));
        assert_eq!(
            collect([0, 0, 0], Dir::East, false, |_| PoleCell::Other),
            Err(Refused::NoPistonPoles)
        );
    }

    #[test]
    fn glued_faces_and_an_extended_sticker_take_their_neighbours() {
        use crate::glue::{Glue, glued_faces};
        // Pushed east from x 1: stone at x 1, a stone glued on top of it (y 1), a loose stone
        // north of it, and an extended sticker at x 2 facing up holding a block at (2, 1, 0).
        let glue = [Glue::span([1, 0, 0], [1, 1, 0])];
        let at = |p: [i32; 3]| -> Option<Found> {
            let stone = Found {
                state: 1,
                movable: true,
                glued: glued_faces(&glue, p),
                ..Default::default()
            };
            match p {
                [1, 0, 0] | [1, 1, 0] | [1, 0, -1] | [2, 1, 0] => Some(stone),
                [2, 0, 0] => Some(Found {
                    state: 2,
                    sticker: Some(Dir::Up),
                    ..stone
                }),
                _ => None,
            }
        };
        let mut got = gather([1, 0, 0], Dir::East, &[], at).unwrap();
        got.sort();
        assert_eq!(
            got,
            vec![
                ([0, 0, 0], 1),
                ([0, 1, 0], 1),
                ([1, 0, 0], 2),
                ([1, 1, 0], 1)
            ],
            "the glued and stuck blocks come, the loose one stays"
        );
        // A sticker facing bedrock refuses the move.
        let stuck = |p: [i32; 3]| match p {
            [1, 0, 0] => Some(Found {
                movable: true,
                sticker: Some(Dir::Up),
                ..Default::default()
            }),
            [1, 1, 0] => Some(Found::default()),
            _ => None,
        };
        assert_eq!(
            gather([1, 0, 0], Dir::East, &[], stuck),
            Err(Refused::Unmovable([1, 1, 0]))
        );
    }

    #[test]
    fn it_pushes_what_is_ahead_and_moves_at_its_speed() {
        let stone = |p: [i32; 3]| {
            (p[0] >= 1 && p[0] <= 2 && p[1] == 0 && p[2] == 0).then_some(Found {
                state: 1,
                sail_facing: None,
                sticky: None,
                movable: true,
                chassis: None,
                ..Default::default()
            })
        };
        let got = gather([1, 0, 0], Dir::East, &[], stone).unwrap();
        assert_eq!(got, vec![([0, 0, 0], 1), ([1, 0, 0], 1)]);
        // Facing east: -64 rpm extends 0.125 blocks a tick, 64 retracts (none left at 0); the
        // far end stops it.
        assert_eq!(movement(64.0, Dir::East, 0.0, 3), 0.0);
        assert_eq!(movement(-64.0, Dir::East, 0.0, 3), 0.125);
        assert_eq!(movement(-64.0, Dir::East, 2.95, 3), 0.049999952);
        assert_eq!(movement(64.0, Dir::North, 1.0, 3), -0.125);
    }

    #[test]
    fn a_plain_piston_pulls_nothing_back_and_a_sticky_one_its_front_block() {
        // Stone in a row at x 1..3 in front of a head facing east; a torch on the first.
        let at = |p: [i32; 3]| match p {
            [1..=3, 0, 0] => Some(Found {
                state: 1,
                movable: true,
                ..Default::default()
            }),
            [1, 1, 0] => Some(Found {
                state: 2,
                movable: true,
                attached: Dir::Down.bit(),
                unsupportive: 0x3F,
                ..Default::default()
            }),
            _ => None,
        };
        let mut out = gather_piston([1, 0, 0], Dir::East, false, false, &[], at).unwrap();
        out.sort();
        assert_eq!(
            out,
            vec![
                ([0, 0, 0], 1),
                ([0, 1, 0], 2),
                ([1, 0, 0], 1),
                ([2, 0, 0], 1)
            ],
            "extending pushes the row and what hangs on it"
        );
        assert!(
            gather_piston([1, 0, 0], Dir::East, true, false, &[], at)
                .unwrap()
                .is_empty()
        );
        let mut back = gather_piston([1, 0, 0], Dir::East, true, true, &[], at).unwrap();
        back.sort();
        assert_eq!(back, vec![([0, 0, 0], 1), ([0, 1, 0], 2)]);
    }

    #[test]
    fn a_retracting_sticky_piston_pulls_along_its_way_back() {
        // A sticky head at x 0 facing east (excluded), a slime block at x 1 holding a stone
        // beside it (x 1, z 1), and a block behind that one toward the piston (x 0, z 1): the
        // slime and its stone come back, pushing the block in their way (`forcedDirection` is
        // toward the piston while retracting).
        let world = |behind: Found| {
            move |p: [i32; 3]| match p {
                [1, 0, 0] => Some(Found {
                    state: 1,
                    movable: true,
                    sticky: Some(crate::contraption::Sticky::Slime),
                    ..Default::default()
                }),
                [1, 0, 1] => Some(Found {
                    state: 2,
                    movable: true,
                    ..Default::default()
                }),
                [0, 0, 1] => Some(behind.clone()),
                _ => None,
            }
        };
        let stone = Found {
            state: 3,
            movable: true,
            ..Default::default()
        };
        let mut back =
            gather_piston([1, 0, 0], Dir::East, true, true, &[[0, 0, 0]], world(stone.clone()))
                .unwrap();
        back.sort();
        assert_eq!(back, vec![([-1, 0, 1], 3), ([0, 0, 0], 1), ([0, 0, 1], 2)]);
        // An unmovable block in that way refuses the move, named by the block pushing it.
        let obsidian = Found {
            movable: false,
            ..stone.clone()
        };
        assert_eq!(
            gather_piston([1, 0, 0], Dir::East, true, true, &[[0, 0, 0]], world(obsidian)),
            Err(Refused::Unmovable([1, 0, 1]))
        );
        // Push-only (glazed terracotta) in front of the head stays as the piston retracts, but
        // is pushed as it extends; a head facing back stops the line.
        let glazed = |p: [i32; 3]| {
            (p == [1, 0, 0]).then(|| Found {
                state: 4,
                movable: true,
                push_only: true,
                ..Default::default()
            })
        };
        assert!(
            gather_piston([1, 0, 0], Dir::East, true, true, &[], glazed)
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            gather_piston([1, 0, 0], Dir::East, false, true, &[], glazed).unwrap(),
            vec![([0, 0, 0], 4)]
        );
        let facing_back = |p: [i32; 3]| {
            (p == [1, 0, 0]).then(|| Found {
                state: 5,
                movable: true,
                head: Some(Dir::West),
                ..Default::default()
            })
        };
        assert!(
            gather_piston([1, 0, 0], Dir::East, false, false, &[], facing_back)
                .unwrap()
                .is_empty()
        );
    }
}

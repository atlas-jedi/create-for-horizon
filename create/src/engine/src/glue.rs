//! Create's super glue (`SuperGlueEntity`, `SuperGlueSelectionHelper`, `SuperGlueSelectionPacket`)
//! as plain data: a glue area is a box of whole blocks; two blocks are glued along a face when
//! one area holds both, and a contraption's structure search then takes the other block too.
//! Also the sticker (`StickerBlock`): extended, it holds on to the block it faces.

use crate::contraption::{Found, attached_towards, not_supportive};
use crate::kinetics::Dir;
use std::collections::HashSet;

/// A glue area (`SuperGlueEntity`'s bounding box): the blocks `min..=max` on every axis.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Glue {
    pub min: [i32; 3],
    pub max: [i32; 3],
}

fn add(a: [i32; 3], b: [i32; 3]) -> [i32; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

impl Glue {
    /// `SuperGlueEntity.span`: the box from one block to another, both included.
    pub fn span(a: [i32; 3], b: [i32; 3]) -> Glue {
        Glue {
            min: [0, 1, 2].map(|i| a[i].min(b[i])),
            max: [0, 1, 2].map(|i| a[i].max(b[i])),
        }
    }

    /// `contains`: the block's centre is inside the box.
    pub fn contains(&self, p: [i32; 3]) -> bool {
        (0..3).all(|i| self.min[i] <= p[i] && p[i] <= self.max[i])
    }

    /// `AABB.getSize()` (the mean edge) above 1.01: more than one block.
    pub fn more_than_one(&self) -> bool {
        let edges: i32 = (0..3).map(|i| self.max[i] - self.min[i] + 1).sum();
        f64::from(edges) / 3.0 > 1.01
    }

    /// The column whose chunk holds the entity (`resetPositionToBB`: the box's centre in x and
    /// z, its floor in y), which saves and loads it.
    pub fn column(&self) -> (i32, i32) {
        let centre = |i: usize| (f64::from(self.min[i]) + f64::from(self.max[i]) + 1.0) / 2.0;
        (
            (centre(0).floor() as i32) >> 4,
            (centre(2).floor() as i32) >> 4,
        )
    }

    /// The box moved by whole blocks.
    pub fn moved(&self, by: [i32; 3]) -> Glue {
        Glue {
            min: add(self.min, by),
            max: add(self.max, by),
        }
    }
}

/// `SuperGlueEntity.isGlued`: an area holds both the block and its neighbour toward `d`.
pub fn is_glued(areas: &[Glue], pos: [i32; 3], d: Dir) -> bool {
    let next = add(pos, d.offset());
    areas.iter().any(|g| g.contains(pos) && g.contains(next))
}

/// The faces of `pos` some area glues (`Dir::bit` each), for the structure search.
pub fn glued_faces(areas: &[Glue], pos: [i32; 3]) -> u8 {
    let mut faces = 0;
    for g in areas.iter().filter(|g| g.contains(pos)) {
        for d in Dir::ALL {
            if g.contains(add(pos, d.offset())) {
                faces |= d.bit();
            }
        }
    }
    faces
}

/// Why glue could not be put down (`SuperGlueSelectionPacket.handle` returns early).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refused {
    /// `to.closerThan(from, 25)` fails.
    TooFar,
    /// `searchGlueGroup` found nothing that holds together, or not the second corner.
    NotConnected,
}

/// `SuperGlueEntity.isValidFace`: the face `d` of a block (None: one that needs no moving, or
/// air) can hold glue: it hangs on toward it, or it needs moving and the face pushes something.
pub fn valid_face(f: Option<&Found>, d: Dir) -> bool {
    f.is_some_and(|f| attached_towards(f, d) || !not_supportive(f, d))
}

/// `SuperGlueSelectionHelper.searchGlueGroup(level, from, to, includeOther)`: from `from`, the
/// blocks the glue from `from` to `to` would hold together: each neighbour inside the box, or
/// outside it already stuck on (its side toward the block is sticky, `isSideSticky`; with
/// `include_other`, also the block's own side, or glue already there, `isGlued`), that needs
/// moving, the faces between them both able to hold glue (`isValidFace`). None when that is just
/// the second corner alone. `at` reads a block (None: needs no moving), `sticky` its sticky sides
/// (`Dir::bit` each), `glued` whether glue already holds a block toward a side.
pub fn search_group(
    from: [i32; 3],
    to: [i32; 3],
    include_other: bool,
    at: impl Fn([i32; 3]) -> Option<Found>,
    sticky: impl Fn([i32; 3]) -> u8,
    glued: impl Fn([i32; 3], Dir) -> bool,
) -> Option<HashSet<[i32; 3]>> {
    let area = Glue::span(from, to);
    let mut frontier = vec![from];
    let mut visited = HashSet::from([from]);
    let mut attached = HashSet::new();
    while !frontier.is_empty() {
        let current = frontier.remove(0);
        attached.insert(current);
        let here = at(current);
        for d in Dir::ALL {
            let next = add(current, d.offset());
            let glue_present = include_other && glued(current, d);
            let already = (include_other && sticky(current) & d.bit() != 0)
                || sticky(next) & d.opposite().bit() != 0;
            if !already && !glue_present && !area.contains(next) {
                continue;
            }
            let there = at(next);
            if there.is_none() {
                continue;
            }
            if !valid_face(here.as_ref(), d) || !valid_face(there.as_ref(), d.opposite()) {
                continue;
            }
            if visited.insert(next) {
                frontier.push(next);
            }
        }
    }
    if attached.len() < 2 && attached.contains(&to) {
        return None;
    }
    Some(attached)
}

/// `squaredDistance < r²` between whole blocks (`Vec3i.closerThan`).
fn closer_than(a: [i32; 3], b: [i32; 3], r: i64) -> bool {
    let d2: i64 = (0..3).map(|i| i64::from(a[i] - b[i]).pow(2)).sum();
    d2 < r * r
}

/// What a second click with the glue does on the client (`SuperGlueSelectionHandler`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SecondClick {
    /// 24 blocks or more from the first corner: `super_glue.too_far`, the corner kept.
    TooFar,
    /// No group (`searchGlueGroup` gave none: the first corner clicked again): this block is
    /// the first corner now (`super_glue.first_pos`).
    FirstAgain,
    /// The group doesn't reach it: `super_glue.cannot_reach`, the corner kept.
    CannotReach,
    /// `confirm`: the selection is sent, the corner forgotten (`super_glue.success`).
    Confirm,
}

/// `SuperGlueSelectionHandler.tick` and `onMouseInput` for a click on `pos` with the first
/// corner at `first`; `group` is `searchGlueGroup(first, pos, true)`.
pub fn second_click(
    first: [i32; 3],
    pos: [i32; 3],
    group: Option<&HashSet<[i32; 3]>>,
) -> SecondClick {
    if !closer_than(first, pos, 24) {
        return SecondClick::TooFar;
    }
    match group {
        None => SecondClick::FirstAgain,
        Some(g) if !g.contains(&pos) => SecondClick::CannotReach,
        Some(_) => SecondClick::Confirm,
    }
}

/// `SuperGlueSelectionPacket.handle` on the server (the player's reach and the item's charges
/// aside): the area from `from` to `to`, if they are under 25 blocks apart and the group
/// (`searchGlueGroup(from, to, false)`) holds the second corner.
pub fn place(
    from: [i32; 3],
    to: [i32; 3],
    at: impl Fn([i32; 3]) -> Option<Found>,
    sticky: impl Fn([i32; 3]) -> u8,
) -> Result<Glue, Refused> {
    if !closer_than(to, from, 25) {
        return Err(Refused::TooFar);
    }
    match search_group(from, to, false, at, sticky, |_, _| false) {
        Some(group) if group.contains(&to) => Ok(Glue::span(from, to)),
        _ => Err(Refused::NotConnected),
    }
}

/// `AABB.clip(from, to)`: where the segment first enters the box `min..max` through one of its
/// faces (a start inside the box enters none).
pub fn clip(min: [f64; 3], max: [f64; 3], from: [f64; 3], to: [f64; 3]) -> Option<[f64; 3]> {
    let delta = [0, 1, 2].map(|i| to[i] - from[i]);
    let mut best = 1.0;
    let mut hit = false;
    for axis in 0..3 {
        let plane = if delta[axis] > 1.0e-7 {
            min[axis]
        } else if delta[axis] < -1.0e-7 {
            max[axis]
        } else {
            continue;
        };
        let t = (plane - from[axis]) / delta[axis];
        let (a, b) = ((axis + 1) % 3, (axis + 2) % 3);
        let pa = from[a] + t * delta[a];
        let pb = from[b] + t * delta[b];
        if 0.0 < t
            && t < best
            && min[a] - 1.0e-7 < pa
            && pa < max[a] + 1.0e-7
            && min[b] - 1.0e-7 < pb
            && pb < max[b] + 1.0e-7
        {
            best = t;
            hit = true;
        }
    }
    hit.then(|| [0, 1, 2].map(|i| from[i] + best * delta[i]))
}

/// `SuperGlueSelectionHandler.tick`'s pick: of the areas (whole-block boxes), the one the look
/// from `eye` to `target` enters nearest the eye (`getBoundingBox().clip`), and where it enters.
pub fn pick(areas: &[Glue], eye: [f64; 3], target: [f64; 3]) -> Option<(usize, [f64; 3])> {
    let mut best: Option<(usize, [f64; 3], f64)> = None;
    for (i, g) in areas.iter().enumerate() {
        let min = g.min.map(f64::from);
        let max = g.max.map(|v| f64::from(v) + 1.0);
        let Some(p) = clip(min, max, eye, target) else {
            continue;
        };
        let d2: f64 = (0..3).map(|k| (p[k] - eye[k]).powi(2)).sum();
        if best.is_some_and(|b| d2 > b.2) {
            continue;
        }
        best = Some((i, p, d2));
    }
    best.map(|(i, p, _)| (i, p))
}

/// `SuperGlueEntity.spawnParticles`: slime along the twelve edges of the area, every half
/// block (world positions).
pub fn edge_points(g: &Glue) -> Vec<[f64; 3]> {
    let origin = g.min.map(f64::from);
    let size = [0, 1, 2].map(|i| f64::from(g.max[i] - g.min[i] + 1));
    let mut out = Vec::new();
    for axis in 0..3 {
        let (a2, a3) = match axis {
            0 => (1, 2),
            1 => (0, 2),
            _ => (0, 1),
        };
        let steps = (size[axis] * 2.0) as i32;
        for i in 0..=steps {
            for o1 in [0.0, 1.0] {
                for o2 in [0.0, 1.0] {
                    let mut v = origin;
                    v[axis] += f64::from(i) / 2.0;
                    v[a2] += size[a2] * o1;
                    v[a3] += size[a3] * o2;
                    out.push(v);
                }
            }
        }
    }
    out
}

/// What a contraption takes of the glue (`glueToRemove`, `removeBlocksFromWorld`): the areas
/// that glued one of its blocks to a neighbour the search could take (`skip` names the
/// anchoring blocks), and each shrunk to the blocks it holds of the contraption, kept when it
/// still holds more than one (offsets as `blocks`). The others stay in the world.
pub fn carried(
    areas: &[Glue],
    blocks: &[[i32; 3]],
    skip: impl Fn([i32; 3]) -> bool,
) -> (Vec<usize>, Vec<Glue>) {
    let mut used = Vec::new();
    let mut shrunk = Vec::new();
    for (i, g) in areas.iter().enumerate() {
        let glues = blocks.iter().any(|&p| {
            g.contains(p)
                && Dir::ALL.into_iter().any(|d| {
                    let n = add(p, d.offset());
                    !skip(n) && g.contains(n)
                })
        });
        if !glues {
            continue;
        }
        used.push(i);
        let mut bounds: Option<Glue> = None;
        for &p in blocks.iter().filter(|&&p| g.contains(p)) {
            bounds = Some(match bounds {
                None => Glue { min: p, max: p },
                Some(b) => Glue {
                    min: [0, 1, 2].map(|a| b.min[a].min(p[a])),
                    max: [0, 1, 2].map(|a| b.max[a].max(p[a])),
                },
            });
        }
        if let Some(b) = bounds.filter(Glue::more_than_one) {
            shrunk.push(b);
        }
    }
    (used, shrunk)
}

/// `StickerBlock.neighborChanged`: a change of power flips `powered`; turned on, it also flips
/// `extended`. The new (powered, extended), or None when nothing changes.
pub fn sticker_neighbor(powered: bool, extended: bool, signal: bool) -> Option<(bool, bool)> {
    if powered == signal {
        return None;
    }
    Some((signal, if signal { !extended } else { extended }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glue_holds_faces_inside_its_area() {
        let g = Glue::span([2, 0, 0], [0, 1, 0]);
        assert_eq!((g.min, g.max), ([0, 0, 0], [2, 1, 0]));
        assert!(g.contains([1, 1, 0]) && !g.contains([3, 0, 0]));
        assert!(is_glued(&[g], [0, 0, 0], Dir::East));
        assert!(!is_glued(&[g], [2, 0, 0], Dir::East));
        assert_eq!(
            glued_faces(&[g], [0, 0, 0]),
            Dir::East.bit() | Dir::Up.bit()
        );
        assert!(!Glue::span([0, 0, 0], [0, 0, 0]).more_than_one());
        assert!(Glue::span([0, 0, 0], [1, 0, 0]).more_than_one());
        assert_eq!(Glue::span([-3, 0, 0], [-1, 0, 0]).column(), (-1, 0));
    }

    #[test]
    fn the_glue_group_needs_both_corners_held_together() {
        let stone = || Found {
            state: 1,
            movable: true,
            ..Found::default()
        };
        let none = |_: [i32; 3]| 0u8;
        // Stone at x 0..=2 except a gap at 1: from 0 to 2 the group is just the first block.
        let gap = |p: [i32; 3]| (p[1] == 0 && p[2] == 0 && (p[0] == 0 || p[0] == 2)).then(stone);
        assert_eq!(
            place([0, 0, 0], [2, 0, 0], gap, none),
            Err(Refused::NotConnected)
        );
        let row = |p: [i32; 3]| (p[1] == 0 && p[2] == 0 && (0..=2).contains(&p[0])).then(stone);
        assert_eq!(
            place([0, 0, 0], [2, 0, 0], row, none),
            Ok(Glue::span([0, 0, 0], [2, 0, 0]))
        );
        assert_eq!(
            place([0, 0, 0], [30, 0, 0], row, none),
            Err(Refused::TooFar)
        );
        // A slime block beside the box joins the group by its sticky side.
        let with_slime = |p: [i32; 3]| match p {
            [0..=2, 0, 0] | [0, 1, 0] => Some(stone()),
            _ => None,
        };
        let slime = |p: [i32; 3]| if p == [0, 1, 0] { 0x3f } else { 0 };
        let group = search_group([0, 0, 0], [2, 0, 0], false, with_slime, slime, |_, _| false)
            .unwrap();
        assert!(group.contains(&[0, 1, 0]) && group.len() == 4);
        // The first corner clicked again: no group.
        assert_eq!(
            search_group([0, 0, 0], [0, 0, 0], true, row, none, |_, _| false),
            None
        );
    }

    #[test]
    fn glue_holds_only_faces_that_can_take_it() {
        // A drill at x 1 facing east (its head pushes nothing) between two stones: from x 0 to
        // x 2 the glue reaches the drill but not past its head.
        let at = |p: [i32; 3]| {
            (p[1] == 0 && p[2] == 0 && (0..=2).contains(&p[0])).then(|| Found {
                state: 1,
                movable: true,
                unsupportive: if p[0] == 1 { Dir::East.bit() } else { 0 },
                ..Found::default()
            })
        };
        let g = search_group([0, 0, 0], [2, 0, 0], false, at, |_| 0, |_, _| false).unwrap();
        assert!(g.contains(&[1, 0, 0]) && !g.contains(&[2, 0, 0]));
        assert_eq!(
            second_click([0, 0, 0], [2, 0, 0], Some(&g)),
            SecondClick::CannotReach
        );
        // Existing glue counts only for the client's search (`includeOther`).
        let lone = |p: [i32; 3]| {
            (p == [0, 0, 0] || p == [0, 1, 0]).then(|| Found {
                state: 1,
                movable: true,
                ..Found::default()
            })
        };
        let glued = |p: [i32; 3], d: Dir| p == [0, 0, 0] && d == Dir::Up;
        let mine = search_group([0, 0, 0], [0, 0, 0], true, lone, |_| 0, glued).unwrap();
        assert!(mine.contains(&[0, 1, 0]));
        assert_eq!(
            search_group([0, 0, 0], [0, 0, 0], false, lone, |_| 0, glued),
            None
        );
    }

    #[test]
    fn a_second_click_too_far_keeps_the_first_corner() {
        let g: HashSet<[i32; 3]> = [[0, 0, 0], [23, 0, 0]].into();
        assert_eq!(
            second_click([0, 0, 0], [24, 0, 0], Some(&g)),
            SecondClick::TooFar
        );
        assert_eq!(
            second_click([0, 0, 0], [23, 0, 0], Some(&g)),
            SecondClick::Confirm
        );
        assert_eq!(second_click([0, 0, 0], [0, 0, 0], None), SecondClick::FirstAgain);
    }

    #[test]
    fn the_look_picks_the_nearest_glue_it_enters() {
        let far = Glue::span([5, 0, 0], [6, 0, 0]);
        let near = Glue::span([2, 0, 0], [3, 0, 0]);
        let around = Glue::span([-1, -1, -1], [1, 1, 1]);
        let eye = [0.5, 0.5, 0.5];
        let target = [9.5, 0.5, 0.5];
        // The area the eye is in is never picked (`clip` enters no face from inside).
        let (i, at) = pick(&[far, around, near], eye, target).unwrap();
        assert_eq!(i, 2);
        assert!((at[0] - 2.0).abs() < 1e-9);
        assert_eq!(pick(&[around], eye, target), None);
        assert_eq!(pick(&[far], eye, [4.0, 0.5, 0.5]), None, "out of reach");
        // Twelve edges every half block: a single block has 3 halves per edge.
        assert_eq!(edge_points(&Glue::span([0, 0, 0], [0, 0, 0])).len(), 36);
    }

    #[test]
    fn a_contraption_takes_the_glue_it_used_shrunk_to_its_blocks() {
        let areas = [
            Glue::span([0, 0, 0], [5, 0, 0]),
            Glue::span([10, 0, 0], [11, 0, 0]),
        ];
        let (used, shrunk) = carried(&areas, &[[0, 0, 0], [1, 0, 0]], |_| false);
        assert_eq!(used, vec![0]);
        assert_eq!(shrunk, vec![Glue::span([0, 0, 0], [1, 0, 0])]);
        // One block of it only: used, but nothing left to carry.
        let (used, shrunk) = carried(&areas, &[[5, 0, 0]], |_| false);
        assert_eq!((used, shrunk), (vec![0], vec![]));
    }

    #[test]
    fn a_sticker_extends_on_each_rising_signal() {
        assert_eq!(sticker_neighbor(false, false, true), Some((true, true)));
        assert_eq!(sticker_neighbor(true, true, false), Some((false, true)));
        assert_eq!(sticker_neighbor(false, true, true), Some((true, false)));
        assert_eq!(sticker_neighbor(true, true, true), None);
    }
}

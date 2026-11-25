//! Create's contraptions (`Contraption.searchMovedStructure`, `BlockMovementChecks`), for the
//! windmill bearing first (`BearingContraption`, `WindmillBearingBlockEntity`): the structure in
//! front of the bearing, as Create gathers it, and the speed its sails give.

use crate::kinetics::Dir;
use std::collections::{HashSet, VecDeque};

/// A block as the search reads it: its id, whether it is in `create:windmill_sails` (and so a
/// `SailBlock` and its `facing`), whether it is slime or honey, and whether it can move at all.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Found {
    pub state: u32,
    pub sail_facing: Option<Dir>,
    /// In `create:windmill_sails` without being a `SailBlock` (wool, a copycat of wool:
    /// `BearingContraption.getSailBlock`): it counts toward a windmill's sails, nothing more.
    pub sail: bool,
    pub sticky: Option<Sticky>,
    pub movable: bool,
    /// A linear chassis (`LinearChassisBlock`) and what it holds on to.
    pub chassis: Option<Chassis>,
    /// Its faces super glue holds to the block beside them (`SuperGlueEntity.isGlued`, a
    /// `Dir::bit` each).
    pub glued: u8,
    /// An extended sticker's facing (`StickerBlock.EXTENDED`): it takes the block it faces.
    pub sticker: Option<Dir>,
    /// The sides it hangs on to the block beside (`BlockMovementChecks.isBlockAttachedTowards`
    /// for all but sails: a torch down, a ladder or button the way it faces), a `Dir::bit` each.
    pub attached: u8,
    /// The faces that push nothing in front of them (`BlockMovementChecks.isNotSupportive`: a
    /// drill's head, a sail's flat sides, anything brittle), a `Dir::bit` each.
    pub unsupportive: u8,
    /// A belt segment (`BeltBlock`): the whole belt moves with it (`moveBelt`).
    pub belt: Option<crate::belt::Segment>,
    /// `BlockMovementChecks.isBrittle`: it hangs on to another block (a torch, a ladder, a
    /// door, a carpet). Slime and honey never pull it in, a chassis stops at it and a search
    /// does not start from it.
    pub brittle: bool,
    /// `PushReaction.PUSH_ONLY` (glazed terracotta, gearboxes): it sticks to no slime or honey
    /// either way, and a retracting piston does not pull it.
    pub push_only: bool,
    /// What moving this block brings with it (`Contraption.moveBlock`), world positions joining
    /// the search: the block a windmill bearing faces (`moveWindmillBearing`), the block under a
    /// pulley's rope (`movePulley`), a piston's poles, head and the block it holds
    /// (`moveMechanicalPiston`, `movePistonPole`, `movePistonHead`), a cart assembler under it,
    /// the other half of a double chest, a gantry carriage's front and shafts and a gantry
    /// shaft's line and carriages (`moveGantryPinion`, `moveGantryShaft`).
    pub drags: Vec<[i32; 3]>,
    /// Blocks it takes along as they are, with no checks (`movePulley`'s rope and magnet,
    /// `addBlock`): world positions.
    pub carries: Vec<[i32; 3]>,
    /// A mechanical bearing's facing (`moveBearing`): the structure decides whether what it
    /// faces rides on its own, stabilized (`canBeStabilized`), or joins this one.
    pub bearing: Option<Dir>,
    /// A mechanical piston head's facing (`isPistonHead`): an extending piston's line of blocks
    /// stops at a head facing back at it (`PistonContraption.addToInitialFrontier`).
    pub head: Option<Dir>,
    /// A mechanical piston in the middle of moving (`moveMechanicalPiston` with `MOVING`): the
    /// search gives up and nothing assembles (no assembly exception).
    pub halts: bool,
}

/// `Contraption.moveBlock`'s extended sticker: the block it faces joins the frontier (an
/// unmovable one there then refuses the structure).
pub(crate) fn sticker_target(pos: [i32; 3], f: &Found) -> Option<[i32; 3]> {
    f.sticker.map(|d| add(pos, d.offset()))
}

/// How a kind of structure treats what it meets (`Contraption`'s overrides).
pub struct Rules<'a> {
    /// `forcedDirection`: the way it is pushed (a piston's), which takes the block ahead of
    /// each of its blocks and refuses the structure when that block can't move; none for the
    /// structures that turn or hang.
    pub forced: Option<Dir>,
    /// `isAnchoringBlockAt`: what it never takes (its bearing, its piston and poles).
    pub skip: &'a dyn Fn([i32; 3]) -> bool,
    /// `canBeStabilized(facing, localPos)` for a mechanical bearing on it at a world position
    /// facing `facing`: what that bearing faces then rides on its own, stabilized
    /// (`pendingSubContraptions`), instead of joining this structure.
    pub stabilizes: &'a dyn Fn(Dir, [i32; 3]) -> bool,
}

/// Never stabilized (`TranslatingContraption.canBeStabilized` with the default
/// `stabiliseStableContraptions` off, `StabilizedContraption`, `CarriageContraption`).
pub fn never(_: Dir, _: [i32; 3]) -> bool {
    false
}

/// What a search gathered: each block's offset from the origin and state, its sails
/// (`BearingContraption.sailBlocks`), and the mechanical bearings on it whose faced structure
/// rides on its own, stabilized (`pendingSubContraptions`: each bearing's world position and
/// facing).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Searched {
    pub blocks: Vec<([i32; 3], u32)>,
    pub sails: u32,
    pub subs: Vec<([i32; 3], Dir)>,
}

/// `Contraption.moveBlock`'s slime and honey: `!isBrittle(other) && canStickTo` both ways, and
/// neither push-only nor facing the other with a face that pushes nothing. `d` goes from `a`
/// to `b`.
pub(crate) fn can_stick(a: &Found, b: &Found, d: Dir) -> bool {
    !b.brittle
        && sticks(a, b)
        && !a.push_only
        && !b.push_only
        && !not_supportive(a, d)
        && !not_supportive(b, d.opposite())
}

/// `searchMovedStructure`'s start: the block there unless it is brittle.
fn starts_at(start: [i32; 3], at: &impl Fn([i32; 3]) -> Option<Found>) -> VecDeque<[i32; 3]> {
    if at(start).is_some_and(|f| f.brittle) {
        VecDeque::new()
    } else {
        VecDeque::from([start])
    }
}

/// `Contraption.searchMovedStructure`'s loop over `frontier` (`moveBlock`): each block that
/// needs moving joins, with what its chassis, sticker, belt, bearings, pulleys and pistons bring
/// (`drags`, `carries`), and each neighbour that slime or honey holds, that hangs on to it, that
/// glue holds to it, or that is ahead the way it is pushed (`rules.forced`). Offsets from
/// `origin`.
pub fn search(
    mut frontier: VecDeque<[i32; 3]>,
    origin: [i32; 3],
    rules: &Rules<'_>,
    at: &impl Fn([i32; 3]) -> Option<Found>,
) -> Result<Searched, Refused> {
    let mut visited: HashSet<[i32; 3]> = HashSet::new();
    let mut taken: HashSet<[i32; 3]> = HashSet::new();
    let mut out = Searched::default();
    let skip = rules.skip;
    let forced = rules.forced;
    let local = |p: [i32; 3]| [p[0] - origin[0], p[1] - origin[1], p[2] - origin[2]];
    while let Some(pos) = frontier.pop_front() {
        if !visited.insert(pos) || skip(pos) {
            continue;
        }
        let Some(found) = at(pos) else {
            continue;
        };
        if !found.movable {
            return Err(Refused::Unmovable(pos));
        }
        // `moveMechanicalPiston`: a moving piston ends the search, and nothing assembles.
        if found.halts {
            return Err(Refused::Empty);
        }
        let join = |p: [i32; 3], frontier: &mut VecDeque<[i32; 3]>| {
            if !visited.contains(&p) && !skip(p) {
                frontier.push_back(p);
            }
        };
        if let Some(c) = found.chassis {
            for p in chassis_reach(pos, c, forced, at) {
                join(p, &mut frontier);
            }
        }
        for p in belt_neighbours(pos, &found) {
            join(p, &mut frontier);
        }
        // An extended sticker takes what it faces, unless that face of it pushes nothing.
        if let (Some(p), Some(d)) = (sticker_target(pos, &found), found.sticker)
            && at(p).is_none_or(|o| !not_supportive(&o, d.opposite()))
        {
            join(p, &mut frontier);
        }
        for &p in &found.drags {
            join(p, &mut frontier);
        }
        // `moveBearing`: what a mechanical bearing faces joins, unless it rides stabilized.
        if let Some(f) = found.bearing {
            if (rules.stabilizes)(f, pos) {
                if !out.subs.contains(&(pos, f)) {
                    out.subs.push((pos, f));
                }
            } else {
                join(add(pos, f.offset()), &mut frontier);
            }
        }
        for d in Dir::ALL {
            let next = add(pos, d.offset());
            if skip(next) {
                continue;
            }
            let Some(other) = at(next) else {
                continue;
            };
            if !other.movable {
                // `unmovableBlock(pos, state)`: named by the block pushing it.
                if Some(d) == forced {
                    return Err(Refused::Unmovable(pos));
                }
                continue;
            }
            if visited.contains(&next) {
                continue;
            }
            if can_stick(&found, &other, d)
                || attached_towards(&other, d.opposite())
                || glued_toward(&found, d)
                || (Some(d) == forced && !not_supportive(&found, d))
            {
                frontier.push_back(next);
            }
        }
        // `addBlock`: a block already taken is replaced, not counted again.
        if taken.insert(pos) {
            // `BearingContraption.addBlock`: a block in `create:windmill_sails`.
            if found.sail || found.sail_facing.is_some() {
                out.sails += 1;
            }
            out.blocks.push((local(pos), found.state));
        }
        // `movePulley`: its rope and magnet go as they are.
        for &p in &found.carries {
            if let Some(r) = at(p)
                && taken.insert(p)
            {
                out.blocks.push((local(p), r.state));
            }
        }
        if out.blocks.len() > MAX_BLOCKS {
            return Err(Refused::TooLarge);
        }
    }
    Ok(out)
}

/// `Contraption.moveBelt`: a belt segment takes the segments before and after it.
pub(crate) fn belt_neighbours(pos: [i32; 3], f: &Found) -> impl Iterator<Item = [i32; 3]> + '_ {
    [true, false]
        .into_iter()
        .filter_map(move |forward| f.belt.as_ref()?.next(pos, forward))
}

/// `faceHasGlue`: glue on this face of the block takes its neighbour.
pub(crate) fn glued_toward(f: &Found, d: Dir) -> bool {
    f.glued & d.bit() != 0
}

/// A linear chassis as the search reads it: its kind (`sameKind`: linear or secondary), axis,
/// glued ends (`sticky_top` toward the positive end, `sticky_bottom`) and range.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Chassis {
    pub secondary: bool,
    pub axis: crate::kinetics::Axis,
    pub sticky_top: bool,
    pub sticky_bottom: bool,
    pub range: i32,
    /// A radial chassis (`RadialChassisBlock`): its glued sides (a `Dir::bit` each, across its
    /// axis) instead of glued ends.
    pub radial: Option<u8>,
}

/// `RadialChassisBlock.getGlueableSide`: the `sticky_*` property a face of a radial chassis on
/// `axis` glues (none along its axis).
pub fn radial_side(axis: crate::kinetics::Axis, face: Dir) -> Option<&'static str> {
    use crate::kinetics::Axis;
    Some(match (axis, face) {
        (Axis::X, Dir::North) => "sticky_west",
        (Axis::X, Dir::South) => "sticky_east",
        (Axis::X, Dir::Up) => "sticky_north",
        (Axis::X, Dir::Down) => "sticky_south",
        (Axis::Y, Dir::North) => "sticky_north",
        (Axis::Y, Dir::South) => "sticky_south",
        (Axis::Y, Dir::East) => "sticky_east",
        (Axis::Y, Dir::West) => "sticky_west",
        (Axis::Z, Dir::Up) => "sticky_north",
        (Axis::Z, Dir::Down) => "sticky_south",
        (Axis::Z, Dir::East) => "sticky_east",
        (Axis::Z, Dir::West) => "sticky_west",
        _ => return None,
    })
}

/// `ChassisBlockEntity.getIncludedBlockPositionsRadial`: from each glued side, the blocks in the
/// chassis' plane reached through blocks that need moving, closer than its range and a half.
fn radial_reach(
    pos: [i32; 3],
    c: Chassis,
    sides: u8,
    at: &impl Fn([i32; 3]) -> Option<Found>,
) -> Vec<[i32; 3]> {
    let mut out = Vec::new();
    let limit = (f64::from(c.range) + 0.5).powi(2);
    for facing in Dir::ALL {
        if facing.axis() == c.axis || sides & facing.bit() == 0 {
            continue;
        }
        let mut frontier = VecDeque::from([add(pos, facing.offset())]);
        let mut visited = HashSet::new();
        while let Some(p) = frontier.pop_front() {
            if visited.contains(&p) {
                continue;
            }
            let d: [i32; 3] = std::array::from_fn(|i| p[i] - pos[i]);
            let dist = f64::from(d[0] * d[0] + d[1] * d[1] + d[2] * d[2]);
            if dist >= limit {
                continue;
            }
            // `isMovementNecessary`, and nothing brittle.
            let Some(f) = at(p).filter(|f| !f.brittle) else {
                continue;
            };
            visited.insert(p);
            if p != pos && !out.contains(&p) {
                out.push(p);
            }
            for offset in Dir::ALL {
                // Back through the chassis only the way of this glued side.
                if offset.axis() == c.axis
                    || (p == pos && offset != facing)
                    || not_supportive(&f, offset)
                {
                    continue;
                }
                frontier.push_back(add(p, offset.offset()));
            }
        }
    }
    out
}

/// `ChassisBlockEntity`'s range when not set (`maxChassisRange / 2`).
pub const CHASSIS_RANGE: i32 = 8;

/// `Contraption.moveChassis` for the chassis at `pos`: the chassis beside it of its kind on its
/// axis (`addAttachedChasses`), then `getIncludedBlockPositionsLinear`: from each end, along its
/// axis, up to its range of blocks that need moving, while that end is glued or is the way the
/// structure moves.
pub fn chassis_reach(
    pos: [i32; 3],
    c: Chassis,
    forced: Option<Dir>,
    at: &impl Fn([i32; 3]) -> Option<Found>,
) -> Vec<[i32; 3]> {
    // `addAttachedChasses` for a radial chassis: the radial chassis on its axis either side.
    if let Some(sides) = c.radial {
        let mut out = radial_reach(pos, c, sides, at);
        for positive in [true, false] {
            let d = Dir::ALL
                .into_iter()
                .find(|d| d.axis() == c.axis && d.positive() == positive)
                .unwrap_or(Dir::Up);
            let next = add(pos, d.offset());
            if let Some(Found {
                chassis: Some(o), ..
            }) = at(next)
                && o.radial.is_some()
                && o.axis == c.axis
                && !out.contains(&next)
            {
                out.push(next);
            }
        }
        let _ = forced;
        return out;
    }
    let mut out = Vec::new();
    for d in Dir::ALL {
        let next = add(pos, d.offset());
        if let Some(Found {
            chassis: Some(o), ..
        }) = at(next)
            && o.radial.is_none()
            && o.secondary == c.secondary
            && o.axis == c.axis
        {
            out.push(next);
        }
    }
    for positive in [true, false] {
        let facing = Dir::ALL
            .into_iter()
            .find(|d| d.axis() == c.axis && d.positive() == positive)
            .unwrap_or(Dir::Up);
        let sticky = if positive {
            c.sticky_top
        } else {
            c.sticky_bottom
        };
        if forced != Some(facing) && !sticky {
            continue;
        }
        let o = facing.offset();
        for i in 1..=c.range {
            let current = [pos[0] + o[0] * i, pos[1] + o[1] * i, pos[2] + o[2] * i];
            // Up to air, a brittle block, or a face that pushes nothing on.
            let Some(f) = at(current).filter(|f| !f.brittle) else {
                break;
            };
            out.push(current);
            if not_supportive(&f, facing) {
                break;
            }
        }
    }
    out
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sticky {
    Slime,
    Honey,
}

/// Why a structure could not be gathered (`AssemblyException`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refused {
    Unmovable([i32; 3]),
    TooLarge,
    NotEnoughSails(u32),
    Empty,
    /// A piston with no poles (`noPistonPoles`) or more than `maxPistonPoles`
    /// (`tooManyPistonPoles`).
    NoPistonPoles,
    TooManyPistonPoles,
    /// An elevator with no redstone contact facing sideways, more than one, or whose column
    /// another elevator runs (`no_contacts`, `too_many_contacts`, `column_conflict`).
    NoContacts,
    TooManyContacts,
    ColumnConflict,
}

/// A gathered structure: each block's offset from the bearing and state, and its sail count.
pub type Gathered = (Vec<([i32; 3], u32)>, u32);

/// `kinetics.maxBlocksMoved`.
pub const MAX_BLOCKS: usize = 2048;
/// `kinetics.minimumWindmillSails` and `windmillSailsPerRPM`.
pub const MIN_SAILS: u32 = 8;
pub const SAILS_PER_RPM: u32 = 8;

fn add(a: [i32; 3], b: [i32; 3]) -> [i32; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

/// `BlockMovementChecks.isBlockAttachedTowards`: a sail holds on to the blocks beside it in its
/// own plane; other blocks to the sides they hang on (`Found::attached`).
pub(crate) fn attached_towards(f: &Found, toward: Dir) -> bool {
    f.attached & toward.bit() != 0 || f.sail_facing.is_some_and(|d| toward.axis() != d.axis())
}

/// `BlockMovementChecks.isNotSupportive`: the face `facing` of the block pushes nothing.
pub(crate) fn not_supportive(f: &Found, facing: Dir) -> bool {
    f.unsupportive & facing.bit() != 0 || f.sail_facing.is_some_and(|d| facing.axis() == d.axis())
}

/// `canStickTo` both ways, as vanilla pistons treat sticky blocks: slime and honey blocks stick
/// to any block but each other.
pub(crate) fn sticks(a: &Found, b: &Found) -> bool {
    let either = a.sticky.is_some() || b.sticky.is_some();
    let clash = matches!(
        (a.sticky, b.sticky),
        (Some(Sticky::Slime), Some(Sticky::Honey)) | (Some(Sticky::Honey), Some(Sticky::Slime))
    );
    either && !clash
}

/// `BearingContraption.assemble` for a windmill bearing at `bearing` facing `facing`: the blocks
/// (offset from the bearing, state) and how many of them are sails. `at` reads the world: None
/// for a block that needn't move (air, a fluid).
pub fn gather_windmill(
    bearing: [i32; 3],
    facing: Dir,
    at: impl Fn([i32; 3]) -> Option<Found>,
) -> Result<Gathered, Refused> {
    gather_bearing(bearing, facing, MIN_SAILS, at)
}

/// [`gather_windmill`] with its stabilized bearings set apart ([`gather_bearing_stabilizing`]).
pub fn gather_windmill_stabilizing(
    bearing: [i32; 3],
    facing: Dir,
    at: impl Fn([i32; 3]) -> Option<Found>,
) -> Result<Searched, Refused> {
    gather_bearing_stabilizing(bearing, facing, MIN_SAILS, at)
}

/// `BearingContraption.assemble` for any bearing: a windmill needs `min_sails` (a mechanical
/// bearing none). A mechanical bearing on it is never stabilized here (what it faces joins):
/// [`gather_bearing_with`] says which are.
pub fn gather_bearing(
    bearing: [i32; 3],
    facing: Dir,
    min_sails: u32,
    at: impl Fn([i32; 3]) -> Option<Found>,
) -> Result<Gathered, Refused> {
    let s = gather_bearing_with(bearing, facing, min_sails, &never, at)?;
    Ok((s.blocks, s.sails))
}

/// `BearingContraption.canBeStabilized`: a mechanical bearing on a bearing's structure at
/// `local` (from the block in front of the bearing) facing `facing` is stabilized on the
/// bearing's own axis, unless it sits right in front of the bearing facing back at it.
pub fn bearing_stabilizes(parent: Dir, facing: Dir, local: [i32; 3]) -> bool {
    if facing.opposite() == parent && local == [0; 3] {
        return false;
    }
    facing.axis() == parent.axis()
}

/// `BearingContraption.assemble` (a mechanical or windmill bearing's): [`gather_bearing`] with
/// the mechanical bearings on it that `canBeStabilized` set apart (`Searched::subs`): on its
/// axis, but for one right in front of it facing back. Their positions are the world's.
pub fn gather_bearing_stabilizing(
    bearing: [i32; 3],
    facing: Dir,
    min_sails: u32,
    at: impl Fn([i32; 3]) -> Option<Found>,
) -> Result<Searched, Refused> {
    let front = add(bearing, facing.offset());
    let stabilizes = |f: Dir, p: [i32; 3]| {
        bearing_stabilizes(facing, f, [p[0] - front[0], p[1] - front[1], p[2] - front[2]])
    };
    gather_bearing_with(bearing, facing, min_sails, &stabilizes, at)
}

/// `ClockworkContraption.canBeStabilized`: a mechanical bearing on a clock's hand at `local`
/// (from the hand's anchor) facing `facing` is stabilized on the clock's axis, unless it is the
/// anchor or faces it.
pub fn clockwork_stabilizes(parent: Dir, facing: Dir, local: [i32; 3]) -> bool {
    if local == [0; 3] || add(local, facing.offset()) == [0; 3] {
        return false;
    }
    facing.axis() == parent.axis()
}

/// [`gather_from`] with `stabilizes` saying which mechanical bearings on it carry what they
/// face stabilized (`Searched::subs`, world positions).
pub fn gather_from_stabilizing(
    start: [i32; 3],
    origin: [i32; 3],
    skip: impl Fn([i32; 3]) -> bool,
    stabilizes: &dyn Fn(Dir, [i32; 3]) -> bool,
    at: impl Fn([i32; 3]) -> Option<Found>,
) -> Result<Searched, Refused> {
    let rules = Rules {
        forced: None,
        skip: &skip,
        stabilizes,
    };
    search(starts_at(start, &at), origin, &rules, &at)
}

/// `StabilizedContraption.assemble(level, pos)`: what the mechanical bearing at `bearing` faces
/// (`searchMovedStructure` from the block in front, `canBeStabilized` false: a bearing on it
/// takes what it faces along), offsets from `origin`. Create assembles none for an error or
/// nothing found (`onEntityCreated` goes on to the next).
pub fn gather_stabilized(
    bearing: [i32; 3],
    facing: Dir,
    origin: [i32; 3],
    at: impl Fn([i32; 3]) -> Option<Found>,
) -> Option<Vec<([i32; 3], u32)>> {
    let blocks = gather_from(add(bearing, facing.offset()), origin, |_| false, at)
        .ok()?
        .0;
    (!blocks.is_empty()).then_some(blocks)
}

/// A `StabilizedContraption` riding a turning structure (`stabilizedSubContraptions`): what a
/// mechanical bearing on it faces, kept upright as it turns. Offsets from the turning
/// structure's anchor block (its bearing), as that structure's are.
#[derive(Clone, Debug, PartialEq)]
pub struct Stabilized {
    /// The bearing it hangs on (`BlockFace.pos`) and its facing (`getFace`).
    pub bearing: [i32; 3],
    pub facing: Dir,
    pub blocks: Vec<([i32; 3], u32)>,
    /// Its contraption controls, the kinds they turned off and its glue.
    pub extras: crate::actors::Carried,
}

/// `StabilizedBearingMovementBehaviour.getCounterRotationAngle`'s test (`canBeStabilized`): the
/// mechanical bearings of a turning structure (`blocks`, offsets from its bearing; `facing_of`
/// a mechanical bearing state's facing) whose tops turn back against it, by its rule
/// (`stabilizes`, offsets from its anchor `anchor`). A stabilized structure need not hang there.
pub fn still_tops(
    blocks: &[([i32; 3], u32)],
    anchor: [i32; 3],
    facing_of: impl Fn(u32) -> Option<Dir>,
    stabilizes: impl Fn(Dir, [i32; 3]) -> bool,
) -> Vec<[i32; 3]> {
    blocks
        .iter()
        .filter(|(o, s)| {
            facing_of(*s).is_some_and(|f| {
                stabilizes(f, [o[0] - anchor[0], o[1] - anchor[1], o[2] - anchor[2]])
            })
        })
        .map(|(o, _)| *o)
        .collect()
}

/// How far apart the keys of a structure's stabilized structures are ([`sub_key`]): more than
/// the height of any world, so no block and no other structure's key meets one.
pub const SUB_KEY_STEP: i32 = 1 << 16;

/// The key the `i`th stabilized structure riding the structure keyed `key` keeps what it
/// carries under (its own `MountedStorageManager`, actors and block data): `key` moved up
/// `i + 1` steps of [`SUB_KEY_STEP`].
pub fn sub_key(key: [i32; 3], i: usize) -> [i32; 3] {
    let up = SUB_KEY_STEP.wrapping_mul(i as i32 + 1);
    [key[0], key[1].wrapping_add(up), key[2]]
}

/// The structure and index a [`sub_key`] stands for, for a key at a world height; None for a
/// key that is no stabilized structure's (a block's own).
pub fn sub_of_key(key: [i32; 3]) -> Option<([i32; 3], usize)> {
    let k = (i64::from(key[1]) + i64::from(SUB_KEY_STEP / 2)).div_euclid(i64::from(SUB_KEY_STEP));
    if k < 1 {
        return None;
    }
    let y = i64::from(key[1]) - k * i64::from(SUB_KEY_STEP);
    Some(([key[0], y as i32, key[2]], (k - 1) as usize))
}

impl Stabilized {
    /// `BlockFace.getConnectedPos`: the block in front of its bearing, where it is anchored.
    pub fn front(&self) -> [i32; 3] {
        add(self.bearing, self.facing.offset())
    }

    /// Whether the block at offset `o` is one of its own.
    pub fn holds(&self, o: [i32; 3]) -> bool {
        self.blocks.iter().any(|(p, _)| *p == o)
    }

    /// `AbstractContraptionEntity.getPassengerPosition` for it: how far its blocks are from
    /// where they were taken with the structure carrying it turned `deg` about `axis` through
    /// its anchor's centre. Its front's centre turns; its blocks do not (its `yaw` stays at its
    /// initial yaw).
    pub fn shift(&self, axis: crate::kinetics::Axis, deg: f64) -> [f64; 3] {
        let c = self.front().map(f64::from);
        let r = crate::actors::turn_vec(c, axis, deg);
        [0, 1, 2].map(|i| r[i] - c[i])
    }

    /// `disassemble` after the structure carrying it went back turned `quarter` about `axis`:
    /// its anchor set at `transform.apply(connectedPos)`, its own turn none (`-yaw +
    /// initialYaw`). The offset its block at `o` goes back to.
    pub fn placed(&self, o: [i32; 3], axis: crate::kinetics::Axis, quarter: i32) -> [i32; 3] {
        let c = self.front();
        let r = rotate(c, quarter, axis);
        [0, 1, 2].map(|i| r[i] + o[i] - c[i])
    }
}

/// [`gather_bearing`], with `stabilizes` saying which mechanical bearings on it carry what they
/// face stabilized (`Searched::subs`).
pub fn gather_bearing_with(
    bearing: [i32; 3],
    facing: Dir,
    min_sails: u32,
    stabilizes: &dyn Fn(Dir, [i32; 3]) -> bool,
    at: impl Fn([i32; 3]) -> Option<Found>,
) -> Result<Searched, Refused> {
    let start = add(bearing, facing.offset());
    let skip = |p: [i32; 3]| p == bearing;
    let rules = Rules {
        forced: None,
        skip: &skip,
        stabilizes,
    };
    let s = search(starts_at(start, &at), bearing, &rules, &at)?;
    if s.sails < min_sails {
        return Err(Refused::NotEnoughSails(s.sails));
    }
    if s.blocks.is_empty() {
        return Err(Refused::Empty);
    }
    Ok(s)
}

/// `Contraption.searchMovedStructure` from `start` with no forced direction: the block there
/// (unless it is brittle) and what holds on to it (slime, honey, sails in their plane, chassis,
/// what bearings, pulleys and pistons on it bring), never a block `skip` names
/// (`isAnchoringBlockAt`, a clock's other hand). Offsets from `origin`, and the sails.
pub fn gather_from(
    start: [i32; 3],
    origin: [i32; 3],
    skip: impl Fn([i32; 3]) -> bool,
    at: impl Fn([i32; 3]) -> Option<Found>,
) -> Result<Gathered, Refused> {
    let rules = Rules {
        forced: None,
        skip: &skip,
        stabilizes: &never,
    };
    let s = search(starts_at(start, &at), origin, &rules, &at)?;
    Ok((s.blocks, s.sails))
}

/// [`gather_from`] from a frontier taken as it is (`addToInitialFrontier` clearing it: a
/// cart's block above it, even a brittle one).
pub fn gather_frontier(
    frontier: &[[i32; 3]],
    origin: [i32; 3],
    skip: impl Fn([i32; 3]) -> bool,
    at: impl Fn([i32; 3]) -> Option<Found>,
) -> Result<Gathered, Refused> {
    let rules = Rules {
        forced: None,
        skip: &skip,
        stabilizes: &never,
    };
    let s = search(frontier.iter().copied().collect(), origin, &rules, &at)?;
    Ok((s.blocks, s.sails))
}

/// [`gather_frontier`] with every mechanical bearing on it carrying what it faces stabilized
/// (`MountedContraption.canBeStabilized` is always true): `Searched::subs` in world positions.
pub fn gather_frontier_stabilizing(
    frontier: &[[i32; 3]],
    origin: [i32; 3],
    skip: impl Fn([i32; 3]) -> bool,
    at: impl Fn([i32; 3]) -> Option<Found>,
) -> Result<Searched, Refused> {
    let rules = Rules {
        forced: None,
        skip: &skip,
        stabilizes: &|_, _| true,
    };
    search(frontier.iter().copied().collect(), origin, &rules, &at)
}

/// `GantryContraption.assemble` (`searchMovedStructure(level, carriage, null)`, the carriage's
/// `moveGantryPinion` bringing the block in front of it): the carriage and what holds on to it,
/// never the shaft it rides on (`isAnchoringBlockAt`); no forced direction, so a chassis on it
/// reaches only its glued ends. `moving` is the way it will move (unused by the search).
/// Offsets from the carriage.
pub fn gather_gantry(
    carriage: [i32; 3],
    facing: Dir,
    moving: Dir,
    at: impl Fn([i32; 3]) -> Option<Found>,
) -> Result<Vec<([i32; 3], u32)>, Refused> {
    let _ = moving;
    let shaft = add(carriage, facing.opposite().offset());
    let skip = |p: [i32; 3]| p == shaft;
    let rules = Rules {
        forced: None,
        skip: &skip,
        stabilizes: &never,
    };
    // The carriage's pinion side (`moveGantryPinion`) whether or not its block says so.
    let mut frontier = starts_at(carriage, &at);
    frontier.push_back(add(carriage, facing.offset()));
    Ok(search(frontier, carriage, &rules, &at)?.blocks)
}

/// `WindmillBearingBlockEntity.getGeneratedSpeed`: a sail per 8 gives an rpm, 1 to 16,
/// clockwise positive.
pub fn windmill_speed(sails: u32, clockwise: bool) -> f32 {
    let rpm = (sails / SAILS_PER_RPM).clamp(1, 16) as f32;
    if clockwise { rpm } else { -rpm }
}

/// A mechanical bearing's turning (`MechanicalBearingBlockEntity.angle`): the angle at game time
/// `since` and the speed since then; the angle at any later tick follows without ticking.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Turn {
    pub angle: f32,
    pub since: i64,
    pub rpm: f32,
    /// `sequencedAngleLimit`: the degrees it may still turn from `since` (a sequenced
    /// gearshift's or valve handle's turn), none for no limit.
    pub limit: Option<f32>,
}

impl Turn {
    /// `angle += convertToAngular(speed)` (`speed × 0.3`° a tick) each tick, `% 360`.
    /// Each tick's turn is held to the limit left, which it uses up.
    pub fn angle_at(&self, now: i64) -> f32 {
        (self.angle + self.turned(now)) % 360.0
    }

    /// The degrees turned from `since` to `now`.
    fn turned(&self, now: i64) -> f32 {
        let d = self.rpm * 0.3 * (now - self.since) as f32;
        match self.limit {
            Some(l) => d.clamp(-l, l),
            None => d,
        }
    }

    /// The speed changed at `now`; what is left of the limit goes on.
    pub fn rebase(&mut self, now: i64, rpm: f32) {
        let turned = self.turned(now);
        self.angle = (self.angle + turned) % 360.0;
        self.limit = self.limit.map(|l| (l - turned.abs()).max(0.0));
        self.since = now;
        self.rpm = rpm;
    }
}

/// `StructureTransform`'s angle: the nearest quarter turn (`Math.round(angle / 90) × 90`, in
/// -90..270).
pub fn quarter(angle: f32) -> i32 {
    let mut a = ((angle / 90.0 + 0.5).floor() as i32 * 90) % 360;
    if a < -90 {
        a += 360;
    }
    a
}

/// `StructureTransform.rotation.ordinal()`: how many `getClockWise(axis)` turns a facing takes.
pub fn clockwise_turns(quarter: i32) -> u8 {
    match quarter {
        -90 | 270 => 1,
        180 => 2,
        90 => 3,
        _ => 0,
    }
}

/// `VecHelper.rotate` by a quarter-turn angle about `axis`, exact for whole blocks.
pub fn rotate(v: [i32; 3], quarter: i32, axis: crate::kinetics::Axis) -> [i32; 3] {
    use crate::kinetics::Axis;
    let (s, c) = match quarter.rem_euclid(360) {
        90 => (1, 0),
        180 => (0, -1),
        270 => (-1, 0),
        _ => (0, 1),
    };
    let [x, y, z] = v;
    match axis {
        Axis::X => [x, y * c - z * s, z * c + y * s],
        Axis::Y => [x * c + z * s, y, z * c - x * s],
        Axis::Z => [x * c - y * s, y * c + x * s, z],
    }
}

/// `StructureTransform` as a moved block sees it: the quarter turns taking its offsets and
/// directions from where it was to where it went back (none for a structure that only slid).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Transform {
    /// Column `i` is where unit vector `i` goes.
    m: [[i32; 3]; 3],
}

impl Transform {
    pub const NONE: Transform = Transform {
        m: [[1, 0, 0], [0, 1, 0], [0, 0, 1]],
    };

    /// The turn of `place` (offset in the structure to where it went back), from where it puts
    /// the three unit offsets; none when that is no quarter-turn rotation.
    pub fn from_place(place: impl Fn([i32; 3]) -> [i32; 3]) -> Transform {
        let o = place([0; 3]);
        let col = |e: [i32; 3]| {
            let p = place(e);
            [p[0] - o[0], p[1] - o[1], p[2] - o[2]]
        };
        let cols = [col([1, 0, 0]), col([0, 1, 0]), col([0, 0, 1])];
        let m = [0, 1, 2].map(|r| [0, 1, 2].map(|c| cols[c][r]));
        let unit = |v: [i32; 3]| v.iter().map(|x| x.abs()).sum::<i32>() == 1;
        let rows_unit = m.iter().all(|r| unit(*r));
        let det = m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
            - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
            + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0]);
        if cols.iter().all(|c| unit(*c)) && rows_unit && det == 1 {
            Transform { m }
        } else {
            Transform::NONE
        }
    }

    /// Offset `v` turned.
    pub fn vec(&self, v: [i32; 3]) -> [i32; 3] {
        let m = &self.m;
        [0, 1, 2].map(|r| m[r][0] * v[0] + m[r][1] * v[1] + m[r][2] * v[2])
    }

    /// Direction `d` turned.
    pub fn dir(&self, d: crate::kinetics::Dir) -> crate::kinetics::Dir {
        let v = self.vec(d.offset());
        crate::kinetics::Dir::ALL
            .into_iter()
            .find(|x| x.offset() == v)
            .unwrap_or(d)
    }

    /// Whether it turns anything.
    pub fn turns(&self) -> bool {
        *self != Transform::NONE
    }

    /// The axis it turns about and by how much ([`rotate`]'s quarter, in degrees); none when it
    /// turns nothing (or is no single quarter turn about an axis).
    pub fn quarter_axis(&self) -> Option<(i32, crate::kinetics::Axis)> {
        use crate::kinetics::Axis;
        for axis in [Axis::X, Axis::Y, Axis::Z] {
            for quarter in [90, 180, 270] {
                let same = [[1, 0, 0], [0, 1, 0], [0, 0, 1]]
                    .into_iter()
                    .all(|e| self.vec(e) == rotate(e, quarter, axis));
                if same {
                    return Some((quarter, axis));
                }
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_transform_is_the_turn_its_placement_makes() {
        use crate::kinetics::{Axis, Dir};
        let slid = Transform::from_place(|o| [o[0] + 3, o[1] - 2, o[2] + 7]);
        assert_eq!(slid, Transform::NONE);
        assert!(!slid.turns());
        let turned = Transform::from_place(|o| {
            let r = rotate(o, 90, Axis::Y);
            [r[0] + 10, r[1], r[2] - 4]
        });
        assert!(turned.turns());
        for d in Dir::ALL {
            let v = rotate(d.offset(), 90, Axis::Y);
            assert_eq!(turned.dir(d).offset(), v, "{d:?}");
        }
        assert_eq!(turned.dir(Dir::Up), Dir::Up);
        assert_eq!(turned.quarter_axis(), Some((90, Axis::Y)));
        assert_eq!(slid.quarter_axis(), None);
        // Not a rotation (a mirror): none.
        assert_eq!(
            Transform::from_place(|o| [-o[0], o[1], o[2]]),
            Transform::NONE
        );
    }

    #[test]
    fn a_sequenced_turn_stops_at_its_limit() {
        // 32 rpm (9.6° a tick) held to 45°: stops after 4.7 ticks, at 45.
        let mut t = Turn {
            angle: 10.0,
            since: 0,
            rpm: -32.0,
            limit: Some(45.0),
        };
        assert!((t.angle_at(2) - (10.0 - 19.2)).abs() < 1e-3);
        assert!((t.angle_at(7) + 35.0).abs() < 1e-3);
        // A stall part way keeps what is left.
        t.rebase(2, 0.0);
        assert!((t.limit.unwrap() - 25.8).abs() < 1e-3);
        t.rebase(5, -32.0);
        assert!((t.angle_at(9) + 35.0).abs() < 1e-3);
    }

    #[test]
    fn a_stopped_bearing_snaps_to_the_nearest_quarter_turn() {
        use crate::kinetics::Axis;
        let mut t = Turn {
            angle: 0.0,
            since: 0,
            rpm: 16.0,
            limit: None,
        };
        // 16 rpm: 4.8° a tick; after 20 ticks 96°.
        assert!((t.angle_at(20) - 96.0).abs() < 1e-3);
        t.rebase(20, -16.0);
        assert!((t.angle_at(30) - 48.0).abs() < 1e-3);
        assert_eq!(
            (
                quarter(96.0),
                quarter(44.0),
                quarter(-100.0),
                quarter(200.0)
            ),
            (90, 0, -90, 180)
        );
        assert_eq!(
            (
                clockwise_turns(90),
                clockwise_turns(-90),
                clockwise_turns(180)
            ),
            (3, 1, 2)
        );
        // East of an up-facing bearing's anchor, a quarter turn about y goes north
        // (counterclockwise from above).
        assert_eq!(rotate([1, 0, 0], 90, Axis::Y), [0, 0, -1]);
        assert_eq!(rotate([0, 1, 0], 90, Axis::X), [0, 0, 1]);
        assert_eq!(rotate([2, 1, 0], 180, Axis::Z), [-2, -1, 0]);
    }

    #[test]
    fn a_chassis_carries_its_glued_end_and_its_linked_chassis() {
        use crate::kinetics::Axis;
        // A vertical chassis at the origin glued on top, a second one above it, stone above
        // both up to y 4, air at 5; another stone below (its bottom isn't glued).
        let chassis = Chassis {
            secondary: false,
            axis: Axis::Y,
            sticky_top: true,
            sticky_bottom: false,
            range: CHASSIS_RANGE,
            radial: None,
        };
        let at = |p: [i32; 3]| -> Option<Found> {
            if p[0] != 0 || p[2] != 0 || !(-1..=4).contains(&p[1]) {
                return None;
            }
            Some(Found {
                state: 1,
                sail_facing: None,
                sticky: None,
                movable: true,
                chassis: (p[1] == 0 || p[1] == 1).then_some(chassis),
                ..Default::default()
            })
        };
        let reach = chassis_reach([0, 0, 0], chassis, None, &at);
        assert_eq!(
            reach,
            vec![[0, 1, 0], [0, 1, 0], [0, 2, 0], [0, 3, 0], [0, 4, 0]]
        );
        // Moved down, its bottom end goes too.
        assert!(chassis_reach([0, 0, 0], chassis, Some(Dir::Down), &at).contains(&[0, -1, 0]));
        let range = Chassis {
            range: 2,
            ..chassis
        };
        assert!(!chassis_reach([0, 0, 0], range, None, &at).contains(&[0, 3, 0]));
    }

    #[test]
    fn a_radial_chassis_carries_its_glued_sides_within_its_radius() {
        use crate::kinetics::Axis;
        // A radial chassis on y glued on its east side; a 5x5 floor of stone around it at its
        // height and one stone above it; range 2.
        let chassis = Chassis {
            secondary: false,
            axis: Axis::Y,
            sticky_top: false,
            sticky_bottom: false,
            range: 2,
            radial: Some(Dir::East.bit()),
        };
        let at = |p: [i32; 3]| {
            let floor = p[1] == 0 && p[0].abs() <= 3 && p[2].abs() <= 3;
            (floor || p == [0, 1, 0]).then(|| Found {
                state: 1,
                movable: true,
                chassis: (p == [0, 0, 0]).then_some(chassis),
                ..Found::default()
            })
        };
        let reach = chassis_reach([0, 0, 0], chassis, None, &at);
        assert!(reach.contains(&[1, 0, 0]) && reach.contains(&[2, 0, 0]));
        // Through the plane round the chassis, within 2.5 of it.
        assert!(reach.contains(&[-2, 0, 0]) && reach.contains(&[1, 0, 2]));
        assert!(!reach.contains(&[2, 0, 2]), "2.83 away");
        assert!(!reach.contains(&[3, 0, 0]));
        assert!(!reach.contains(&[0, 1, 0]), "not along its axis");
        assert!(!reach.contains(&[0, 0, 0]));
        assert_eq!(radial_side(Axis::X, Dir::Up), Some("sticky_north"));
        assert_eq!(radial_side(Axis::Z, Dir::North), None);
    }

    fn sail() -> Found {
        Found {
            state: 7,
            sail_facing: Some(Dir::North),
            sticky: None,
            movable: true,
            chassis: None,
            ..Default::default()
        }
    }

    #[test]
    fn sails_in_front_of_the_bearing_gather_in_their_plane() {
        // A bearing at the origin facing north; a cross of sails at z = -1 in the xy plane
        // (facing north), and a stone behind the plane that nothing holds.
        let at = |p: [i32; 3]| match p {
            [x, y, -1]
                if (x == 0 && (-4..=4).contains(&y)) || (y == 0 && (-4..=4).contains(&x)) =>
            {
                Some(sail())
            }
            [0, 0, -2] => Some(Found {
                state: 1,
                sail_facing: None,
                sticky: None,
                movable: true,
                chassis: None,
                ..Default::default()
            }),
            _ => None,
        };
        let (blocks, sails) = gather_windmill([0, 0, 0], Dir::North, at).unwrap();
        assert_eq!(sails, 17);
        assert_eq!(blocks.len(), 17, "the stone behind is not attached");
        assert!(blocks.iter().all(|(o, _)| o[2] == -1));
        assert_eq!(windmill_speed(sails, true), 2.0);
        assert_eq!(windmill_speed(200, false), -16.0);
        assert_eq!(windmill_speed(3, true), 1.0);
    }

    #[test]
    fn too_few_sails_or_an_unmovable_block_refuse() {
        let few = |p: [i32; 3]| (p[2] == -1 && p[0] == 0 && (0..3).contains(&p[1])).then(sail);
        assert_eq!(
            gather_windmill([0, 0, 0], Dir::North, few),
            Err(Refused::NotEnoughSails(3))
        );
        let stuck = |p: [i32; 3]| {
            (p == [0, 0, -1]).then(|| Found {
                movable: false,
                chassis: None,
                ..sail()
            })
        };
        assert_eq!(
            gather_windmill([0, 0, 0], Dir::North, stuck),
            Err(Refused::Unmovable([0, 0, -1]))
        );
    }

    fn block(state: u32) -> Found {
        Found {
            state,
            movable: true,
            ..Default::default()
        }
    }

    #[test]
    fn slime_holds_neither_brittle_nor_push_only_blocks_nor_faces_that_push_nothing() {
        // A slime block at the origin with, around it: a brittle block hanging on to something
        // else east, glazed terracotta (push-only) west, a drill whose head faces the slime
        // north, and stones south and above.
        let at = |p: [i32; 3]| match p {
            [0, 0, 0] => Some(Found {
                sticky: Some(Sticky::Slime),
                ..block(1)
            }),
            [1, 0, 0] => Some(Found {
                brittle: true,
                unsupportive: 0x3F,
                ..block(2)
            }),
            [-1, 0, 0] => Some(Found {
                push_only: true,
                ..block(3)
            }),
            [0, 0, -1] => Some(Found {
                unsupportive: Dir::South.bit(),
                ..block(4)
            }),
            [0, 0, 1] | [0, 1, 0] => Some(block(5)),
            _ => None,
        };
        let (mut blocks, _) = gather_from([0, 0, 0], [0, 0, 0], |_| false, at).unwrap();
        blocks.sort();
        assert_eq!(
            blocks,
            vec![([0, 0, 0], 1), ([0, 0, 1], 5), ([0, 1, 0], 5)],
            "only the stones stick"
        );
        // A brittle block is no search's start, but a cart takes the block above it as it is.
        let torch = |p: [i32; 3]| {
            (p == [0, 0, 0]).then(|| Found {
                brittle: true,
                ..block(6)
            })
        };
        assert_eq!(
            gather_from([0, 0, 0], [0, 0, 0], |_| false, torch),
            Ok((vec![], 0))
        );
        assert_eq!(
            gather_frontier(&[[0, 0, 0]], [0, 0, 0], |_| false, torch),
            Ok((vec![([0, 0, 0], 6)], 0))
        );
    }

    #[test]
    fn bearings_pulleys_and_pistons_on_a_structure_bring_theirs() {
        // A stone at the origin glued to what is above it: a windmill bearing (y 1) whose
        // faced stone it drags, a mechanical bearing facing west (y 2), and a rope pulley (y 3)
        // whose rope (x 5, y 1..2) rides as it is, with the stone under it (x 5, y 0).
        let at = |p: [i32; 3]| match p {
            [0, 0, 0] => Some(Found {
                glued: Dir::Up.bit(),
                ..block(1)
            }),
            [0, 1, 0] => Some(Found {
                drags: vec![[1, 1, 0]],
                glued: Dir::Up.bit(),
                ..block(2)
            }),
            [0, 2, 0] => Some(Found {
                bearing: Some(Dir::West),
                glued: Dir::Up.bit(),
                ..block(3)
            }),
            [0, 3, 0] => Some(Found {
                carries: vec![[5, 2, 0], [5, 1, 0]],
                drags: vec![[5, 0, 0]],
                ..block(4)
            }),
            [1, 1, 0] | [-1, 2, 0] | [5, 0, 0] => Some(block(5)),
            [5, 1, 0] | [5, 2, 0] => Some(Found {
                movable: false,
                brittle: true,
                ..block(6)
            }),
            _ => None,
        };
        let (mut blocks, _) = gather_from([0, 0, 0], [0, 0, 0], |_| false, at).unwrap();
        blocks.sort();
        assert_eq!(
            blocks,
            vec![
                ([-1, 2, 0], 5),
                ([0, 0, 0], 1),
                ([0, 1, 0], 2),
                ([0, 2, 0], 3),
                ([0, 3, 0], 4),
                ([1, 1, 0], 5),
                ([5, 0, 0], 5),
                ([5, 1, 0], 6),
                ([5, 2, 0], 6),
            ]
        );
        // On a bearing turning about x (facing east), a bearing facing west is stabilized: what
        // it faces stays out of the structure (`pendingSubContraptions`).
        let s = gather_bearing_with(
            [0, -1, 0],
            Dir::Up,
            0,
            &|f, p| bearing_stabilizes(Dir::East, f, p),
            at,
        )
        .unwrap();
        assert_eq!(s.subs, vec![([0, 2, 0], Dir::West)]);
        assert!(!s.blocks.iter().any(|(o, _)| *o == [-1, 3, 0]));
        assert!(bearing_stabilizes(Dir::Up, Dir::Down, [0, 1, 0]));
        assert!(
            !bearing_stabilizes(Dir::Up, Dir::Down, [0, 0, 0]),
            "right in front, facing back"
        );
        assert!(!bearing_stabilizes(Dir::Up, Dir::East, [0, 3, 0]), "across");
        // A moving piston ends the search with nothing assembled.
        let moving = |p: [i32; 3]| {
            (p == [0, 0, 0]).then(|| Found {
                halts: true,
                ..block(7)
            })
        };
        assert_eq!(
            gather_from([0, 0, 0], [0, 0, 0], |_| false, moving),
            Err(Refused::Empty)
        );
    }

    #[test]
    fn a_ferris_wheel_s_cabins_hang_upright_and_go_back_where_it_stops() {
        // A bearing at the origin facing east (axis x) with an arm up from the block in front:
        // stone at (1, 0..2, 0), a mechanical bearing facing west at (1, 3, 0) on top of it
        // (glued), and a cabin: stone at (0, 3, 0) and (0, 2, 0) under it, glued together.
        let at = |p: [i32; 3]| match p {
            [1, 0..=2, 0] => Some(Found {
                glued: Dir::Up.bit(),
                ..block(1)
            }),
            [1, 3, 0] => Some(Found {
                bearing: Some(Dir::West),
                ..block(2)
            }),
            [0, 3, 0] => Some(Found {
                glued: Dir::Down.bit(),
                ..block(3)
            }),
            [0, 2, 0] => Some(block(3)),
            _ => None,
        };
        let s = gather_bearing_stabilizing([0, 0, 0], Dir::East, 0, at).unwrap();
        assert_eq!(s.subs, vec![([1, 3, 0], Dir::West)]);
        assert_eq!(s.blocks.len(), 4, "the arm and the bearing, not the cabin");
        let cabin = gather_stabilized([1, 3, 0], Dir::West, [0, 0, 0], at).unwrap();
        assert_eq!(cabin, vec![([0, 3, 0], 3), ([0, 2, 0], 3)]);
        // A bearing right in front facing back is not stabilized.
        let facing_back = |p: [i32; 3]| {
            (p == [1, 0, 0]).then(|| Found {
                bearing: Some(Dir::West),
                ..block(2)
            })
        };
        assert!(
            gather_bearing_stabilizing([0, 0, 0], Dir::East, 0, facing_back)
                .unwrap()
                .subs
                .is_empty()
        );
        let sub = Stabilized {
            bearing: [1, 3, 0],
            facing: Dir::West,
            blocks: cabin,
            extras: Default::default(),
        };
        // Turned a quarter about x, its front (0, 3, 0) goes to (0, 0, 3): its blocks slide
        // there, upright.
        let shift = sub.shift(crate::kinetics::Axis::X, 90.0);
        assert!(shift[0].abs() < 1e-9 && (shift[1] + 3.0).abs() < 1e-9);
        assert!((shift[2] - 3.0).abs() < 1e-9);
        assert_eq!(sub.placed([0, 2, 0], crate::kinetics::Axis::X, 90), [0, -1, 3]);
        assert_eq!(sub.placed([0, 2, 0], crate::kinetics::Axis::X, 0), [0, 2, 0]);
        assert!(sub.holds([0, 3, 0]) && !sub.holds([1, 3, 0]));
        // Nothing in front: no cabin.
        assert_eq!(gather_stabilized([1, 3, 0], Dir::East, [0; 3], at), None);
        // Each cabin keeps what it carries under its own key, found back from it.
        for (bearing, i) in [([3, -60, 7], 0), ([-5, 300, 2], 4)] {
            let k = sub_key(bearing, i);
            assert_ne!(k, bearing);
            assert_eq!(sub_of_key(k), Some((bearing, i)));
        }
        assert_eq!(sub_of_key([3, -60, 7]), None);
        // Its bearing's top turns back against the wheel; so would an on-axis bearing's with
        // nothing in front, not one across the axis.
        let tops = still_tops(
            &[([1, 3, 0], 2), ([1, 1, 0], 4), ([1, 2, 0], 1)],
            [1, 0, 0],
            |s| match s {
                2 => Some(Dir::West),
                4 => Some(Dir::Up),
                _ => None,
            },
            |f, l| bearing_stabilizes(Dir::East, f, l),
        );
        assert_eq!(tops, vec![[1, 3, 0]]);
    }

    #[test]
    fn a_sticker_takes_what_it_faces_unless_that_face_pushes_nothing() {
        let at = |faces_back: bool| {
            move |p: [i32; 3]| match p {
                [0, 0, 0] => Some(Found {
                    sticker: Some(Dir::Up),
                    ..block(1)
                }),
                [0, 1, 0] => Some(Found {
                    unsupportive: if faces_back { Dir::Down.bit() } else { 0 },
                    ..block(2)
                }),
                _ => None,
            }
        };
        let taken = |faces_back| {
            gather_from([0, 0, 0], [0, 0, 0], |_| false, at(faces_back))
                .unwrap()
                .0
                .len()
        };
        assert_eq!((taken(false), taken(true)), (2, 1));
    }

    #[test]
    fn a_radial_chassis_takes_only_what_its_glued_side_reaches() {
        // A radial chassis on y at the origin glued north, stone north and south of it.
        let chassis = Chassis {
            secondary: false,
            axis: crate::kinetics::Axis::Y,
            sticky_top: false,
            sticky_bottom: false,
            range: 2,
            radial: Some(Dir::North.bit()),
        };
        let at = |p: [i32; 3]| match p {
            [0, 0, 0] => Some(Found {
                movable: true,
                chassis: Some(chassis),
                ..Default::default()
            }),
            [0, 0, -1] | [0, 0, 1] => Some(Found {
                movable: true,
                ..Default::default()
            }),
            _ => None,
        };
        assert_eq!(
            chassis_reach([0, 0, 0], chassis, None, &at),
            vec![[0, 0, -1]]
        );
    }
}

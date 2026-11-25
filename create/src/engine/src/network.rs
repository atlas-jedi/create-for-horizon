//! Create's track graph built from the world (`TrackPropagator.onRailAdded`,
//! `ITrackBlock.getConnected`, `TrackBlock.getConnected`, `TrackNodeLocation`,
//! `isValidGraphNodeLocation`): every track block joins the two ends it runs between (a crossing
//! joins its centre to each of its four ends), every curve its two starts; a place where track
//! ends, branches, bends out of line or a curve starts is a node (and a straight run gets one at
//! each chunk line, as Create's); the straight runs between nodes are straight edges, curves are
//! curved ones. Built once when track changes, never per tick.
//!
//! Stations, signals and track observers become points on the edges of the track they picked
//! (`EdgePointStorage`, `TrackTargetingBehaviour`), and signals split the graph into sections
//! (`SignalEdgeGroup`, `SignalPropagator`): the track between signals, joined through nodes.
//! Signals picking the same place are one boundary (`SignalBoundary`, `canMerge`) with two sides,
//! one for each way a train runs past it. Each block placed with its item keeps the track and the
//! way it picked (`TrackTargetingBehaviour.targetTrack`, `targetDirection`, and on a curve
//! `targetBezier`: a [`Target`]), on any track a train runs (straight, diagonal, sloped, or a
//! curve's segment: `TrackGraphHelper.getGraphLocationAt`, `getBezierGraphLocationAt`), and
//! stands for the trains running the way the player looked. One with none (placed by a command,
//! or saved before blocks kept their own) binds to the straight track beside it, a signal
//! standing for the trains that have it on their right.

use crate::bezier::{Curve, Vec3};
use crate::graph::{Graph, Point};
use crate::track::TrackShape;
use crate::track_curves::Curves;
use std::collections::HashMap;

/// A node's place: `TrackNodeLocation`'s half-block x and z, and twice its floored y.
pub type Key = [i32; 3];

/// `EdgeGroupColor`: the colours a signal section is shown in while a signal is held (yellow,
/// green, blue, orange, lavender, red, cyan, brown, white), RGB.
pub const SECTION_COLOURS: [u32; 9] = [
    0xEBC255, 0x51C054, 0x5391E1, 0xE36E36, 0xCB92BA, 0xA43538, 0x6EDAD9, 0xA17C58, 0xE5E1DC,
];
/// `EdgeGroupColor.WHITE`, the colour left when the other eight are taken.
const WHITE: usize = 8;

fn sub(a: Vec3, b: Vec3) -> Vec3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn add(a: Vec3, b: Vec3) -> Vec3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn scale(a: Vec3, s: f64) -> Vec3 {
    [a[0] * s, a[1] * s, a[2] * s]
}
fn dot(a: Vec3, b: Vec3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn length(a: Vec3) -> f64 {
    dot(a, a).sqrt()
}
fn normalize(a: Vec3) -> Vec3 {
    let l = length(a);
    if l < 1.0e-4 {
        [0.0; 3]
    } else {
        scale(a, 1.0 / l)
    }
}
/// Java's `Math.round`.
fn round(a: f64) -> i64 {
    (a + 0.5).floor() as i64
}

/// `new TrackNodeLocation(vec)`.
pub fn key(v: Vec3) -> Key {
    [
        round(v[0] * 2.0) as i32,
        v[1].floor() as i32 * 2,
        round(v[2] * 2.0) as i32,
    ]
}

/// `TrackNodeLocation.getLocation`.
pub fn key_position(k: Key) -> Vec3 {
    [
        f64::from(k[0]) / 2.0,
        f64::from(k[1]) / 2.0,
        f64::from(k[2]) / 2.0,
    ]
}

/// A place on the graph: an edge and how far along it.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Location {
    pub edge: usize,
    pub position: f64,
}

/// What stands on an edge (`TrackEdgePoint`): a station or a signal, by its index.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mark {
    Station(usize),
    Signal(usize),
    /// A track observer (`TrackObserver`), by its index in `Network::observers`.
    Observer(usize),
}

/// What a signal block is (`SignalBlock.TYPE`): an entry signal holds trains while the section
/// beyond is occupied; a cross signal while any section of the chain beyond it up to the next
/// entry signal is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum SignalType {
    #[default]
    Entry,
    Cross,
}

/// A place on a curve (`BezierTrackPointLocation`): the curve's other track block
/// (`curveTarget`, the key of the curve in its first block's connections) and the segment
/// picked (`segment`, half a block each).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct BezierPoint {
    pub to: [i32; 3],
    pub segment: i32,
}

/// What a station, signal or track observer picked with its item (`TrackTargetingBehaviour`): its
/// track block (`targetTrack`, here where it is, not from the block), `targetDirection` (`front`:
/// true for `POSITIVE`, picked looking against the track's first axis, or against a curve's run)
/// and, on a curve, `targetBezier` (`track` is then the curve's first block).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Target {
    pub track: [i32; 3],
    pub front: bool,
    pub bezier: Option<BezierPoint>,
}

impl Target {
    /// `TrackTargetingBlockItem.useOn`'s pick on track `shape` looked at along `look`
    /// (`getNearestTrackAxis`): `front` when the look runs against its axis' horizontal part.
    /// None on a junction (`no_junctions`).
    pub fn pick(track: [i32; 3], shape: TrackShape, look: [f64; 3]) -> Option<Target> {
        if shape.is_junction() {
            return None;
        }
        let axis = shape.axes()[0];
        let flat = normalize([axis[0], 0.0, axis[2]]);
        let front = dot(look, flat) < 0.0;
        Some(Target {
            track,
            front,
            bezier: None,
        })
    }

    /// `useOnCurve`'s pick on segment `segment` of the curve from `track` to `to`: `front` when
    /// the look runs against the curve there.
    pub fn on_curve(track: [i32; 3], to: [i32; 3], segment: i32, front: bool) -> Target {
        Target {
            track,
            front,
            bezier: Some(BezierPoint { to, segment }),
        }
    }
}

/// `EdgePointType`: what a track-targeting block puts on the track.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PointKind {
    Station,
    Signal,
    Observer,
}

/// `TrackTargetingBlockItem.OverlapResult`: whether a pick can take a point, and the
/// `track_target.*` key saying why not.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Overlap {
    Valid,
    Occupied,
    Junction,
    NoTrack,
}

impl Overlap {
    /// `feedback`: the lang key's last part (`create.track_target.` + this).
    pub fn feedback(self) -> Option<&'static str> {
        match self {
            Overlap::Valid => None,
            Overlap::Occupied => Some("occupied"),
            Overlap::Junction => Some("no_junctions"),
            Overlap::NoTrack => Some("invalid"),
        }
    }
}

/// `StationBlockEntity`'s track to assemble on (`edgePoint.isOrthogonal`): the track its pick is
/// on when that is straight along x or z (none on a curve or a slope), or with no pick the
/// straight track beside it.
pub fn station_track(
    tracks: &HashMap<[i32; 3], TrackShape>,
    targets: &HashMap<[i32; 3], Target>,
    pos: [i32; 3],
) -> Option<[i32; 3]> {
    match targets.get(&pos) {
        Some(t) => (t.bezier.is_none()
            && matches!(tracks.get(&t.track), Some(TrackShape::Xo | TrackShape::Zo)))
        .then_some(t.track),
        None => target_track(tracks, pos),
    }
}

/// `StationBlockEntity.getAssemblyDirection` for a station that picked straight track `shape`:
/// the nearest direction (x and z) to the track's axis times its target direction.
pub fn assembly_direction(shape: TrackShape, target: &Target) -> [i32; 3] {
    let axis = shape.axes()[0];
    let s = if target.front { 1.0 } else { -1.0 };
    nearest_horizontal([axis[0] * s, axis[1] * s, axis[2] * s])
}

/// `Direction.getNearest(x, 0, z)` as a unit along x or z (`getNearest`'s first best wins: north,
/// south, west, east in its order; north when none is ahead at all).
pub fn nearest_horizontal(v: Vec3) -> [i32; 3] {
    let mut best = [0, 0, -1];
    // `Float.MIN_VALUE`.
    let mut best_dot = f64::from(f32::from_bits(1));
    for d in [[0, 0, -1], [0, 0, 1], [-1, 0, 0], [1, 0, 0]] {
        let p = v[0] * f64::from(d[0]) + v[2] * f64::from(d[2]);
        if p > best_dot {
            best_dot = p;
            best = d;
        }
    }
    best
}

/// A signal boundary (`SignalBoundary`): where it stands (on the piece's even edge), and for each
/// side, the signal blocks standing for the trains running that way (side 0: along the even
/// edge, side 1: along its reverse) and the side's type (`types`: its first block's), and the
/// signals ending the chain a cross signal looks down (`collectChainedSignals`: boundary and
/// side, leaving the section beyond).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Boundary {
    pub at: Location,
    pub blocks: [Vec<[i32; 3]>; 2],
    pub types: [SignalType; 2],
    pub chain: [Vec<(usize, usize)>; 2],
}

/// The side of a boundary a train running along `edge` passes it from.
#[inline]
pub fn side_of(edge: usize) -> usize {
    edge & 1
}

/// A piece of track between two places: a track block's run (its block), a crossing's half, or
/// a curve.
struct Segment {
    ends: [Key; 2],
    turn: Option<Curve>,
    /// A curve's first track block and its other (`bePositions`).
    curve: Option<([i32; 3], [i32; 3])>,
    block: Option<[i32; 3]>,
    direction: Vec3,
}

/// A dimension's track graph and what stands on it.
#[derive(Clone, Debug, Default)]
pub struct Network {
    pub graph: Graph,
    pub nodes: HashMap<Key, usize>,
    pub keys: Vec<Key>,
    /// Straight track blocks: the edge they lie on and how far along it their centre is.
    pub blocks: HashMap<[i32; 3], Location>,
    /// Stations bound to the graph: their block and where on it.
    pub stations: Vec<([i32; 3], Location)>,
    /// Signal boundaries (`SignalBoundary`), each where the track it picked is.
    pub signals: Vec<Boundary>,
    /// Each station, signal and observer bound to the graph: the track and way it stands for
    /// (its own pick, or for a signal or observer with none, the one it was given beside its
    /// track).
    pub bound: HashMap<[i32; 3], Target>,
    /// Track observers bound to the graph: their block and where on the piece's even edge.
    pub observers: Vec<([i32; 3], Location)>,
    /// Each curve's edge from its first track block's end to its other's, by the two blocks.
    pub curve_edges: HashMap<([i32; 3], [i32; 3]), usize>,
    /// Per edge, what stands on it, by how far along it.
    pub marks: Vec<Vec<(f64, Mark)>>,
    /// Signal sections: each piece of track's first slot (its stretches between signals are
    /// consecutive slots), and each slot's section.
    slot_base: Vec<usize>,
    section_of: Vec<usize>,
}

/// The straight track a station or signal looks at (`TrackTargetingBehaviour`, as this port
/// places it): the track block beside it running straight along x or z.
pub fn target_track(tracks: &HashMap<[i32; 3], TrackShape>, pos: [i32; 3]) -> Option<[i32; 3]> {
    for o in [[0, 0, -1], [0, 0, 1], [-1, 0, 0], [1, 0, 0]] {
        let t = [pos[0] + o[0], pos[1], pos[2] + o[2]];
        if matches!(tracks.get(&t), Some(TrackShape::Xo | TrackShape::Zo)) {
            return Some(t);
        }
    }
    None
}

/// `TrackPropagator.isValidGraphNodeLocation` for a place and the pieces meeting there.
fn is_node(at: Key, here: &[usize], segments: &[Segment]) -> bool {
    if here.len() != 2 {
        return true;
    }
    // `viaTurn` forces a node at both ends of a curve.
    if here.iter().any(|&s| segments[s].turn.is_some()) {
        return true;
    }
    // `notInLineWith`: either way within about 29°.
    let (a, b) = (segments[here[0]].direction, segments[here[1]].direction);
    if dot(a, b).abs() < 7.0 / 8.0 {
        return true;
    }
    let v = key_position(at);
    let centred_x = (v[0] - round(v[0]) as f64).abs() >= 1.0e-5;
    let centred_z = (v[2] - round(v[2]) as f64).abs() >= 1.0e-5;
    if centred_x && !centred_z {
        return round(v[2]) % 16 == 0;
    }
    round(v[0]) % 16 == 0
}

impl Network {
    /// The graph of `tracks` (block → shape) and the `curves` between them (only those whose
    /// both blocks are track), with `stations` and `signals` bound to the track beside them.
    pub fn build(
        tracks: &HashMap<[i32; 3], TrackShape>,
        curves: &Curves,
        stations: &[[i32; 3]],
        signals: &[([i32; 3], SignalType)],
    ) -> Network {
        Network::build_targeted(tracks, curves, stations, signals, &HashMap::new())
    }

    /// [`Network::build`] with the signals that picked their track and way (`targets`); the
    /// others bind to the track beside them.
    pub fn build_targeted(
        tracks: &HashMap<[i32; 3], TrackShape>,
        curves: &Curves,
        stations: &[[i32; 3]],
        signals: &[([i32; 3], SignalType)],
        targets: &HashMap<[i32; 3], Target>,
    ) -> Network {
        let mut segments: Vec<Segment> = Vec::new();
        let mut positions: Vec<[i32; 3]> = tracks.keys().copied().collect();
        positions.sort_unstable();
        for p in &positions {
            let shape = tracks[p];
            if shape == TrackShape::None {
                continue;
            }
            let axes = shape.axes();
            // `getElevationAtCenter`: half a block up on a slope.
            let elevation = if axes[0][1] != 0.0 { 0.5 } else { 0.0 };
            let centre = [
                f64::from(p[0]) + 0.5,
                f64::from(p[1]) + elevation,
                f64::from(p[2]) + 0.5,
            ];
            if let [axis] = axes {
                let half = scale(*axis, 0.5);
                segments.push(Segment {
                    ends: [key(sub(centre, half)), key(add(centre, half))],
                    turn: None,
                    curve: None,
                    block: Some(*p),
                    direction: normalize(*axis),
                });
            } else {
                for axis in axes {
                    for side in [-0.5, 0.5] {
                        segments.push(Segment {
                            ends: [key(centre), key(add(centre, scale(*axis, side)))],
                            turn: None,
                            curve: None,
                            block: None,
                            direction: normalize(*axis),
                        });
                    }
                }
            }
        }
        let mut turns: Vec<(&[i32; 3], &crate::track_curves::Connection)> = curves
            .at
            .iter()
            .flat_map(|(p, list)| list.iter().filter(|c| c.primary).map(move |c| (p, c)))
            .filter(|(p, c)| tracks.contains_key(*p) && tracks.contains_key(&c.to))
            .collect();
        turns.sort_unstable_by_key(|(p, c)| (**p, c.to));
        for (p, c) in turns {
            segments.push(Segment {
                ends: [key(c.curve.starts[0]), key(c.curve.starts[1])],
                turn: Some(c.curve.clone()),
                curve: Some((*p, c.to)),
                block: None,
                direction: [0.0; 3],
            });
        }
        let mut at: HashMap<Key, Vec<usize>> = HashMap::new();
        for (i, s) in segments.iter().enumerate() {
            at.entry(s.ends[0]).or_default().push(i);
            if s.ends[1] != s.ends[0] {
                at.entry(s.ends[1]).or_default().push(i);
            }
        }
        let mut places: Vec<Key> = at.keys().copied().collect();
        places.sort_unstable();
        let mut net = Network::default();
        for &p in &places {
            if is_node(p, &at[&p], &segments) {
                net.add_node(p);
            }
        }
        // Walk each piece from its node to the next node: one edge.
        let mut used = vec![false; segments.len()];
        let mut on: Vec<([i32; 3], f64)> = Vec::new();
        let mut n = 0;
        while n < net.keys.len() {
            let start = net.keys[n];
            n += 1;
            for &first in &at[&start] {
                if used[first] {
                    continue;
                }
                on.clear();
                let (mut here, mut s, mut run) = (start, first, 0.0);
                let end = loop {
                    used[s] = true;
                    let seg = &segments[s];
                    let next = if seg.ends[0] == here {
                        seg.ends[1]
                    } else {
                        seg.ends[0]
                    };
                    let step = length(sub(key_position(next), key_position(here)));
                    if let Some(b) = seg.block {
                        on.push((b, run + step / 2.0));
                    }
                    run += step;
                    if net.nodes.contains_key(&next) {
                        break next;
                    }
                    match at[&next].iter().find(|&&o| o != s && !used[o]) {
                        Some(&o) => {
                            here = next;
                            s = o;
                        }
                        // A loop with no node on it: one here.
                        None => break next,
                    }
                };
                let a = net.nodes[&start];
                let b = net.add_node(end);
                let turn = segments[first].turn.as_ref().map(|c| {
                    if segments[first].ends[0] == start {
                        c.clone()
                    } else {
                        Curve::new(
                            [c.starts[1], c.starts[0]],
                            [c.axes[1], c.axes[0]],
                            [c.normals[1], c.normals[0]],
                        )
                    }
                });
                let e = net.graph.edges.len();
                net.graph.connect(a, b, turn);
                if let Some(blocks) = segments[first].curve {
                    let forward = segments[first].ends[0] == start;
                    net.curve_edges.insert(blocks, if forward { e } else { e ^ 1 });
                }
                for &(block, along) in &on {
                    net.blocks.insert(
                        block,
                        Location {
                            edge: e,
                            position: along,
                        },
                    );
                }
            }
        }
        net.marks = vec![Vec::new(); net.graph.edges.len()];
        for &pos in stations {
            // Its own pick (`createEdgePoint` on `determineGraphLocation`), else the track beside
            // it.
            let loc = match targets.get(&pos) {
                Some(t) => net.place(tracks, t),
                None => target_track(tracks, pos).and_then(|t| net.blocks.get(&t).copied()),
            };
            // `createEdgePoint`: another point exactly there keeps it off the graph.
            let Some(loc) = loc.filter(|l| !net.taken(*l)) else {
                continue;
            };
            if let Some(t) = targets.get(&pos) {
                net.bound.insert(pos, *t);
            }
            let mark = Mark::Station(net.stations.len());
            net.stations.push((pos, loc));
            net.mark(loc, mark);
        }
        let mut signals: Vec<([i32; 3], SignalType)> = signals.to_vec();
        signals.sort_unstable_by_key(|s| s.0);
        for &(pos, kind) in &signals {
            // Its own pick (`targetTrack`, `targetDirection`, `targetBezier`), else the track
            // beside it, for the trains that have it on their right: the place on the edge the
            // trains it stands for run along, and the pick it stands for.
            let placed = match targets.get(&pos) {
                Some(t) => net.place(tracks, t).map(|l| (l, *t)),
                None => net.beside(tracks, pos, true),
            };
            let Some((place, target)) = placed else {
                continue;
            };
            // On the piece's even edge, side 0 for the trains running along it.
            let (at, side) = if place.edge.is_multiple_of(2) {
                (place, 0)
            } else {
                let at = Location {
                    edge: place.edge ^ 1,
                    position: net.graph.length(place.edge) - place.position,
                };
                (at, 1)
            };
            let b =
                match net.signals.iter().position(|b| {
                    b.at.edge == at.edge && (b.at.position - at.position).abs() < 1.0e-6
                }) {
                    Some(b) => b,
                    None => {
                        // A station or observer exactly there keeps a new signal off it.
                        if net.taken(at) {
                            continue;
                        }
                        net.signals.push(Boundary {
                            at,
                            ..Boundary::default()
                        });
                        net.mark(at, Mark::Signal(net.signals.len() - 1));
                        net.signals.len() - 1
                    }
                };
            net.bound.insert(pos, target);
            let boundary = &mut net.signals[b];
            // `blockEntityAdded`: the side takes its first block's type.
            if boundary.blocks[side].is_empty() {
                boundary.types[side] = kind;
            }
            boundary.blocks[side].push(pos);
        }
        for m in &mut net.marks {
            m.sort_by(|a, b| a.0.total_cmp(&b.0));
        }
        net.sections();
        net.chains();
        net
    }

    /// Track observers on the track they picked (`targets`), else the straight track beside
    /// them, each on its piece's even edge; they split no section.
    pub fn bind_observers(
        &mut self,
        tracks: &HashMap<[i32; 3], TrackShape>,
        observers: &[[i32; 3]],
        targets: &HashMap<[i32; 3], Target>,
    ) {
        for &pos in observers {
            let placed = match targets.get(&pos) {
                Some(t) => self.place(tracks, t).map(|l| (l, *t)),
                None => self.beside(tracks, pos, false),
            };
            let Some((loc, target)) = placed.filter(|(l, _)| !self.taken(*l)) else {
                continue;
            };
            let at = if loc.edge.is_multiple_of(2) {
                loc
            } else {
                Location {
                    edge: loc.edge ^ 1,
                    position: self.graph.length(loc.edge) - loc.position,
                }
            };
            self.bound.insert(pos, target);
            let mark = Mark::Observer(self.observers.len());
            self.observers.push((pos, at));
            self.mark(at, mark);
        }
        for m in &mut self.marks {
            m.sort_by(|a, b| a.0.total_cmp(&b.0));
        }
    }

    /// `TrackGraphHelper.getGraphLocationAt` (straight, diagonal or sloped track) and
    /// `getBezierGraphLocationAt` (a curve's segment) for `target`, as `createEdgePoint` puts its
    /// point: the place on the edge the trains it stands for run along. On straight track that is
    /// the block's centre on the edge running against the track's first axis when `front`, along
    /// it otherwise; on a curve, `(segment + 1) / 2` from the curve's first end on the edge from
    /// it, or as far from the first end on the edge toward it when `front`. None when the track
    /// is a junction or not on the graph, or the curve is gone.
    pub fn place(
        &self,
        tracks: &HashMap<[i32; 3], TrackShape>,
        target: &Target,
    ) -> Option<Location> {
        if let Some(b) = target.bezier {
            if !tracks.contains_key(&target.track) {
                return None;
            }
            let &e = self.curve_edges.get(&(target.track, b.to))?;
            let length = self.graph.length(e);
            let along = f64::from((b.segment + 1) as f32 / 2.0);
            return Some(if target.front {
                Location {
                    edge: e ^ 1,
                    position: length - along,
                }
            } else {
                Location {
                    edge: e,
                    position: along,
                }
            });
        }
        let shape = *tracks.get(&target.track)?;
        if shape.is_junction() || shape == TrackShape::None {
            return None;
        }
        let loc = *self.blocks.get(&target.track)?;
        let axis = shape.axes()[0];
        let travel = if target.front {
            scale(axis, -1.0)
        } else {
            axis
        };
        Some(if dot(self.graph.direction(loc.edge, true), travel) < 0.0 {
            Location {
                edge: loc.edge ^ 1,
                position: self.graph.length(loc.edge) - loc.position,
            }
        } else {
            loc
        })
    }

    /// `TrackTargetingBlockItem.withGraphLocation` for a `kind` picking `target`: a junction (on
    /// no curve) refused, a place off the graph `NO_TRACK`, and `OCCUPIED` when another point of
    /// its edge is within 0.75 of it, save a signal within 0.25 of a new signal
    /// (`canCoexistWith`), which it joins.
    pub fn overlap(
        &self,
        tracks: &HashMap<[i32; 3], TrackShape>,
        target: &Target,
        kind: PointKind,
    ) -> Overlap {
        let Some(shape) = tracks.get(&target.track) else {
            return Overlap::NoTrack;
        };
        if target.bezier.is_none() && shape.is_junction() {
            return Overlap::Junction;
        }
        let Some(loc) = self.place(tracks, target) else {
            return Overlap::NoTrack;
        };
        let occupied = self.marks[loc.edge].iter().any(|&(at, mark)| {
            let d = (at - loc.position).abs();
            d <= 0.75
                && !(kind == PointKind::Signal && matches!(mark, Mark::Signal(_)) && d < 0.25)
        });
        if occupied {
            Overlap::Occupied
        } else {
            Overlap::Valid
        }
    }

    /// Whether a point stands exactly at `loc` (`EdgeData.get`, within `Mth.equal`).
    fn taken(&self, loc: Location) -> bool {
        self.marks[loc.edge]
            .iter()
            .any(|&(at, _)| (at - loc.position).abs() < 1.0e-5)
    }

    /// A block with no pick of its own, on the straight track beside it: the place of the track
    /// block's centre on the edge running the way it is given (for a signal, the way of the
    /// trains having it on their right; for an observer, the way it looks), and that as a pick.
    fn beside(
        &self,
        tracks: &HashMap<[i32; 3], TrackShape>,
        pos: [i32; 3],
        signal: bool,
    ) -> Option<(Location, Target)> {
        let track = target_track(tracks, pos)?;
        let loc = *self.blocks.get(&track)?;
        let even = if loc.edge.is_multiple_of(2) {
            loc
        } else {
            Location {
                edge: loc.edge ^ 1,
                position: self.graph.length(loc.edge) - loc.position,
            }
        };
        let way = self.facing(even.edge, track, pos);
        let axis = tracks[&track].axes()[0];
        let along = f64::from(way[0]) * axis[0] + f64::from(way[1]) * axis[2];
        // A signal's marker faces against its trains, an observer's the way it looks
        // (`prepareTrackOverlay` turns it to the axis times the target direction).
        let front = if signal { along < 0.0 } else { along > 0.0 };
        let target = Target {
            track,
            front,
            bezier: None,
        };
        let d = self.graph.direction(even.edge, true);
        let runs_along = f64::from(way[0]) * d[0] + f64::from(way[1]) * d[2] >= 0.0;
        let place = if !signal || runs_along {
            even
        } else {
            Location {
                edge: even.edge ^ 1,
                position: self.graph.length(even.edge) - even.position,
            }
        };
        Some((place, target))
    }

    /// Whether the train whose wheel points are `points` (front first, all facing its front)
    /// stands over the place `at` (on an even edge): between its first and last point
    /// (`frontSignalListener` added it, `backSignalListener` not yet taken it off). Each stretch
    /// between neighbouring points is followed from the back one up to 8 edges on.
    pub fn covers(&self, points: &[Point], at: Location) -> bool {
        let g = &self.graph;
        // `at` along edge `e`, or None when it is on another piece.
        let along = |e: usize| -> Option<f64> {
            if e == at.edge {
                Some(at.position)
            } else if e == at.edge ^ 1 {
                Some(g.length(e) - at.position)
            } else {
                None
            }
        };
        for w in points.windows(2) {
            let (front, back) = (w[0], w[1]);
            if front.edge == back.edge {
                if along(back.edge).is_some_and(|p| p >= back.position && p <= front.position) {
                    return true;
                }
                continue;
            }
            if along(back.edge).is_some_and(|p| p >= back.position) {
                return true;
            }
            if along(front.edge).is_some_and(|p| p <= front.position) {
                return true;
            }
            // The edges between: the shortest way (in edges) from the back's edge to the front's.
            let mut parent: Vec<(usize, usize)> = vec![(back.edge, usize::MAX)];
            let mut frontier = vec![back.edge];
            let mut found = None;
            'search: for _ in 0..8 {
                let mut next = Vec::new();
                for &e in &frontier {
                    for n in g.onward_iter(e) {
                        if parent.iter().any(|(x, _)| *x == n) {
                            continue;
                        }
                        parent.push((n, e));
                        if n == front.edge {
                            found = Some(e);
                            break 'search;
                        }
                        next.push(n);
                    }
                }
                frontier = next;
            }
            let mut e = found.unwrap_or(back.edge);
            while e != back.edge {
                if along(e).is_some() {
                    return true;
                }
                e = parent
                    .iter()
                    .find(|(x, _)| *x == e)
                    .map_or(back.edge, |p| p.1);
            }
        }
        false
    }

    fn add_node(&mut self, k: Key) -> usize {
        if let Some(&n) = self.nodes.get(&k) {
            return n;
        }
        let n = self.graph.node(key_position(k));
        self.nodes.insert(k, n);
        self.keys.push(k);
        n
    }

    /// The way along the track a block beside it at `pos` looks, from the track block `track`
    /// it targets, on even edge `edge`: the edge's way when the block is on its right, else
    /// back (the trains a signal with no pick of its own stands for), snapped to x or z.
    fn facing(&self, edge: usize, track: [i32; 3], pos: [i32; 3]) -> [i32; 2] {
        let d = self.graph.direction(edge, true);
        let right = [-d[2], 0.0, d[0]];
        let beside = [
            f64::from(pos[0] - track[0]),
            0.0,
            f64::from(pos[2] - track[2]),
        ];
        let s = if dot(right, beside) < 0.0 { -1.0 } else { 1.0 };
        [(d[0] * s).round() as i32, (d[2] * s).round() as i32]
    }

    /// `TrackTargetingBehaviour.render` for the signal at `pos`: the pick it stands for (its
    /// track, target direction and curve point, from which `prepareTrackOverlay` turns the
    /// marker), and how its marker shows (`SignalBoundary.getOverlayFor`: the first block of the
    /// boundary's first side with one draws it, `DUAL` when the other side has blocks too; the
    /// rest skip). None when it is on no boundary or skips.
    pub fn signal_marker(&self, pos: [i32; 3]) -> Option<(Target, bool)> {
        let &target = self.bound.get(&pos)?;
        let b = self
            .signals
            .iter()
            .find(|b| b.blocks.iter().any(|s| s.contains(&pos)))?;
        let first = if b.blocks[0].is_empty() { 1 } else { 0 };
        if b.blocks[first].first() != Some(&pos) {
            return None;
        }
        let dual = first == 0 && !b.blocks[1].is_empty();
        Some((target, dual))
    }

    /// `TrackTargetingBehaviour.render` for the track observer at `pos`: the pick it stands for.
    pub fn observer_marker(&self, pos: [i32; 3]) -> Option<Target> {
        self.observers.iter().find(|o| o.0 == pos)?;
        self.bound.get(&pos).copied()
    }

    /// The pick a station bound to the graph stands for (None for one with no pick of its own).
    pub fn station_marker(&self, pos: [i32; 3]) -> Option<Target> {
        self.stations.iter().find(|s| s.0 == pos)?;
        self.bound.get(&pos).copied()
    }

    fn mark(&mut self, loc: Location, mark: Mark) {
        let length = self.graph.length(loc.edge);
        self.marks[loc.edge].push((loc.position, mark));
        self.marks[Graph::reverse(loc.edge)].push((length - loc.position, mark));
    }

    /// `SignalPropagator`: the stretches between signals, joined through nodes.
    fn sections(&mut self) {
        let pieces = self.graph.edges.len() / 2;
        let mut base = Vec::with_capacity(pieces + 1);
        let mut slots = 0;
        for u in 0..pieces {
            base.push(slots);
            slots += 1 + self.signal_count(u);
        }
        base.push(slots);
        let mut parent: Vec<usize> = (0..slots).collect();
        fn find(parent: &mut [usize], mut i: usize) -> usize {
            while parent[i] != i {
                parent[i] = parent[parent[i]];
                i = parent[i];
            }
            i
        }
        for n in 0..self.graph.nodes.len() {
            let mut first = None;
            for &e in &self.graph.out[n] {
                let u = e / 2;
                // Leaving the node along a piece's own way: its first stretch; else its last.
                let slot = if e.is_multiple_of(2) {
                    base[u]
                } else {
                    base[u + 1] - 1
                };
                let root = find(&mut parent, slot);
                match first {
                    None => first = Some(root),
                    Some(f) if f != root => parent[root] = f,
                    _ => {}
                }
            }
        }
        self.section_of = (0..slots).map(|i| find(&mut parent, i)).collect();
        self.slot_base = base;
    }

    fn signal_count(&self, piece: usize) -> usize {
        self.marks[piece * 2]
            .iter()
            .filter(|(_, m)| matches!(m, Mark::Signal(_)))
            .count()
    }

    /// How many sections there can be (the size of an occupancy table).
    pub fn section_slots(&self) -> usize {
        self.section_of.len()
    }

    /// The section a place on an edge is in; one exactly at a signal is in the section beyond
    /// it, the way the edge runs.
    pub fn section(&self, edge: usize, position: f64) -> usize {
        let u = edge / 2;
        let length = self.graph.length(edge);
        let (along, beyond) = if edge.is_multiple_of(2) {
            (position, true)
        } else {
            (length - position, false)
        };
        let k = self.marks[u * 2]
            .iter()
            .filter(|(p, m)| {
                matches!(m, Mark::Signal(_)) && if beyond { *p <= along } else { *p < along }
            })
            .count();
        self.section_of[self.slot_base[u] + k]
    }

    /// The section a train enters passing boundary `b` from `side` (`getGroup`).
    pub fn beyond(&self, b: usize, side: usize) -> usize {
        let at = self.signals[b].at;
        if side == 0 {
            self.section(at.edge, at.position)
        } else {
            self.section(at.edge ^ 1, self.graph.length(at.edge) - at.position)
        }
    }

    /// `SignalEdgeGroup.resolveColor` for every section: the `EdgeGroupColor` (an index into
    /// [`SECTION_COLOURS`]) of each section by its id, the first colour none of the sections
    /// across its signals has (`findNextAvailable`; white when all eight are taken). Create
    /// resolves a section's colour as its signal is placed and keeps it; here each section takes
    /// its colour as the boundaries come, in order, against those already coloured, so sections
    /// side by side still differ. Track with no signal on its network is the graph's fallback
    /// section (`putGraphWithDefaultGroup`), which keeps the default yellow.
    pub fn section_colours(&self) -> HashMap<usize, usize> {
        let mut order: Vec<usize> = Vec::new();
        let mut adjacent: HashMap<usize, Vec<usize>> = HashMap::new();
        for b in 0..self.signals.len() {
            let sides = [self.beyond(b, 0), self.beyond(b, 1)];
            for (k, s) in sides.into_iter().enumerate() {
                if !order.contains(&s) {
                    order.push(s);
                }
                let other = sides[1 - k];
                if other != s {
                    let list = adjacent.entry(s).or_default();
                    if !list.contains(&other) {
                        list.push(other);
                    }
                }
            }
        }
        let mut colours: HashMap<usize, usize> = HashMap::new();
        for s in order {
            // `strikeFrom`: white strikes nothing.
            let mask = adjacent
                .get(&s)
                .into_iter()
                .flatten()
                .filter_map(|a| colours.get(a))
                .filter(|&&c| c != WHITE)
                .fold(0u32, |m, &c| m | 1 << c);
            let colour = (0..SECTION_COLOURS.len())
                .find(|&c| mask & (1 << c) == 0)
                .unwrap_or(WHITE);
            colours.insert(s, colour);
        }
        colours
    }

    /// `TrackGraphVisualizer.visualiseSignalEdgeGroups`, as a signal in hand shows it: every
    /// piece of track with an end within 50 blocks of `eye`, drawn 5/64 above the track in the
    /// colour of the section each stretch of it is in (`SignalEdgeGroup.color`), with a gap at
    /// each signal (1/16 block each side on straight track, 1/8 on a curve, which is drawn as its
    /// segments' chords). Each line: its ends and its colour (RGB).
    pub fn signal_overlay(&self, eye: Vec3) -> Vec<(Vec3, Vec3, u32)> {
        let colours = self.section_colours();
        let rgb = |section: usize| SECTION_COLOURS[colours.get(&section).copied().unwrap_or(0)];
        let lift = [0.0, f64::from(5.0f32 / 64.0), 0.0];
        let mut out = Vec::new();
        let mut line = |a: Vec3, b: Vec3, colour: u32| {
            if a != b {
                out.push((add(a, lift), add(b, lift), colour));
            }
        };
        let g = &self.graph;
        for u in 0..g.edges.len() / 2 {
            let e = u * 2;
            let (from, to) = (g.nodes[g.edges[e].from], g.nodes[g.edges[e].to]);
            if length(sub(from, eye)) > 50.0 && length(sub(to, eye)) > 50.0 {
                continue;
            }
            let mut signals: Vec<f64> = self.marks[e]
                .iter()
                .filter(|(_, m)| matches!(m, Mark::Signal(_)))
                .map(|(p, _)| *p)
                .collect();
            signals.sort_by(f64::total_cmp);
            let stretch = |k: usize| rgb(self.section_of[self.slot_base[u] + k]);
            let len = g.length(e);
            let at = |t: f64| g.position(e, t);
            match &g.edges[e].turn {
                None if signals.is_empty() => line(at(0.0), at(1.0), stretch(0)),
                None => {
                    let gap = f64::from(1.0f32 / 16.0) / len;
                    let mut prev = 0.0;
                    for (k, p) in signals.iter().enumerate() {
                        let start = prev + if prev == 0.0 { 0.0 } else { gap };
                        prev = p / len;
                        line(at(start), at(prev - gap), stretch(k));
                    }
                    line(at(prev + gap), at(1.0), stretch(signals.len()));
                }
                Some(curve) => {
                    let width = f64::from(1.0f32 / 8.0);
                    let count = curve.segments.max(1);
                    let mut next = signals.iter().copied().enumerate();
                    let mut boundary = next.next();
                    let mut colour = stretch(0);
                    let mut previous: Option<Vec3> = None;
                    for i in 0..=count {
                        let f = f64::from(i as f32 / count as f32);
                        let position = f * curve.length;
                        let mut current = at(f);
                        if let Some(prev) = previous {
                            let mut from = prev;
                            if let Some((k, b)) = boundary
                                && position > b
                            {
                                current = at((b - width) / curve.length);
                                line(from, current, colour);
                                current = at((b + width) / curve.length);
                                from = current;
                                colour = stretch(k + 1);
                                boundary = next.next();
                            }
                            line(from, current, colour);
                        }
                        previous = Some(current);
                    }
                }
            }
        }
        out
    }

    /// `SignalPropagator.collectChainedSignals` for every cross signal side: the boundaries a
    /// train leaves the section beyond it by (and the side it passes them from), once per build.
    fn chains(&mut self) {
        for b in 0..self.signals.len() {
            for side in 0..2 {
                if self.signals[b].types[side] != SignalType::Cross
                    || self.signals[b].blocks[side].is_empty()
                {
                    continue;
                }
                let inside = self.beyond(b, side);
                let mut chain = Vec::new();
                for o in 0..self.signals.len() {
                    for k in 0..2 {
                        if (o, k) == (b, 1 - side) {
                            continue;
                        }
                        if self.beyond(o, 1 - k) == inside && self.beyond(o, k) != inside {
                            chain.push((o, k));
                        }
                    }
                }
                self.signals[b].chain[side] = chain;
            }
        }
    }

    /// The point on the track nearest `at` (within half a block) facing along `direction`, on
    /// either way of its edge.
    pub fn locate(&self, at: Vec3, direction: Vec3) -> Option<Point> {
        let g = &self.graph;
        let mut best: Option<(f64, usize, f64)> = None;
        for e in (0..g.edges.len()).step_by(2) {
            let span = g.length(e);
            let (t, d) = match &g.edges[e].turn {
                None => {
                    let (a, b) = (g.nodes[g.edges[e].from], g.nodes[g.edges[e].to]);
                    let ab = sub(b, a);
                    let l2 = dot(ab, ab);
                    let t = if l2 == 0.0 {
                        0.0
                    } else {
                        (dot(sub(at, a), ab) / l2).clamp(0.0, 1.0)
                    };
                    (t, length(sub(at, add(a, scale(ab, t)))))
                }
                Some(c) => {
                    let steps = (c.length * 8.0).ceil().max(8.0) as usize;
                    let mut found = (0.0, f64::MAX);
                    for i in 0..=steps {
                        let t = i as f64 / steps as f64;
                        let d = length(sub(at, c.position(t)));
                        if d < found.1 {
                            found = (t, d);
                        }
                    }
                    found
                }
            };
            if d < 0.5 && best.is_none_or(|b| d < b.0) {
                best = Some((d, e, t * span));
            }
        }
        let (_, e, position) = best?;
        let p = Point {
            edge: e,
            position,
            blocked: false,
        };
        let span = g.length(e);
        let t = if span == 0.0 { 0.0 } else { position / span };
        let ahead = g.position(e, (t + 0.01).min(1.0));
        let behind = g.position(e, (t - 0.01).max(0.0));
        Some(if dot(sub(ahead, behind), direction) < 0.0 {
            p.flipped(g)
        } else {
            p
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::track_curves::Saved;

    const UP: Vec3 = [0.0, 1.0, 0.0];

    pub fn line(
        tracks: &mut HashMap<[i32; 3], TrackShape>,
        from: [i32; 3],
        step: [i32; 3],
        n: i32,
        shape: TrackShape,
    ) {
        for i in 0..n {
            tracks.insert(
                [
                    from[0] + step[0] * i,
                    from[1] + step[1] * i,
                    from[2] + step[2] * i,
                ],
                shape,
            );
        }
    }

    #[test]
    fn a_straight_line_is_one_edge_per_chunk() {
        let mut t = HashMap::new();
        line(&mut t, [1, 64, 3], [1, 0, 0], 10, TrackShape::Xo);
        let n = Network::build(&t, &Curves::default(), &[], &[]);
        // Two nodes, one piece of track both ways.
        assert_eq!(n.graph.nodes.len(), 2);
        assert_eq!(n.graph.edges.len(), 2);
        assert!((n.graph.length(0) - 10.0).abs() < 1e-9);
        assert_eq!(n.graph.nodes[0], [1.0, 64.0, 3.5]);
        // Each block's centre on it.
        let loc = n.blocks[&[4, 64, 3]];
        assert!((loc.position - 3.5).abs() < 1e-9, "{loc:?}");
        // Over a chunk line: a node there.
        let mut t = HashMap::new();
        line(&mut t, [10, 64, 3], [1, 0, 0], 10, TrackShape::Xo);
        let n = Network::build(&t, &Curves::default(), &[], &[]);
        assert_eq!(n.graph.nodes.len(), 3);
        assert!(n.nodes.contains_key(&key([16.0, 64.0, 3.5])));
        // A slope up and on: nodes where it bends.
        let mut t = HashMap::new();
        line(&mut t, [0, 64, 1], [0, 0, 1], 3, TrackShape::Zo);
        line(&mut t, [0, 64, 4], [0, 1, 1], 3, TrackShape::As);
        line(&mut t, [0, 67, 7], [0, 0, 1], 3, TrackShape::Zo);
        let n = Network::build(&t, &Curves::default(), &[], &[]);
        assert_eq!(n.graph.edges.len(), 6, "{:?}", n.keys);
        let slope = n.blocks[&[0, 65, 5]].edge;
        assert!((n.graph.length(slope) - 3.0 * 2f64.sqrt()).abs() < 1e-9);
        // A 45° bend is too sharp to run over (`canTravelTo`): Create's ramps onto a slope are
        // curves laid by the track item.
        let onward = n.graph.onward(n.blocks[&[0, 64, 2]].edge);
        assert!(onward.is_empty());
        assert_eq!(n.graph.onward(slope).len(), 0);
    }

    /// Straight along x from x = -20 to 2 at z = 0, a quarter turn of radius 7.5 to the south,
    /// and straight on south at x = 10 from z = 8 to 30.
    pub fn l_shape() -> (HashMap<[i32; 3], TrackShape>, Curves) {
        let mut t = HashMap::new();
        line(&mut t, [-20, 64, 0], [1, 0, 0], 23, TrackShape::Xo);
        line(&mut t, [10, 64, 8], [0, 0, 1], 23, TrackShape::Zo);
        let mut c = Curves::default();
        c.add(Saved {
            from: [2, 64, 0],
            to: [10, 64, 8],
            starts: [[3.0, 64.0, 0.5], [10.5, 64.0, 8.0]],
            axes: [[1.0, 0.0, 0.0], [0.0, 0.0, -1.0]],
            normals: [UP, UP],
            girder: false,
        });
        (t, c)
    }

    #[test]
    fn a_quarter_turn_joins_two_straights() {
        let (t, c) = l_shape();
        let n = Network::build(&t, &c, &[], &[]);
        let turn = (0..n.graph.edges.len())
            .find(|&e| n.graph.edges[e].turn.is_some())
            .unwrap();
        assert!((n.graph.length(turn) - 11.78).abs() < 0.05);
        // From the x straight, through the turn and onto the z straight.
        let west = n.blocks[&[0, 64, 0]].edge;
        let next = n.graph.onward(west);
        assert_eq!(next.len(), 1);
        let after = n.graph.onward(next[0]);
        assert_eq!(after.len(), 1);
        let south = n.blocks[&[10, 64, 9]].edge;
        assert!(after[0] == south || n.graph.edges[after[0]].to == n.graph.edges[south].from);
        // Nothing runs off the end of the curve sideways.
        assert!(n.graph.edges.iter().all(|e| e.from != e.to));
    }

    #[test]
    fn a_junction_is_a_node_with_three_ways() {
        let (mut t, c) = l_shape();
        // The x straight goes on past where the turn leaves.
        line(&mut t, [3, 64, 0], [1, 0, 0], 10, TrackShape::Xo);
        let n = Network::build(&t, &c, &[], &[]);
        let at = n.nodes[&key([3.0, 64.0, 0.5])];
        assert_eq!(n.graph.out[at].len(), 3);
        let west = n.blocks[&[1, 64, 0]].edge;
        let mut onward = n.graph.onward(west);
        onward.sort();
        assert_eq!(onward.len(), 2, "on east, or into the turn");
        assert_eq!(
            onward
                .iter()
                .filter(|&&e| n.graph.edges[e].turn.is_some())
                .count(),
            1
        );
        // A crossing: its centre is a node of four, and no train turns there.
        let mut t = HashMap::new();
        line(&mut t, [0, 64, 5], [1, 0, 0], 5, TrackShape::Xo);
        line(&mut t, [6, 64, 5], [1, 0, 0], 5, TrackShape::Xo);
        line(&mut t, [5, 64, 0], [0, 0, 1], 5, TrackShape::Zo);
        line(&mut t, [5, 64, 6], [0, 0, 1], 5, TrackShape::Zo);
        t.insert([5, 64, 5], TrackShape::CrO);
        let n = Network::build(&t, &Curves::default(), &[], &[]);
        let centre = n.nodes[&key([5.5, 64.0, 5.5])];
        assert_eq!(n.graph.out[centre].len(), 4);
        let into = n.graph.out[centre]
            .iter()
            .map(|&e| Graph::reverse(e))
            .next()
            .unwrap();
        assert_eq!(n.graph.onward(into).len(), 1, "straight across only");
    }

    #[test]
    fn a_network_with_no_signal_shows_in_the_fallback_yellow() {
        let mut t = HashMap::new();
        line(&mut t, [0, 64, 0], [1, 0, 0], 10, TrackShape::Xo);
        let n = Network::build(&t, &Curves::default(), &[], &[]);
        let lines = n.signal_overlay([5.0, 65.6, 0.5]);
        assert_eq!(lines.len(), 1);
        let (a, b, rgb) = lines[0];
        assert_eq!(rgb, SECTION_COLOURS[0]);
        let lift = f64::from(5.0f32 / 64.0);
        assert_eq!(a, [0.0, 64.0 + lift, 0.5]);
        assert_eq!(b, [10.0, 64.0 + lift, 0.5]);
        // Out of reach: nothing.
        assert!(n.signal_overlay([200.0, 64.0, 0.5]).is_empty());
    }

    #[test]
    fn signals_split_the_overlay_into_sections_of_their_own_colours() {
        let mut t = HashMap::new();
        line(&mut t, [0, 64, 0], [1, 0, 0], 15, TrackShape::Xo);
        let n = Network::build(
            &t,
            &Curves::default(),
            &[],
            &[
                ([5, 64, 1], SignalType::Entry),
                ([10, 64, -1], SignalType::Entry),
            ],
        );
        let colours = n.section_colours();
        let e = n.blocks[&[0, 64, 0]].edge;
        let (a, b, c) = (n.section(e, 2.0), n.section(e, 7.0), n.section(e, 13.0));
        // Each section differs from the one across its signal.
        assert_ne!(colours[&a], colours[&b]);
        assert_ne!(colours[&b], colours[&c]);
        let lines = n.signal_overlay([7.0, 65.6, 0.5]);
        assert_eq!(lines.len(), 3, "{lines:?}");
        // A gap of 1/16 either side of each signal (at x = 5.5 and 10.5).
        let gap = f64::from(1.0f32 / 16.0);
        let xs: Vec<(f64, f64)> = lines.iter().map(|(p, q, _)| (p[0], q[0])).collect();
        assert!((xs[0].0 - 0.0).abs() < 1e-6 && (xs[0].1 - (5.5 - gap)).abs() < 1e-4, "{xs:?}");
        assert!((xs[1].0 - (5.5 + gap)).abs() < 1e-4 && (xs[1].1 - (10.5 - gap)).abs() < 1e-4);
        assert!((xs[2].0 - (10.5 + gap)).abs() < 1e-4 && (xs[2].1 - 15.0).abs() < 1e-6);
        let rgb: Vec<u32> = lines.iter().map(|l| l.2).collect();
        assert_eq!(rgb[0], SECTION_COLOURS[colours[&a]]);
        assert_eq!(rgb[1], SECTION_COLOURS[colours[&b]]);
        assert_eq!(rgb[2], SECTION_COLOURS[colours[&c]]);
    }

    #[test]
    fn a_curve_shows_as_its_segments_chords() {
        let mut t = HashMap::new();
        t.insert([0, 64, 0], TrackShape::Xo);
        t.insert([8, 64, 8], TrackShape::Zo);
        let mut c = Curves::default();
        c.add(Saved {
            from: [0, 64, 0],
            to: [8, 64, 8],
            starts: [[1.0, 64.0, 0.5], [8.5, 64.0, 8.0]],
            axes: [[1.0, 0.0, 0.0], [0.0, 0.0, -1.0]],
            normals: [UP, UP],
            girder: false,
        });
        let n = Network::build(&t, &c, &[], &[]);
        let curve = n.graph.edges.iter().find_map(|e| e.turn.clone()).unwrap();
        let lines = n.signal_overlay([4.0, 65.0, 4.0]);
        // The curve's segments, plus the two track blocks' own runs.
        assert_eq!(lines.len(), curve.segments + 2, "{}", lines.len());
        assert!(lines.iter().all(|l| l.2 == SECTION_COLOURS[0]));
    }

    #[test]
    fn signals_split_the_track_into_sections() {
        let mut t = HashMap::new();
        line(&mut t, [0, 64, 0], [1, 0, 0], 15, TrackShape::Xo);
        // Signals beside x = 5 and x = 10, a station beside x = 12.
        let n = Network::build(
            &t,
            &Curves::default(),
            &[[12, 64, 1]],
            &[
                ([5, 64, 1], SignalType::Entry),
                ([10, 64, -1], SignalType::Entry),
            ],
        );
        assert_eq!(n.signals.len(), 2);
        assert_eq!(n.stations.len(), 1);
        let e = n.blocks[&[0, 64, 0]].edge;
        let (a, b, c) = (n.section(e, 2.0), n.section(e, 7.0), n.section(e, 13.0));
        assert!(a != b && b != c && a != c);
        // At the signal: the section beyond it, either way.
        assert_eq!(n.section(e, 5.5), b);
        assert_eq!(n.section(Graph::reverse(e), 15.0 - 5.5), a);
        // A point found again by where it is.
        let p = n.locate([7.25, 64.0, 0.5], [-1.0, 0.0, 0.0]).unwrap();
        assert_eq!(p.edge, Graph::reverse(e));
        assert!((p.position - 7.75).abs() < 1e-6);
    }

    #[test]
    fn a_signal_stands_for_the_trains_that_have_it_on_their_right() {
        let mut t = HashMap::new();
        line(&mut t, [0, 64, 0], [1, 0, 0], 20, TrackShape::Xo);
        // South of the track at x = 5: for trains running east; north at x = 10: west; both
        // sides at x = 15 (one boundary, a cross signal for the trains running west).
        let n = Network::build(
            &t,
            &Curves::default(),
            &[],
            &[
                ([5, 64, 1], SignalType::Entry),
                ([10, 64, -1], SignalType::Entry),
                ([15, 64, 1], SignalType::Entry),
                ([15, 64, -1], SignalType::Cross),
            ],
        );
        assert_eq!(n.signals.len(), 3, "the two at x = 15 are one boundary");
        let east = n.blocks[&[0, 64, 0]].edge;
        let east_side = side_of(east);
        let at = |x: f64| {
            n.signals
                .iter()
                .find(|b| {
                    let p = if b.at.edge == east {
                        b.at.position
                    } else {
                        20.0 - b.at.position
                    };
                    (p - x).abs() < 1e-6
                })
                .unwrap()
        };
        assert_eq!(at(5.5).blocks[east_side], [[5, 64, 1]]);
        assert!(at(5.5).blocks[1 - east_side].is_empty());
        assert_eq!(at(10.5).blocks[1 - east_side], [[10, 64, -1]]);
        assert!(at(10.5).blocks[east_side].is_empty());
        let both = at(15.5);
        assert_eq!(both.types[east_side], SignalType::Entry);
        assert_eq!(both.types[1 - east_side], SignalType::Cross);
        // The cross signal (westbound at x = 15) looks down its section to the boundary at
        // x = 10, passed westward.
        let b15 = n
            .signals
            .iter()
            .position(|b| std::ptr::eq(b, both))
            .unwrap();
        let b10 = n
            .signals
            .iter()
            .position(|b| std::ptr::eq(b, at(10.5)))
            .unwrap();
        assert_eq!(both.chain[1 - east_side], [(b10, 1 - east_side)]);
        assert!(
            both.chain[east_side].is_empty(),
            "an entry side has no chain"
        );
        assert_ne!(n.beyond(b15, east_side), n.beyond(b15, 1 - east_side));
    }

    #[test]
    fn a_picked_signal_stands_for_the_way_the_player_looked() {
        let mut t = HashMap::new();
        line(&mut t, [0, 64, 0], [1, 0, 0], 20, TrackShape::Xo);
        // Looking east (along x) on the x track: not `front`; west: `front`.
        let east = Target::pick([5, 64, 0], TrackShape::Xo, [1.0, -0.5, 0.1]).unwrap();
        let west = Target::pick([10, 64, 0], TrackShape::Xo, [-1.0, -0.5, 0.1]).unwrap();
        assert!(!east.front && west.front);
        assert!(Target::pick([0; 3], TrackShape::Pd, [1.0, 0.0, 0.0]).is_some());
        assert_eq!(Target::pick([0; 3], TrackShape::CrO, [1.0, 0.0, 0.0]), None);
        // North of the track (where the rule of the right would make them westbound) and one
        // further away, and a signal with no pick beside x = 15.
        let targets = HashMap::from([([5, 64, -1], east), ([10, 64, -3], west)]);
        let n = Network::build_targeted(
            &t,
            &Curves::default(),
            &[],
            &[
                ([5, 64, -1], SignalType::Entry),
                ([10, 64, -3], SignalType::Entry),
                ([15, 64, -1], SignalType::Entry),
            ],
            &targets,
        );
        assert_eq!(n.signals.len(), 3);
        let e = n.blocks[&[0, 64, 0]].edge;
        let east_side = side_of(e);
        let at = |x: f64| {
            n.signals
                .iter()
                .find(|b| {
                    let p = if b.at.edge == e {
                        b.at.position
                    } else {
                        20.0 - b.at.position
                    };
                    (p - x).abs() < 1e-6
                })
                .unwrap()
        };
        assert_eq!(at(5.5).blocks[east_side], [[5, 64, -1]]);
        assert_eq!(at(10.5).blocks[1 - east_side], [[10, 64, -3]]);
        assert_eq!(at(15.5).blocks[1 - east_side], [[15, 64, -1]]);
        // The marker stands for each pick; the one with none for the westbound trains, its
        // model turned east (the axis, `front`).
        assert_eq!(n.signal_marker([5, 64, -1]), Some((east, false)));
        assert_eq!(n.signal_marker([10, 64, -3]), Some((west, false)));
        let legacy = Target {
            track: [15, 64, 0],
            front: true,
            bezier: None,
        };
        assert_eq!(n.signal_marker([15, 64, -1]), Some((legacy, false)));
    }

    /// The direction of edge `e` at its start.
    fn heading(n: &Network, e: usize) -> Vec3 {
        n.graph.direction(e, true)
    }

    fn at_place(n: &Network, l: Location) -> Vec3 {
        let span = n.graph.length(l.edge);
        n.graph.position(l.edge, l.position / span)
    }

    fn near(a: Vec3, b: Vec3) -> bool {
        (0..3).all(|k| (a[k] - b[k]).abs() < 1e-4)
    }

    #[test]
    fn points_go_on_diagonal_and_sloped_track() {
        let mut t = HashMap::new();
        line(&mut t, [0, 64, 0], [1, 0, 1], 10, TrackShape::Pd);
        let h = std::f64::consts::FRAC_1_SQRT_2;
        // Looking along the diagonal (+x +z): the trains running that way.
        let along = Target::pick([4, 64, 4], TrackShape::Pd, [h, -0.3, h]).unwrap();
        assert!(!along.front);
        let back = Target::pick([6, 64, 6], TrackShape::Pd, [-h, -0.3, -h]).unwrap();
        assert!(back.front);
        let targets = HashMap::from([
            ([4, 64, 5], along),
            ([6, 64, 7], back),
            ([2, 64, 3], Target::pick([2, 64, 2], TrackShape::Pd, [h, 0.0, h]).unwrap()),
        ]);
        let n = Network::build_targeted(
            &t,
            &Curves::default(),
            &[[2, 64, 3]],
            &[
                ([4, 64, 5], SignalType::Entry),
                ([6, 64, 7], SignalType::Entry),
            ],
            &targets,
        );
        let l = n.place(&t, &along).unwrap();
        assert!(near(at_place(&n, l), [4.5, 64.0, 4.5]), "{l:?}");
        assert!(dot(heading(&n, l.edge), [1.0, 0.0, 1.0]) > 0.0);
        let l = n.place(&t, &back).unwrap();
        assert!(near(at_place(&n, l), [6.5, 64.0, 6.5]));
        assert!(dot(heading(&n, l.edge), [1.0, 0.0, 1.0]) < 0.0);
        // Both signals on the diagonal, for opposite ways; the station too.
        assert_eq!(n.signals.len(), 2);
        let sides: Vec<usize> = n
            .signals
            .iter()
            .map(|b| if b.blocks[0].is_empty() { 1 } else { 0 })
            .collect();
        assert_ne!(sides[0], sides[1]);
        assert_eq!(n.stations.len(), 1);
        assert!(near(at_place(&n, n.stations[0].1), [2.5, 64.0, 2.5]));

        // A ramp rising south: its centre half a block up.
        let mut t = HashMap::new();
        line(&mut t, [0, 64, 0], [0, 1, 1], 4, TrackShape::As);
        let up = Target::pick([0, 65, 1], TrackShape::As, [0.0, 0.0, 1.0]).unwrap();
        assert!(!up.front);
        let n = Network::build_targeted(
            &t,
            &Curves::default(),
            &[],
            &[([1, 65, 1], SignalType::Entry)],
            &HashMap::from([([1, 65, 1], up)]),
        );
        let l = n.place(&t, &up).unwrap();
        assert!(near(at_place(&n, l), [0.5, 65.5, 1.5]), "{:?}", at_place(&n, l));
        assert!(dot(heading(&n, l.edge), [0.0, 1.0, 1.0]) > 0.0);
        assert_eq!(n.signals.len(), 1);
        assert_eq!(n.signal_marker([1, 65, 1]), Some((up, false)));
    }

    #[test]
    fn points_go_on_curves() {
        let (t, c) = l_shape();
        let from = [2, 64, 0];
        let to = [10, 64, 8];
        let on = |segment: i32, front: bool| Target::on_curve(from, to, segment, front);
        let n = Network::build(&t, &c, &[], &[]);
        let e = n.curve_edges[&(from, to)];
        assert!(n.graph.edges[e].turn.is_some());
        let len = n.graph.length(e);
        // Segment 5: three blocks from the curve's first end, along it; `front` the other way,
        // the same place.
        let a = n.place(&t, &on(5, false)).unwrap();
        assert_eq!(a.edge, e);
        assert!((a.position - 3.0).abs() < 1e-9);
        let b = n.place(&t, &on(5, true)).unwrap();
        assert_eq!(b.edge, e ^ 1);
        assert!((b.position - (len - 3.0)).abs() < 1e-9);
        assert!(near(at_place(&n, a), at_place(&n, b)));
        // The curve's first end runs east, so the place is beyond x = 3, short of the turn's end.
        let p = at_place(&n, a);
        assert!(p[0] > 3.0 && p[0] < 10.5 && p[2] > 0.5 && p[2] < 8.0, "{p:?}");
        // Off a curve that is gone, or its other end's copy: nowhere.
        assert_eq!(n.place(&t, &Target::on_curve(to, from, 5, false)), None);
        assert_eq!(n.place(&t, &Target::on_curve(from, [99, 64, 0], 5, false)), None);

        // A station, a signal and an observer on the curve.
        let targets = HashMap::from([
            ([4, 64, 3], on(3, false)),
            ([6, 64, 5], on(9, true)),
            ([8, 64, 6], on(15, false)),
        ]);
        let mut n = Network::build_targeted(
            &t,
            &c,
            &[[4, 64, 3]],
            &[([6, 64, 5], SignalType::Entry)],
            &targets,
        );
        n.bind_observers(&t, &[[8, 64, 6]], &targets);
        assert_eq!(n.stations.len(), 1);
        assert_eq!(n.stations[0].1.edge, e);
        assert_eq!(n.signals.len(), 1);
        assert_eq!(n.observers.len(), 1);
        assert!(n.observers[0].1.edge.is_multiple_of(2));
        assert_eq!(n.station_marker([4, 64, 3]), Some(on(3, false)));
        assert_eq!(n.signal_marker([6, 64, 5]), Some((on(9, true), false)));
        assert_eq!(n.observer_marker([8, 64, 6]), Some(on(15, false)));
        // The signal splits the track into sections at the curve.
        let before = n.section(n.blocks[&[0, 64, 0]].edge, 0.0);
        let after = n.section(n.blocks[&[10, 64, 20]].edge, 0.0);
        assert_ne!(before, after);

        // Overlaps: the station's segment is taken; a signal half a block from the signal is too
        // close, the same segment joins it, a block on is clear.
        use PointKind::{Signal, Station};
        assert_eq!(n.overlap(&t, &on(3, false), Station), Overlap::Occupied);
        assert_eq!(n.overlap(&t, &on(3, true), Signal), Overlap::Occupied);
        assert_eq!(n.overlap(&t, &on(9, false), Signal), Overlap::Valid);
        assert_eq!(n.overlap(&t, &on(9, true), Station), Overlap::Occupied);
        assert_eq!(n.overlap(&t, &on(10, true), Signal), Overlap::Occupied);
        assert_eq!(n.overlap(&t, &on(11, true), Signal), Overlap::Valid);
        assert_eq!(
            n.overlap(&t, &Target::on_curve(to, from, 5, false), Station),
            Overlap::NoTrack
        );
    }

    #[test]
    fn junctions_and_missing_track_are_refused() {
        let mut t = HashMap::new();
        line(&mut t, [0, 64, 5], [1, 0, 0], 5, TrackShape::Xo);
        t.insert([5, 64, 5], TrackShape::CrO);
        let n = Network::build(&t, &Curves::default(), &[], &[]);
        let cross = Target {
            track: [5, 64, 5],
            front: false,
            bezier: None,
        };
        assert_eq!(n.overlap(&t, &cross, PointKind::Station), Overlap::Junction);
        let none = Target {
            track: [5, 70, 5],
            ..cross
        };
        assert_eq!(n.overlap(&t, &none, PointKind::Signal), Overlap::NoTrack);
        let ok = Target {
            track: [2, 64, 5],
            ..cross
        };
        assert_eq!(n.overlap(&t, &ok, PointKind::Observer), Overlap::Valid);
        assert_eq!(Overlap::Junction.feedback(), Some("no_junctions"));
        assert_eq!(Overlap::NoTrack.feedback(), Some("invalid"));
        assert_eq!(Overlap::Valid.feedback(), None);
    }

    #[test]
    fn a_station_assembles_on_its_straight_pick_only() {
        let (mut t, _) = l_shape();
        line(&mut t, [0, 64, 20], [0, 1, 1], 3, TrackShape::As);
        let x = Target::pick([0, 64, 0], TrackShape::Xo, [1.0, 0.0, 0.0]).unwrap();
        let targets = HashMap::from([
            ([0, 64, 1], x),
            ([3, 64, 1], Target::on_curve([2, 64, 0], [10, 64, 8], 2, false)),
            ([1, 65, 21], Target::pick([0, 65, 21], TrackShape::As, [0.0, 0.0, 1.0]).unwrap()),
        ]);
        assert_eq!(station_track(&t, &targets, [0, 64, 1]), Some([0, 64, 0]));
        assert_eq!(station_track(&t, &targets, [3, 64, 1]), None);
        assert_eq!(station_track(&t, &targets, [1, 65, 21]), None);
        // With no pick, the straight track beside it.
        assert_eq!(station_track(&t, &targets, [-5, 64, 1]), Some([-5, 64, 0]));
        // Looking east (not `front`): bogeys go west of it, against the trains it stands for.
        assert_eq!(assembly_direction(TrackShape::Xo, &x), [-1, 0, 0]);
        let front = Target { front: true, ..x };
        assert_eq!(assembly_direction(TrackShape::Xo, &front), [1, 0, 0]);
        assert_eq!(nearest_horizontal([0.7, 0.0, 0.7]), [0, 0, 1]);
    }
}

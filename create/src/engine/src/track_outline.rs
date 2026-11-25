//! Aiming at a curve (`TrackBlockOutline.pickCurves`, `drawCurveSelection`): the segment of a
//! curved track under the player's view, found by testing each segment's box
//! (`AllShapes.TRACK_ORTHO`, turned along the curve there) against the view's ray, and the
//! outline drawn around it.

use crate::bezier::{Curve, Vec3};
use crate::track::TrackShape;
use crate::track_render::{model_angles, rotate};

fn add(a: Vec3, b: Vec3) -> Vec3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn sub(a: Vec3, b: Vec3) -> Vec3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn scale(a: Vec3, s: f64) -> Vec3 {
    [a[0] * s, a[1] * s, a[2] * s]
}
fn distance_sqr(a: Vec3, b: Vec3) -> f64 {
    let d = sub(a, b);
    d[0] * d[0] + d[1] * d[1] + d[2] * d[2]
}
/// `Vec3.normalize`.
fn normalize(a: Vec3) -> Vec3 {
    let l = distance_sqr(a, [0.0; 3]).sqrt();
    if l < f64::from(1.0e-5f32) {
        [0.0; 3]
    } else {
        scale(a, 1.0 / l)
    }
}
/// `AngleHelper.deg`.
fn deg(angle: f64) -> f64 {
    if angle == 0.0 {
        0.0
    } else {
        f64::from((angle * 180.0 / std::f64::consts::PI) as f32)
    }
}

/// An axis-aligned box (`AABB`), min then max corner.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Aabb {
    pub min: Vec3,
    pub max: Vec3,
}

impl Aabb {
    /// `AABB.contains`.
    pub fn contains(&self, p: Vec3) -> bool {
        (0..3).all(|k| p[k] >= self.min[k] && p[k] < self.max[k])
    }

    /// `AABB.clip(from, to)`: where the segment from `from` to `to` enters the box, nearest
    /// `from` (none when it starts inside or misses it).
    pub fn clip(&self, from: Vec3, to: Vec3) -> Option<Vec3> {
        let d = sub(to, from);
        let mut best = 1.0;
        let mut hit = false;
        // Each face the segment runs into: its axis, the other two, and the face's plane.
        for k in 0..3 {
            let (a, b) = ((k + 1) % 3, (k + 2) % 3);
            let side = if d[k] > 1.0e-7 {
                self.min[k]
            } else if d[k] < -1.0e-7 {
                self.max[k]
            } else {
                continue;
            };
            let t = (side - from[k]) / d[k];
            let ea = from[a] + t * d[a];
            let eb = from[b] + t * d[b];
            if 0.0 < t
                && t < best
                && self.min[a] - 1.0e-7 < ea
                && ea < self.max[a] + 1.0e-7
                && self.min[b] - 1.0e-7 < eb
                && eb < self.max[b] + 1.0e-7
            {
                best = t;
                hit = true;
            }
        }
        hit.then(|| add(from, scale(d, best)))
    }
}

/// `Runtime.bounds`: the box around the curve's points at each segment, 1.375 out each way.
pub fn bounds(curve: &Curve) -> Aabb {
    let (a, b) = (curve.starts[0], curve.starts[1]);
    let mut min: Vec3 = std::array::from_fn(|k| a[k].min(b[k]));
    let mut max: Vec3 = std::array::from_fn(|k| a[k].max(b[k]));
    let segments = curve.segments;
    for i in 0..=segments {
        let t = i as f32 / segments as f32;
        let p = curve.position(f64::from(t));
        for k in 0..3 {
            min[k] = min[k].min(p[k]);
            max[k] = max[k].max(p[k]);
        }
    }
    let grow = f64::from(1.375f32);
    Aabb {
        min: min.map(|v| v - grow),
        max: max.map(|v| v + grow),
    }
}

/// A segment aimed at (`BezierPointSelection`): the curve's first track block and its other
/// (`BezierTrackPointLocation.curveTarget`), the segment, and its box's middle, turn
/// (`getModelAngles`, radians) and way along the curve.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Selection {
    pub track: [i32; 3],
    pub to: [i32; 3],
    pub segment: i32,
    pub anchor: Vec3,
    pub angles: Vec3,
    pub direction: Vec3,
}

/// `AllShapes.TRACK_ORTHO.get(SOUTH)`'s box, moved by `(-.5, -height / 2, -.5)`.
const SEGMENT_BOX: Aabb = Aabb {
    min: [-1.375, -0.125, -0.5],
    max: [1.375, 0.125, 0.5],
};

/// `TrackBlockOutline.pickCurves`: of the curves (each by its first track block, its other and
/// itself, primary copies only), the segment the view from `eye` along `look` meets first.
/// `reach_sqr` is the squared distance to what the crosshair is on (`f64::MAX` for nothing),
/// `range` the player's block reach; Create traces `min(reach_sqr, range) + 1` along the view.
pub fn pick<'a>(
    curves: impl IntoIterator<Item = ([i32; 3], [i32; 3], &'a Curve)>,
    eye: Vec3,
    look: Vec3,
    reach_sqr: f64,
    range: f64,
) -> Option<Selection> {
    let mut max_range = reach_sqr;
    let target = add(eye, scale(look, max_range.min(range) + 1.0));
    let mut result = None;
    for (track, to, bc) in curves {
        let b = bounds(bc);
        if !b.contains(eye) && b.clip(eye, target).is_none() {
            continue;
        }
        let lut = &bc.step_lut;
        let segments = (bc.length * 2.0) as i32;
        let mut best_segment = -1;
        let mut best_distance = f64::MAX;
        let mut new_max_range = max_range;
        let t_of = |i: usize| f64::from(lut[i] * i as f32 / segments as f32);
        for i in 0..lut.len().saturating_sub(2) {
            let (t, t1, t2) = (t_of(i), t_of(i + 1), t_of(i + 2));
            let v1 = bc.position(t);
            let v2 = bc.position(t2);
            let diff = sub(v2, v1);
            let angles = model_angles(bc.normal(t1), diff);
            let anchor = add(v1, scale(diff, 0.5));
            // Create turns both by the pitch, then the yaw.
            let local = |v: Vec3| rotate(rotate(v, deg(-angles[0]), 0), deg(-angles[1]), 1);
            let origin = local(sub(eye, anchor));
            let direction = local(sub(target, eye));
            let Some(clip) = SEGMENT_BOX.clip(origin, add(origin, direction)) else {
                continue;
            };
            let centre = [0.0, f64::from(0.25f32), 0.0];
            if best_segment != -1 && best_distance < distance_sqr(clip, centre) {
                continue;
            }
            let d = distance_sqr(clip, origin);
            if d > max_range {
                continue;
            }
            best_segment = i as i32;
            new_max_range = d;
            best_distance = distance_sqr(clip, centre);
            result = Some(Selection {
                track,
                to,
                segment: i as i32,
                anchor,
                angles,
                direction: normalize(diff),
            });
        }
        if best_segment != -1 {
            max_range = new_max_range;
        }
    }
    result
}

/// `drawCurveSelection`: the twelve edges of the segment's box (`TRACK_ORTHO` facing south),
/// turned along the curve at its anchor, in world coordinates.
pub fn outline(s: &Selection) -> Vec<(Vec3, Vec3)> {
    let (min, max) = ([-0.875, 0.0, 0.0], [1.875, 0.25, 1.0]);
    let corner = |m: usize| -> Vec3 {
        std::array::from_fn(|k| if m & (1 << k) != 0 { max[k] } else { min[k] })
    };
    // `translate(vec + (0, .125, 0)).rotateY(y).rotateX(x).translate(-.5, -.125, -.5)`.
    let place = |v: Vec3| -> Vec3 {
        let v = sub(v, [0.5, 0.125, 0.5]);
        let (sx, cx) = (s.angles[0] as f32).sin_cos();
        let (sy, cy) = (s.angles[1] as f32).sin_cos();
        let (sx, cx, sy, cy) = (f64::from(sx), f64::from(cx), f64::from(sy), f64::from(cy));
        let x_turned = [v[0], v[1] * cx - v[2] * sx, v[1] * sx + v[2] * cx];
        let y_turned = [
            x_turned[0] * cy + x_turned[2] * sy,
            x_turned[1],
            -x_turned[0] * sy + x_turned[2] * cy,
        ];
        add(add(y_turned, s.anchor), [0.0, 0.125, 0.0])
    };
    let mut out = Vec::with_capacity(12);
    for m in 0..8usize {
        for k in 0..3 {
            if m & (1 << k) == 0 {
                out.push((place(corner(m)), place(corner(m | (1 << k)))));
            }
        }
    }
    out
}

/// A box in sixteenths of a block (`Block.box`).
type Px = ([f64; 3], [f64; 3]);

/// `TrackVoxelShapes.orthogonal` along z (`TRACK_ORTHO` facing south) and along x (east).
const ORTHO_SOUTH: Px = ([-14.0, 0.0, 0.0], [30.0, 4.0, 16.0]);
const ORTHO_EAST: Px = ([0.0, 0.0, -14.0], [16.0, 4.0, 30.0]);
/// `longOrthogonalZ`, `longOrthogonalX`, `longOrthogonalZOffset`.
const LONG_Z: Px = ([-14.0, 0.0, -3.3], [30.0, 4.0, 19.3]);
const LONG_X: Px = ([-3.3, 0.0, -14.0], [19.3, 4.0, 30.0]);
const LONG_Z_OFFSET: Px = ([-14.0, 0.0, 0.0], [30.0, 4.0, 24.0]);

/// `VoxelShape.forAllEdges` of the union of `boxes` (in sixteenths): the edges of the merged
/// shape, each run of an edge along one axis joined into one line, in blocks.
fn shape_edges(boxes: &[Px]) -> Vec<(Vec3, Vec3)> {
    let grid: [Vec<f64>; 3] = std::array::from_fn(|k| {
        let mut v: Vec<f64> = boxes.iter().flat_map(|b| [b.0[k], b.1[k]]).collect();
        v.sort_by(f64::total_cmp);
        v.dedup();
        v
    });
    let size: [usize; 3] = std::array::from_fn(|k| grid[k].len().saturating_sub(1));
    let full = |c: [i64; 3]| -> bool {
        if (0..3).any(|k| c[k] < 0 || c[k] >= size[k] as i64) {
            return false;
        }
        let mid: [f64; 3] = std::array::from_fn(|k| {
            let i = c[k] as usize;
            (grid[k][i] + grid[k][i + 1]) / 2.0
        });
        boxes
            .iter()
            .any(|b| (0..3).all(|k| b.0[k] < mid[k] && mid[k] < b.1[k]))
    };
    let point = |c: [usize; 3]| -> Vec3 { std::array::from_fn(|k| grid[k][c[k]] / 16.0) };
    let mut out = Vec::new();
    // `forAllAxisEdges` for each axis `a`, the other two `b`, `c`.
    for a in 0..3 {
        let (b, c) = ((a + 1) % 3, (a + 2) % 3);
        for i in 0..=size[b] {
            for j in 0..=size[c] {
                let mut start: Option<usize> = None;
                for k in 0..=size[a] {
                    let (mut count, mut parity) = (0, 0);
                    for di in 0..=1i64 {
                        for dj in 0..=1i64 {
                            let mut cell = [0i64; 3];
                            cell[a] = k as i64;
                            cell[b] = i as i64 + di - 1;
                            cell[c] = j as i64 + dj - 1;
                            if full(cell) {
                                count += 1;
                                parity ^= di ^ dj;
                            }
                        }
                    }
                    let edge = count == 1 || count == 3 || (count == 2 && parity & 1 == 0);
                    if edge {
                        start.get_or_insert(k);
                    } else if let Some(s) = start.take() {
                        let mut from = [0; 3];
                        from[a] = s;
                        from[b] = i;
                        from[c] = j;
                        let mut to = from;
                        to[a] = k;
                        out.push((point(from), point(to)));
                    }
                }
            }
        }
    }
    out
}

/// `TrackBlockOutline.walkShapes`: the outline a track block of `shape` shows in place of its
/// block's (`drawCustomBlockSelection`), from the block's lower corner. Empty for no track.
pub fn block_outline(shape: TrackShape) -> Vec<(Vec3, Vec3)> {
    use crate::track_render::Pose;
    use TrackShape as S;
    let quarter = f64::from(std::f32::consts::FRAC_PI_4);
    let centred = |p: Pose, radians: f64| {
        p.translate([0.5, 0.5, 0.5])
            .rotate(1, radians)
            .translate([-0.5, -0.5, -0.5])
    };
    let mut out = Vec::new();
    let mut draw = |pose: &Pose, boxes: &[Px]| {
        out.extend(
            shape_edges(boxes)
                .into_iter()
                .map(|(a, b)| (pose.apply(a), pose.apply(b))),
        );
    };
    let mut pose = Pose::new();
    match shape {
        S::Xo | S::CrNdx | S::CrPdx => draw(&pose, &[ORTHO_EAST]),
        S::Zo | S::CrNdz | S::CrPdz => draw(&pose, &[ORTHO_SOUTH]),
        _ => {}
    }
    if shape.is_portal() {
        // `asPortal(d)`, turned by `AngleHelper.horizontalAngle(d)` (south 0, west -90, north
        // 180, east -270).
        let angle: f32 = match shape {
            S::Ts => 0.0,
            S::Tw => -90.0,
            S::Tn => 180.0,
            _ => -270.0,
        };
        draw(&centred(pose, f64::from(angle.to_radians())), &[LONG_Z_OFFSET]);
        return out;
    }
    match shape {
        S::Pd | S::CrPdx | S::CrPdz => {
            pose = centred(pose, quarter);
            draw(&pose, &[LONG_Z]);
        }
        S::Nd | S::CrNdx | S::CrNdz => {
            pose = centred(pose, -quarter);
            draw(&pose, &[LONG_Z]);
        }
        _ => {}
    }
    match shape {
        S::CrO => draw(&pose, &[ORTHO_SOUTH, ORTHO_EAST]),
        S::CrD => {
            pose = centred(pose, quarter);
            draw(&pose, &[LONG_Z, LONG_X]);
        }
        _ => {}
    }
    if !matches!(shape, S::Ae | S::An | S::Aw | S::As) {
        return out;
    }
    let turn = std::f64::consts::PI - f64::from((shape.model_rotation() as f32).to_radians());
    let pose = centred(pose.translate([0.0, 1.0, 0.0]), turn)
        .rotate(0, quarter)
        .translate([0.0, -3.0 / 16.0, 1.0 / 16.0]);
    draw(&pose, &[LONG_Z]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const UP: Vec3 = [0.0, 1.0, 0.0];

    fn turn() -> Curve {
        Curve::new(
            [[3.0, 64.0, 0.5], [10.5, 64.0, 8.0]],
            [[1.0, 0.0, 0.0], [0.0, 0.0, -1.0]],
            [UP, UP],
        )
    }

    #[test]
    fn a_straight_track_shows_its_twelve_edges() {
        let lines = block_outline(TrackShape::Zo);
        assert_eq!(lines.len(), 12);
        let xs: Vec<f64> = lines.iter().flat_map(|(a, b)| [a[0], b[0]]).collect();
        assert!(xs.iter().all(|x| *x == -0.875 || *x == 1.875));
        assert!(block_outline(TrackShape::None).is_empty());
    }

    #[test]
    fn a_crossing_shows_the_union_not_both_boxes() {
        // Two crossing boxes merge into the outline of a plus sign, with no edge inside it.
        let lines = block_outline(TrackShape::CrO);
        let inside = |p: Vec3| p[0] > 0.0 && p[0] < 1.0 && p[2] > 0.0 && p[2] < 1.0;
        assert!(!lines.iter().any(|(a, b)| {
            let mid: Vec3 = std::array::from_fn(|k| (a[k] + b[k]) / 2.0);
            inside(mid)
        }));
        // Each of the plus sign's 12 corners has a vertical edge.
        let vertical = lines.iter().filter(|(a, b)| a[1] != b[1]).count();
        assert_eq!(vertical, 12);
    }

    #[test]
    fn a_diagonal_track_is_turned_an_eighth() {
        let lines = block_outline(TrackShape::Pd);
        assert_eq!(lines.len(), 12);
        let far = lines
            .iter()
            .flat_map(|(a, b)| [*a, *b])
            .map(|p| (p[0] - 0.5).hypot(p[2] - 0.5))
            .fold(0.0, f64::max);
        // The long box's far corner from the centre: 22 px along, 11.3 px across.
        let corner = (22.0f64 / 16.0).hypot(11.3 / 16.0);
        assert!((far - corner).abs() < 1e-3, "{far}");
        // No edge runs along x or z any more.
        assert!(lines.iter().all(|(a, b)| a[1] != b[1] || (a[0] != b[0] && a[2] != b[2])));
    }

    #[test]
    fn a_slope_outline_climbs_with_it() {
        let lines = block_outline(TrackShape::An);
        assert_eq!(lines.len(), 12);
        let ys: Vec<f64> = lines.iter().flat_map(|(a, b)| [a[1], b[1]]).collect();
        let (lo, hi) = (
            ys.iter().copied().fold(f64::MAX, f64::min),
            ys.iter().copied().fold(f64::MIN, f64::max),
        );
        assert!(hi - lo > 1.0, "{lo} {hi}");
    }

    #[test]
    fn a_box_is_entered_where_the_segment_meets_it() {
        let b = Aabb {
            min: [0.0; 3],
            max: [1.0; 3],
        };
        assert_eq!(b.clip([0.5, 2.0, 0.5], [0.5, -1.0, 0.5]), Some([0.5, 1.0, 0.5]));
        assert_eq!(b.clip([2.0, 2.0, 0.5], [3.0, -1.0, 0.5]), None);
        // From inside, nothing (`AABB.clip`).
        assert_eq!(b.clip([0.5, 0.5, 0.5], [0.5, -1.0, 0.5]), None);
        assert!(b.contains([0.0, 0.5, 0.99]) && !b.contains([1.0, 0.5, 0.5]));
    }

    #[test]
    fn looking_down_at_a_curve_picks_the_segment_under_the_view() {
        let c = turn();
        let curves = [([2, 64, 0], [10, 64, 8], &c)];
        // Straight down onto the curve a third of the way along.
        let p = c.position(1.0 / 3.0);
        let eye = [p[0], p[1] + 3.0, p[2]];
        let s = pick(curves, eye, [0.0, -1.0, 0.0], f64::MAX, 4.5).unwrap();
        assert_eq!((s.track, s.to), ([2, 64, 0], [10, 64, 8]));
        // The segment's middle is under the eye (within its half-block length).
        let d = distance_sqr([s.anchor[0], 0.0, s.anchor[2]], [p[0], 0.0, p[2]]);
        assert!(d < 0.5, "{s:?} {p:?}");
        assert!(s.segment > 0 && s.segment < c.segments as i32 - 1);
        // The way along the curve there runs from its first end on.
        let ahead = c.position(0.4);
        let along = sub(ahead, p);
        assert!(along[0] * s.direction[0] + along[2] * s.direction[2] > 0.0);
        // A block in the way nearer than the curve, out of reach, or looking up: none.
        assert!(pick(curves, eye, [0.0, -1.0, 0.0], 1.0, 4.5).is_none());
        assert!(pick(curves, [p[0], p[1] + 9.0, p[2]], [0.0, -1.0, 0.0], f64::MAX, 4.5).is_none());
        assert!(pick(curves, eye, [0.0, 1.0, 0.0], f64::MAX, 4.5).is_none());
    }

    #[test]
    fn the_outline_is_the_segment_box_around_its_anchor() {
        let s = Selection {
            track: [0; 3],
            to: [0; 3],
            segment: 0,
            anchor: [10.0, 64.0, 10.0],
            angles: [0.0; 3],
            direction: [0.0, 0.0, 1.0],
        };
        let lines = outline(&s);
        assert_eq!(lines.len(), 12);
        let xs: Vec<f64> = lines.iter().flat_map(|(a, b)| [a[0], b[0]]).collect();
        let min = xs.iter().copied().fold(f64::MAX, f64::min);
        let max = xs.iter().copied().fold(f64::MIN, f64::max);
        assert!((min - 8.625).abs() < 1e-9 && (max - 11.375).abs() < 1e-9);
        let ys: Vec<f64> = lines.iter().flat_map(|(a, b)| [a[1], b[1]]).collect();
        assert!(ys.iter().all(|y| (*y - 64.0).abs() < 1e-9 || (*y - 64.25).abs() < 1e-9));
    }
}

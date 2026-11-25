//! Create's voxel shapes (`AllShapes.java`), as the content mod's `shapes` lists: boxes in block
//! units, rotated from the orientation Create writes them in, as its `VoxelShaper` does.

use serde_json::{Value, json};

/// A box in Create's pixels: `[x0, y0, z0, x1, y1, z1]`.
pub type Px = [f64; 6];

/// `AllShapes.SIX_VOXEL_POLE`, for the Y axis.
pub const SIX_VOXEL_POLE: Px = [5., 0., 5., 11., 16., 11.];
/// `AllShapes.SMALL_GEAR_SHAPE` (with the pole).
pub const SMALL_GEAR: Px = [2., 6., 2., 14., 10., 14.];
/// `AllShapes.LARGE_GEAR_SHAPE` (with the pole).
pub const LARGE_GEAR: Px = [0., 6., 0., 16., 10., 16.];
/// `AllShapes.MOTOR_BLOCK`, facing up.
pub const MOTOR_BLOCK: Px = [3., 0., 3., 13., 14., 13.];
/// `AllShapes.GAUGE_SHAPE_UP`: the plate and the dial box on it.
pub const GAUGE: [Px; 2] = [[1., 0., 0., 15., 2., 16.], [2., 2., 1., 14., 14., 15.]];
/// `AllShapes.CRANK`, facing up.
pub const CRANK: [Px; 2] = [[5., 0., 5., 11., 6., 11.], [1., 3., 1., 15., 8., 15.]];

/// A Y-axis box turned to `axis` (`VoxelShaper.forAxis`).
fn to_axis(b: Px, axis: &str) -> Px {
    match axis {
        "x" => [b[1], b[0], b[2], b[4], b[3], b[5]],
        "z" => [b[0], b[2], b[1], b[3], b[5], b[4]],
        _ => b,
    }
}

/// An up-facing box turned to `facing` (`VoxelShaper.forDirectional`), about the block's centre.
fn to_facing(b: Px, facing: &str) -> Px {
    let turn = |p: [f64; 3]| -> [f64; 3] {
        let [x, y, z] = [p[0] - 8., p[1] - 8., p[2] - 8.];
        let r = match facing {
            "down" => [x, -y, -z],
            "north" => [x, z, -y],
            "south" => [x, -z, y],
            "east" => [y, -x, z],
            "west" => [-y, x, z],
            _ => [x, y, z],
        };
        [r[0] + 8., r[1] + 8., r[2] + 8.]
    };
    let (a, c) = (turn([b[0], b[1], b[2]]), turn([b[3], b[4], b[5]]));
    [
        a[0].min(c[0]),
        a[1].min(c[1]),
        a[2].min(c[2]),
        a[0].max(c[0]),
        a[1].max(c[1]),
        a[2].max(c[2]),
    ]
}

fn units(b: Px) -> Value {
    json!(b.map(|v| v / 16.0))
}

/// `AllShapes.FUNNEL_FLOOR`.
pub const FUNNEL_FLOOR: [Px; 3] = [
    [2., -2., 2., 14., 8., 14.],
    [1., 1., 1., 15., 8., 15.],
    [0., 4., 0., 16., 10., 16.],
];
/// `AllShapes.FUNNEL_CEILING`.
pub const FUNNEL_CEILING: [Px; 3] = [
    [2., 8., 2., 14., 18., 14.],
    [1., 8., 1., 15., 15., 15.],
    [0., 6., 0., 16., 12., 16.],
];
/// `AllShapes.FUNNEL_WALL`, facing north.
pub const FUNNEL_WALL: [Px; 7] = [
    [2., 2., 14., 14., 14., 18.],
    [1., 8., 12., 15., 15., 14.],
    [0.1, 13., 7., 15.9, 15., 11.],
    [0.1, 9., 8., 15.9, 13., 12.],
    [0.1, 5., 9., 15.9, 9., 13.],
    [0.1, 1., 10., 15.9, 5., 14.],
    [0.1, -1., 11., 15.9, 1., 15.],
];
/// `AllShapes.BELT_FUNNEL_RETRACTED`, facing north.
pub const BELT_FUNNEL_RETRACTED: [Px; 2] =
    [[2., -2., 14., 14., 14., 18.], [0., -5., 8., 16., 16., 14.]];
/// `AllShapes.BELT_FUNNEL_EXTENDED`, facing north.
pub const BELT_FUNNEL_EXTENDED: [Px; 4] = [
    [2., -2., 14., 14., 14., 18.],
    [3., -4., 10., 13., 13., 14.],
    [2., -4., 6., 14., 14., 10.],
    [0., -5., 0., 16., 16., 6.],
];
/// `AllShapes.BELT_FUNNEL_PERPENDICULAR` (pushing and pulling), facing north.
pub const BELT_FUNNEL_PERPENDICULAR: [Px; 7] = [
    [2., -2., 14., 14., 14., 18.],
    [1., 8., 12., 15., 15., 14.],
    [0.1, 13., 7., 15.9, 15., 11.],
    [0.1, 9., 8., 15.9, 13., 12.],
    [0.1, 5., 9., 15.9, 9., 13.],
    [0.1, 1., 10., 15.9, 5., 14.],
    [0.1, -3., 11., 15.9, 1., 15.],
];

/// `FunnelBlock.getShape`: `FUNNEL_CEILING` facing down, `FUNNEL_FLOOR` facing up, else
/// `FUNNEL_WALL` turned to its facing. (Item entities collide with a sideways funnel's
/// `FUNNEL_COLLISION` instead: [`funnel_item_collision`].)
pub fn funnel_shapes() -> Value {
    let mut out = by_horizontal(&FUNNEL_WALL);
    let list = |boxes: &[Px]| json!(boxes.iter().map(|b| units(*b)).collect::<Vec<_>>());
    out["values"]["down"] = list(&FUNNEL_CEILING);
    out["values"]["up"] = list(&FUNNEL_FLOOR);
    out
}

/// A `shapes` object selecting by a horizontal `facing` and `property`: each value of
/// `property` its own boxes, written facing north and turned as [`by_horizontal`] turns them.
pub fn by_horizontal_and(property: &str, cases: &[(&str, &[Px])]) -> Value {
    let turned: Vec<(&str, Value)> = cases.iter().map(|(v, b)| (*v, by_horizontal(b))).collect();
    let mut values = serde_json::Map::new();
    for f in ["north", "south", "west", "east"] {
        for (v, shapes) in &turned {
            values.insert(format!("{f},{v}"), shapes["values"][f].clone());
        }
    }
    json!({ "properties": ["facing", property], "values": values })
}

/// `BeltFunnelBlock.getShape`: its shape's boxes (`Shape.shaper`) turned to its facing.
pub fn belt_funnel_shapes() -> Value {
    by_horizontal_and(
        "shape",
        &[
            ("retracted", &BELT_FUNNEL_RETRACTED),
            ("extended", &BELT_FUNNEL_EXTENDED),
            ("pushing", &BELT_FUNNEL_PERPENDICULAR),
            ("pulling", &BELT_FUNNEL_PERPENDICULAR),
        ],
    )
}

/// Whether side `side` of a tunnel on `axis` with `shape` is open
/// (`BeltTunnelBlockEntity.updateTunnelConnections`' sides): its ends, and the sides across it a
/// T or a cross opens (`T_LEFT` closes the east side of a tunnel along z, the north of one along
/// x; `T_RIGHT` the other).
fn tunnel_open(axis: &str, shape: &str, side: &str) -> bool {
    if matches!((axis, side), ("x", "east" | "west") | ("z", "north" | "south")) {
        return true;
    }
    let positive = matches!(side, "east" | "north");
    match shape {
        "t_left" => !positive,
        "t_right" => positive,
        "cross" => true,
        _ => false,
    }
}

/// `BeltTunnelShapes.getShape`: the block from 5 px below (over the belt) to the top, less a
/// 12 px wide, 15 px high opening on each open side, by `axis` and `shape`.
pub fn tunnel_shapes() -> Value {
    let mut values = serde_json::Map::new();
    for axis in ["x", "z"] {
        for shape in ["straight", "window", "closed", "t_left", "t_right", "cross"] {
            let mut boxes: Vec<Px> = vec![[2., -5., 2., 14., 16., 14.]];
            for (x, z) in [(0., 0.), (14., 0.), (0., 14.), (14., 14.)] {
                boxes.push([x, -5., z, x + 2., 16., z + 2.]);
            }
            for side in ["north", "south", "west", "east"] {
                let y = if tunnel_open(axis, shape, side) { 10. } else { -5. };
                boxes.push(match side {
                    "north" => [2., y, 0., 14., 16., 2.],
                    "south" => [2., y, 14., 14., 16., 16.],
                    "west" => [0., y, 2., 2., 16., 14.],
                    _ => [14., y, 2., 16., 16., 14.],
                });
            }
            let list: Vec<Value> = boxes.into_iter().map(units).collect();
            values.insert(format!("{axis},{shape}"), json!(list));
        }
    }
    json!({ "properties": ["axis", "shape"], "values": values })
}

/// A constant shape for a block whose geometry has no state selector.
pub fn fixed(boxes: &[Px]) -> Value {
    json!({ "default": boxes.iter().map(|b| units(*b)).collect::<Vec<_>>() })
}

/// A `shapes` object selecting by `axis`.
pub fn by_axis(boxes: &[Px]) -> Value {
    let values: serde_json::Map<String, Value> = ["x", "y", "z"]
        .iter()
        .map(|a| {
            (
                a.to_string(),
                json!(
                    boxes
                        .iter()
                        .map(|b| units(to_axis(*b, a)))
                        .collect::<Vec<_>>()
                ),
            )
        })
        .collect();
    json!({ "property": "axis", "values": values })
}

/// A `shapes` object giving every value of `property` the same boxes.
pub fn by_property(property: &str, values: &[&str], boxes: &[Px]) -> Value {
    let list: Vec<Value> = boxes.iter().map(|b| units(*b)).collect();
    let values: serde_json::Map<String, Value> = values
        .iter()
        .map(|v| (v.to_string(), json!(list)))
        .collect();
    json!({ "property": property, "values": values })
}

/// A `shapes` object giving each value of `property` its own boxes.
pub fn by_property_values(property: &str, values: &[(&str, &[Px])]) -> Value {
    let values: serde_json::Map<String, Value> = values
        .iter()
        .map(|(v, boxes)| {
            let list: Vec<Value> = boxes.iter().map(|b| units(*b)).collect();
            (v.to_string(), json!(list))
        })
        .collect();
    json!({ "property": property, "values": values })
}

/// A `shapes` object selecting by `facing`, from the up-facing boxes.
pub fn by_facing(boxes: &[Px]) -> Value {
    let values: serde_json::Map<String, Value> = ["down", "up", "north", "south", "west", "east"]
        .iter()
        .map(|f| {
            (
                f.to_string(),
                json!(
                    boxes
                        .iter()
                        .map(|b| units(to_facing(*b, f)))
                        .collect::<Vec<_>>()
                ),
            )
        })
        .collect();
    json!({ "property": "facing", "values": values })
}

/// A `shapes` object selecting by a horizontal `facing`, from boxes written facing north
/// (`VoxelShaper.forHorizontal(NORTH)`), turned about the block's centre.
pub fn by_horizontal(boxes: &[Px]) -> Value {
    let turn = turn_horizontal;
    let values: serde_json::Map<String, Value> = ["north", "south", "west", "east"]
        .iter()
        .map(|f| {
            (
                f.to_string(),
                json!(boxes.iter().map(|b| units(turn(b, f))).collect::<Vec<_>>()),
            )
        })
        .collect();
    json!({ "property": "facing", "values": values })
}

/// A box written facing north turned to the horizontal `f` about the block's centre
/// (`VoxelShaper.forHorizontal(NORTH)`).
fn turn_horizontal(b: &Px, f: &str) -> Px {
    let (x0, z0, x1, z1) = (b[0], b[2], b[3], b[5]);
    let (a, c) = match f {
        "east" => ([16. - z0, x0], [16. - z1, x1]),
        "south" => ([16. - x0, 16. - z0], [16. - x1, 16. - z1]),
        "west" => ([z0, 16. - x0], [z1, 16. - x1]),
        _ => ([x0, z0], [x1, z1]),
    };
    [
        a[0].min(c[0]),
        b[1],
        a[1].min(c[1]),
        a[0].max(c[0]),
        b[4],
        a[1].max(c[1]),
    ]
}

/// A box written facing south (`VoxelShaper.forHorizontal(SOUTH)`) turned to the horizontal `f`.
fn turn_from_south(b: &Px, f: &str) -> Px {
    turn_horizontal(&turn_horizontal(b, "south"), f)
}

/// A `shapes` table selecting by a face-attached block's face (`face_property`: `face`, or a
/// nixie tube's `double_face`) and its horizontal `facing`, each state's boxes `boxes(face,
/// facing)`.
fn by_face_and_facing(
    face_property: &str,
    faces: &[&str],
    boxes: impl Fn(&str, &str) -> Vec<Px>,
) -> Value {
    let mut values = serde_json::Map::new();
    for facing in ["north", "south", "east", "west"] {
        for face in faces {
            let list: Vec<Value> = boxes(face, facing).into_iter().map(units).collect();
            values.insert(format!("{face},{facing}"), json!(list));
        }
    }
    json!({ "properties": [face_property, "facing"], "values": values })
}

/// `FaceAttachedHorizontalDirectionalBlock.getConnectedDirection`: up on the floor, down on the
/// ceiling, its facing on a wall.
fn connected_direction<'a>(face: &str, facing: &'a str) -> &'a str {
    match face {
        "floor" => "up",
        "ceiling" => "down",
        _ => facing,
    }
}

/// `AllShapes.PLACARD` (`forDirectional(UP)`).
pub const PLACARD: Px = [2., 0., 2., 14., 3., 14.];

/// `PlacardBlock.getShape`: `PLACARD` turned to its connected direction.
pub fn placard_shapes() -> Value {
    by_face_and_facing("face", &["floor", "ceiling", "wall"], |face, facing| {
        vec![to_facing(PLACARD, connected_direction(face, facing))]
    })
}

/// `AllShapes.CLIPBOARD_FLOOR`, `CLIPBOARD_CEILING` and `CLIPBOARD_WALL`, facing south.
pub const CLIPBOARD_FLOOR: Px = [3., 0., 1., 13., 1., 15.];
pub const CLIPBOARD_CEILING: Px = [3., 15., 1., 13., 16., 15.];
pub const CLIPBOARD_WALL: Px = [3., 1., 0., 13., 15., 1.];

/// `ClipboardBlock.getShape`: its face's board turned to its facing.
pub fn clipboard_shapes() -> Value {
    by_face_and_facing("face", &["floor", "ceiling", "wall"], |face, facing| {
        let board = match face {
            "floor" => CLIPBOARD_FLOOR,
            "ceiling" => CLIPBOARD_CEILING,
            _ => CLIPBOARD_WALL,
        };
        vec![turn_from_south(&board, facing)]
    })
}

/// `AllShapes.NIXIE_TUBE` and `NIXIE_TUBE_CEILING` (`forHorizontalAxis`, along z), and
/// `NIXIE_TUBE_WALL` (`forHorizontal(SOUTH)`).
pub const NIXIE_TUBE: [Px; 2] = [[9., 0., 5., 15., 12., 11.], [1., 0., 5., 7., 12., 11.]];
pub const NIXIE_TUBE_CEILING: [Px; 2] = [[9., 4., 5., 15., 16., 11.], [1., 4., 5., 7., 16., 11.]];
pub const NIXIE_TUBE_WALL: [Px; 2] = [[5., 9., 0., 11., 15., 12.], [5., 1., 0., 11., 7., 12.]];

/// `NixieTubeBlock.getShape`: on the floor or the ceiling, its shape along the axis clockwise of
/// its facing (x for north and south); on a wall, either way round, the wall shape turned to its
/// facing.
pub fn nixie_tube_shapes() -> Value {
    let faces = ["floor", "ceiling", "wall", "wall_reversed"];
    by_face_and_facing("double_face", &faces, |face, facing| {
        let along = |boxes: &[Px]| -> Vec<Px> {
            match facing {
                "north" | "south" => boxes.iter().map(|b| turn_horizontal(b, "east")).collect(),
                _ => boxes.to_vec(),
            }
        };
        match face {
            "floor" => along(&NIXIE_TUBE),
            "ceiling" => along(&NIXIE_TUBE_CEILING),
            _ => NIXIE_TUBE_WALL
                .iter()
                .map(|b| turn_from_south(b, facing))
                .collect(),
        }
    })
}

/// `AllShapes.FUNNEL_COLLISION` (`forDirectional(UP)`): the 4 px plate at a funnel's back that
/// dropped items collide with instead of its outline.
pub const FUNNEL_COLLISION: Px = [0., 0., 0., 16., 4., 16.];

/// `FunnelBlock.getCollisionShape` for an item entity: a funnel facing sideways gives
/// `FUNNEL_COLLISION` turned to its facing, a floor or ceiling funnel its own shape.
pub fn funnel_item_collision() -> Value {
    let mut out = funnel_shapes();
    for f in ["north", "south", "west", "east"] {
        out["values"][f] = json!([units(to_facing(FUNNEL_COLLISION, f))]);
    }
    out
}

/// `BeltFunnelBlock.getCollisionShape` for an item entity: a pushing or pulling belt funnel gives
/// `FUNNEL_COLLISION` turned to its facing, a retracted or extended one its own shape.
pub fn belt_funnel_item_collision() -> Value {
    let mut out = belt_funnel_shapes();
    for f in ["north", "south", "west", "east"] {
        for shape in ["pushing", "pulling"] {
            out["values"][format!("{f},{shape}")] =
                json!([units(to_facing(FUNNEL_COLLISION, f))]);
        }
    }
    out
}

/// `AllShapes.CHUTE`: a chute facing down, its wide top over its narrower bottom.
pub const CHUTE: [Px; 2] = [[1., 8., 1., 15., 16., 15.], [2., 0., 2., 14., 8., 14.]];

/// `ChuteShapes.createSlope` (`AllShapes.CHUTE_SLOPE`, `forHorizontal(SOUTH)`): sixteen
/// `PANEL`s (`box(1, -15, 0, 15, 4, 1)`), the `i`th moved `i` px up and `i` px south, so the
/// slope reaches 15 px into the block below and 3 px into the one above. Written here facing
/// north (a half turn), as [`turn_horizontal`] takes its boxes.
fn chute_slope() -> Vec<Px> {
    (0..16)
        .map(|i| {
            let i = f64::from(i);
            [1., -15. + i, 15. - i, 15., 4. + i, 16. - i]
        })
        .collect()
}

/// `ChuteShapes.getShape` and `getCollisionShape` by `facing` and `shape`, as (outline,
/// collision). Facing down: `AllShapes.CHUTE`, a full block for an intersection or an encased
/// chute. A diagonal: its slope turned to its facing; an intersection or encased one is a full
/// block with the slope cut to `INTERSECTION_MASK` (16 px under it to its top). Collision is the
/// outline cut to `COLLISION_MASK` (its bottom to 24 px).
pub fn chute_shapes() -> (Value, Value) {
    const FULL: Px = [0., 0., 0., 16., 16., 16.];
    const BELOW: Px = [0., -16., 0., 16., 0., 16.];
    const COLLISION_MASK: Px = [0., 0., 0., 16., 24., 16.];
    let mut outline = serde_json::Map::new();
    let mut collision = serde_json::Map::new();
    // The barrel template's `facing` has all six values, and the game wants a shape for every
    // state, so `up` (which a chute never takes) gets the downward chute's.
    for facing in ["down", "up", "north", "south", "west", "east"] {
        for shape in ["normal", "window", "encased", "intersection"] {
            let intersection = matches!(shape, "encased" | "intersection");
            let boxes: Vec<Px> = match (facing, intersection) {
                ("down" | "up", true) => vec![FULL],
                ("down" | "up", false) => CHUTE.to_vec(),
                (f, _) => {
                    let slope = chute_slope().into_iter().map(|b| turn_horizontal(&b, f));
                    if intersection {
                        // The block and what of the slope the mask keeps outside it: the part
                        // in the block below.
                        std::iter::once(FULL)
                            .chain(slope.filter_map(|b| belt::and(b, BELOW)))
                            .collect()
                    } else {
                        slope.collect()
                    }
                }
            };
            let cut: Vec<Px> = boxes
                .iter()
                .filter_map(|b| belt::and(*b, COLLISION_MASK))
                .collect();
            let key = format!("{facing},{shape}");
            let list = |b: &[Px]| json!(b.iter().map(|b| units(*b)).collect::<Vec<_>>());
            outline.insert(key.clone(), list(&boxes));
            collision.insert(key, list(&cut));
        }
    }
    let table = |values| json!({ "properties": ["facing", "shape"], "values": values });
    (table(outline), table(collision))
}

/// Create's `BeltShapes`: a belt's outline (`getShape`: the belt's part and its casing) and its
/// collision (`getCollisionShape`: the outline cut at 19 px, `AllShapes.BELT_COLLISION_MASK`), as
/// `shapes` tables over `facing`, `slope`, `part` and `casing`. Built as Create builds them: parts
/// for a belt facing south, composed of a south and a north half, turned by `VoxelShaper`'s
/// rotations (`rotatedCopy`, `VecHelper.rotate`).
pub fn belt_shapes() -> (Value, Value) {
    let mut outline = serde_json::Map::new();
    let mut collision = serde_json::Map::new();
    for facing in ["north", "south", "west", "east"] {
        for slope in ["horizontal", "upward", "downward", "vertical", "sideways"] {
            for part in ["start", "middle", "end", "pulley"] {
                for casing in [false, true] {
                    let mut boxes = belt::shape(facing, slope, part);
                    boxes.extend(belt::casing(facing, slope, part, casing));
                    let boxes = belt::simplify(boxes);
                    let cut = belt::simplify(
                        boxes
                            .iter()
                            .filter_map(|b| belt::and(*b, [0., 0., 0., 16., 19., 16.]))
                            .collect(),
                    );
                    assert!(
                        boxes.len() <= 16,
                        "belt {facing},{slope},{part},{casing}: {} boxes",
                        boxes.len()
                    );
                    let key = format!("{facing},{slope},{part},{casing}");
                    let list = |b: &[Px]| json!(b.iter().map(|b| units(*b)).collect::<Vec<_>>());
                    outline.insert(key.clone(), list(&boxes));
                    collision.insert(key, list(&cut));
                }
            }
        }
    }
    let table =
        |values| json!({ "properties": ["facing", "slope", "part", "casing"], "values": values });
    (table(outline), table(collision))
}

mod belt {
    use super::Px;

    const SOUTH_MASK: Px = [0., -5., 8., 16., 21., 16.];
    const NORTH_MASK: Px = [0., -5., 0., 16., 21., 8.];
    const FLAT_FULL: Px = [1., 3., 0., 15., 13., 16.];
    const FLAT_END: [Px; 2] = [[1., 4., 0., 15., 12., 16.], [1., 3., 1., 15., 13., 15.]];
    const SIDE_FULL: Px = [3., 1., 0., 13., 15., 16.];
    const SIDE_END: [Px; 2] = [[4., 1., 0., 12., 15., 16.], [3., 1., 1., 13., 15., 15.]];
    const CASING_11PX: Px = [0., 0., 0., 16., 11., 16.];
    const PARTIAL_CASING: Px = [0., 0., 5., 16., 11., 16.];

    /// The intersection of two boxes, if they overlap in a volume.
    pub fn and(a: Px, b: Px) -> Option<Px> {
        let r = [
            a[0].max(b[0]),
            a[1].max(b[1]),
            a[2].max(b[2]),
            a[3].min(b[3]),
            a[4].min(b[4]),
            a[5].min(b[5]),
        ];
        (r[0] < r[3] && r[1] < r[4] && r[2] < r[5]).then_some(r)
    }

    /// `makeSlopePart`: sixteen slices one pixel apart along z, each a pixel up (descending: up
    /// toward the north) or down.
    fn slope_boxes(ascending: bool) -> Vec<Px> {
        (0..16)
            .map(|i| {
                let y = if ascending { 10 - i } else { i - 5 } as f64;
                let z = -(i as f64);
                [1., y, 15. + z, 15., 11. + y, 16. + z]
            })
            .collect()
    }

    /// `compose`: the south half of one part and the north half of another.
    fn compose(south: &[Px], north: &[Px]) -> Vec<Px> {
        let mut out: Vec<Px> = south.iter().filter_map(|b| and(*b, SOUTH_MASK)).collect();
        out.extend(north.iter().filter_map(|b| and(*b, NORTH_MASK)));
        out
    }

    /// `VecHelper.rotate` of a point about the block's centre by X, then Y, then Z degrees, as
    /// `VoxelShaper.rotatedCopy` turns a box's corners (in floats, as `Mth.sin` gives).
    fn rotate(p: [f64; 3], rot: [f64; 3]) -> [f64; 3] {
        let [mut x, mut y, mut z] = [p[0] - 8., p[1] - 8., p[2] - 8.];
        for (axis, deg) in rot.iter().enumerate() {
            if *deg == 0. {
                continue;
            }
            let a = (*deg as f32 / 180.0 * std::f32::consts::PI) as f64;
            let (s, c) = (a.sin(), a.cos());
            (x, y, z) = match axis {
                0 => (x, y * c - z * s, z * c + y * s),
                1 => (x * c + z * s, y, z * c - x * s),
                _ => (x * c - y * s, y * c + x * s, z),
            };
        }
        [x + 8., y + 8., z + 8.].map(|v| (v * 1.0e4).round() / 1.0e4)
    }

    fn rotated(boxes: &[Px], rot: [f64; 3]) -> Vec<Px> {
        if rot == [0.; 3] {
            return boxes.to_vec();
        }
        boxes
            .iter()
            .map(|b| {
                let (a, c) = (
                    rotate([b[0], b[1], b[2]], rot),
                    rotate([b[3], b[4], b[5]], rot),
                );
                [
                    a[0].min(c[0]),
                    a[1].min(c[1]),
                    a[2].min(c[2]),
                    a[0].max(c[0]),
                    a[1].max(c[1]),
                    a[2].max(c[2]),
                ]
            })
            .collect()
    }

    /// `Direction.get2DDataValue` times 90 (`horizontalAngleFromDirection`) and `toYRot`.
    fn angle(facing: &str) -> f64 {
        match facing {
            "west" => 90.,
            "north" => 180.,
            "east" => 270.,
            _ => 0.,
        }
    }

    /// `VoxelShaper.forHorizontal(shape, SOUTH).get(facing)`.
    fn horizontal(boxes: &[Px], facing: &str) -> Vec<Px> {
        rotated(boxes, [0., -angle(facing), 0.])
    }

    /// `forHorizontalAxis(shape, Z).get(axis)`: the shape for south, or for east.
    fn horizontal_axis(boxes: &[Px], facing: &str) -> Vec<Px> {
        let along_x = matches!(facing, "east" | "west");
        horizontal(boxes, if along_x { "east" } else { "south" })
    }

    /// `VerticalBeltShaper.make(shape).get(facing)`: the south shape stood up (X by -90), then
    /// turned to `facing` (X by 180 for a negative direction, Y by `-toYRot`).
    fn vertical(boxes: &[Px], facing: &str) -> Vec<Px> {
        let up = rotated(boxes, [-90., 0., 0.]);
        let negative = matches!(facing, "north" | "west");
        rotated(&up, [if negative { 180. } else { 0. }, -angle(facing), 0.])
    }

    /// `getBeltShape`.
    pub fn shape(facing: &str, slope: &str, part: &str) -> Vec<Px> {
        let full = matches!(part, "middle" | "pulley");
        let start = part == "start";
        let ending = |end: &[Px], other: &[Px]| -> Vec<Px> {
            if start {
                compose(other, end)
            } else {
                compose(end, other)
            }
        };
        match slope {
            "vertical" if full => {
                let positive = if matches!(facing, "east" | "west") {
                    "east"
                } else {
                    "south"
                };
                vertical(&[FLAT_FULL], positive)
            }
            "vertical" => vertical(&ending(&FLAT_END, &[FLAT_FULL]), facing),
            "horizontal" if full => horizontal_axis(&[FLAT_FULL], facing),
            "horizontal" => horizontal(&ending(&FLAT_END, &[FLAT_FULL]), facing),
            "sideways" if full => horizontal_axis(&[SIDE_FULL], facing),
            "sideways" => horizontal(&ending(&SIDE_END, &[SIDE_FULL]), facing),
            _ => {
                let part = slope_part(slope);
                if full {
                    horizontal(&part, facing)
                } else {
                    horizontal(&ending(&FLAT_END, &part), facing)
                }
            }
        }
    }

    fn slope_part(slope: &str) -> Vec<Px> {
        slope_boxes(slope == "upward")
    }

    /// `getCasingShape`.
    pub fn casing(facing: &str, slope: &str, part: &str, casing: bool) -> Vec<Px> {
        if !casing || matches!(slope, "vertical" | "sideways") {
            return Vec::new();
        }
        let opposite = match facing {
            "north" => "south",
            "south" => "north",
            "west" => "east",
            _ => "west",
        };
        let upward = slope == "upward";
        let partial = |f: &str| horizontal(&[PARTIAL_CASING], f);
        match (slope, part) {
            ("horizontal", _) => vec![CASING_11PX],
            (_, "middle" | "pulley") => partial(if upward { facing } else { opposite }),
            (_, "start") if upward => vec![CASING_11PX],
            (_, "start") => partial(opposite),
            (_, _) if !upward => vec![CASING_11PX],
            _ => partial(facing),
        }
    }

    /// The same union in fewer boxes: boxes inside another dropped, and pairs that make one
    /// box together joined.
    pub fn simplify(mut boxes: Vec<Px>) -> Vec<Px> {
        let inside = |a: &Px, b: &Px| (0..3).all(|i| a[i] >= b[i] && a[i + 3] <= b[i + 3]);
        loop {
            let mut changed = false;
            'outer: for i in 0..boxes.len() {
                for j in 0..boxes.len() {
                    if i == j {
                        continue;
                    }
                    let (a, b) = (boxes[i], boxes[j]);
                    if inside(&a, &b) {
                        boxes.remove(i);
                        changed = true;
                        break 'outer;
                    }
                    // Equal on two axes and touching or overlapping on the third.
                    for axis in 0..3 {
                        let same = (0..3)
                            .filter(|k| *k != axis)
                            .all(|k| a[k] == b[k] && a[k + 3] == b[k + 3]);
                        if same && a[axis] <= b[axis + 3] && b[axis] <= a[axis + 3] {
                            let mut m = a;
                            m[axis] = a[axis].min(b[axis]);
                            m[axis + 3] = a[axis + 3].max(b[axis + 3]);
                            boxes[i] = m;
                            boxes.remove(j);
                            changed = true;
                            break 'outer;
                        }
                    }
                }
            }
            if !changed {
                return boxes;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A table's boxes for `key`, in pixels.
    fn px_of(t: &Value, key: &str) -> Vec<Px> {
        serde_json::from_value::<Vec<Px>>(t["values"][key].clone())
            .unwrap()
            .into_iter()
            .map(|b| b.map(|v| v * 16.))
            .collect()
    }

    #[test]
    fn chutes_take_create_shapes_by_their_facing_and_shape() {
        let (outline, collision) = chute_shapes();
        // Every state of the barrel template's six facings has a shape, or the game refuses
        // the block ("no state value for up,normal").
        for table in [&outline, &collision] {
            assert_eq!(table["values"].as_object().unwrap().len(), 6 * 4);
        }
        assert_eq!(px_of(&outline, "up,normal"), CHUTE.to_vec());
        // Facing down: Create's two boxes, or a full block where chutes meet.
        assert_eq!(px_of(&outline, "down,normal"), CHUTE.to_vec());
        assert_eq!(px_of(&collision, "down,window"), CHUTE.to_vec());
        assert_eq!(
            px_of(&outline, "down,intersection"),
            [[0., 0., 0., 16., 16., 16.]]
        );
        // A diagonal facing south: sixteen panels rising a pixel a step toward the south, from
        // 15 px into the block below to 3 px into the one above.
        let south = px_of(&outline, "south,normal");
        assert_eq!(south.len(), 16);
        assert_eq!(south[0], [1., -15., 0., 15., 4., 1.]);
        assert_eq!(south[15], [1., 0., 15., 15., 19., 16.]);
        // Facing north, the same slope turned: it rises toward the north.
        assert_eq!(px_of(&outline, "north,normal")[15], [1., 0., 0., 15., 19., 1.]);
        // Facing east: the panels run along x.
        assert_eq!(px_of(&outline, "east,window")[0], [0., -15., 1., 1., 4., 15.]);
        // Collision keeps only what is above the block's bottom.
        let cut = px_of(&collision, "south,normal");
        assert_eq!(cut[0], [1., 0., 0., 15., 4., 1.]);
        assert!(cut.iter().all(|b| b[1] >= 0.));
        // An intersection: the block, with the slope's part in the block below; it collides
        // as a full block.
        let x = px_of(&outline, "west,encased");
        assert_eq!(x[0], [0., 0., 0., 16., 16., 16.]);
        assert!(x[1..].iter().all(|b| b[4] <= 0.));
        assert_eq!(
            px_of(&collision, "west,encased"),
            [[0., 0., 0., 16., 16., 16.]]
        );
    }

    #[test]
    fn items_collide_with_a_sideways_funnels_back_plate() {
        // A funnel facing north hangs on the block south of it: its back plate is at the south.
        let wall = funnel_item_collision();
        assert_eq!(px_of(&wall, "north"), [[0., 0., 12., 16., 16., 16.]]);
        assert_eq!(px_of(&wall, "east"), [[0., 0., 0., 4., 16., 16.]]);
        // Floor and ceiling funnels keep their own shape.
        assert_eq!(wall["values"]["up"], funnel_shapes()["values"]["up"]);
        let belt = belt_funnel_item_collision();
        assert_eq!(px_of(&belt, "south,pushing"), [[0., 0., 0., 16., 16., 4.]]);
        assert_eq!(
            belt["values"]["south,extended"],
            belt_funnel_shapes()["values"]["south,extended"]
        );
    }

    #[test]
    fn poles_turn_to_their_axis_and_motors_to_their_facing() {
        assert_eq!(to_axis(SIX_VOXEL_POLE, "x"), [0., 5., 5., 16., 11., 11.]);
        assert_eq!(to_axis(SIX_VOXEL_POLE, "z"), [5., 5., 0., 11., 11., 16.]);
        // A motor facing north has its body at the south end, open to the north.
        assert_eq!(to_facing(MOTOR_BLOCK, "north"), [3., 3., 2., 13., 13., 16.]);
        assert_eq!(to_facing(MOTOR_BLOCK, "east"), [0., 3., 3., 14., 13., 13.]);
        assert_eq!(to_facing(MOTOR_BLOCK, "down"), [3., 2., 3., 13., 16., 13.]);
        // A belt funnel facing north has its body to the south; facing east, to the west.
        let v = by_horizontal(&[[0., -5., 8., 16., 16., 14.]]);
        let east: Vec<f64> = serde_json::from_value(v["values"]["east"][0].clone()).unwrap();
        assert_eq!(east, [2. / 16., -5. / 16., 0., 8. / 16., 1., 1.]);
    }

    #[test]
    fn funnels_and_tunnels_take_create_shapes_by_their_state() {
        let px = |t: &Value, key: &str| -> Vec<Px> {
            serde_json::from_value::<Vec<Px>>(t["values"][key].clone())
                .unwrap()
                .into_iter()
                .map(|b| b.map(|v| v * 16.0))
                .collect()
        };
        // A floor funnel reaches 10 px up, its top full width; a wall funnel facing east has its
        // back past the west face.
        let f = funnel_shapes();
        assert!(px(&f, "up").contains(&[0., 4., 0., 16., 10., 16.]));
        assert!(px(&f, "down").contains(&[0., 6., 0., 16., 12., 16.]));
        assert!(px(&f, "east").contains(&[-2., 2., 2., 2., 14., 14.]));
        // An extended belt funnel covers the belt up to its far side; a retracted one does not.
        let b = belt_funnel_shapes();
        assert_eq!(b["properties"], json!(["facing", "shape"]));
        assert!(px(&b, "north,extended").contains(&[0., -5., 0., 16., 16., 6.]));
        assert_eq!(px(&b, "north,retracted").len(), 2);
        assert_eq!(px(&b, "east,pulling"), px(&b, "east,pushing"));
        // A straight tunnel along x is open at its east and west ends only; a T opens one side.
        let t = tunnel_shapes();
        let straight = px(&t, "x,straight");
        assert!(straight.contains(&[2., -5., 0., 14., 16., 2.]));
        assert!(straight.contains(&[14., 10., 2., 16., 16., 14.]));
        let tee = px(&t, "x,t_left");
        assert!(tee.contains(&[2., -5., 0., 14., 16., 2.]));
        assert!(tee.contains(&[2., 10., 14., 14., 16., 16.]));
        let tee = px(&t, "z,t_right");
        assert!(tee.contains(&[0., -5., 2., 2., 16., 14.]));
        assert!(tee.contains(&[14., 10., 2., 16., 16., 14.]));
        assert!(px(&t, "z,cross").iter().filter(|b| b[1] == 10.).count() == 4);
    }

    #[test]
    fn face_attached_blocks_take_create_shapes_by_face_and_facing() {
        // A placard is a 3 px plate against the block it hangs on (`getConnectedDirection`).
        let p = placard_shapes();
        assert_eq!(p["properties"], json!(["face", "facing"]));
        assert_eq!(p["values"].as_object().unwrap().len(), 3 * 4);
        assert_eq!(px_of(&p, "floor,east"), [PLACARD]);
        assert_eq!(px_of(&p, "ceiling,north"), [[2., 13., 2., 14., 16., 14.]]);
        assert_eq!(px_of(&p, "wall,north"), [[2., 2., 13., 14., 14., 16.]]);
        assert_eq!(px_of(&p, "wall,west"), [[13., 2., 2., 16., 14., 14.]]);
        // A clipboard's boards are written facing south and turn with it, on the floor too.
        let c = clipboard_shapes();
        assert_eq!(px_of(&c, "wall,south"), [CLIPBOARD_WALL]);
        assert_eq!(px_of(&c, "wall,north"), [[3., 1., 15., 13., 15., 16.]]);
        assert_eq!(px_of(&c, "wall,east"), [[0., 1., 3., 1., 15., 13.]]);
        assert_eq!(px_of(&c, "floor,east"), [[1., 0., 3., 15., 1., 13.]]);
        // A nixie tube on the floor or ceiling lies along the axis clockwise of its facing; on a
        // wall, either way round, it is the wall shape turned.
        let n = nixie_tube_shapes();
        assert_eq!(n["properties"], json!(["double_face", "facing"]));
        assert_eq!(n["values"].as_object().unwrap().len(), 4 * 4);
        assert_eq!(px_of(&n, "floor,east"), NIXIE_TUBE.to_vec());
        assert_eq!(
            px_of(&n, "floor,north"),
            [[5., 0., 9., 11., 12., 15.], [5., 0., 1., 11., 12., 7.]]
        );
        assert_eq!(px_of(&n, "ceiling,south")[0], [5., 4., 9., 11., 16., 15.]);
        assert_eq!(px_of(&n, "wall,south"), NIXIE_TUBE_WALL.to_vec());
        assert_eq!(px_of(&n, "wall_reversed,west"), px_of(&n, "wall,west"));
        assert_eq!(px_of(&n, "wall,east")[0], [0., 9., 5., 12., 15., 11.]);
    }

    #[test]
    fn belts_take_create_shapes_by_facing_slope_part_and_casing() {
        let (outline, collision) = belt_shapes();
        let px = |t: &Value, key: &str| -> Vec<Px> {
            serde_json::from_value::<Vec<Px>>(t["values"][key].clone())
                .unwrap()
                .into_iter()
                .map(|b| b.map(|v| v * 16.0))
                .collect()
        };
        assert_eq!(
            px(&outline, "north,horizontal,middle,false"),
            [[1., 3., 0., 15., 13., 16.]]
        );
        assert_eq!(
            px(&outline, "east,horizontal,pulley,false"),
            [[0., 3., 1., 16., 13., 15.]]
        );
        // A flat belt in its casing is the casing's 11 px and the belt over it.
        let cased = px(&outline, "south,horizontal,middle,true");
        assert!(cased.contains(&[0., 0., 0., 16., 11., 16.]));
        // Descending south: low at the south end, high at the north, past the block both ways.
        let slope = px(&outline, "south,downward,middle,false");
        assert_eq!(slope.len(), 16);
        assert!(slope.contains(&[1., -5., 15., 15., 6., 16.]));
        assert!(slope.contains(&[1., 10., 0., 15., 21., 1.]));
        // Ascending north rises toward the north, as descending south does.
        let mut up = px(&outline, "north,upward,middle,false");
        up.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let mut down = slope.clone();
        down.sort_by(|a, b| a.partial_cmp(b).unwrap());
        assert_eq!(up, down);
        // Collision is cut at 19 px.
        assert!(
            px(&collision, "south,downward,middle,false")
                .iter()
                .all(|b| b[4] <= 19.)
        );
        // Vertical: a panel across the facing's axis.
        assert_eq!(
            px(&outline, "south,vertical,middle,false"),
            [[1., 0., 3., 15., 16., 13.]]
        );
        assert_eq!(
            px(&outline, "west,vertical,pulley,false"),
            [[3., 0., 1., 13., 16., 15.]]
        );
        // Every state fits the declaration's 16 boxes.
        for v in outline["values"].as_object().unwrap().values() {
            assert!(!v.as_array().unwrap().is_empty() && v.as_array().unwrap().len() <= 16);
        }
    }
}

//! Create's sliding doors (`SlidingDoorBlock`, `SlidingDoorBlockEntity`): the iron door's
//! template places them, keeps their halves together and follows redstone; these are Create's own
//! rules on top: a double door opens and closes as one, `visible` turns off while the door is open
//! or sliding, and on again once its panel has slid shut. A client's own view of the panel, which
//! its renderer draws while the door is hidden, is a [`Panel`].

use crate::kinetics::Dir;
use crate::parts::StateInfo;

/// A sliding door's state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Door {
    pub facing: Dir,
    pub upper: bool,
    pub hinge_right: bool,
    pub open: bool,
    pub visible: bool,
}

impl Door {
    pub fn of(info: &StateInfo) -> Option<Door> {
        Some(Door {
            facing: info.facing?,
            upper: info.upper,
            hinge_right: info.hinge_right,
            open: info.open,
            visible: info.visible,
        })
    }

    /// Where the other door of a double door would stand: `pos.relative(hinge == LEFT ?
    /// facing.getClockWise() : facing.getCounterClockWise())`.
    pub fn partner(&self, pos: [i32; 3]) -> [i32; 3] {
        let side = if self.hinge_right {
            counter_clockwise(self.facing)
        } else {
            clockwise(self.facing)
        };
        let o = side.offset();
        [pos[0] + o[0], pos[1] + o[1], pos[2] + o[2]]
    }

    /// The other half's position.
    pub fn other_half(&self, pos: [i32; 3]) -> [i32; 3] {
        [pos[0], pos[1] + if self.upper { -1 } else { 1 }, pos[2]]
    }

    /// `SlidingDoorBlock.isDoubleDoor(this, hinge, facing, other)`, `other` being the same block:
    /// the other hinge, the same facing and half, and not (yet) as open as this.
    pub fn is_double(&self, other: &Door) -> bool {
        other.hinge_right != self.hinge_right
            && other.facing == self.facing
            && other.open != self.open
            && other.upper == self.upper
    }
}

/// What `DoorBlock.getHinge` reads on one side of a door being placed: whether a lower half of
/// the same door stands there, and whether the block there and the one above it are full
/// collision cubes (`isCollisionShapeFullBlock`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HingeSide {
    pub door: bool,
    pub full: bool,
    pub full_above: bool,
}

/// The sides `DoorBlock.getHinge` looks at for a door facing `facing` at `pos`: its left
/// (`facing.getCounterClockWise()`) and its right (`getClockWise()`).
pub fn hinge_sides(facing: Dir, pos: [i32; 3]) -> [[i32; 3]; 2] {
    [counter_clockwise(facing), clockwise(facing)].map(|d| {
        let o = d.offset();
        [pos[0] + o[0], pos[1] + o[1], pos[2] + o[2]]
    })
}

/// `DoorBlock.getHinge` (`SlidingDoorBlock.getStateForPlacement` keeps its `super`'s hinge):
/// true for the right hinge. A door beside a lower door on its left hinges right, and one on
/// its right left, so a second door placed beside a first makes a double door; else the hinge
/// goes to the side with more full blocks; else to the half of the block the click landed in.
/// `click` is the click's place within the block, x and z.
pub fn placement_hinge(facing: Dir, left: HingeSide, right: HingeSide, click: [f64; 2]) -> bool {
    let full = |s: HingeSide| i32::from(s.full) + i32::from(s.full_above);
    let i = full(right) - full(left);
    if (!left.door || right.door) && i <= 0 {
        if (!right.door || left.door) && i >= 0 {
            let [j, _, k] = facing.offset();
            let [d0, d1] = click;
            let left_hinge = (j >= 0 || d1 >= 0.5)
                && (j <= 0 || d1 <= 0.5)
                && (k >= 0 || d0 <= 0.5)
                && (k <= 0 || d0 >= 0.5);
            !left_hinge
        } else {
            false
        }
    } else {
        true
    }
}

fn clockwise(d: Dir) -> Dir {
    match d {
        Dir::North => Dir::East,
        Dir::East => Dir::South,
        Dir::South => Dir::West,
        Dir::West => Dir::North,
        other => other,
    }
}

fn counter_clockwise(d: Dir) -> Dir {
    match d {
        Dir::North => Dir::West,
        Dir::West => Dir::South,
        Dir::South => Dir::East,
        Dir::East => Dir::North,
        other => other,
    }
}

/// The panel's slide (`SlidingDoorBlockEntity.animation`): catnip's `LerpedFloat.linear()`,
/// chased at 0.15 a tick toward 1 when open, 0 when closed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Slide {
    value: f32,
    previous: f32,
    target: f32,
    chasing: bool,
}

/// `Mth.equal` for doubles.
fn equal(a: f64, b: f64) -> bool {
    (b - a).abs() < 1.0e-5
}

impl Slide {
    /// A block entity made or loaded with its door (`startWithValue(isOpen ? 1 : 0)`).
    pub fn at_rest(open: bool) -> Slide {
        let v = if open { 1.0 } else { 0.0 };
        Slide {
            value: v,
            previous: v,
            target: v,
            chasing: false,
        }
    }

    /// `LerpedFloat.settled`.
    pub fn settled(&self) -> bool {
        equal(f64::from(self.previous), f64::from(self.value))
            && (!self.chasing || equal(f64::from(self.value), f64::from(self.target)))
    }

    /// Whether the panel is still and where its door wants it: nothing more to tick.
    pub fn at_target(&self) -> bool {
        self.settled() && self.chasing
    }

    /// One server tick of `SlidingDoorBlockEntity.tick` for a door `open` and `visible` now:
    /// true when the closed panel has just come to rest while hidden (`showBlockModel`: the
    /// door turns visible, with an iron door's closing sound at half volume).
    pub fn tick(&mut self, open: bool, visible: bool) -> bool {
        let was_settled = self.settled();
        // chase(open ? 1 : 0, .15f, Chaser.LINEAR)
        self.target = if open { 1.0 } else { 0.0 };
        self.chasing = true;
        // tickChaser
        self.previous = self.value;
        if equal(f64::from(self.value), f64::from(self.target)) {
            self.value = self.target;
        } else {
            let (c, t, s) = (
                f64::from(self.value),
                f64::from(self.target),
                f64::from(0.15f32),
            );
            self.value = (c + (t - c).clamp(-s, s)) as f32;
        }
        !open && !was_settled && self.settled() && !visible
    }

    pub fn value(&self) -> f32 {
        self.value
    }
}

/// `SlidingDoorBlockEntity.bridgeTicks` counts up to 2 while the door is open.
const BRIDGE_TICKS: f64 = 2.0;

/// A client's view of a door's panel (`SlidingDoorBlockEntity.animation` and `bridgeTicks` as a
/// client ticks them), worked out from the game time with the partial tick: where it was at
/// `since` and where it is heading, and how long the renderer keeps drawing it once the door
/// shows its block again.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Panel {
    from: f32,
    since: f64,
    target: f32,
    /// When `bridgeTicks` would have counted up from 0 to what it has while the door is open
    /// (it goes on from what it had when the door opens again before running out).
    opened: f64,
    /// `bridgeTicks` when the door last closed: it holds while the door slides shut hidden.
    bridge: f64,
    /// When the closed door was first seen shown again: its bridge runs out from then, a tick
    /// at a time (`bridgeTicks--` while closed and visible).
    shown: Option<f64>,
}

impl Panel {
    /// A door first seen at `now` (`startWithValue(isOpen ? 1 : 0)`, `bridgeTicks` 0).
    pub fn new(open: bool, now: f64) -> Panel {
        let target = if open { 1.0 } else { 0.0 };
        Panel {
            from: target,
            since: now,
            target,
            opened: now,
            bridge: 0.0,
            shown: None,
        }
    }

    /// `LerpedFloat.getValue` of a linear chase at 0.15 a tick.
    pub fn value(&self, now: f64) -> f32 {
        let moved = ((now - self.since).max(0.0) * 0.15) as f32;
        if self.target > self.from {
            (self.from + moved).min(self.target)
        } else {
            (self.from - moved).max(self.target)
        }
    }

    /// `bridgeTicks` at `now`: counting up while the door is open, else what it had.
    fn bridge_at(&self, now: f64) -> f64 {
        if self.target > 0.5 {
            (now - self.opened).clamp(0.0, BRIDGE_TICKS)
        } else {
            self.bridge
        }
    }

    /// The door is `open` at `now` (`animation.chase(open ? 1 : 0, .15f, LINEAR)`): the panel
    /// turns toward it from where it has got.
    pub fn follow(&mut self, open: bool, now: f64) {
        let target = if open { 1.0 } else { 0.0 };
        if self.target == target {
            return;
        }
        if open {
            self.opened = now - self.bridge;
        } else {
            self.bridge = self.bridge_at(now);
        }
        self.from = self.value(now);
        self.since = now;
        self.target = target;
    }

    /// `shouldRenderSpecial`: the door is hidden, or shown again less than its bridge ago.
    pub fn special(&mut self, visible: bool, now: f64) -> bool {
        if !visible {
            self.shown = None;
            return true;
        }
        if self.target > 0.5 {
            // Open and shown, which Create's own writes never leave a door: the bridge counts.
            return self.bridge_at(now) > 0.0;
        }
        let shown = *self.shown.get_or_insert(now);
        now - shown < self.bridge
    }
}

/// `SlidingDoorRenderer`'s slide of a sliding (not folding) door at `value` (0 closed, 1 open):
/// along `facing.getClockWise()` (the other way for a left hinge) by `value² × 13/16`, and out
/// along its facing by up to 1/32 over the first tenth of the way.
pub fn slide_offset(facing: Dir, hinge_right: bool, value: f32) -> [f32; 3] {
    let along = if hinge_right {
        clockwise(facing)
    } else {
        counter_clockwise(facing)
    };
    let out = (value * 10.0).clamp(0.0, 1.0) / 32.0;
    let slide = value * value * 13.0 / 16.0;
    let (a, f) = (along.offset(), facing.offset());
    [0, 1, 2].map(|i| a[i] as f32 * slide + f[i] as f32 * out)
}

/// One leaf of a folding door (`SlidingDoorRenderer`, `isFoldingDoor`) at `value`: the steps
/// of its pose in order, as the renderer applies them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FoldStep {
    Translate([f32; 3]),
    /// Degrees about y through `pivot` (blocks).
    TurnY {
        degrees: f32,
        pivot: [f32; 3],
    },
}

/// The pose of a folding door's leaf: `left` the first of the two partials drawn (the left
/// leaf, or the right one for a right hinge, `partials.get(left ^ flip)`), and whether the left
/// partial model is the one to draw.
pub fn fold_pose(facing: Dir, hinge_right: bool, left: bool, value: f32) -> (bool, Vec<FoldStep>) {
    use FoldStep::*;
    let flip = hinge_right;
    let f = if flip { -1.0 } else { 1.0 };
    let out = (value * 10.0).clamp(0.0, 1.0) / 32.0;
    let o = facing.offset();
    let mut steps = vec![
        Translate([0.0, -1.0 / 512.0, 0.0]),
        Translate([o[0] as f32 * out, o[1] as f32 * out, o[2] as f32 * out]),
        TurnY {
            degrees: crate::decor::horizontal_angle(clockwise(facing)),
            pivot: [0.5; 3],
        },
    ];
    if flip {
        steps.push(Translate([0.0, 0.0, 1.0]));
    }
    steps.push(TurnY {
        degrees: 91.0 * f * value * value,
        pivot: [0.0; 3],
    });
    if !left {
        steps.push(Translate([0.0, 0.0, f / 2.0]));
        steps.push(TurnY {
            degrees: -181.0 * f * value * value,
            pivot: [0.0; 3],
        });
    }
    if flip {
        steps.push(Translate([0.0, 0.0, -0.5]));
    }
    (left ^ flip, steps)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn door(facing: Dir, hinge_right: bool, open: bool) -> Door {
        Door {
            facing,
            upper: false,
            hinge_right,
            open,
            visible: !open,
        }
    }

    #[test]
    fn a_door_placed_in_a_doorway_hinges_like_the_game_s() {
        let open = HingeSide::default();
        let wall = HingeSide {
            full: true,
            full_above: true,
            ..open
        };
        let door = HingeSide { door: true, ..open };
        // Facing west, the left is south (+z) and the right north (-z).
        assert_eq!(
            hinge_sides(Dir::West, [-1, 63, -58]),
            [[-1, 63, -57], [-1, 63, -59]]
        );
        // A two-wide doorway between walls, facing west: the south door hinges left on its wall
        // whatever the click or the order, the north one right; they make a double door.
        for click in [[0.2, 0.2], [0.8, 0.8], [0.5, 0.1]] {
            assert!(!placement_hinge(Dir::West, wall, open, click));
            assert!(!placement_hinge(Dir::West, wall, door, click));
            assert!(placement_hinge(Dir::West, open, wall, click));
            assert!(placement_hinge(Dir::West, door, wall, click));
        }
        let south = door_hinged(Dir::West, false);
        let north = door_hinged(Dir::West, true);
        // Opened, the south door takes the north one with it (`isDoubleDoor`).
        assert!(south.with_open(true).is_double(&north));
        assert_eq!(south.partner([-1, 63, -58]), [-1, 63, -59]);
        // A lower door beside decides it, walls or not; with none, the click's half does
        // (facing north: the west half hinges left).
        assert!(placement_hinge(Dir::North, door, open, [0.9, 0.5]));
        assert!(!placement_hinge(Dir::North, open, door, [0.1, 0.5]));
        assert!(!placement_hinge(Dir::North, open, open, [0.3, 0.5]));
        assert!(placement_hinge(Dir::North, open, open, [0.7, 0.5]));
        // Doors on both sides: the click again.
        assert!(placement_hinge(Dir::South, door, door, [0.3, 0.5]));
        assert!(!placement_hinge(Dir::South, door, door, [0.7, 0.5]));
    }

    fn door_hinged(facing: Dir, hinge_right: bool) -> Door {
        door(facing, hinge_right, false)
    }

    impl Door {
        fn with_open(self, open: bool) -> Door {
            Door { open, ..self }
        }
    }

    #[test]
    fn a_sliding_panel_slides_toward_its_hinge_side_and_out() {
        // Facing north with the hinge right: it slides east (clockwise), 13/16 when open, and
        // 1/32 north.
        let open = slide_offset(Dir::North, true, 1.0);
        assert_eq!(open, [13.0 / 16.0, 0.0, -1.0 / 32.0]);
        // A left hinge slides it west; half way it has gone a quarter of the way.
        let half = slide_offset(Dir::North, false, 0.5);
        assert_eq!(half, [-13.0 / 64.0, 0.0, -1.0 / 32.0]);
        assert_eq!(slide_offset(Dir::East, true, 0.0), [0.0; 3]);
    }

    #[test]
    fn folding_leaves_turn_against_each_other() {
        let (first, steps) = fold_pose(Dir::South, false, true, 1.0);
        assert!(first);
        assert!(steps.contains(&FoldStep::TurnY {
            degrees: 91.0,
            pivot: [0.0; 3]
        }));
        let (model_left, steps) = fold_pose(Dir::South, true, false, 1.0);
        assert!(model_left, "a right hinge draws the left leaf second");
        assert!(steps.contains(&FoldStep::TurnY {
            degrees: 181.0,
            pivot: [0.0; 3]
        }));
    }

    #[test]
    fn double_doors_stand_on_the_hinge_side_away() {
        // Facing north, hinge left: the other door is to the east (clockwise).
        let d = door(Dir::North, false, true);
        assert_eq!(d.partner([0, 64, 0]), [1, 64, 0]);
        // Its partner (hinge right, to the east) points back west.
        let p = door(Dir::North, true, false);
        assert_eq!(p.partner([1, 64, 0]), [0, 64, 0]);
        assert!(d.is_double(&p), "opened, its closed partner follows");
        assert!(!d.is_double(&door(Dir::North, true, true)), "already open");
        assert!(!d.is_double(&door(Dir::North, false, false)), "same hinge");
        assert!(!d.is_double(&door(Dir::South, true, false)), "other facing");
        let upper = Door { upper: true, ..p };
        assert!(!d.is_double(&upper), "other half");
        assert_eq!(upper.other_half([1, 65, 0]), [1, 64, 0]);
    }

    #[test]
    fn a_closed_door_shows_its_model_once_the_panel_has_slid_shut() {
        // Fully open, then closed: 0.15 a tick takes 7 ticks to reach 0; it is settled (and
        // shown) on the 8th.
        let mut s = Slide::at_rest(true);
        assert!(!s.tick(true, false), "open and still");
        let mut shown = None;
        for t in 1..=12 {
            if s.tick(false, false) {
                shown = Some(t);
                break;
            }
        }
        assert_eq!(shown, Some(8));
        assert_eq!(s.value(), 0.0);
        assert!(s.at_target());
        // Opened then closed before it slid all the way: it shows when back at 0.
        let mut s = Slide::at_rest(false);
        for _ in 0..3 {
            assert!(!s.tick(true, false));
        }
        let mut t = 0;
        while !s.tick(false, false) {
            t += 1;
            assert!(t < 20);
        }
        assert_eq!(t + 1, 4, "0.45 back to 0 in 3 ticks, settled on the 4th");
        // A door that never opened (closed and visible) does nothing.
        let mut s = Slide::at_rest(false);
        assert!(!s.tick(false, true));
        assert!(s.at_target());
    }

    #[test]
    fn a_door_shown_again_is_drawn_two_ticks_more() {
        // Open long enough to fill the bridge, then closed: hidden while it slides shut.
        let mut p = Panel::new(false, 0.0);
        p.follow(true, 10.0);
        assert!(p.special(false, 11.0));
        p.follow(false, 30.0);
        assert!(p.special(false, 35.0), "sliding shut, hidden");
        // Shown again at 40: drawn until 42, then the block draws itself.
        assert!(p.special(true, 40.0));
        assert!(p.special(true, 41.5));
        assert!(!p.special(true, 42.0));
        // Opened for one tick only: one tick of bridge.
        let mut p = Panel::new(false, 0.0);
        p.follow(true, 10.0);
        p.follow(false, 11.0);
        assert!(p.special(true, 20.0));
        assert!(!p.special(true, 21.0));
        // A door first seen closed and shown never draws its panel.
        let mut p = Panel::new(false, 0.0);
        assert!(!p.special(true, 0.0));
    }

    #[test]
    fn a_client_panel_chases_the_door_from_where_it_has_got() {
        let mut p = Panel::new(false, 0.0);
        p.follow(true, 0.0);
        assert!((p.value(2.0) - 0.3).abs() < 1e-6);
        p.follow(false, 2.0);
        assert!((p.value(3.0) - 0.15).abs() < 1e-6);
        assert_eq!(p.value(10.0), 0.0);
        // First seen open: at rest open.
        assert_eq!(Panel::new(true, 5.0).value(6.0), 1.0);
    }
}

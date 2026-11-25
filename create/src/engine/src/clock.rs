//! Create's clockwork bearing (`ClockworkBearingBlockEntity`, `ClockworkContraption`): an hour
//! hand in front of the bearing and a minute hand in front of that, each turning toward the
//! angle the day time gives it, at most its speed a tick.

use crate::contraption::{Found, Refused};
use crate::kinetics::Dir;

/// `ClockworkBearingBlockEntity.ClockHands` (the bearing's scroll option, `HOUR_FIRST` first).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Hands {
    #[default]
    HourFirst,
    MinuteFirst,
    HourFirst24,
}

impl Hands {
    pub fn index(self) -> u8 {
        self as u8
    }

    pub fn from_index(i: u8) -> Hands {
        match i {
            1 => Hands::MinuteFirst,
            2 => Hands::HourFirst24,
            _ => Hands::HourFirst,
        }
    }
}

/// `AngleHelper.getShortestAngleDiff`.
pub fn shortest_angle_diff(current: f64, target: f64) -> f32 {
    let current = current % 360.0;
    let target = target % 360.0;
    ((((target - current) % 360.0) + 540.0) % 360.0 - 180.0) as f32
}

/// The day time a clock reads: `(dayTime × (natural ? 1 : 24)) % 24000`.
fn day(day_time: i64, natural: bool) -> i32 {
    (day_time.wrapping_mul(if natural { 1 } else { 24 }) % 24000) as i32
}

/// `getHourTarget`: `offset` is the bearing's facing's axis direction (+1 or -1).
pub fn hour_target(day_time: i64, natural: bool, positive: bool, cycle24: bool) -> f32 {
    let hours = (day(day_time, natural) / 1000 + 6) % 24;
    let offset = if positive { 1 } else { -1 };
    let (per, modulo) = if cycle24 { (24.0, 24) } else { (12.0, 12) };
    (offset * -360) as f32 / per * (hours % modulo) as f32
}

/// `getMinuteTarget`.
pub fn minute_target(day_time: i64, natural: bool, positive: bool) -> f32 {
    let minutes = (day(day_time, natural) % 1000) * 60 / 1000;
    let offset = if positive { 1 } else { -1 };
    (offset * -360) as f32 / 60.0 * minutes as f32
}

/// A running clock's hands (`hourAngle`, `minuteAngle`) and its mode.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Clock {
    pub hour: f32,
    pub minute: f32,
    pub hands: Hands,
}

/// What a tick reads: the bearing's speed, the level's day time, whether its dimension is
/// natural and whether the bearing faces a positive direction.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sky {
    pub rpm: f32,
    pub day_time: i64,
    pub natural: bool,
    pub positive: bool,
}

/// `getAngularSpeed` (server): `-|speed × 3 / 10|` degrees a tick.
pub fn angular_speed(rpm: f32) -> f32 {
    -(rpm * 3.0 / 10.0).abs()
}

fn toward(angle: f32, target: f32, speed: f32) -> f32 {
    if speed == 0.0 {
        return 0.0;
    }
    let diff = shortest_angle_diff(f64::from(angle), f64::from(target));
    if diff < 0.0 {
        speed.max(diff)
    } else {
        (-speed).min(diff)
    }
}

impl Clock {
    /// `getHourArmSpeed`, `getMinuteArmSpeed`: degrees a tick toward their targets.
    pub fn speeds(&self, sky: &Sky) -> (f32, f32) {
        let speed = angular_speed(sky.rpm);
        let hour = |cycle24| hour_target(sky.day_time, sky.natural, sky.positive, cycle24);
        let minute = || minute_target(sky.day_time, sky.natural, sky.positive);
        let hour_target = match self.hands {
            Hands::HourFirst => hour(false),
            Hands::MinuteFirst => minute(),
            Hands::HourFirst24 => hour(true),
        };
        let minute_target = match self.hands {
            Hands::MinuteFirst => hour(false),
            _ => minute(),
        };
        (
            toward(self.hour, hour_target, speed / 2.0),
            toward(self.minute, minute_target, speed),
        )
    }

    /// One tick of a running clock (`tick`): each hand turns by its speed, `% 360`.
    pub fn tick(&mut self, sky: &Sky) {
        self.tick_holding(sky, (false, false));
    }

    /// [`Clock::tick`] with the hands an actor stalls held where they are
    /// (`hourHand.isStalled()`, `minuteHand.isStalled()`).
    pub fn tick_holding(&mut self, sky: &Sky, held: (bool, bool)) {
        let (h, m) = self.speeds(sky);
        if !held.0 {
            self.hour = (self.hour + h) % 360.0;
        }
        if !held.1 {
            self.minute = (self.minute + m) % 360.0;
        }
    }
}

/// The key the minute hand's storage blocks' items are kept under (`minuteHand` is a
/// contraption of its own, with its own `MountedStorageManager`), apart from the hour hand's
/// (kept under the bearing's position): a step of `contraption::SUB_KEY_STEP` down from the
/// bearing, which is no stabilized structure's key.
pub fn minute_key(pos: [i32; 3]) -> [i32; 3] {
    [
        pos[0],
        pos[1].wrapping_sub(crate::contraption::SUB_KEY_STEP),
        pos[2],
    ]
}

/// A structure's blocks: offsets and states.
pub type Blocks = Vec<([i32; 3], u32)>;

/// A clock's two structures: the hour hand's blocks, and the minute hand's with how many
/// blocks the hour hand reaches out (`offset`); offsets from the bearing.
#[derive(Clone, Debug, PartialEq)]
pub struct Arms {
    pub hour: Blocks,
    pub minute: Option<(i32, Blocks)>,
    /// The mechanical bearings on them that ride stabilized (`pendingSubContraptions`): the
    /// hand (0 the hour's, 1 the minute's), the bearing's world position and its facing.
    pub subs: Vec<(u8, [i32; 3], Dir)>,
}

/// `ClockworkContraption.assembleClockworkAt` for a bearing at `bearing` facing `facing`: the
/// hour hand is what is in front of the bearing; if it reaches `i` blocks straight out (fewer
/// than 16), the minute hand is what is in front of that, less the hour hand's blocks. None
/// when there is nothing in front.
pub fn gather(
    bearing: [i32; 3],
    facing: Dir,
    at: impl Fn([i32; 3]) -> Option<Found>,
) -> Result<Option<Arms>, Refused> {
    use crate::contraption::{clockwork_stabilizes, gather_from_stabilizing};
    let o = facing.offset();
    let along = |n: i32| [0, 1, 2].map(|a| bearing[a] + o[a] * n);
    // Each hand's `canBeStabilized`, from its own anchor.
    let rule = |anchor: [i32; 3]| {
        move |f: Dir, p: [i32; 3]| {
            clockwork_stabilizes(facing, f, [0, 1, 2].map(|a| p[a] - anchor[a]))
        }
    };
    let hour_rule = rule(along(1));
    let found = gather_from_stabilizing(along(1), bearing, |p| p == bearing, &hour_rule, &at)?;
    let hour = found.blocks;
    if hour.is_empty() {
        return Ok(None);
    }
    let mut subs: Vec<(u8, [i32; 3], Dir)> =
        found.subs.into_iter().map(|(p, f)| (0, p, f)).collect();
    // `hourArmBlocks`: the first offset along the facing (from the hour hand's anchor) it
    // doesn't hold, below 16.
    let holds = |i: i32| {
        let l = [0, 1, 2].map(|a| o[a] * (i + 1));
        hour.iter().any(|(p, _)| *p == l)
    };
    let reach = (0..16).find(|&i| !holds(i)).unwrap_or(0);
    let mut minute = None;
    if reach > 0 {
        let skip = |p: [i32; 3]| {
            p == bearing || {
                let l = [0, 1, 2].map(|a| p[a] - bearing[a]);
                hour.iter().any(|(q, _)| *q == l)
            }
        };
        let minute_rule = rule(along(reach + 1));
        let found = gather_from_stabilizing(along(reach + 1), bearing, skip, &minute_rule, &at)?;
        if !found.blocks.is_empty() {
            minute = Some((reach, found.blocks));
            subs.extend(found.subs.into_iter().map(|(p, f)| (1, p, f)));
        }
    }
    Ok(Some(Arms { hour, minute, subs }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_minute_hand_keeps_its_storage_apart_from_the_hour_hand_s() {
        let p = [3, 70, -5];
        let m = minute_key(p);
        assert_ne!(m, p);
        assert_eq!(crate::contraption::sub_of_key(m), None);
        assert!((0..8).all(|i| crate::contraption::sub_key(p, i) != m));
    }

    #[test]
    fn a_hand_an_actor_stalls_stays_while_the_other_turns() {
        let sky = Sky {
            rpm: 32.0,
            day_time: 6000,
            natural: true,
            positive: true,
        };
        let mut c = Clock {
            hour: 123.0,
            minute: 77.0,
            hands: Hands::HourFirst,
        };
        let (h, m) = c.speeds(&sky);
        assert!(h != 0.0 && m != 0.0);
        c.tick_holding(&sky, (true, false));
        assert_eq!(c.hour, 123.0);
        assert_eq!(c.minute, (77.0 + m) % 360.0);
        let minute = c.minute;
        c.tick_holding(&sky, (false, true));
        assert!(c.hour != 123.0);
        assert_eq!(c.minute, minute);
    }

    #[test]
    fn a_bearing_on_a_hand_rides_upright_unless_it_is_the_hand_s_anchor() {
        use crate::contraption::clockwork_stabilizes;
        // A clock facing north: its hour hand anchored at (0, 0, -1).
        assert!(clockwork_stabilizes(Dir::North, Dir::South, [0, 2, 0]));
        assert!(!clockwork_stabilizes(Dir::North, Dir::North, [0, 0, 0]), "the anchor");
        assert!(!clockwork_stabilizes(Dir::North, Dir::South, [0, 0, -1]), "facing the anchor");
        assert!(!clockwork_stabilizes(Dir::North, Dir::Up, [0, 2, 0]), "across");
        // A hand of stone up from (0, 0, -1) with a south-facing bearing on its tip (0, 2, -1):
        // the bearing rides on the hour hand, stabilized.
        let at = |p: [i32; 3]| match p {
            [0, 0..=1, -1] => Some(Found {
                state: 1,
                movable: true,
                glued: Dir::Up.bit(),
                ..Default::default()
            }),
            [0, 2, -1] => Some(Found {
                state: 2,
                movable: true,
                bearing: Some(Dir::South),
                ..Default::default()
            }),
            _ => None,
        };
        let arms = gather([0, 0, 0], Dir::North, at).unwrap().unwrap();
        assert_eq!(arms.subs, vec![(0, [0, 2, -1], Dir::South)]);
        assert_eq!(arms.hour.len(), 3);
    }

    fn sky(day_time: i64) -> Sky {
        Sky {
            rpm: 64.0,
            day_time,
            natural: true,
            positive: false,
        }
    }

    #[test]
    fn hands_point_at_the_day_time() {
        // Day time 0 is 6 o'clock: a north-facing clock's hour hand at +180°, minutes at 0.
        assert_eq!(hour_target(0, true, false, false), 180.0);
        assert_eq!(minute_target(0, true, false), 0.0);
        // 6000 is noon: the hour hand straight up (0 of 12 hours), 24-hour mode at 180°.
        assert_eq!(hour_target(6000, true, false, false), 0.0);
        assert_eq!(hour_target(6000, true, false, true), 180.0);
        // 1500: 7:30, so 210° and 180° (30 minutes); facing south turns the other way.
        assert_eq!(hour_target(1500, true, false, false), 210.0);
        assert_eq!(minute_target(1500, true, false), 180.0);
        assert_eq!(minute_target(1500, true, true), -180.0);
        assert_eq!(hour_target(1500, true, true, false), -210.0);
        // 18250 (00:15): minutes 15 → 90°; hours 0.
        assert_eq!(minute_target(18_250, true, false), 90.0);
        assert_eq!(hour_target(18_250, true, false, false), 0.0);
        // An unnatural dimension's clock runs 24 times as fast.
        assert_eq!(
            minute_target(10, false, false),
            minute_target(240, true, false)
        );
        // A whole day later it reads the same.
        assert_eq!(hour_target(25_500, true, false, false), 210.0);
        assert_eq!(shortest_angle_diff(350.0, 10.0), 20.0);
        assert_eq!(shortest_angle_diff(10.0, 350.0), -20.0);
        assert_eq!(shortest_angle_diff(0.0, 210.0), -150.0);
    }

    #[test]
    fn hands_turn_toward_their_targets_and_stop_there() {
        let same = |a: f32, b: f32| shortest_angle_diff(f64::from(a), f64::from(b)).abs() < 1e-3;
        // 64 rpm: 19.2° a tick for the minute hand, 9.6° for the hour hand.
        let mut c = Clock::default();
        let s = sky(1500);
        let (h, m) = c.speeds(&s);
        // 210° is shorter the other way round (-150°); 180° away goes the negative way.
        assert!((h + 9.6).abs() < 1e-4 && (m + 19.2).abs() < 1e-4, "{h} {m}");
        for _ in 0..40 {
            c.tick(&s);
        }
        // There after enough ticks, and still.
        assert!(same(c.minute, 180.0), "{}", c.minute);
        assert!(same(c.hour, 210.0), "{}", c.hour);
        let before = c;
        c.tick(&s);
        assert_eq!(c, before);
        // A minute later (16.7 ticks), the minute hand moves on by 6°.
        let later = sky(1517);
        c.tick(&later);
        assert!(same(c.minute, 186.0), "{}", c.minute);
        // Still: nothing moves.
        let mut still = Clock::default();
        still.tick(&Sky { rpm: 0.0, ..s });
        assert_eq!(still, Clock::default());
        // Minute first: the hour hand shows minutes (180°), the minute hand hours (210°).
        let mut mf = Clock {
            hands: Hands::MinuteFirst,
            ..Clock::default()
        };
        for _ in 0..40 {
            mf.tick(&s);
        }
        assert!(same(mf.hour, 180.0) && same(mf.minute, 210.0), "{mf:?}");
    }

    #[test]
    fn the_minute_hand_is_what_lies_beyond_the_hour_hand() {
        use crate::contraption::Sticky;
        // A bearing at the origin facing north: slime at z -1 and stone stuck to it at z -2
        // (the hour hand, reaching two blocks out), then a stone at z -3 (the minute hand).
        let at = |p: [i32; 3]| {
            let f = |state, sticky| {
                Some(Found {
                    state,
                    sail_facing: None,
                    sticky,
                    movable: true,
                    chassis: None,
                    ..Default::default()
                })
            };
            match p {
                [0, 0, -1] => f(1, Some(Sticky::Slime)),
                [0, 0, -2] => f(2, None),
                [0, 0, -3] => f(3, None),
                _ => None,
            }
        };
        let hands = gather([0, 0, 0], Dir::North, at).unwrap().unwrap();
        assert_eq!(hands.hour, vec![([0, 0, -1], 1), ([0, 0, -2], 2)]);
        assert_eq!(hands.minute, Some((2, vec![([0, 0, -3], 3)])));
        // Nothing in front: no clock.
        assert_eq!(gather([0, 0, 0], Dir::North, |_| None), Ok(None));
    }
}

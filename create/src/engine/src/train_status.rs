//! What a train tells its owner (`TrainStatus`): a missing conductor, no way to its next
//! station, the end of the track, a crash and the like, each said once until it is resolved
//! (`navigation`, `track`, `conductor`), queued (three at most) and told in the owner's chat
//! under the train's name when they are on (`tick`); and who may drive it where
//! (`hasForwardConductor`, `hasBackwardConductor`, `CarriageContraption.inControl`).

/// An argument of a status line: text, or the id of the level the train is in (the glue's).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Arg {
    Text(String),
    Level,
}

/// A status line (`displayInformation`): ` - ` in gray, then `create.train.status.<key>` in
/// green for good news (`0xD5ECC2`), else in rose (`0xFFD3B4`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Line {
    pub key: &'static str,
    pub args: Vec<Arg>,
    pub good: bool,
}

impl Line {
    /// The line's colour.
    pub fn color(&self) -> u32 {
        if self.good { GOOD } else { BAD }
    }

    /// Its translation key.
    pub fn translation(&self) -> String {
        format!("create.train.status.{}", self.key)
    }
}

/// `displayInformation`'s colours.
pub const GOOD: u32 = 0xD5_EC_C2;
pub const BAD: u32 = 0xFF_D3_B4;
/// `TrainStatus.addMessage`: messages kept at most while the owner is away.
pub const QUEUED_MAX: usize = 3;
/// The header the queue is told under (`train.status`, gold, with the train's name).
pub const HEADER: &str = "create.train.status";

/// `StatusMessage`: the lines told together.
pub type Message = Vec<Line>;

/// `TrainStatus`: whether a navigation, track or conductor trouble has been told (and not yet
/// resolved), and the messages waiting for the owner.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Status {
    pub navigation: bool,
    pub track: bool,
    pub conductor: bool,
    pub queued: Vec<Message>,
}

impl Status {
    /// `addMessage`: the oldest goes past three.
    pub fn add(&mut self, message: Message) {
        self.queued.push(message);
        if self.queued.len() > QUEUED_MAX {
            self.queued.remove(0);
        }
    }

    fn display(&mut self, key: &'static str, good: bool, args: Vec<Arg>) {
        self.add(vec![Line { key, args, good }]);
    }

    /// `failedNavigation`: no way to any station the instruction names.
    pub fn failed_navigation(&mut self) {
        if self.navigation {
            return;
        }
        self.display("no_path", false, Vec::new());
        self.navigation = true;
    }

    /// `failedNavigationNoTarget`: no station on its track is named as the filter asks.
    pub fn failed_navigation_no_target(&mut self, filter: &str) {
        if self.navigation {
            return;
        }
        self.display("no_match", false, vec![Arg::Text(filter.to_owned())]);
        self.navigation = true;
    }

    /// `failedPackageNoTarget`: no postbox takes the package addressed to `address`.
    pub fn failed_package_no_target(&mut self, address: &str) {
        if self.navigation {
            return;
        }
        self.display(
            "no_package_target",
            false,
            vec![Arg::Text(address.to_owned())],
        );
        self.navigation = true;
    }

    /// `successfulNavigation`.
    pub fn successful_navigation(&mut self) {
        if !self.navigation {
            return;
        }
        self.display("navigation_success", true, Vec::new());
        self.navigation = false;
    }

    /// `foundConductor`.
    pub fn found_conductor(&mut self) {
        if !self.conductor {
            return;
        }
        self.display("found_driver", true, Vec::new());
        self.conductor = false;
    }

    /// `missingConductor`.
    pub fn missing_conductor(&mut self) {
        if self.conductor {
            return;
        }
        self.display("missing_driver", false, Vec::new());
        self.conductor = true;
    }

    /// `missingCorrectConductor`: a conductor faces the other way only.
    pub fn missing_correct_conductor(&mut self) {
        if self.conductor {
            return;
        }
        self.display("opposite_driver", false, Vec::new());
        self.conductor = true;
    }

    /// `manualControls`: a player took the controls of a train running its schedule.
    pub fn manual_controls(&mut self) {
        self.display("paused_for_manual", true, Vec::new());
    }

    /// `failedMigration`: its track went and none is found under it.
    pub fn failed_migration(&mut self) {
        if self.track {
            return;
        }
        self.display("track_missing", false, Vec::new());
        self.track = true;
    }

    /// `highStress`: its couplings stretched too far (derailed).
    pub fn high_stress(&mut self) {
        if self.track {
            return;
        }
        self.display("coupling_stress", false, Vec::new());
        self.track = true;
    }

    /// `endOfTrack`.
    pub fn end_of_track(&mut self) {
        if self.track {
            return;
        }
        self.display("end_of_track", false, Vec::new());
        self.track = true;
    }

    /// `crash`: told every time, with where its first carriage is in the level it is in
    /// (`collision.where`: the level's id and `BlockPos.toShortString`).
    pub fn crash(&mut self, at: Option<[i32; 3]>) {
        let mut lines = vec![Line {
            key: "collision",
            args: Vec::new(),
            good: false,
        }];
        if let Some(p) = at {
            lines.push(Line {
                key: "collision.where",
                args: vec![Arg::Level, Arg::Text(short_string(p))],
                good: false,
            });
        }
        self.add(lines);
    }

    /// `successfulMigration`: on the track again (found again, or put back by the wrench).
    pub fn successful_migration(&mut self) {
        if !self.track {
            return;
        }
        self.display("back_on_track", true, Vec::new());
        self.track = false;
    }

    /// `trackOK`: moving freely, a track trouble may be told again.
    pub fn track_ok(&mut self) {
        self.track = false;
    }

    /// `newSchedule`.
    pub fn new_schedule(&mut self) {
        self.navigation = false;
        self.conductor = false;
    }

    /// `tick` with the owner on: what to tell them (the header first), the queue emptied.
    pub fn take(&mut self) -> Vec<Message> {
        std::mem::take(&mut self.queued)
    }
}

/// `BlockPos.toShortString`.
pub fn short_string(p: [i32; 3]) -> String {
    format!("{}, {}, {}", p[0], p[1], p[2])
}

/// Which ways a conductor at the carriage block `at` drives (`CarriageContraption.inControl`
/// in both directions along the train): train controls right beside it toward the train's
/// front (`heading`, a unit along x or z as assembled) or back, facing it (their `facing` the
/// way back to it). `[forward, backward]`; `controls(pos)` is the facing (a unit) of the train
/// controls at `pos` of the carriage, None for any other block.
pub fn in_control(
    at: [i32; 3],
    heading: [i32; 2],
    controls: impl Fn([i32; 3]) -> Option<[i32; 2]>,
) -> [bool; 2] {
    let mut sides = [false; 2];
    for (k, s) in [1, -1].into_iter().enumerate() {
        let d = [heading[0] * s, heading[1] * s];
        let beside = [at[0] + d[0], at[1], at[2] + d[1]];
        if controls(beside) == Some([-d[0], -d[1]]) {
            sides[k] = true;
        }
    }
    sides
}

/// `Carriage.presentConductors` gathered (`hasForwardConductor`, `hasBackwardConductor`).
pub fn any_side(carriages: &[[bool; 2]]) -> [bool; 2] {
    carriages
        .iter()
        .fold([false; 2], |a, c| [a[0] || c[0], a[1] || c[1]])
}

/// `Navigation.findPathTo`'s ways it may go: forward with a conductor facing its front, back
/// (when it has controls both ways, `doubleEnded`) with one facing its back; either way while
/// its schedule is paused (a player drives it, or it has none).
pub fn may_drive(conductors: [bool; 2], double_ended: bool, paused: bool) -> [bool; 2] {
    [
        conductors[0] || paused,
        (double_ended && conductors[1]) || paused,
    ]
}

/// `Navigation.tick`'s and `startNavigation`'s check for a scheduled train going to its
/// station (backwards when `behind`): fine, or the trouble to tell.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Crew {
    Fine,
    /// `missingConductor`.
    Missing,
    /// `missingCorrectConductor`: one faces the other way.
    Opposite,
}

/// `Navigation.tick`'s check: backing to its station needs a conductor facing back, else one
/// facing front (told as missing outright, as Create does going forward).
pub fn crew_for(conductors: [bool; 2], behind: bool) -> Crew {
    let [front, back] = conductors;
    match (behind, front, back) {
        (true, _, true) | (false, true, _) => Crew::Fine,
        (true, true, false) => Crew::Opposite,
        _ => Crew::Missing,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(s: &Status) -> Vec<Vec<&'static str>> {
        s.queued
            .iter()
            .map(|m| m.iter().map(|l| l.key).collect())
            .collect()
    }

    #[test]
    fn each_trouble_is_told_once_until_resolved() {
        let mut s = Status::default();
        s.missing_conductor();
        s.missing_conductor();
        s.missing_correct_conductor();
        s.failed_navigation();
        s.failed_navigation_no_target("Mine*");
        s.end_of_track();
        s.high_stress();
        assert_eq!(
            keys(&s),
            vec![
                vec!["missing_driver"],
                vec!["no_path"],
                vec!["end_of_track"]
            ]
        );
        let told = s.take();
        assert_eq!(told.len(), 3);
        assert!(s.queued.is_empty());
        // Resolved: told as good news, and the trouble may be told again.
        s.found_conductor();
        s.successful_navigation();
        s.successful_migration();
        assert_eq!(
            keys(&s),
            vec![
                vec!["found_driver"],
                vec!["navigation_success"],
                vec!["back_on_track"]
            ]
        );
        assert!(
            s.queued
                .iter()
                .flatten()
                .all(|l| l.good && l.color() == GOOD)
        );
        s.found_conductor();
        s.successful_navigation();
        assert_eq!(s.queued.len(), 3, "nothing to resolve");
        s.take();
        s.failed_navigation_no_target("Mine*");
        assert_eq!(s.queued[0][0].args, vec![Arg::Text("Mine*".into())]);
        assert_eq!(s.queued[0][0].translation(), "create.train.status.no_match");
        assert_eq!(s.queued[0][0].color(), BAD);
    }

    #[test]
    fn the_queue_keeps_the_last_three() {
        let mut s = Status::default();
        for _ in 0..5 {
            s.manual_controls();
        }
        s.crash(Some([3, 64, -7]));
        assert_eq!(s.queued.len(), QUEUED_MAX);
        let last = s.queued.last().unwrap();
        assert_eq!(last[0].key, "collision");
        assert_eq!(
            last[1].args,
            vec![Arg::Level, Arg::Text("3, 64, -7".into())]
        );
        // A crash is told every time.
        s.crash(None);
        assert_eq!(s.queued.last().unwrap().len(), 1);
    }

    #[test]
    fn moving_again_or_a_new_schedule_clears_the_flags() {
        let mut s = Status::default();
        s.end_of_track();
        s.track_ok();
        s.end_of_track();
        assert_eq!(s.queued.len(), 2);
        s.failed_navigation();
        s.missing_conductor();
        s.new_schedule();
        assert!(!s.navigation && !s.conductor);
        s.successful_navigation();
        s.found_conductor();
        assert_eq!(s.queued.len(), 3, "no good news without trouble");
    }

    #[test]
    fn a_seat_is_in_control_of_controls_facing_it() {
        // A train heading +x: controls one block ahead facing back (-x) put the seat in
        // forward control; behind it facing +x, backward control.
        let at = [2, 1, 0];
        let ahead = |p: [i32; 3]| (p == [3, 1, 0]).then_some([-1, 0]);
        assert_eq!(in_control(at, [1, 0], ahead), [true, false]);
        let behind = |p: [i32; 3]| (p == [1, 1, 0]).then_some([1, 0]);
        assert_eq!(in_control(at, [1, 0], behind), [false, true]);
        // Facing away from the seat, sideways or not beside it: not in control.
        let away = |p: [i32; 3]| (p == [3, 1, 0]).then_some([1, 0]);
        assert_eq!(in_control(at, [1, 0], away), [false, false]);
        let side = |p: [i32; 3]| (p == [2, 1, 1]).then_some([0, -1]);
        assert_eq!(in_control(at, [1, 0], side), [false, false]);
        // Along z too.
        let z = |p: [i32; 3]| (p == [2, 1, -1]).then_some([0, 1]);
        assert_eq!(in_control(at, [0, -1], z), [true, false]);
    }

    #[test]
    fn who_may_drive_where() {
        assert_eq!(any_side(&[[false, false], [false, true]]), [false, true]);
        assert_eq!(may_drive([true, true], false, false), [true, false]);
        assert_eq!(may_drive([false, true], true, false), [false, true]);
        assert_eq!(may_drive([false, false], false, true), [true, true]);
        assert_eq!(crew_for([true, false], false), Crew::Fine);
        assert_eq!(crew_for([true, false], true), Crew::Opposite);
        assert_eq!(crew_for([false, true], false), Crew::Missing);
        assert_eq!(crew_for([false, true], true), Crew::Fine);
        assert_eq!(crew_for([false, false], true), Crew::Missing);
    }
}

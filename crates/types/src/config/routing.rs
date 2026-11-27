use super::{Config, PickModel, ReviewKind, RoutingMode, SEAT_ROWS, SeatPhase, Standing};

impl Config {
    pub fn routing_mode(&self, seat: &str, at: Standing) -> RoutingMode {
        self.seats.get(seat).and_then(|s| s.at(at).and_then(|p| p.routing).or(s.routing)).unwrap_or_default()
    }

    pub fn router_list(&self, seat: &str, at: Standing) -> (&'static str, &[PickModel]) {
        let picks = self.picks.as_ref();
        let (name, list) = match (seat, at.phase, at.review) {
            ("planner", ..) => ("picks.planner", picks.map(|p| &p.planner)),
            ("foreman", SeatPhase::Report, _) => ("picks.foreman.report", picks.map(|p| &p.foreman.report)),
            ("foreman", ..) => ("picks.foreman.work", picks.map(|p| &p.foreman.work)),
            ("review-foreman", ..) => ("picks.review-foreman", picks.map(|p| &p.review_foreman)),
            ("researcher", ..) => ("picks.researcher", picks.map(|p| &p.researcher)),
            ("baseline", ..) => ("picks.baseline", picks.map(|p| &p.baseline)),
            ("reviewer", _, Some(ReviewKind::Inside)) => ("picks.reviewer.inside", picks.map(|p| &p.reviewer.inside)),
            ("reviewer", _, Some(ReviewKind::Outside)) => {
                ("picks.reviewer.outside", picks.map(|p| &p.reviewer.outside))
            }
            _ => return ("", &[]),
        };
        (name, list.map(Vec::as_slice).unwrap_or_default())
    }

    pub fn routing_enabled(&self) -> bool {
        SEAT_ROWS.iter().any(|&(name, at)| self.routing_mode(name, at) != RoutingMode::Fixed)
            || self.projects.values().flat_map(|p| p.passes.values()).any(|p| p.mode == crate::lookback::Trigger::Auto)
    }
}

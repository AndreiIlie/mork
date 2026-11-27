use super::*;

// Two consecutive completed rounds under the current plan stall when none of the prior round's
// findings were resolved and every one repeats unresolved in the new round (fac-694).
pub(super) fn stalemate_prev(rc: &Card, pending: &PendingTriage) -> Option<(u32, BTreeSet<u32>)> {
    let review = rc.review.as_deref()?;
    let prev = review
        .current()
        .rev()
        .find(|r| r.n < pending.round && matches!(r.verdict, Some(Verdict::Passed | Verdict::Failed)))?;
    let prev_set: BTreeSet<u32> = prev.named.iter().copied().collect();
    if prev.verdict != Some(Verdict::Failed) || prev_set.is_empty() {
        return None;
    }
    let resolved = resolved_of(rc);
    if prev_set.iter().any(|n| resolved.contains(n)) {
        return None;
    }
    let curr_open: BTreeSet<u32> = pending.items.iter().map(|i| i.n).filter(|n| !resolved.contains(n)).collect();
    (!curr_open.is_empty() && prev_set.is_subset(&curr_open)).then_some((prev.n, prev_set))
}

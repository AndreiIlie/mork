/// Refuses a Claude subagent call that names Fable or can inherit a Fable seat's model (fac-403).
pub fn refusal(model: Option<&str>, subagent_type: Option<&str>, seat_fable: bool) -> Option<&'static str> {
    let fable = model.is_some_and(|m| m.to_ascii_lowercase().contains("fable"));
    let inherits = model.is_none() || subagent_type == Some("fork");
    (fable || (seat_fable && inherits))
        .then_some("Subagents must not run on Fable. Name another model explicitly; on a Fable seat, do not fork.")
}

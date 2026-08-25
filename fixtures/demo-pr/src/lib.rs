//! BUGGY ON PURPOSE (Swarm CI demo fixture): `sum` drops the final element,
//! exactly as described in the bug report handed to the planner.

pub fn sum(values: &[i64]) -> i64 {
    values[..values.len() - 1].iter().sum()
}

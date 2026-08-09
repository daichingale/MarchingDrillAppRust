//! Deterministic projection solver for formation-design suggestions.
//! It never mutates a [`Document`](crate::Document); callers explicitly apply its Edit.
use crate::{Document, DrillError, Edit, Point, SetId};

#[derive(Clone, Debug, PartialEq)]
pub enum Constraint {
    Fixed { performer: usize, position: Point },
    InsideField { min: Point, max: Point },
    MinimumDistance { distance: f32 },
    MaximumStep { origins: Vec<Point>, distance: f32 },
    ShapeFollow { targets: Vec<Point>, strength: f32 },
}
#[derive(Clone, Copy, Debug)]
pub struct SolverLimits {
    pub max_performers: usize,
    pub max_constraints: usize,
    pub max_iterations: u16,
    pub tolerance: f32,
}
impl Default for SolverLimits {
    fn default() -> Self {
        Self {
            max_performers: 4_000,
            max_constraints: 64,
            max_iterations: 80,
            tolerance: 0.001,
        }
    }
}
#[derive(Clone, Debug, PartialEq)]
pub enum SolverError {
    Limit(&'static str),
    Invalid(&'static str),
    Cancelled,
}
impl std::fmt::Display for SolverError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Limit(v) => write!(f, "constraint solver limit exceeded: {v}"),
            Self::Invalid(v) => write!(f, "invalid constraint: {v}"),
            Self::Cancelled => f.write_str("constraint solve cancelled"),
        }
    }
}
impl std::error::Error for SolverError {}
#[derive(Clone, Debug)]
pub struct SolverProposal {
    pub positions: Vec<Point>,
    pub iterations: u16,
    pub max_correction: f32,
    pub converged: bool,
}
impl SolverProposal {
    pub fn as_edit(&self, doc: &Document, set_id: SetId) -> Result<Edit, DrillError> {
        let set = doc
            .sets
            .iter()
            .find(|s| s.id == set_id)
            .ok_or(DrillError::MissingSet)?;
        if set.positions.len() != self.positions.len() {
            return Err(DrillError::InvalidEdit);
        }
        Ok(Edit::MovePerformers {
            set_id,
            performer_ids: doc.performers.iter().map(|p| p.id).collect(),
            positions: self.positions.clone(),
        })
    }
}

pub fn solve_constraints(
    initial: &[Point],
    constraints: &[Constraint],
    limits: SolverLimits,
    mut keep_going: impl FnMut(f32) -> bool,
) -> Result<SolverProposal, SolverError> {
    if initial.len() > limits.max_performers {
        return Err(SolverError::Limit("performers"));
    }
    if constraints.len() > limits.max_constraints {
        return Err(SolverError::Limit("constraints"));
    }
    if limits.max_iterations == 0 || !limits.tolerance.is_finite() || limits.tolerance < 0.0 {
        return Err(SolverError::Invalid("limits"));
    }
    if initial.iter().any(|p| !finite(*p)) {
        return Err(SolverError::Invalid("initial positions"));
    }
    let n = initial.len();
    let mut fixed = vec![None; n];
    validate(constraints, n, &mut fixed)?;
    let mut out = initial.to_vec();
    for (i, p) in fixed.iter().enumerate() {
        if let Some(p) = p {
            out[i] = *p
        }
    }
    let mut final_correction = 0.0;
    let mut completed = 0;
    for iteration in 0..limits.max_iterations {
        if !keep_going(f32::from(iteration) / f32::from(limits.max_iterations)) {
            return Err(SolverError::Cancelled);
        }
        let before = out.clone();
        for c in constraints {
            match c {
                Constraint::Fixed {
                    performer,
                    position,
                } => out[*performer] = *position,
                Constraint::InsideField { min, max } => {
                    for (i, p) in out.iter_mut().enumerate() {
                        if fixed[i].is_none() {
                            p.x = p.x.clamp(min.x, max.x);
                            p.y = p.y.clamp(min.y, max.y)
                        }
                    }
                }
                Constraint::MaximumStep { origins, distance } => {
                    for (i, (p, o)) in out.iter_mut().zip(origins).enumerate() {
                        if fixed[i].is_none() {
                            let dx = p.x - o.x;
                            let dy = p.y - o.y;
                            let d = (dx * dx + dy * dy).sqrt();
                            if d > *distance && d > 0.0 {
                                let s = *distance / d;
                                p.x = o.x + dx * s;
                                p.y = o.y + dy * s
                            }
                        }
                    }
                }
                Constraint::ShapeFollow { targets, strength } => {
                    for (i, (p, t)) in out.iter_mut().zip(targets).enumerate() {
                        if fixed[i].is_none() {
                            p.x += (t.x - p.x) * strength;
                            p.y += (t.y - p.y) * strength
                        }
                    }
                }
                Constraint::MinimumDistance { distance } => {
                    project_separation(&mut out, &fixed, *distance)
                }
            }
        }
        // Separation is the final movable projection so later soft shape or
        // step projections cannot silently reintroduce a collision.
        for c in constraints {
            if let Constraint::MinimumDistance { distance } = c {
                project_separation(&mut out, &fixed, *distance);
            }
        }
        for (i, p) in fixed.iter().enumerate() {
            if let Some(p) = p {
                out[i] = *p
            }
        }
        final_correction = out
            .iter()
            .zip(&before)
            .map(|(a, b)| distance(*a, *b))
            .fold(0.0, f32::max);
        completed = iteration + 1;
        if final_correction <= limits.tolerance {
            break;
        }
    }
    keep_going(1.0);
    Ok(SolverProposal {
        positions: out,
        iterations: completed,
        max_correction: final_correction,
        converged: final_correction <= limits.tolerance,
    })
}
fn validate(cs: &[Constraint], n: usize, fixed: &mut [Option<Point>]) -> Result<(), SolverError> {
    for c in cs {
        match c {
            Constraint::Fixed {
                performer,
                position,
            } => {
                if *performer >= n || !finite(*position) {
                    return Err(SolverError::Invalid("fixed point"));
                }
                fixed[*performer] = Some(*position)
            }
            Constraint::InsideField { min, max } => {
                if !finite(*min) || !finite(*max) || min.x > max.x || min.y > max.y {
                    return Err(SolverError::Invalid("field bounds"));
                }
            }
            Constraint::MinimumDistance { distance } => {
                if !distance.is_finite() || *distance < 0.0 {
                    return Err(SolverError::Invalid("distance"));
                }
            }
            Constraint::MaximumStep { origins, distance } => {
                if !distance.is_finite()
                    || *distance < 0.0
                    || origins.len() != n
                    || origins.iter().any(|p| !finite(*p))
                {
                    return Err(SolverError::Invalid("step origins"));
                }
            }
            Constraint::ShapeFollow { targets, strength } => {
                if targets.len() != n
                    || targets.iter().any(|p| !finite(*p))
                    || !strength.is_finite()
                    || !(0.0..=1.0).contains(strength)
                {
                    return Err(SolverError::Invalid("shape targets"));
                }
            }
        }
    }
    Ok(())
}
fn project_separation(p: &mut [Point], fixed: &[Option<Point>], minimum: f32) {
    if minimum <= 0.0 {
        return;
    }
    for a in 0..p.len() {
        for b in a + 1..p.len() {
            let dx = p[b].x - p[a].x;
            let dy = p[b].y - p[a].y;
            let d2 = dx * dx + dy * dy;
            if d2 >= minimum * minimum {
                continue;
            }
            let d = d2.sqrt();
            let (nx, ny) = if d > 1e-8 {
                (dx / d, dy / d)
            } else {
                let angle =
                    ((a.wrapping_mul(73856093) ^ b.wrapping_mul(19349663)) % 6283) as f32 / 1000.0;
                (angle.cos(), angle.sin())
            };
            let gap = minimum - d;
            match (fixed[a].is_some(), fixed[b].is_some()) {
                (true, true) => {}
                (true, false) => {
                    p[b].x += nx * gap;
                    p[b].y += ny * gap
                }
                (false, true) => {
                    p[a].x -= nx * gap;
                    p[a].y -= ny * gap
                }
                (false, false) => {
                    p[a].x -= nx * gap * 0.5;
                    p[a].y -= ny * gap * 0.5;
                    p[b].x += nx * gap * 0.5;
                    p[b].y += ny * gap * 0.5
                }
            }
        }
    }
}
fn finite(p: Point) -> bool {
    p.x.is_finite() && p.y.is_finite()
}
fn distance(a: Point, b: Point) -> f32 {
    ((a.x - b.x).powi(2) + (a.y - b.y).powi(2)).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;
    fn p(x: f32, y: f32) -> Point {
        Point { x, y }
    }
    #[test]
    fn satisfies_fixed_bounds_spacing_step_and_shape() {
        let initial = vec![p(-5., 0.), p(0., 0.), p(0., 0.)];
        let cs = vec![
            Constraint::Fixed {
                performer: 0,
                position: p(1., 1.),
            },
            Constraint::ShapeFollow {
                targets: vec![p(1., 1.), p(5., 5.), p(8., 5.)],
                strength: 0.4,
            },
            Constraint::MinimumDistance { distance: 2. },
            Constraint::InsideField {
                min: p(0., 0.),
                max: p(10., 10.),
            },
            Constraint::MaximumStep {
                origins: vec![p(1., 1.), p(0., 0.), p(0., 0.)],
                distance: 8.,
            },
        ];
        let r = solve_constraints(&initial, &cs, Default::default(), |_| true).unwrap();
        assert_eq!(r.positions[0], p(1., 1.));
        assert!(
            r.positions
                .iter()
                .all(|v| v.x >= 0. && v.x <= 10. && v.y >= 0. && v.y <= 10.)
        );
        assert!(distance(r.positions[1], r.positions[2]) > 1.99);
    }
    #[test]
    fn deterministic_and_cancellable() {
        let input = vec![p(0., 0.); 20];
        let c = [Constraint::MinimumDistance { distance: 1. }];
        let a = solve_constraints(&input, &c, Default::default(), |_| true).unwrap();
        let b = solve_constraints(&input, &c, Default::default(), |_| true).unwrap();
        assert_eq!(a.positions, b.positions);
        assert_eq!(
            solve_constraints(&input, &c, Default::default(), |_| false).unwrap_err(),
            SolverError::Cancelled
        );
    }
    #[test]
    fn hostile_inputs_rejected() {
        let l = SolverLimits {
            max_performers: 1,
            ..Default::default()
        };
        assert!(matches!(
            solve_constraints(&[p(0., 0.), p(1., 1.)], &[], l, |_| true),
            Err(SolverError::Limit(_))
        ));
        assert!(solve_constraints(&[p(f32::NAN, 0.)], &[], Default::default(), |_| true).is_err());
    }
    #[test]
    fn thousand_people_is_bounded() {
        let input = (0..1000)
            .map(|i| p((i % 40) as f32, (i / 40) as f32))
            .collect::<Vec<_>>();
        let c = [Constraint::MinimumDistance { distance: 0.8 }];
        let start = std::time::Instant::now();
        let r = solve_constraints(
            &input,
            &c,
            SolverLimits {
                max_iterations: 4,
                ..Default::default()
            },
            |_| true,
        )
        .unwrap();
        assert_eq!(r.positions.len(), 1000);
        assert!(start.elapsed() < std::time::Duration::from_secs(2));
    }
}

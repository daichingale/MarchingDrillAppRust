//! Pure 3D perspective projection for rendering a stadium / "3D" view of a
//! formation with nothing more than egui's 2D painter.
//!
//! # Coordinate convention
//!
//! World space is **right-handed**:
//!
//! * `+X` runs across the field and equals a field [`crate::Point::x`].
//! * `+Z` runs along field depth and equals a field [`crate::Point::y`].
//! * `+Y` points up, i.e. height above the turf.
//!
//! So a field `Point { x, y }` sitting at turf height `h` maps to the world
//! vector `[x, h, y]` (see [`field_to_world`]).
//!
//! The view matrix follows the OpenGL `gluLookAt` convention: the camera looks
//! down its local `-Z` axis, so points in front of the camera have positive
//! clip-space `w`. The perspective matrix is the standard OpenGL projection.
//! Matrices are stored **row-major** as `[[f32; 4]; 4]` and applied as
//! `matrix * column_vector`.

use serde::{Deserialize, Serialize};

/// Map a field point at turf height `height` into right-handed world space.
///
/// `Point { x, y }` -> `[x, height, y]`.
pub fn field_to_world(point: crate::Point, height: f32) -> [f32; 3] {
    [point.x, height, point.y]
}

/// An orbit camera: it looks at `target` from `distance` away, with the eye
/// direction set by `yaw` (rotation about the world `+Y` axis) and `pitch`
/// (elevation above the horizontal plane), both in radians.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Camera {
    /// World-space point the camera looks at.
    pub target: [f32; 3],
    /// Rotation about the world `+Y` axis, in radians.
    pub yaw: f32,
    /// Elevation of the eye above the horizontal plane, in radians.
    pub pitch: f32,
    /// Distance from `target` to the eye.
    pub distance: f32,
    /// Vertical field of view, in radians.
    pub fov_y_rad: f32,
    /// Near clip plane (world units, must be > 0).
    pub near: f32,
    /// Far clip plane (world units).
    pub far: f32,
}

impl Default for Camera {
    fn default() -> Self {
        Self {
            target: [0.0, 0.0, 0.0],
            yaw: 0.0,
            pitch: 0.3,
            distance: 30.0,
            fov_y_rad: std::f32::consts::FRAC_PI_3,
            near: 0.1,
            far: 2000.0,
        }
    }
}

impl Camera {
    /// The world-space eye position, orbiting `target` at `distance`.
    ///
    /// At `yaw = 0`, `pitch = 0` the eye sits at `target + [0, 0, distance]`,
    /// i.e. on the `+Z` side looking down `-Z`. The returned point is always
    /// exactly `distance` away from `target`.
    pub fn position(&self) -> [f32; 3] {
        let (sy, cy) = self.yaw.sin_cos();
        let (sp, cp) = self.pitch.sin_cos();
        // Unit offset from target to eye (length 1 for any yaw/pitch).
        let offset = [cp * sy, sp, cp * cy];
        [
            self.target[0] + offset[0] * self.distance,
            self.target[1] + offset[1] * self.distance,
            self.target[2] + offset[2] * self.distance,
        ]
    }

    /// Project a world point to screen pixels (origin top-left, `y` down).
    ///
    /// Returns `None` when the point is behind the camera (clip-space `w <= 0`)
    /// or falls outside the `near..far` clip range.
    pub fn project(&self, world: [f32; 3], viewport_w: f32, viewport_h: f32) -> Option<[f32; 2]> {
        let aspect = if viewport_h > 0.0 {
            viewport_w / viewport_h
        } else {
            1.0
        };
        let view = self.view_matrix();
        let proj = self.perspective_matrix(aspect);
        let clip = mat4_mul_vec4(
            &proj,
            &mat4_mul_vec4(&view, &[world[0], world[1], world[2], 1.0]),
        );

        let w = clip[3];
        if w <= f32::EPSILON {
            // Behind the camera (or on the eye plane).
            return None;
        }
        let ndc = [clip[0] / w, clip[1] / w, clip[2] / w];
        if ndc[2] < -1.0 || ndc[2] > 1.0 {
            // Nearer than `near` or farther than `far`.
            return None;
        }
        let sx = (ndc[0] * 0.5 + 0.5) * viewport_w;
        let sy = (0.5 - ndc[1] * 0.5) * viewport_h; // flip y: NDC up -> screen down
        Some([sx, sy])
    }

    /// Convenience wrapper: project a field point at turf height `height`.
    pub fn project_point(
        &self,
        point: crate::Point,
        height: f32,
        vw: f32,
        vh: f32,
    ) -> Option<[f32; 2]> {
        self.project(field_to_world(point, height), vw, vh)
    }

    /// Row-major `gluLookAt` view matrix (world -> view space).
    fn view_matrix(&self) -> [[f32; 4]; 4] {
        let eye = self.position();
        let f = normalize(sub(self.target, eye)); // forward (into the scene)
        let up = [0.0, 1.0, 0.0];
        let s = normalize(cross(f, up)); // right
        let u = cross(s, f); // recomputed up
        [
            [s[0], s[1], s[2], -dot(s, eye)],
            [u[0], u[1], u[2], -dot(u, eye)],
            [-f[0], -f[1], -f[2], dot(f, eye)],
            [0.0, 0.0, 0.0, 1.0],
        ]
    }

    /// Row-major OpenGL perspective projection matrix (view -> clip space).
    fn perspective_matrix(&self, aspect: f32) -> [[f32; 4]; 4] {
        let t = (self.fov_y_rad * 0.5).tan();
        let n = self.near;
        let f = self.far;
        [
            [1.0 / (aspect * t), 0.0, 0.0, 0.0],
            [0.0, 1.0 / t, 0.0, 0.0],
            [0.0, 0.0, -(f + n) / (f - n), -2.0 * f * n / (f - n)],
            [0.0, 0.0, -1.0, 0.0],
        ]
    }

    /// Low-angle view from the front sideline, as if seated in the stands.
    pub fn audience_view(grid: &crate::GridConfig) -> Camera {
        Camera {
            target: field_center(grid),
            yaw: std::f32::consts::PI, // eye in front of the field (-Z side)
            pitch: 0.20,
            distance: fit_distance(grid, std::f32::consts::FRAC_PI_3) * 1.05,
            ..Camera::default()
        }
    }

    /// High, steep view from behind and above the audience (a "press box").
    pub fn press_box(grid: &crate::GridConfig) -> Camera {
        Camera {
            target: field_center(grid),
            yaw: std::f32::consts::PI,
            pitch: 0.95,
            distance: fit_distance(grid, std::f32::consts::FRAC_PI_3) * 1.15,
            ..Camera::default()
        }
    }

    /// Near top-down bird's-eye view.
    pub fn overhead(grid: &crate::GridConfig) -> Camera {
        Camera {
            target: field_center(grid),
            yaw: std::f32::consts::PI,
            pitch: 1.45, // ~83 degrees; avoids the exactly-vertical singularity
            distance: fit_distance(grid, std::f32::consts::FRAC_PI_3),
            ..Camera::default()
        }
    }
}

/// World-space center of the field at turf height 0.
fn field_center(grid: &crate::GridConfig) -> [f32; 3] {
    [grid.width * 0.5, 0.0, grid.height * 0.5]
}

/// Distance at which a field of this size roughly fills the vertical FOV.
fn fit_distance(grid: &crate::GridConfig, fov_y: f32) -> f32 {
    let diag = (grid.width * grid.width + grid.height * grid.height).sqrt();
    (diag * 0.5) / (fov_y * 0.5).tan()
}

// --- Small 3D vector helpers (no external math crate) -----------------------

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn normalize(v: [f32; 3]) -> [f32; 3] {
    let len = dot(v, v).sqrt();
    if len <= f32::EPSILON {
        [0.0, 0.0, 0.0]
    } else {
        [v[0] / len, v[1] / len, v[2] / len]
    }
}

/// Row-major 4x4 matrix times a column 4-vector.
fn mat4_mul_vec4(m: &[[f32; 4]; 4], v: &[f32; 4]) -> [f32; 4] {
    let mut out = [0.0f32; 4];
    for (r, row) in m.iter().enumerate() {
        out[r] = row[0] * v[0] + row[1] * v[1] + row[2] * v[2] + row[3] * v[3];
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{GridConfig, Point};

    const VW: f32 = 800.0;
    const VH: f32 = 600.0;

    fn approx(a: f32, b: f32, eps: f32) -> bool {
        (a - b).abs() <= eps
    }

    #[test]
    fn field_to_world_swaps_depth_axis() {
        assert_eq!(
            field_to_world(Point { x: 3.0, y: 7.0 }, 2.0),
            [3.0, 2.0, 7.0]
        );
    }

    #[test]
    fn position_is_distance_from_target() {
        let cameras = [
            Camera::default(),
            Camera::audience_view(&GridConfig::default()),
            Camera::press_box(&GridConfig::default()),
            Camera::overhead(&GridConfig::default()),
        ];
        for cam in cameras {
            let p = cam.position();
            let d = ((p[0] - cam.target[0]).powi(2)
                + (p[1] - cam.target[1]).powi(2)
                + (p[2] - cam.target[2]).powi(2))
            .sqrt();
            assert!(
                approx(d, cam.distance, 1e-3),
                "distance {d} vs {}",
                cam.distance
            );
        }
    }

    #[test]
    fn target_projects_to_viewport_center() {
        let cam = Camera::audience_view(&GridConfig::default());
        let s = cam.project(cam.target, VW, VH).expect("target visible");
        assert!(approx(s[0], VW * 0.5, 0.05), "x {}", s[0]);
        assert!(approx(s[1], VH * 0.5, 0.05), "y {}", s[1]);
    }

    #[test]
    fn point_behind_camera_returns_none() {
        // Simple hand-set camera at +Z looking down -Z toward the origin.
        let cam = Camera {
            target: [0.0, 0.0, 0.0],
            yaw: 0.0,
            pitch: 0.0,
            distance: 10.0,
            ..Camera::default()
        };
        // Eye is at [0,0,10]; a point well behind it (larger +Z) is not visible.
        assert!(cam.project([0.0, 0.0, 20.0], VW, VH).is_none());
    }

    #[test]
    fn hand_set_camera_projects_predictably() {
        // Eye at [0,0,10], forward = -Z, right = +X, up = +Y.
        let cam = Camera {
            target: [0.0, 0.0, 0.0],
            yaw: 0.0,
            pitch: 0.0,
            distance: 10.0,
            fov_y_rad: std::f32::consts::FRAC_PI_2,
            near: 0.1,
            far: 100.0,
        };
        assert_eq!(cam.position(), [0.0, 0.0, 10.0]);

        // Origin (the target) lands dead center.
        let c = cam.project([0.0, 0.0, 0.0], VW, VH).unwrap();
        assert!(approx(c[0], VW * 0.5, 0.01));
        assert!(approx(c[1], VH * 0.5, 0.01));

        // A point to the +X (world right) projects to the right of center.
        let right = cam.project([1.0, 0.0, 0.0], VW, VH).unwrap();
        assert!(right[0] > VW * 0.5, "right x {}", right[0]);
        assert!(approx(right[1], VH * 0.5, 0.01), "right y {}", right[1]);

        // A point above (+Y) projects above center (smaller screen y).
        let up = cam.project([0.0, 1.0, 0.0], VW, VH).unwrap();
        assert!(up[1] < VH * 0.5, "up y {}", up[1]);
        assert!(approx(up[0], VW * 0.5, 0.01), "up x {}", up[0]);

        // With a 90-degree vertical FOV at distance 10, the half-height of the
        // view plane is 10, so a +Y=10 point sits at the top edge (ndc_y = 1).
        let edge = cam.project([0.0, 10.0, 0.0], VW, VH).unwrap();
        assert!(approx(edge[1], 0.0, 0.01), "edge y {}", edge[1]);
    }

    #[test]
    fn overhead_orders_points_by_x() {
        let grid = GridConfig::default();
        let cam = Camera::overhead(&grid);
        let a = cam
            .project_point(Point { x: 20.0, y: 26.0 }, 0.0, VW, VH)
            .expect("a visible");
        let b = cam
            .project_point(Point { x: 80.0, y: 26.0 }, 0.0, VW, VH)
            .expect("b visible");
        assert!(a[0].is_finite() && b[0].is_finite());
        // Distinct field x -> distinct, well-separated screen x. (This preset
        // faces the field from behind, so larger field x maps to smaller
        // screen x; the mapping is finite and strictly ordered either way.)
        assert!(
            (a[0] - b[0]).abs() > 1.0,
            "not separated: {} vs {}",
            a[0],
            b[0]
        );
        assert!(b[0] < a[0], "expected {} < {}", b[0], a[0]);
    }

    #[test]
    fn presets_frame_the_field_center() {
        let grid = GridConfig::default();
        let center = field_center(&grid);
        for cam in [
            Camera::audience_view(&grid),
            Camera::press_box(&grid),
            Camera::overhead(&grid),
        ] {
            assert_eq!(cam.target, center);
            let s = cam.project(center, VW, VH).expect("center visible");
            assert!(approx(s[0], VW * 0.5, 0.05));
            assert!(approx(s[1], VH * 0.5, 0.05));
        }
    }
}

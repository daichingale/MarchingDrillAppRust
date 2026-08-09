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
    pub fn at_eye(eye: [f32; 3], target: [f32; 3], fov_y_rad: f32) -> Self {
        let d = sub(eye, target);
        let distance = dot(d, d).sqrt().max(1.0e-4);
        let offset = [d[0] / distance, d[1] / distance, d[2] / distance];
        Self {
            target,
            yaw: offset[0].atan2(offset[2]),
            pitch: offset[1].clamp(-1.0, 1.0).asin(),
            distance,
            fov_y_rad,
            ..Self::default()
        }
    }

    pub fn end_zone(grid: &crate::GridConfig, near: bool) -> Self {
        let eye = if near {
            [grid.width * 0.5, 2.0, -10.0]
        } else {
            [grid.width * 0.5, 2.0, grid.height + 10.0]
        };
        Self::at_eye(eye, field_center(grid), std::f32::consts::FRAC_PI_3)
    }

    /// Place the eye at a venue-neutral audience seat and frame field center.
    pub fn from_seat(
        grid: &crate::GridConfig,
        stand: &crate::stadium::StandSection,
        row: u16,
        along_frac: f32,
    ) -> Self {
        Self::at_eye(
            stand.seat_eye_position(grid, along_frac, row),
            field_center(grid),
            std::f32::consts::FRAC_PI_3,
        )
    }

    /// First-person diagnostic view in the performer's facing direction.
    pub fn performer_pov(pos: crate::Point, heading_rad: f32, lookahead_m: f32) -> Self {
        let eye = field_to_world(pos, 1.6);
        let lookahead = if lookahead_m.is_finite() {
            lookahead_m.clamp(0.1, 500.0)
        } else {
            10.0
        };
        let heading = if heading_rad.is_finite() {
            heading_rad
        } else {
            0.0
        };
        let target = [
            eye[0] + heading.sin() * lookahead,
            eye[1],
            eye[2] + heading.cos() * lookahead,
        ];
        Self::at_eye(eye, target, std::f32::consts::FRAC_PI_4)
    }
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
    pub fn view_matrix(&self) -> [[f32; 4]; 4] {
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

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct CameraPose {
    pub eye: [f32; 3],
    pub target: [f32; 3],
    pub up: [f32; 3],
    pub fov_y_rad: f32,
    pub near: f32,
    pub far: f32,
}

impl CameraPose {
    pub fn project(&self, world: [f32; 3], vw: f32, vh: f32) -> Option<[f32; 2]> {
        let view = look_at_matrix(self.eye, self.target, self.up);
        let proj = perspective_matrix(
            self.fov_y_rad,
            (vw / vh.max(1.0)).max(0.001),
            self.near,
            self.far,
        );
        project_with(&view, &proj, world, vw, vh)
    }
    pub fn orbit_camera(self) -> Camera {
        Camera::at_eye(self.eye, self.target, self.fov_y_rad)
    }
}

impl From<Camera> for CameraPose {
    fn from(camera: Camera) -> Self {
        Self {
            eye: camera.position(),
            target: camera.target,
            up: [0.0, 1.0, 0.0],
            fov_y_rad: camera.fov_y_rad,
            near: camera.near,
            far: camera.far,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct CameraKeyframe {
    pub count: f32,
    pub eye: [f32; 3],
    pub target: [f32; 3],
    pub roll_rad: f32,
    pub fov_y_rad: f32,
    /// How this keyframe travels to the following keyframe.
    #[serde(default)]
    pub interpolation: CameraInterpolation,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum CameraInterpolation {
    /// Smooth Catmull-Rom motion, suitable for a moving camera.
    #[default]
    Smooth,
    /// Straight, constant-rate motion between poses.
    Linear,
    /// Hold this pose until the next keyframe (a hard keyframe cut).
    Hold,
}

impl CameraKeyframe {
    pub fn from_camera(count: f32, camera: Camera) -> Self {
        Self {
            count,
            eye: camera.position(),
            target: camera.target,
            roll_rad: 0.0,
            fov_y_rad: camera.fov_y_rad,
            interpolation: CameraInterpolation::Smooth,
        }
    }
    pub fn validate(&self) -> Result<(), crate::DrillError> {
        let finite = self.count.is_finite()
            && self.eye.iter().chain(&self.target).all(|v| v.is_finite())
            && self.roll_rad.is_finite()
            && self.fov_y_rad.is_finite();
        if !finite
            || self.count < 0.0
            || !(0.05..=std::f32::consts::PI - 0.05).contains(&self.fov_y_rad)
            || dot(sub(self.eye, self.target), sub(self.eye, self.target)) < 1.0e-8
        {
            return Err(crate::DrillError::InvalidEdit);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CameraTrack {
    pub id: crate::CameraId,
    pub name: String,
    keyframes: Vec<CameraKeyframe>,
    pub near: f32,
    pub far: f32,
}

impl CameraTrack {
    pub fn new(id: crate::CameraId, name: impl Into<String>) -> Self {
        Self {
            id,
            name: name.into(),
            keyframes: Vec::new(),
            near: 0.1,
            far: 2000.0,
        }
    }
    pub fn keyframes(&self) -> &[CameraKeyframe] {
        &self.keyframes
    }
    pub fn insert_keyframe(
        &mut self,
        frame: CameraKeyframe,
    ) -> Result<Option<CameraKeyframe>, crate::DrillError> {
        frame.validate()?;
        match self
            .keyframes
            .binary_search_by(|k| k.count.total_cmp(&frame.count))
        {
            Ok(i) => Ok(Some(std::mem::replace(&mut self.keyframes[i], frame))),
            Err(i) => {
                self.keyframes.insert(i, frame);
                Ok(None)
            }
        }
    }
    pub fn remove_keyframe(&mut self, count: f32) -> Option<CameraKeyframe> {
        let i = self
            .keyframes
            .binary_search_by(|k| k.count.total_cmp(&count))
            .ok()?;
        Some(self.keyframes.remove(i))
    }
    pub fn evaluate(&self, count: f32) -> Option<CameraPose> {
        let first = *self.keyframes.first()?;
        if count <= first.count {
            return Some(pose(first, self.near, self.far));
        }
        let last = *self.keyframes.last()?;
        if count >= last.count {
            return Some(pose(last, self.near, self.far));
        }
        let right = self.keyframes.partition_point(|k| k.count <= count);
        let a = self.keyframes[right - 1];
        let b = self.keyframes[right];
        let t = ((count - a.count) / (b.count - a.count)).clamp(0.0, 1.0);
        let p0 = self
            .keyframes
            .get(right.wrapping_sub(2))
            .copied()
            .unwrap_or(a);
        let p3 = self.keyframes.get(right + 1).copied().unwrap_or(b);
        if a.interpolation == CameraInterpolation::Hold {
            return Some(pose(a, self.near, self.far));
        }
        let eye = if a.interpolation == CameraInterpolation::Linear {
            lerp3(a.eye, b.eye, t)
        } else {
            catmull(p0.eye, a.eye, b.eye, p3.eye, t)
        };
        let target = if a.interpolation == CameraInterpolation::Linear {
            lerp3(a.target, b.target, t)
        } else {
            catmull(p0.target, a.target, b.target, p3.target, t)
        };
        let roll = lerp_angle(a.roll_rad, b.roll_rad, t);
        let forward = normalize(sub(target, eye));
        let base_right = normalize(cross(forward, [0.0, 1.0, 0.0]));
        let base_up = normalize(cross(base_right, forward));
        let up = add(scale(base_up, roll.cos()), scale(base_right, roll.sin()));
        Some(CameraPose {
            eye,
            target,
            up,
            fov_y_rad: a.fov_y_rad + (b.fov_y_rad - a.fov_y_rad) * t,
            near: self.near,
            far: self.far,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct CameraCut {
    pub count: f32,
    pub camera: crate::CameraId,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
pub struct CameraProgram {
    pub tracks: Vec<CameraTrack>,
    pub cuts: Vec<CameraCut>,
}

impl CameraProgram {
    pub fn default_for_grid(grid: &crate::GridConfig) -> Self {
        Self::default_for_grid_localized(grid, crate::Locale::Ja)
    }

    pub fn default_for_grid_localized(grid: &crate::GridConfig, locale: crate::Locale) -> Self {
        let id = crate::CameraId::new(1).expect("non-zero camera id");
        let mut track = CameraTrack::new(
            id,
            match locale {
                crate::Locale::Ja => "メインカメラ",
                crate::Locale::En => "Main Camera",
            },
        );
        track
            .insert_keyframe(CameraKeyframe::from_camera(0.0, Camera::press_box(grid)))
            .expect("preset is valid");
        Self {
            tracks: vec![track],
            cuts: vec![CameraCut {
                count: 0.0,
                camera: id,
            }],
        }
    }
    pub fn active_track(&self, count: f32) -> Option<&CameraTrack> {
        let cut = self
            .cuts
            .iter()
            .rev()
            .find(|cut| cut.count <= count)
            .or_else(|| self.cuts.first())?;
        self.tracks.iter().find(|track| track.id == cut.camera)
    }
    pub fn evaluate(&self, count: f32) -> Option<CameraPose> {
        self.active_track(count)?.evaluate(count)
    }
    pub fn insert_cut(&mut self, cut: CameraCut) -> Result<Option<CameraCut>, crate::DrillError> {
        if !cut.count.is_finite()
            || cut.count < 0.0
            || !self.tracks.iter().any(|t| t.id == cut.camera)
        {
            return Err(crate::DrillError::InvalidEdit);
        }
        match self
            .cuts
            .binary_search_by(|item| item.count.total_cmp(&cut.count))
        {
            Ok(index) => Ok(Some(std::mem::replace(&mut self.cuts[index], cut))),
            Err(index) => {
                self.cuts.insert(index, cut);
                Ok(None)
            }
        }
    }
    pub fn remove_cut(&mut self, count: f32) -> Option<CameraCut> {
        let index = self
            .cuts
            .binary_search_by(|item| item.count.total_cmp(&count))
            .ok()?;
        Some(self.cuts.remove(index))
    }
    pub fn validate(&self, max_count: f32) -> Result<(), crate::DrillError> {
        let mut ids = std::collections::BTreeSet::new();
        if self.tracks.iter().any(|t| {
            !ids.insert(t.id)
                || t.name.len() > crate::MAX_TEXT_BYTES
                || t.near <= 0.0
                || t.far <= t.near
        }) {
            return Err(crate::DrillError::InvalidEdit);
        }
        for track in &self.tracks {
            for frame in &track.keyframes {
                frame.validate()?;
                if frame.count > max_count {
                    return Err(crate::DrillError::InvalidEdit);
                }
            }
            if track.keyframes.windows(2).any(|w| w[0].count >= w[1].count) {
                return Err(crate::DrillError::InvalidEdit);
            }
        }
        if self.cuts.iter().any(|c| {
            !c.count.is_finite() || c.count < 0.0 || c.count > max_count || !ids.contains(&c.camera)
        }) || self.cuts.windows(2).any(|w| w[0].count >= w[1].count)
        {
            return Err(crate::DrillError::InvalidEdit);
        }
        Ok(())
    }
}

fn lerp3(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
    ]
}

fn pose(k: CameraKeyframe, near: f32, far: f32) -> CameraPose {
    let forward = normalize(sub(k.target, k.eye));
    let right = normalize(cross(forward, [0.0, 1.0, 0.0]));
    let base_up = normalize(cross(right, forward));
    CameraPose {
        eye: k.eye,
        target: k.target,
        up: add(
            scale(base_up, k.roll_rad.cos()),
            scale(right, k.roll_rad.sin()),
        ),
        fov_y_rad: k.fov_y_rad,
        near,
        far,
    }
}
fn catmull(p0: [f32; 3], p1: [f32; 3], p2: [f32; 3], p3: [f32; 3], t: f32) -> [f32; 3] {
    let t2 = t * t;
    let t3 = t2 * t;
    std::array::from_fn(|i| {
        0.5 * ((2.0 * p1[i])
            + (-p0[i] + p2[i]) * t
            + (2.0 * p0[i] - 5.0 * p1[i] + 4.0 * p2[i] - p3[i]) * t2
            + (-p0[i] + 3.0 * p1[i] - 3.0 * p2[i] + p3[i]) * t3)
    })
}
fn lerp_angle(a: f32, b: f32, t: f32) -> f32 {
    let d = (b - a + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI;
    a + d * t
}
fn add(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn scale(a: [f32; 3], s: f32) -> [f32; 3] {
    [a[0] * s, a[1] * s, a[2] * s]
}
fn look_at_matrix(eye: [f32; 3], target: [f32; 3], up: [f32; 3]) -> [[f32; 4]; 4] {
    let f = normalize(sub(target, eye));
    let s = normalize(cross(f, up));
    let u = cross(s, f);
    [
        [s[0], s[1], s[2], -dot(s, eye)],
        [u[0], u[1], u[2], -dot(u, eye)],
        [-f[0], -f[1], -f[2], dot(f, eye)],
        [0.0, 0.0, 0.0, 1.0],
    ]
}
fn perspective_matrix(fov: f32, aspect: f32, n: f32, f: f32) -> [[f32; 4]; 4] {
    let t = (fov * 0.5).tan();
    [
        [1.0 / (aspect * t), 0.0, 0.0, 0.0],
        [0.0, 1.0 / t, 0.0, 0.0],
        [0.0, 0.0, -(f + n) / (f - n), -2.0 * f * n / (f - n)],
        [0.0, 0.0, -1.0, 0.0],
    ]
}
fn project_with(
    v: &[[f32; 4]; 4],
    p: &[[f32; 4]; 4],
    w: [f32; 3],
    vw: f32,
    vh: f32,
) -> Option<[f32; 2]> {
    let c = mat4_mul_vec4(p, &mat4_mul_vec4(v, &[w[0], w[1], w[2], 1.0]));
    if c[3] <= f32::EPSILON {
        return None;
    }
    let z = c[2] / c[3];
    if !(-1.0..=1.0).contains(&z) {
        return None;
    }
    Some([
        (c[0] / c[3] * 0.5 + 0.5) * vw,
        (0.5 - c[1] / c[3] * 0.5) * vh,
    ])
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

    #[test]
    fn keyframes_are_sorted_replaced_and_count_deterministic() {
        let id = crate::CameraId::new(7).unwrap();
        let mut track = CameraTrack::new(id, "A");
        let grid = GridConfig::default();
        let a = CameraKeyframe::from_camera(0.0, Camera::audience_view(&grid));
        let b = CameraKeyframe::from_camera(16.0, Camera::overhead(&grid));
        track.insert_keyframe(b).unwrap();
        track.insert_keyframe(a).unwrap();
        assert_eq!(
            track
                .keyframes()
                .iter()
                .map(|k| k.count)
                .collect::<Vec<_>>(),
            vec![0.0, 16.0]
        );
        let first = track.evaluate(8.0).unwrap();
        for _ in 0..100 {
            assert_eq!(track.evaluate(8.0), Some(first));
        }
        let mut replacement = b;
        replacement.eye[1] += 3.0;
        assert_eq!(track.insert_keyframe(replacement).unwrap(), Some(b));
        assert_eq!(track.keyframes().len(), 2);
    }

    #[test]
    fn program_cuts_switch_on_exact_count_and_round_trip() {
        let grid = GridConfig::default();
        let a_id = crate::CameraId::new(1).unwrap();
        let b_id = crate::CameraId::new(2).unwrap();
        let mut a = CameraTrack::new(a_id, "Audience");
        let mut b = CameraTrack::new(b_id, "End zone");
        a.insert_keyframe(CameraKeyframe::from_camera(
            0.0,
            Camera::audience_view(&grid),
        ))
        .unwrap();
        b.insert_keyframe(CameraKeyframe::from_camera(
            8.0,
            Camera::end_zone(&grid, true),
        ))
        .unwrap();
        let program = CameraProgram {
            tracks: vec![a, b],
            cuts: vec![
                CameraCut {
                    count: 0.0,
                    camera: a_id,
                },
                CameraCut {
                    count: 8.0,
                    camera: b_id,
                },
            ],
        };
        assert_eq!(program.active_track(7.999).unwrap().id, a_id);
        assert_eq!(program.active_track(8.0).unwrap().id, b_id);
        let json = serde_json::to_string(&program).unwrap();
        let loaded: CameraProgram = serde_json::from_str(&json).unwrap();
        assert_eq!(loaded, program);
    }

    #[test]
    fn hold_and_linear_interpolation_are_explicit() {
        let id = crate::CameraId::new(3).unwrap();
        let grid = GridConfig::default();
        let mut track = CameraTrack::new(id, "shot");
        let mut a = CameraKeyframe::from_camera(0.0, Camera::audience_view(&grid));
        let b = CameraKeyframe::from_camera(10.0, Camera::overhead(&grid));
        a.interpolation = CameraInterpolation::Hold;
        track.insert_keyframe(a).unwrap();
        track.insert_keyframe(b).unwrap();
        assert_eq!(track.evaluate(5.0), track.evaluate(0.0));
        a.interpolation = CameraInterpolation::Linear;
        track.insert_keyframe(a).unwrap();
        let halfway = track.evaluate(5.0).unwrap();
        assert!((halfway.eye[0] - (a.eye[0] + b.eye[0]) * 0.5).abs() < 1.0e-4);
    }

    #[test]
    fn cut_insert_replace_remove_is_sorted() {
        let grid = GridConfig::default();
        let mut program = CameraProgram::default_for_grid(&grid);
        let camera = program.tracks[0].id;
        assert!(
            program
                .insert_cut(CameraCut { count: 8.0, camera })
                .unwrap()
                .is_none()
        );
        assert_eq!(
            program.cuts.iter().map(|cut| cut.count).collect::<Vec<_>>(),
            vec![0.0, 8.0]
        );
        assert!(
            program
                .insert_cut(CameraCut { count: 8.0, camera })
                .unwrap()
                .is_some()
        );
        assert_eq!(program.remove_cut(8.0).unwrap().count, 8.0);
    }

    #[test]
    fn angle_interpolation_uses_shortest_path() {
        let a = 179_f32.to_radians();
        let b = (-179_f32).to_radians();
        assert!((lerp_angle(a, b, 0.5).abs() - std::f32::consts::PI).abs() < 0.001);
    }
}

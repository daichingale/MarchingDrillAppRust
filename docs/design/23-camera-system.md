# 23. カメラシステム（キーフレーム・追従・マルチカメラ）

## 1. 目的と範囲

動画書き出し（`MEDIA_PIPELINE.md` P1「カメラショット、count単位keyframe、追従、補間、動画トラック」）と
3D プレビュー（doc 22）の両方が参照する、単一の決定論的カメラ評価モデルを定義する。

扱うこと:

- count 上に置くカメラキーフレーム（位置・注視点・FOV・ロール）と、その補間。
- 演者・セクション重心への追従（Follow）と、キーフレームとの合成規則。
- 複数カメラトラックと count 上のカット切替（マルチカメラ）。
- ショットプリセット（ワイド／寄り／エンドゾーン／プレスボックス／ドローン軌道）。
- フィールド外・地面下・観客席内へのソフト境界と、意図的に破る手段。
- カメラの急加速検出（`MEDIA_PIPELINE.md` P2）。
- `Document` への永続化と、`Edit` コマンド代数（doc 10 が確定させたカメラ10バリアント）への接続。

扱わないこと（他の設計文書の担当）:

- GPU 側のレンダリング・DisplayList 生成（doc 20/21）。実際に世界座標をどう描くかは doc 20 が担当し、
  本書はその入力となる `CameraPose`（3.1）を確定するところまでを担当する。
- 3D スタジアム座標系そのもの（`field_to_world` の座標変換規約）は doc 22 の担当。本書はそれを前提として使う。
- 演者・セクションのメタデータ（`SectionId` 等）は doc 15 の担当。本書は `PerformerId` の集合として
  境界越しに参照するのみで、セクション概念そのものは定義しない（3.7）。
- 衝突・間隔解析の一般理論は doc 13 の担当。本書はカメラ視点固有の急加速検出のみを扱う。
- `Edit` enum 本体・`DrillError`・`Locale` は doc 10 / doc 42 の担当。本書はそこへ接続する
  カメラ側のメソッド（3.3, 3.9）を用意し、doc 10 が確定させたバリアント名をそのまま使う（3.12）。

## 2. 現状

`crates/drill-core/src/camera.rs`（373行）には以下が既にある。

- `Camera`（33–63行目）: `target: [f32;3]` / `yaw` / `pitch` / `distance` / `fov_y_rad` / `near` / `far`
  を持つ**軌道カメラ**。キーフレームも count も知らない、`drill-app` のインタラクティブな 3D プレビュー用の
  その場限りの視点。
- `Camera::position`（71–81行目）、`view_matrix`（127–139行目、`gluLookAt` 規約・`up` は `[0,1,0]` 固定）、
  `perspective_matrix`（142–152行目）、`project` / `project_point`（87–124行目）。
- `Camera::audience_view` / `press_box` / `overhead`（155–186行目）: `GridConfig` から算出する3種類の固定プリセット。
  いずれも `target` はフィールド中心固定、ロールは常に0。
- `field_to_world`（26–28行目）: `Point{x,y}` を `[x, height, y]` へ写す、doc 22 が定める座標変換。

`crates/drill-app/src/main.rs` での使用（7, 66, 72, 104, 291–397, 444, 726–735, 1437–1438行目）:

- `DrillApp` は `camera: Camera` を**1個だけ**セッション状態として持つ（`Document` には保存されない）。
- マウスドラッグで `yaw`/`pitch`/`distance` を直接書き換える（301–308行目）。ロール操作は無い。
- ボタン押下で `Camera::audience_view` 等に丸ごと差し替える（729–735行目）。キーフレームやアニメーションは無い。

`crates/drill-core/src/lib.rs` に既にあるもの（本書が前提として使う）:

- `Document::timeline_counts`（349–355行目）、`global_count`（357–365行目）、
  `locate_count`（367–382行目、`(set_index, local_count)` を返す。**local_count は 0..1 の progress ではなく
  カウント単位**）、`positions_at`（384–394行目、`progress: f32` を受け取る点に注意）。
- `PerformerId = u32`（19行目）。`SetId` のような安定IDはまだ存在しない（`Set` は `Vec` の添字で参照される）。

`crates/drill-core/src/tempo.rs` の `TempoMap`（本書が全面的に依存する）:

- `seconds_at` / `count_at`（112–170行目）: count と秒の相互変換。ループ無しの単純な `for` で
  イベント配列を毎回先頭から走査する実装（**累積状態を持たない、呼ぶたびに独立して正しい値を返す**）。
  本書のカメラ評価も同じ流儀（3.7 の減衰追従、5章）を踏襲する。

存在しないもの（本書が新規に定義する）:

- count 単位のカメラキーフレーム、補間（Catmull-Rom / slerp）、追従、マルチカメラ、ショットプリセット、
  境界制約、急加速診断。`DrillError` / `Locale` / `Edit` enum / `SetId` も未着手（`DESIGN_GAPS.md` A-1, A-6）。
  本書はこれらが将来入ることを前提にシグネチャを書くが、**現時点でコンパイルできる代替**も明記する（3.12, 9章）。

## 3. 設計

### 3.1 共有型 `CameraPose` と `look_at_matrix` の抽出

動画ラスタライズ（doc 20/31）とプレビュー（doc 22）が同じ行列を使うよう、評価結果の型を1つに定める。
既存の `Camera` はロールを表現できない（`up` が `[0,1,0]` 固定）ため、`up` を明示的に持つ姿勢型を新設する。

```rust
/// Fully resolved camera state at one instant. This is the single output type
/// shared by the interactive 3D preview (doc 22) and offline video rendering
/// (doc 31): both call `CameraProgram::evaluate` (3.9) and consume the result
/// through this type only.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CameraPose {
    pub eye: [f32; 3],
    pub target: [f32; 3],
    /// Not necessarily world `+Y`: this is how roll is represented (3.5).
    pub up: [f32; 3],
    pub fov_y_rad: f32,
    pub near: f32,
    pub far: f32,
}

impl CameraPose {
    pub fn view_matrix(&self) -> [[f32; 4]; 4] {
        look_at_matrix(self.eye, self.target, self.up)
    }

    pub fn perspective_matrix(&self, aspect: f32) -> [[f32; 4]; 4] {
        perspective_matrix(self.fov_y_rad, aspect, self.near, self.far)
    }

    pub fn project(&self, world: [f32; 3], vw: f32, vh: f32) -> Option<[f32; 2]> {
        project_with(&self.view_matrix(), &self.perspective_matrix(safe_aspect(vw, vh)), world, vw, vh)
    }
}

impl From<Camera> for CameraPose {
    /// Exact: the orbit `Camera` has no roll, so `up` is always world `+Y`.
    fn from(cam: Camera) -> Self {
        CameraPose {
            eye: cam.position(),
            target: cam.target,
            up: [0.0, 1.0, 0.0],
            fov_y_rad: cam.fov_y_rad,
            near: cam.near,
            far: cam.far,
        }
    }
}
```

`Camera::view_matrix` / `perspective_matrix` / `project` の実装本体（既存の 127–152, 87–113行目）を
`look_at_matrix` / `perspective_matrix` / `project_with` という自由関数へ抽出し、`Camera` と `CameraPose` の
両方から呼ぶ。**挙動は変えない**（既存テストがそのまま通ることを回帰条件にする）。

`Camera::view_matrix` は現状 `camera.rs:127` で **private** だが、`docs/design/21-gpu-renderer.md`
§「深度規約の落とし穴」（21 517–520行目）が GPU 経路（`drill-gpu`）からの共有を明示的に要求している。
投影行列は深度規約（OpenGL の NDC z `[-1,1]` と wgpu/D3D/Metal の `[0,1]`）が食い違うため
`perspective_matrix` は共有できないが、ビュー行列は規約に依存しないため CPU 経路（`Camera::project`）
と GPU 経路とで**同一の行列**を使い、x/y の投影結果を一致させられる（21 の重心誤差コンフォーマンス
テスト、§3.9 が要求）。したがって本書は以下を可視性の変更として明記する。

```rust
impl Camera {
    // was: fn view_matrix(&self) -> [[f32; 4]; 4]
    pub fn view_matrix(&self) -> [[f32; 4]; 4] {
        look_at_matrix(self.position(), self.target, [0.0, 1.0, 0.0])
    }
}
```

`perspective_matrix` と `project` は private のまま（`project` は既に `pub`）。可視性を上げるのは
`view_matrix` のみで、シグネチャ・返り値・既存の呼び出し元はすべて変わらない
（純粋な `private -> pub` の1語の変更、実装タスク T1 に含める）。

```rust
pub(crate) fn look_at_matrix(eye: [f32; 3], target: [f32; 3], up: [f32; 3]) -> [[f32; 4]; 4] {
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

pub(crate) fn perspective_matrix(fov_y_rad: f32, aspect: f32, near: f32, far: f32) -> [[f32; 4]; 4] {
    let t = (fov_y_rad * 0.5).tan();
    [
        [1.0 / (aspect * t), 0.0, 0.0, 0.0],
        [0.0, 1.0 / t, 0.0, 0.0],
        [0.0, 0.0, -(far + near) / (far - near), -2.0 * far * near / (far - near)],
        [0.0, 0.0, -1.0, 0.0],
    ]
}

pub(crate) fn project_with(
    view: &[[f32; 4]; 4],
    proj: &[[f32; 4]; 4],
    world: [f32; 3],
    vw: f32,
    vh: f32,
) -> Option<[f32; 2]> {
    let clip = mat4_mul_vec4(proj, &mat4_mul_vec4(view, &[world[0], world[1], world[2], 1.0]));
    let w = clip[3];
    if w <= f32::EPSILON {
        return None;
    }
    let ndc = [clip[0] / w, clip[1] / w, clip[2] / w];
    if !(-1.0..=1.0).contains(&ndc[2]) {
        return None;
    }
    Some([(ndc[0] * 0.5 + 0.5) * vw, (0.5 - ndc[1] * 0.5) * vh])
}

fn safe_aspect(vw: f32, vh: f32) -> f32 {
    if vh > 0.0 { vw / vh } else { 1.0 }
}
```

`normalize` / `sub` / `cross` / `dot` / `mat4_mul_vec4` は既存の private ヘルパー（199–233行目）をそのまま使う。

### 3.2 最小クォータニオン

`drill-core` は `serde`/`serde_json` 以外へ依存しない（00-conventions.md）ため外部数学クレートは使わず、
姿勢補間に必要な最小限だけを自前で持つ。**`Document` へは保存しない内部計算専用の型**（`Serialize` 不要）。

```rust
/// Minimal unit quaternion, scoped to what camera orientation interpolation
/// needs (3.4). Not part of any persisted type — a pure computation helper.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Quat { x: f32, y: f32, z: f32, w: f32 }

impl Quat {
    const IDENTITY: Quat = Quat { x: 0.0, y: 0.0, z: 0.0, w: 1.0 };

    /// Orientation whose local `-Z` points from `eye` toward `target`, with
    /// `up_hint` resolving the remaining roll freedom (`[0,1,0]` in practice).
    /// Guarded call sites never pass `eye == target` (see `CameraKeyframe::validate`, 3.3).
    fn look_at(eye: [f32; 3], target: [f32; 3], up_hint: [f32; 3]) -> Quat {
        let f = normalize(sub(target, eye));
        let s = normalize(cross(f, up_hint));
        let u = cross(s, f);
        Quat::from_basis(s, u, [-f[0], -f[1], -f[2]])
    }

    /// Standard (Shepperd) rotation-matrix-to-quaternion conversion. `x`/`y`/`z`
    /// are the matrix's orthonormal column axes.
    fn from_basis(x: [f32; 3], y: [f32; 3], z: [f32; 3]) -> Quat {
        let (m00, m10, m20) = (x[0], x[1], x[2]);
        let (m01, m11, m21) = (y[0], y[1], y[2]);
        let (m02, m12, m22) = (z[0], z[1], z[2]);
        let trace = m00 + m11 + m22;
        if trace > 0.0 {
            let s = (trace + 1.0).sqrt() * 2.0;
            Quat { w: 0.25 * s, x: (m21 - m12) / s, y: (m02 - m20) / s, z: (m10 - m01) / s }
        } else if m00 > m11 && m00 > m22 {
            let s = (1.0 + m00 - m11 - m22).sqrt() * 2.0;
            Quat { w: (m21 - m12) / s, x: 0.25 * s, y: (m01 + m10) / s, z: (m02 + m20) / s }
        } else if m11 > m22 {
            let s = (1.0 + m11 - m00 - m22).sqrt() * 2.0;
            Quat { w: (m02 - m20) / s, x: (m01 + m10) / s, y: 0.25 * s, z: (m12 + m21) / s }
        } else {
            let s = (1.0 + m22 - m00 - m11).sqrt() * 2.0;
            Quat { w: (m10 - m01) / s, x: (m02 + m20) / s, y: (m12 + m21) / s, z: 0.25 * s }
        }
    }

    fn from_axis_angle(axis: [f32; 3], angle: f32) -> Quat {
        let a = normalize(axis);
        let (s, c) = (angle * 0.5).sin_cos();
        Quat { x: a[0] * s, y: a[1] * s, z: a[2] * s, w: c }
    }

    /// Shortest-path slerp; falls back to a normalized lerp when the two
    /// orientations are nearly identical (avoids dividing by `sin(theta) ~= 0`).
    fn slerp(a: Quat, b: Quat, t: f32) -> Quat {
        let mut b = b;
        let mut dot = a.x * b.x + a.y * b.y + a.z * b.z + a.w * b.w;
        if dot < 0.0 {
            b = Quat { x: -b.x, y: -b.y, z: -b.z, w: -b.w };
            dot = -dot;
        }
        let dot = dot.clamp(-1.0, 1.0);
        if dot > 0.9995 {
            return Quat {
                x: a.x + (b.x - a.x) * t,
                y: a.y + (b.y - a.y) * t,
                z: a.z + (b.z - a.z) * t,
                w: a.w + (b.w - a.w) * t,
            }
            .normalized();
        }
        let theta_0 = dot.acos();
        let theta = theta_0 * t;
        let (sin_theta, sin_theta_0) = (theta.sin(), theta_0.sin());
        let s0 = (theta_0 - theta).sin() / sin_theta_0;
        let s1 = sin_theta / sin_theta_0;
        Quat {
            x: a.x * s0 + b.x * s1,
            y: a.y * s0 + b.y * s1,
            z: a.z * s0 + b.z * s1,
            w: a.w * s0 + b.w * s1,
        }
    }

    fn normalized(self) -> Quat {
        let len = (self.x * self.x + self.y * self.y + self.z * self.z + self.w * self.w).sqrt();
        if len <= f32::EPSILON { Quat::IDENTITY } else {
            Quat { x: self.x / len, y: self.y / len, z: self.z / len, w: self.w / len }
        }
    }

    fn rotate_vector(self, v: [f32; 3]) -> [f32; 3] {
        let q = [self.x, self.y, self.z];
        let t = scale3(cross(q, v), 2.0);
        let t2 = cross(q, t);
        [v[0] + self.w * t[0] + t2[0], v[1] + self.w * t[1] + t2[1], v[2] + self.w * t[2] + t2[2]]
    }
}

fn scale3(v: [f32; 3], s: f32) -> [f32; 3] {
    [v[0] * s, v[1] * s, v[2] * s]
}
```

### 3.3 `CameraKeyframe` と `CameraTrack`

キーフレームの時間軸は**count固定**。`f32` を使うのは既存の `TempoMap`/`Document` の count 表現
（`global_count: f32`）と揃えるためで、テンポを変えてもキーフレームの count 値自体は動かない
（＝ショーとの同期が保たれる、という要求を型で満たす）。

```rust
/// A single stable identity for a camera track (see 00-conventions.md #2:
/// identity is a stable ID, order is the `Vec` index).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct CameraId(pub u32);

/// One authored camera pose anchored at a global count. `target` is ignored
/// when the owning track's `look` is `LookMode::Follow` (3.7); it still round-trips
/// through save/load so switching back to `LookMode::Explicit` does not lose data.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct CameraKeyframe {
    /// Global count. Fixed to the show timeline, independent of tempo.
    pub count: f32,
    pub eye: [f32; 3],
    pub target: [f32; 3],
    /// Roll about the forward axis, radians. Always authored explicitly,
    /// applied after look-at orientation regardless of `LookMode` (3.5).
    pub roll_rad: f32,
    pub fov_y_rad: f32,
}

impl CameraKeyframe {
    /// Rejects the invalid inputs listed in 6章: non-finite fields, a
    /// degenerate `eye == target`, and an FOV outside a sane range.
    pub fn validate(&self) -> Result<(), DrillError> {
        let finite = self.count.is_finite()
            && self.eye.iter().all(|v| v.is_finite())
            && self.target.iter().all(|v| v.is_finite())
            && self.roll_rad.is_finite()
            && self.fov_y_rad.is_finite();
        if !finite {
            return Err(DrillError::InvalidCameraKeyframe { reason: NonFiniteField });
        }
        let dx = self.eye[0] - self.target[0];
        let dy = self.eye[1] - self.target[1];
        let dz = self.eye[2] - self.target[2];
        if dx * dx + dy * dy + dz * dz < 1e-8 {
            return Err(DrillError::InvalidCameraKeyframe { reason: DegenerateLookAt });
        }
        if !(0.05..=std::f32::consts::PI - 0.05).contains(&self.fov_y_rad) {
            return Err(DrillError::InvalidCameraKeyframe { reason: FovOutOfRange });
        }
        Ok(())
    }
}

/// Whether a track's authored `target`/`roll` still steer the look direction,
/// or whether a runtime-computed subject overrides `target` (3.7). `roll_rad`
/// from the keyframes always applies in both modes (3.5).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum LookMode {
    Explicit,
    Follow { target: FollowTarget, damping: FollowDamping },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum BoundsMode {
    /// Eye position is soft-clamped to `CameraBounds` every evaluation (3.8).
    Enforced,
    /// Deliberate escape hatch: no clamping (3.8's "意図的に破る手段").
    Free,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CameraTrack {
    pub id: CameraId,
    pub name: String,
    /// Sorted ascending by `count`, unique `count` (invariant, 4章). Use
    /// `insert_keyframe`/`remove_keyframe` rather than mutating this directly.
    keyframes: Vec<CameraKeyframe>,
    pub look: LookMode,
    pub bounds: BoundsMode,
    pub near: f32,
    pub far: f32,
}

impl CameraTrack {
    pub fn new(id: CameraId, name: impl Into<String>) -> Self {
        Self {
            id,
            name: name.into(),
            keyframes: Vec::new(),
            look: LookMode::Explicit,
            bounds: BoundsMode::Enforced,
            near: 0.1,
            far: 2000.0,
        }
    }

    pub fn keyframes(&self) -> &[CameraKeyframe] {
        &self.keyframes
    }

    /// `pub(crate)`: only `CameraProgram::normalize_after_load` (3.9) reaches
    /// past the sorted-unique invariant directly; everything else goes
    /// through `insert_keyframe`/`remove_keyframe`.
    pub(crate) fn keyframes_mut(&mut self) -> &mut Vec<CameraKeyframe> {
        &mut self.keyframes
    }

    /// Inserts, or replaces an existing keyframe at the same `count` (mirrors
    /// `TempoMap::set`'s semantics, 6章 "同一countに複数キーフレーム"). Returns
    /// the keyframe that was displaced, if any, so a future `Edit::apply` can
    /// build the inverse.
    pub fn insert_keyframe(&mut self, keyframe: CameraKeyframe) -> Result<Option<CameraKeyframe>, DrillError> {
        keyframe.validate()?;
        match self.keyframes.iter().position(|k| k.count == keyframe.count) {
            Some(i) => Ok(Some(std::mem::replace(&mut self.keyframes[i], keyframe))),
            None => {
                self.keyframes.push(keyframe);
                self.keyframes.sort_by(|a, b| a.count.total_cmp(&b.count));
                Ok(None)
            }
        }
    }

    pub fn remove_keyframe(&mut self, count: f32) -> Option<CameraKeyframe> {
        let i = self.keyframes.iter().position(|k| k.count == count)?;
        Some(self.keyframes.remove(i))
    }
}
```

### 3.4 位置補間: count 実値を節点とする非一様 Catmull-Rom

位置はキーフレーム間で滑らかに補間する。キーフレームの実際の count 間隔をスプラインの節点にそのまま使う
（空間的な弦長ではなく**時間＝count**で節点を取る）。理由: 「離れた count のキーフレーム間ほどゆっくり動く」
という直感がそのまま速度に反映され、UI上でキーフレームの間隔を詰める操作がそのまま加速の操作になる
（3.6 の急加速検出とも整合する）。境界（最初/最後のキーフレーム）は鏡映で仮想制御点を作る。

```rust
/// Barry–Goldman non-uniform Catmull-Rom, parametrized directly by each
/// keyframe's `count` (not chord length). `count` must lie in `[c1, c2]`.
/// Degenerate spacing (`c1 == c2` etc.) falls back to the midpoint `t = 0.5`
/// rather than dividing by zero — see `guards_zero_spacing` in 7章.
fn catmull_rom_nonuniform(
    (c0, p0): (f32, [f32; 3]),
    (c1, p1): (f32, [f32; 3]),
    (c2, p2): (f32, [f32; 3]),
    (c3, p3): (f32, [f32; 3]),
    count: f32,
) -> [f32; 3] {
    let lerp3 = |a: [f32; 3], b: [f32; 3], t: f32| {
        [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
    };
    let t = |num: f32, den: f32| if den.abs() > f32::EPSILON { num / den } else { 0.5 };

    let a1 = lerp3(p0, p1, t(count - c0, c1 - c0));
    let a2 = lerp3(p1, p2, t(count - c1, c2 - c1));
    let a3 = lerp3(p2, p3, t(count - c2, c3 - c2));
    let b1 = lerp3(a1, a2, t(count - c0, c2 - c0));
    let b2 = lerp3(a2, a3, t(count - c1, c3 - c1));
    lerp3(b1, b2, t(count - c1, c2 - c1))
}

/// Phantom control point for a spline endpoint: reflects `p1` through `p0`
/// so the curve still has a well-defined tangent at the first/last keyframe.
fn phantom_point(p0: [f32; 3], p1: [f32; 3]) -> [f32; 3] {
    [2.0 * p0[0] - p1[0], 2.0 * p0[1] - p1[1], 2.0 * p0[2] - p1[2]]
}
fn phantom_count(c0: f32, c1: f32) -> f32 {
    2.0 * c0 - c1
}
```

`CameraTrack` 側の評価 (3.6 の急加速診断とも共有する「区間特定」を分離する):

```rust
/// Index of the segment `[i, i+1)` containing `count`, plus the endpoints and
/// their phantom neighbors, or `None` if fewer than 2 keyframes exist.
struct Segment<'a> {
    i: usize,
    p: [(f32, [f32; 3]); 4], // (count, eye) for i-1(phantom), i, i+1, i+2(phantom)
    kf1: &'a CameraKeyframe,
    kf2: &'a CameraKeyframe,
}

impl CameraTrack {
    /// `O(log n)` binary search over `keyframes` (sorted by count, 4章).
    fn locate_segment(&self, count: f32) -> Option<Segment<'_>> {
        let n = self.keyframes.len();
        if n < 2 {
            return None;
        }
        let count = count.clamp(self.keyframes[0].count, self.keyframes[n - 1].count);
        // partition_point finds the first keyframe with count > target, i.e.
        // one past the left endpoint of the containing segment.
        let i = self.keyframes.partition_point(|k| k.count <= count).saturating_sub(1).min(n - 2);
        let (kf1, kf2) = (&self.keyframes[i], &self.keyframes[i + 1]);
        let p0 = if i == 0 {
            (phantom_count(kf1.count, kf2.count), phantom_point(kf1.eye, kf2.eye))
        } else {
            (self.keyframes[i - 1].count, self.keyframes[i - 1].eye)
        };
        let p3 = if i + 2 < n {
            (self.keyframes[i + 2].count, self.keyframes[i + 2].eye)
        } else {
            (phantom_count(kf2.count, kf1.count), phantom_point(kf2.eye, kf1.eye))
        };
        Some(Segment { i, p: [p0, (kf1.count, kf1.eye), (kf2.count, kf2.eye), p3], kf1, kf2 })
    }
}
```

### 3.5 姿勢（向き）とロールの補間

位置とは独立に、**向き**はセグメント両端の2キーフレームだけを線形時間 `t` で slerp する
（スプラインで4点を混ぜるのは位置のみ。回転をスプラインで混ぜると等角速度から外れやすく、
実務上は「両端を time-linear slerp」で十分かつ扱いやすいという判断 — 9章に代替案として残す）。
ロールは常に別スカラーとして線形補間し、slerp 後の forward 軸まわりに追加回転として適用する
（Follow 時も同じ経路を通るよう、向きの決定を「base 姿勢 → ロール適用」の2段に固定する）。

```rust
fn segment_t(kf1: &CameraKeyframe, kf2: &CameraKeyframe, count: f32) -> f32 {
    let span = kf2.count - kf1.count;
    if span.abs() > f32::EPSILON { ((count - kf1.count) / span).clamp(0.0, 1.0) } else { 0.0 }
}

const WORLD_UP: [f32; 3] = [0.0, 1.0, 0.0];

/// `base` is the un-rolled look-at orientation; `Follow` mode (3.7) builds it
/// from the runtime subject instead of `kf1`/`kf2`'s authored `target`.
fn resolve_pose(eye: [f32; 3], base_target: [f32; 3], roll_rad: f32, fov_y_rad: f32, near: f32, far: f32) -> CameraPose {
    let base = Quat::look_at(eye, base_target, WORLD_UP);
    let forward = base.rotate_vector([0.0, 0.0, -1.0]);
    let up0 = base.rotate_vector(WORLD_UP);
    let rolled_up = Quat::from_axis_angle(forward, roll_rad).rotate_vector(up0);
    CameraPose {
        eye,
        target: [eye[0] + forward[0], eye[1] + forward[1], eye[2] + forward[2]],
        up: rolled_up,
        fov_y_rad,
        near,
        far,
    }
}
```

Explicit モードでの向き決定は、両端の `look_at` クォータニオンを slerp してから forward を取り直す
（`target` を単純に線形補間すると、カメラ位置が大きく動く区間で不自然な首振りが起きるため）。

```rust
fn explicit_base_target(eye: [f32; 3], kf1: &CameraKeyframe, kf2: &CameraKeyframe, t: f32) -> [f32; 3] {
    let q1 = Quat::look_at(kf1.eye, kf1.target, WORLD_UP);
    let q2 = Quat::look_at(kf2.eye, kf2.target, WORLD_UP);
    let forward = Quat::slerp(q1, q2, t).rotate_vector([0.0, 0.0, -1.0]);
    [eye[0] + forward[0], eye[1] + forward[1], eye[2] + forward[2]]
}
```

`CameraTrack::evaluate`（`LookMode::Explicit` のみ。`Follow` は 3.7 で合成する）:

```rust
impl CameraTrack {
    pub fn evaluate(&self, doc: &Document, cache: &mut CameraFollowCache, global_count: f32) -> Option<CameraPose> {
        self.evaluate_with_hint(doc, cache, global_count, &mut CameraEvalHint::default())
    }

    /// `hint` remembers the last segment index so monotonic playback (scrubbing
    /// forward/backward by small steps) skips the binary search — see 5章.
    /// Random access (timeline scrubbing to an arbitrary count) still returns
    /// the exact same result as `evaluate`, just falling back to `locate_segment`.
    pub fn evaluate_with_hint(
        &self,
        doc: &Document,
        cache: &mut CameraFollowCache,
        global_count: f32,
        hint: &mut CameraEvalHint,
    ) -> Option<CameraPose> {
        if self.keyframes.len() == 1 {
            let k = &self.keyframes[0];
            return Some(resolve_pose(k.eye, k.target, k.roll_rad, k.fov_y_rad, self.near, self.far));
        }
        let seg = self.locate_segment_hinted(global_count, hint)?;
        let eye = catmull_rom_nonuniform(seg.p[0], seg.p[1], seg.p[2], seg.p[3], global_count.clamp(seg.p[1].0, seg.p[2].0));
        let eye = self.clamp_bounds(doc, eye);
        let t = segment_t(seg.kf1, seg.kf2, global_count);
        let roll = seg.kf1.roll_rad + (seg.kf2.roll_rad - seg.kf1.roll_rad) * t;
        let fov = seg.kf1.fov_y_rad + (seg.kf2.fov_y_rad - seg.kf1.fov_y_rad) * t;
        let base_target = match &self.look {
            LookMode::Explicit => explicit_base_target(eye, seg.kf1, seg.kf2, t),
            LookMode::Follow { target, damping } => {
                let subject = damped_follow_point(doc, cache, target, *damping, global_count);
                [subject.x, eye[1], subject.y] // look toward turf height under the subject; see 3.7
            }
        };
        Some(resolve_pose(eye, base_target, roll, fov, self.near, self.far))
    }

    fn clamp_bounds(&self, doc: &Document, eye: [f32; 3]) -> [f32; 3] {
        match self.bounds {
            BoundsMode::Free => eye,
            BoundsMode::Enforced => CameraBounds::from_grid(&doc.grid, 15.0, 120.0).clamp(eye),
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct CameraEvalHint {
    last_segment: Option<usize>,
}
```

`locate_segment_hinted` は `locate_segment`（3.4）と同じ結果を返すが、`hint.last_segment` が
まだ現在の count を含む区間なら `O(1)` で確認だけして返し、外れていれば通常の二分探索にフォールバックして
`hint` を更新する。**評価結果は `hint` の有無に関わらず必ず一致する**（7章のプロパティテストで固定する）。

### 3.6 急加速の検出

`MEDIA_PIPELINE.md` P2「カメラ急加速・衝突診断」に対応する。フレームレート非依存にするため、
count ではなく**実秒**での二階差分（＝加速度、単位: field-unit/s²）を見る。毎フレームではなく
書き出し前の事前スキャン（B-3のジョブ基盤で非同期実行する想定、doc 40 参照）として設計する。

```rust
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CameraAccelerationEvent {
    pub at_count: f32,
    /// Magnitude of the eye's acceleration, field-units per second squared.
    pub accel_units_per_s2: f32,
}

/// Reused across scans so repeated diagnostics runs allocate once (00-conventions #3).
#[derive(Default)]
pub struct AccelScratch {
    samples: Vec<(f32, [f32; 3])>, // (seconds, eye), reused buffer
    events: Vec<CameraAccelerationEvent>,
}

/// Samples the eye path at `samples_per_count` fixed points per count across
/// the track's keyframe range, converts count to seconds via `tempo`, and
/// flags any point whose discrete second derivative exceeds `max_accel`.
pub fn scan_acceleration<'a>(
    track: &CameraTrack,
    doc: &Document,
    tempo: &TempoMap,
    cache: &mut CameraFollowCache,
    samples_per_count: u8,
    max_accel: f32,
    scratch: &'a mut AccelScratch,
) -> &'a [CameraAccelerationEvent] {
    scratch.samples.clear();
    scratch.events.clear();
    let Some((start, end)) = track.count_range() else { return &scratch.events };
    let step = 1.0 / f32::from(samples_per_count.max(1));
    let mut hint = CameraEvalHint::default();
    let mut count = start;
    while count <= end {
        if let Some(pose) = track.evaluate_with_hint(doc, cache, count, &mut hint) {
            scratch.samples.push((tempo.seconds_at(count), pose.eye));
        }
        count += step;
    }
    for w in scratch.samples.windows(3) {
        let ((t0, p0), (t1, p1), (t2, p2)) = (w[0], w[1], w[2]);
        let dt1 = (t1 - t0).max(1e-4);
        let dt2 = (t2 - t1).max(1e-4);
        let a = second_derivative(p0, p1, p2, dt1, dt2);
        let mag = (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt();
        if mag > max_accel {
            scratch.events.push(CameraAccelerationEvent { at_count: t1, accel_units_per_s2: mag });
        }
    }
    &scratch.events
}

fn second_derivative(p0: [f32; 3], p1: [f32; 3], p2: [f32; 3], dt1: f32, dt2: f32) -> [f32; 3] {
    // Non-uniform-sample second derivative (central difference generalization).
    let mut out = [0.0; 3];
    for k in 0..3 {
        let v_prev = (p1[k] - p0[k]) / dt1;
        let v_next = (p2[k] - p1[k]) / dt2;
        out[k] = (v_next - v_prev) / ((dt1 + dt2) * 0.5);
    }
    out
}
```

*(3.6 の擬似コードは意図を示すための簡略版であり、実装タスク T9（8章）で `second_derivative` に一本化して
書き直す。急加速イベントは 3D プレビューのタイムライン上に警告マーカーとして表示する（3.11）。)*

### 3.7 追従（Follow）

`FollowTarget` は `doc 15` が定義する `SectionId` へ踏み込まず、**演者ID集合**として境界越しに参照する。
セクション重心が欲しい場合は呼び出し側（doc 15 のセクション定義を知っている層）がそのセクションに属する
`PerformerId` を集めて `Group` に渡す。単一演者追従は要素数1の `Group` として表現する。

```rust
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum FollowTarget {
    /// One or more performers; the centroid of their positions is followed.
    /// A length-1 group is "follow this one performer"; a longer group
    /// (e.g. every performer in a section) is "follow this section's centroid".
    /// Resolving *which* performers belong to a section is doc 15's job.
    Group(Vec<PerformerId>),
    Ensemble,
    Fixed(Point),
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct FollowDamping {
    /// Exponential lag time constant, seconds. `0.0` = rigid look-at (no lag).
    pub time_constant_seconds: f32,
}
```

**決定論の要件**（「同じ (Document, config, count) からは常に同じカメラ行列」）を、追従の「遅れ」と
両立させる設計が本節の核心。ダンピングを毎フレーム状態を持つ IIR フィルタ（前フレームの出力に依存）
で実装すると、count だけからは値が決まらなくなる（再生順序・スクラブ操作の履歴に依存してしまう）ため
**禁止**。代わりに、生の追従対象位置が「セット区間ごとに区分アフィン」（`positions_at` が線形補間である
現状、`DESIGN_GAPS.md` A-2 の曲線ルートが入った場合は近似になる — 9章に記載）という性質を使い、
一次遅れ系のランプ応答の**厳密な閉形式解**を、count 0 から現在の count まで区間ごとに積み上げて
毎回ゼロから計算する（`TempoMap::seconds_at` が毎回イベント列を先頭から積算するのと同じ流儀）。

一次遅れ系 `dy/dt = (x(t) - y(t)) / tau` に対し、区間内で `x(t) = x0 + slope * (t - t0)`（アフィン）なら、

```
y(t) = x(t) - slope * tau + (y(t0) - x(t0) + slope * tau) * exp(-(t - t0) / tau)
```

という閉形式（ランプ入力の定常遅れが `slope * tau` になる、教科書的な一次遅れ系の解）が成り立つ。
これを区間（＝セット境界）ごとに `y(t0)` を引き継ぎながら合成する。

```rust
/// Per-set-boundary centroid of a `FollowTarget`, invalidated only when the
/// document's formation data actually changes (not every frame). The app
/// layer bumps `revision` whenever positions in `sets` are edited.
pub struct CameraFollowCache {
    revision: u64,
    /// One entry per set boundary (`sets.len()` long): the raw (undamped)
    /// target position at the *start* of that set.
    boundary_targets: Vec<Point>,
}

impl CameraFollowCache {
    fn refresh(&mut self, doc: &Document, target: &FollowTarget, doc_revision: u64) {
        if self.revision == doc_revision && self.boundary_targets.len() == doc.sets.len() {
            return;
        }
        self.boundary_targets.clear();
        self.boundary_targets.extend(doc.sets.iter().map(|_| Point::default()));
        for (set_index, out) in self.boundary_targets.iter_mut().enumerate() {
            *out = raw_target_at_set_start(doc, target, set_index);
        }
        self.revision = doc_revision;
    }
}

fn raw_target_at_set_start(doc: &Document, target: &FollowTarget, set_index: usize) -> Point {
    match target {
        FollowTarget::Fixed(p) => *p,
        FollowTarget::Ensemble => centroid_of(doc.sets[set_index].positions.iter().copied()),
        FollowTarget::Group(ids) => {
            let positions = doc.performers.iter().enumerate()
                .filter(|(_, perf)| ids.contains(&perf.id))
                .filter_map(|(i, _)| doc.sets[set_index].positions.get(i).copied());
            centroid_of(positions)
        }
    }
}

fn centroid_of(points: impl Iterator<Item = Point>) -> Point {
    let (mut n, mut sx, mut sy) = (0u32, 0.0f32, 0.0f32);
    for p in points {
        n += 1;
        sx += p.x;
        sy += p.y;
    }
    if n == 0 { Point::default() } else { Point { x: sx / n as f32, y: sy / n as f32 } }
}

/// Closed-form damped follow position at `global_count`, recomputed fresh
/// from count 0 every call (no per-frame accumulated state — determinism, 4章).
fn damped_follow_point(
    doc: &Document,
    cache: &mut CameraFollowCache,
    target: &FollowTarget,
    damping: FollowDamping,
    global_count: f32,
) -> Point {
    cache.refresh(doc, target, doc_revision_of(doc));
    let tau = damping.time_constant_seconds.max(0.0);
    if tau <= f32::EPSILON || cache.boundary_targets.is_empty() {
        let (set_index, local) = doc.locate_count(global_count);
        return raw_target_progress(doc, target, set_index, local);
    }
    // Walk set boundaries from 0 up to global_count, carrying y(t0) forward.
    let mut y = cache.boundary_targets[0];
    let mut prev_count = 0.0f32;
    let (final_set, _) = doc.locate_count(global_count);
    for set_index in 0..=final_set.min(cache.boundary_targets.len().saturating_sub(1)) {
        let set_len = f32::from(doc.sets[set_index].counts).max(1.0);
        let seg_end_count = (prev_count + set_len).min(global_count);
        let x0 = cache.boundary_targets[set_index];
        let x1 = cache.boundary_targets.get(set_index + 1).copied().unwrap_or(x0);
        let slope = Point { x: (x1.x - x0.x) / set_len, y: (x1.y - x0.y) / set_len };
        let dt = (seg_end_count - prev_count).max(0.0);
        y = damped_ramp(x0, slope, y, dt, tau);
        prev_count = seg_end_count;
        if prev_count >= global_count {
            break;
        }
    }
    y
}

fn damped_ramp(x0: Point, slope: Point, y0: Point, dt: f32, tau: f32) -> Point {
    let decay = (-dt / tau).exp();
    let axis = |x0: f32, slope: f32, y0: f32| {
        let xt = x0 + slope * dt;
        xt - slope * tau + (y0 - x0 + slope * tau) * decay
    };
    Point { x: axis(x0.x, slope.x, y0.x), y: axis(x0.y, slope.y, y0.y) }
}
```

*(`raw_target_progress` は 3.7 冒頭の `raw_target_at_set_start` と同じロジックをセット内 progress
込みで評価する薄いラッパで、`tau == 0` のときの「遅延なし・厳密に対象を注視」経路に使う。
`doc_revision_of` は doc 40/41 が持つ予定のドキュメント改訂カウンタへの前方参照 — 現状は
呼び出し側が明示的にキャッシュを invalidate する `CameraFollowCache::invalidate()` で代替できる。)*

**追従とキーフレームの合成規則**（まとめ）:

1. `eye` の位置は常にキーフレームのスプライン（3.4）が決める。Follow は位置に影響しない
   （＝「対象を追う」は常にカメラの向きの話であり、ドリー軌道はオペレーターが引き続き設計する）。
2. 向きの基準点（`base_target`）は `LookMode::Explicit` ならキーフレームの `target` を slerp（3.5）、
   `LookMode::Follow` なら減衰追従した対象の水平位置＋カメラと同じ高さ（水平を保つ）を使う。
3. `roll_rad` と `fov_y_rad` は `LookMode` に関わらず常にキーフレームから線形補間する。
4. 境界制約（3.8）は `LookMode` に関わらず `eye` にのみ適用する。

### 3.8 フィールド境界（ソフト制約）

```rust
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CameraBounds {
    pub min: [f32; 3],
    pub max: [f32; 3],
}

impl CameraBounds {
    /// `margin` extends past the field edge (stands are outside the field
    /// rectangle); `max_height` bounds how high a crane/drone shot may go.
    pub fn from_grid(grid: &GridConfig, margin: f32, max_height: f32) -> Self {
        CameraBounds {
            min: [-margin, 0.5, -margin],
            max: [grid.width + margin, max_height, grid.height + margin],
        }
    }

    /// Component-wise clamp — "soft" in the sense that it never rejects a
    /// keyframe (validation, 3.3, only rejects NaN/degenerate input); it just
    /// silently pulls the evaluated eye back inside bounds every frame.
    pub fn clamp(&self, eye: [f32; 3]) -> [f32; 3] {
        [
            eye[0].clamp(self.min[0], self.max[0]),
            eye[1].clamp(self.min[1], self.max[1]),
            eye[2].clamp(self.min[2], self.max[2]),
        ]
    }
}
```

意図的に破る手段は `CameraTrack::bounds: BoundsMode::Free`（3.3）。UI では既定 `Enforced` を
チェックボックス等で明示的に外す操作として提供する（3.11）。`min[1] = 0.5` は「地面下へ潜らない」
既定値で、演出上ターフレベルの超低空ショットが欲しい場合も `Free` で意図的に外す。

### 3.9 マルチカメラとカット

```rust
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct CameraCut {
    /// Global count at which `camera` becomes the active/program feed.
    pub at: f32,
    pub camera: CameraId,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct CameraProgram {
    pub tracks: Vec<CameraTrack>,
    /// Sorted ascending by `at`, unique `at` (same duplicate-replace rule as
    /// `CameraTrack::insert_keyframe`, 6章).
    cuts: Vec<CameraCut>,
    next_camera_id: u32,
}

impl CameraProgram {
    /// Allocates a fresh `CameraId` that has never been issued before and
    /// never will be reused (matches doc 10's `IdAllocator` policy: identity
    /// is never recycled, even after the track is removed). Returns `None`
    /// instead of wrapping or panicking once the `u32` space is exhausted —
    /// 00-conventions.md's "パニック禁止経路" applies to ID allocation too.
    /// Replaces the earlier, unchecked `alloc_camera_id` (6章).
    pub fn try_alloc_camera_id(&mut self) -> Option<CameraId> {
        let id = self.next_camera_id;
        let next = id.checked_add(1)?;
        self.next_camera_id = next;
        Some(CameraId(id))
    }

    pub fn cuts(&self) -> &[CameraCut] {
        &self.cuts
    }

    pub fn set_cut(&mut self, cut: CameraCut) -> Option<CameraCut> {
        match self.cuts.iter().position(|c| c.at == cut.at) {
            Some(i) => Some(std::mem::replace(&mut self.cuts[i], cut)),
            None => {
                self.cuts.push(cut);
                self.cuts.sort_by(|a, b| a.at.total_cmp(&b.at));
                None
            }
        }
    }

    /// Removes the cut at `at`, returning it. Symmetric with `set_cut`, so an
    /// `Edit::RemoveCameraCut` can be inverted with a single `SetCameraCut`.
    pub fn remove_cut(&mut self, at: f32) -> Option<CameraCut> {
        let i = self.cuts.iter().position(|c| c.at == at)?;
        Some(self.cuts.remove(i))
    }

    /// Removes every cut pointing at `camera`, returning them in ascending
    /// `at` order. Needed so `RemoveCameraTrack` can be undone: without a way
    /// to recover the cuts a track deletion silently orphaned, restoring the
    /// track on Undo would leave the cut list short (4章, 6章).
    pub fn drain_cuts_for(&mut self, camera: CameraId) -> Vec<CameraCut> {
        let mut drained = Vec::new();
        self.cuts.retain(|c| {
            if c.camera == camera {
                drained.push(*c);
                false
            } else {
                true
            }
        });
        drained
    }

    /// Re-normalizes keyframe/cut ordering and drops any keyframe that fails
    /// `CameraKeyframe::validate` (6章: "同一countに複数キーフレーム" / NaN
    /// input from an untrusted file). Called once from the document load path
    /// (doc 10/41's `Document::from_repr`) — never on the hot per-frame path.
    pub fn normalize_after_load(&mut self) {
        for track in &mut self.tracks {
            let raw = std::mem::take(track.keyframes_mut());
            for keyframe in raw {
                if keyframe.validate().is_ok() {
                    let _ = track.insert_keyframe(keyframe);
                }
            }
        }
        self.cuts.retain(|c| c.at.is_finite() && self.tracks.iter().any(|t| t.id == c.camera));
        self.cuts.sort_by(|a, b| a.at.total_cmp(&b.at));
        self.cuts.dedup_by(|a, b| { if a.at == b.at { *b = *a; true } else { false } });
    }

    /// `O(log n)` over `cuts`. `None` when there are no cuts yet (nothing is
    /// "on program"; the app falls back to whatever track it was already showing).
    pub fn active_camera_at(&self, global_count: f32) -> Option<CameraId> {
        let i = self.cuts.partition_point(|c| c.at <= global_count);
        self.cuts.get(i.checked_sub(1)?).map(|c| c.camera)
    }

    /// The single evaluation entry point video export (doc 31) and the 3D
    /// preview (doc 22) both call.
    pub fn evaluate(&self, doc: &Document, tempo: &TempoMap, global_count: f32) -> Option<CameraPose> {
        let id = self.active_camera_at(global_count)?;
        let track = self.tracks.iter().find(|t| t.id == id)?;
        let mut cache = CameraFollowCache::default(); // see 5章 for why this should be caller-owned in the hot path
        track.evaluate(doc, &mut cache, global_count)
    }
}
```

`normalize_after_load` は `CameraTrack::keyframes_mut(&mut self) -> &mut Vec<CameraKeyframe>`
（`pub(crate)`、3.3 の `keyframes()` と対の内部アクセサ）を使って直接差し替え、その後
`insert_keyframe` を1件ずつ通し直すことで「後勝ち・昇順・重複なし」を保証する（不変条件2）。

`evaluate` はシグネチャの説明用に自前で `CameraFollowCache` を作っているが、**実際の呼び出し側
（プレビューのフレームループ・動画書き出しのワーカー）はキャッシュを毎フレーム使い回す**必要がある
（5章）。そのための低レベル API として `CameraProgram::evaluate_with(doc, tempo, count, cache, hint)` も
用意し、`evaluate` はそれを一時キャッシュで呼ぶ薄いラッパにする。

プレビューのスクラブは「現在の再生 count → `active_camera_at` → その `CameraTrack::evaluate_with_hint`」
をそのまま呼べば良く、カット前後のトラックを跨いだヒントの使い回しは行わない（トラックが変わったら
`hint` をリセットする、程度の単純な規則で十分 — カット頻度は count timeline に対して疎なため）。

### 3.10 ショットプリセット

既存の `Camera::audience_view` / `press_box` / `overhead`（camera.rs 155–186行目）を単発キーフレーム化する
ラッパと、新規の「エンドゾーン」「ドローン軌道」を追加する。1操作でキーフレーム列を生成する。

```rust
pub fn preset_wide(grid: &GridConfig, at_count: f32) -> CameraKeyframe {
    keyframe_from_camera(&Camera::press_box(grid), at_count)
}

pub fn preset_audience(grid: &GridConfig, at_count: f32) -> CameraKeyframe {
    keyframe_from_camera(&Camera::audience_view(grid), at_count)
}

pub fn preset_overhead(grid: &GridConfig, at_count: f32) -> CameraKeyframe {
    keyframe_from_camera(&Camera::overhead(grid), at_count)
}

/// Close-up from the near sideline, framing `subject` (e.g. a soloist's
/// current position) rather than the field center.
pub fn preset_tight(grid: &GridConfig, subject: crate::Point, at_count: f32) -> CameraKeyframe {
    let cam = Camera {
        target: field_to_world(subject, 1.6),
        yaw: std::f32::consts::PI,
        pitch: 0.12,
        distance: 12.0,
        fov_y_rad: std::f32::consts::FRAC_PI_4,
        ..Camera::default()
    };
    keyframe_from_camera(&cam, at_count)
}

/// Low, centered behind the back end zone — the "how the drill reads
/// end-to-end" shot Pyware calls out as a standard angle.
pub fn preset_endzone(grid: &GridConfig, at_count: f32) -> CameraKeyframe {
    let cam = Camera {
        target: field_center(grid),
        yaw: std::f32::consts::FRAC_PI_2,
        pitch: 0.08,
        distance: grid.height.max(1.0) * 1.4,
        ..Camera::default()
    };
    keyframe_from_camera(&cam, at_count)
}

/// A ring of keyframes orbiting the field center at a fixed height, meant to
/// be inserted as a whole `CameraTrack`'s keyframes (a drone fly-around).
pub fn preset_drone_orbit(
    grid: &GridConfig,
    start_count: f32,
    end_count: f32,
    revolutions: f32,
    sample_count: usize,
) -> Vec<CameraKeyframe> {
    let sample_count = sample_count.max(2);
    (0..sample_count)
        .map(|i| {
            let frac = i as f32 / (sample_count - 1) as f32;
            let cam = Camera {
                target: field_center(grid),
                yaw: frac * revolutions * std::f32::consts::TAU,
                pitch: 0.9,
                distance: fit_distance_pub(grid, std::f32::consts::FRAC_PI_3) * 0.8,
                ..Camera::default()
            };
            keyframe_from_camera(&cam, start_count + (end_count - start_count) * frac)
        })
        .collect()
}

fn keyframe_from_camera(cam: &Camera, at_count: f32) -> CameraKeyframe {
    CameraKeyframe { count: at_count, eye: cam.position(), target: cam.target, roll_rad: 0.0, fov_y_rad: cam.fov_y_rad }
}
```

`field_center` / `fit_distance` は既存の private ヘルパー（188–197行目）。`fit_distance` は
`preset_drone_orbit` からも呼べるよう `pub(crate)` へ格上げする（`fit_distance_pub` は本書中の
仮称、実装時は既存名のまま可視性だけ変える）。

### 3.11 UI 要件（doc 43 への引き渡し仕様）

- **count track 上のカメラレーン**: タイムライン（既存の再生範囲バーと同じ count 軸）に、トラックごとに
  1行、キーフレームをダイヤモンド型マーカーで表示する。カット点（3.9）は別レーンに三角マーカー。
- **ドラッグ**: キーフレームの水平ドラッグは `count` を変更する（`CameraTrack::remove_keyframe` →
  新 `count` で `insert_keyframe`、というペアをまとめて1つの `Edit::MoveCameraKeyframe` にする、3.12）。
  既定でカウント整数へスナップ（`GridConfig::snap` と同様の吸着 UX、MEDIA_PIPELINE.md の
  「編集シークはcount吸着」方針に合わせる）。Shift 等の修飾キーで非吸着。
  垂直ドラッグは無し（トラック間移動は明示的な「別トラックへ移動」操作にする — count レーンの
  垂直位置は「どのトラックか」を表すため、ドラッグでの意図しないトラック変更を防ぐ）。
- **急加速警告**（3.6）: 該当 count にトラックレーン上へ警告アイコンを重ね、色だけに頼らない表示
  にする（`PRODUCT_QUALITY.md`「警告は色だけに依存せず文字と形でも示す」を継承）。
- **プレビュー中のスクラブ**: 再生ヘッドを動かすたびに `CameraProgram::active_camera_at` → 該当
  `CameraTrack::evaluate_with_hint` を呼ぶ。ドラッグ中は 60fps 相当で連続呼び出しになるため、
  `CameraEvalHint` と `CameraFollowCache` をタイムラインパネルの状態として保持し、フレームごとに
  再構築しない（5章の性能要件と直結）。
- **境界逸脱の表示**: `BoundsMode::Enforced` でクランプが実際に発生した count 区間は、レーン上に
  細い斜線ハッチングで示す（「意図せずクランプされている」ことを発見可能にする）。
- **ショットプリセット**: ツールバーに5ボタン（ワイド/寄り/エンドゾーン/プレスボックス/ドローン軌道）。
  現在の再生ヘッド count に1キーフレームを挿入する（ドローン軌道のみ、範囲選択が必要なため
  「範囲選択 → ドローン軌道」の2段操作にする）。

### 3.12 `Document` 統合と `Edit` enum への接続

`Document` への統合と `Edit` enum のバリアント設計は、`docs/design/10-document-model.md`
（ドキュメントモデルの所有者、以下「doc 10」）が最終決定している。本書はそれを受けて、
以前ここに書いていた独自の `Edit` バリアント草案と橋渡し案を**撤回**する。

`Document` フィールド（doc 10 §3.2 で確定、`pub(in crate::document) camera_program: CameraProgram`
としてカプセル化され、`Document::camera_program() -> &CameraProgram` 経由で参照専用に公開される）:

```rust
pub struct Document {
    // ...doc 10 が確定させた他フィールド...
    pub(in crate::document) camera_program: camera::CameraProgram,
}
```

`Edit` バリアントは doc 10 §3.3.3.3 が確定させた、以下10個である（`Edit` 本体・payload 型は doc 10 が
所有し、名称・フィールドとも変更しない — 本書からは値として参照するのみ）。

```rust
pub enum Edit {
    // ...doc 10 が定義する他の変異...
    InsertCameraTrack(Box<InsertCameraTrack>),
    RemoveCameraTrack(RemoveCameraTrack),
    RenameCameraTrack(Box<RenameCameraTrack>),
    SetCameraLook(Box<CameraLookChange>),
    SetCameraBoundsMode(CameraBoundsChange),
    InsertCameraKeyframe(Box<InsertCameraKeyframe>),
    RemoveCameraKeyframe(RemoveCameraKeyframe),
    MoveCameraKeyframe(MoveCameraKeyframe),
    SetCameraCut(SetCameraCut),
    RemoveCameraCut(RemoveCameraCut),
}
```

これらの payload（`CameraTrackTemplate` / `InsertCameraTrack` / `RemoveCameraTrack` /
`RenameCameraTrack` / `CameraLookChange` / `CameraBoundsChange` / `InsertCameraKeyframe` /
`RemoveCameraKeyframe` / `MoveCameraKeyframe` / `SetCameraCut` / `RemoveCameraCut`）は
doc 10 §3.3.3.3 が「`apply` が文書から読む」方針（doc 10 の決定 A）に合わせ、`before`/`after` を
持たない形で確定させている。本書側が事前に用意すべきだったのは**置き換えられた値を返す**
メソッド群だけで、それは 3.3（`insert_keyframe` / `remove_keyframe`）と 3.9
（`set_cut` / `remove_cut` / `drain_cuts_for` / `try_alloc_camera_id`）に揃っている。
`Edit::apply` はこれらをそのまま呼び、返り値から逆操作（doc 10 §3.3.3.3 の対応表）を機械的に組み立てる。

**撤回した橋渡し案**: 以前の版は `Edit` 本体が着地するまでの暫定として
`Document::edit_camera(f: impl FnOnce(&mut CameraProgram) -> R) -> R` という、任意のクロージャで
`CameraProgram` を書き換えられるラッパを提案していた。doc 10 のレビューにより、これは
00-conventions.md の不変条件1（「`Document` の変更は必ず `Edit` コマンドを通る。UI から直接
フィールドを書き換えない」）に反する抜け道であると指摘され、**不採用**とする。
`drill-app` はカメラの変更も他のあらゆる変更と同様、最初から doc 10 の `Edit`（上記10バリアント）
だけを経由する。過渡期のショートカットは用意しない。

## 4. 不変条件

1. **決定論**: `CameraTrack::evaluate` / `evaluate_with_hint` / `CameraProgram::evaluate` は
   `(track/program の内容, Document, count)` のみに依存し、それ以外の状態（過去に何 count を
   評価したか、`hint`/`cache` の中身）には依存しない。`hint`/`cache` は結果を変えない純粋な高速化。
   → 7章のプロパティテストで「`hint` あり/なしの結果が任意の count 列で一致する」ことを固定する。
2. **キーフレームの一意性**: `CameraTrack::keyframes()` は常に `count` 昇順・重複なし。
   `insert_keyframe` は同一 `count` を新しい値で置き換える（追加しない）。
3. **有限性**: `CameraKeyframe::validate` を通過したキーフレームのみ `CameraTrack` に入る。
   `eye`/`target`/`roll_rad`/`fov_y_rad`/`count` はすべて有限、かつ `eye != target`
   （閾値 `1e-8`、3.3）。
4. **スプラインの端点通過**: `evaluate(..., count = keyframes[i].count)` は
   （`Follow` でない限り）`keyframes[i]` そのものの `eye`/`roll_rad`/`fov_y_rad` と一致する
   （Catmull-Rom・slerp・線形補間はいずれも端点で厳密に元の値へ収束する）。
5. **カット列の整合**: `CameraProgram::cuts()` は `at` 昇順・重複なし。`active_camera_at` は
   `cuts` が空なら `None`、最初のカットより前の count でも `None`（「まだ何もオンプログラムでない」）。
6. **境界のソフトさ**: `BoundsMode::Enforced` は `eye` の各成分を `CameraBounds` 内に留めるのみで、
   キーフレームの保存値そのものは変更しない（クランプは評価時にのみ効く、保存データは非破壊）。
7. **ヒープ確保ゼロ**: `evaluate_with_hint` と `damped_follow_point` は、`CameraFollowCache` が
   既に対象ドキュメントのリビジョンで温まっていれば、内部でヒープ確保しない
   （`CameraFollowCache::boundary_targets` の再確保は `refresh` がキャッシュミス時のみ行う）。

## 5. 性能

基準規模（演者1,000人/セット64/総カウント2,048）での取り分:

| 操作 | 頻度 | 想定コスト | 16.6ms 予算内の位置づけ |
|---|---|---|---|
| `CameraProgram::active_camera_at` | 毎フレーム | `O(log カット数)`、カット数は数十程度想定 → 無視できる | プレビュー: 予算の対象外に近い |
| `CameraTrack::evaluate_with_hint`（`hint` 命中） | 毎フレーム | 定数（クォータニオン演算数個 + Catmull-Rom 1回） → 数百ns | プレビュー全体で **0.05ms未満** を目安に割り当てる |
| `CameraTrack::locate_segment`（`hint` 不命中、スクラブ） | スクラブ操作時のみ | `O(log キーフレーム数)`、キーフレーム数は1トラックあたり数十〜数百想定 | 操作系イベント時のみなので 16.6ms 予算の対象外（次フレームには `hint` が再び命中する） |
| `CameraFollowCache::refresh`（キャッシュミス、Follow時のみ） | ドキュメント編集直後の1回 | `O(セット数 × 演者数)` = 64 × 1,000 = 64,000 回の加算 → **1ms未満** | 編集操作の一部として許容（再生・プレビューのフレーム予算には乗らない） |
| `damped_follow_point`（キャッシュ温まり済み） | 毎フレーム（Follow使用時） | `O(セット数)` = 64 回の `exp` 呼び出し → **数μs** | 無視できる |
| `scan_acceleration` | 書き出し前1回、または明示的な診断実行時 | `O(range_counts × samples_per_count)`。2,048カウント × 2サンプル/カウント = 4,096回の姿勢評価 → **数ms**（動画エンコード全体の秒〜分オーダーに対して無視できる） | ジョブ基盤（doc 40）でバックグラウンド実行、UIスレッドをブロックしない |

上限規模（演者4,000人/セット256）では `CameraFollowCache::refresh` が 256 × 4,000 = 1,024,000 回の加算
（数ms程度）まで悪化しうるが、これは**編集直後の1回限り**であり、毎フレームでは発生しない
（`revision` が変わらない限り `refresh` は即座に return する、3.7）。この点を該当箇所（3.7 冒頭）に明記済み。

メモリ: `CameraKeyframe` は32バイト程度（`f32`×7 + パディング）。1トラックあたり数百キーフレーム
でも数十KB、`Document` 全体としては無視できる規模。

## 6. 失敗モードと安全性

信頼できない入力（他人のプロジェクトファイル）に対する防御を列挙する。

| 壊れ方 | 対処 |
|---|---|
| 同一 `count` に複数キーフレームを持つ JSON（手編集・壊れたファイル） | デシリアライズ後、`CameraTrack` のロード経路で `insert_keyframe` を1件ずつ通し直し、後勝ちで正規化する（`TempoMap::from_changes` と同じ流儀）。読み込み直後に `keyframes()` が昇順・重複なしになることをロード時テストで固定する。 |
| `eye`/`target`/`fov_y_rad` に NaN/Inf | `CameraKeyframe::validate` が `DrillError::InvalidCameraKeyframe` を返す。ロード経路は不正なキーフレームを**スキップして警告**し、ドリル本体は開ける（`PRODUCT_QUALITY.md` の「音声・画像が欠落してもドリル本体を開ける」と同じ思想を適用）。 |
| `eye == target`（注視点とカメラ位置が一致、ゼロベクトルでの正規化） | `validate` が拒否。UI からの入力はキーフレーム保存前に検証し、保存できないようにする。 |
| `FollowTarget::Group` が削除済みの `PerformerId` を含む | `raw_target_at_set_start` は `filter` で単純に無視するため、実在する演者だけで重心を取る。全員削除済みなら `centroid_of` が空扱いで原点を返す — UI 側は「追従対象が0人」を警告表示する（doc 43 側の責務、本書は安全なフォールバック値を返すところまで）。 |
| `CameraCut.camera` が存在しない `CameraId` を指す（トラック削除後にカットが残る） | `CameraProgram::evaluate` は `tracks.iter().find` が `None` を返し、全体として `None`（描画側は直前のポーズを保持するか、既定カメラにフォールバックする、doc 22/31 の責務）。カット自体を消す `Edit::RemoveCameraTrack` の適用時に、参照している `CameraCut` も同時に除去することを不変条件として明記（実装タスク T7）。 |
| キーフレーム1個だけのトラックで `evaluate` を呼ぶ | `evaluate_with_hint` の冒頭で早期リターンし、その1点の姿勢を返す（3.5 の「端点通過」不変条件と一致）。 |
| `tau`（減衰時定数）に負値や NaN | `damped_follow_point` で `tau.max(0.0)` にガードし、`tau <= EPSILON` は「遅延なし」経路へ落とす（`TempoMap::sanitize_bpm` と同じ「壊れた入力は安全な既定へガードする」流儀）。 |
| `CameraBounds::from_grid` に非正の `grid.width`/`height`（`Document::validate` を素通りした壊れたグリッド） | `Document::validate`（lib.rs 312–314行目）が既にグリッド寸法の正数性を検証しているため、`CameraBounds` 構築時点では保証済みという前提を置く。念のため `clamp` 自体は `min > max` でも `f32::clamp` の仕様通りパニックせず`min`側に丸まる実装（`min`,`max` を渡す前に `min = min.min(max)` で正規化）にする。 |
| 10,000回の `Edit` ストレス（`DESIGN_GAPS.md` C-2） | `InsertCameraKeyframe`/`RemoveCameraKeyframe`/`SetCameraCut` を含むランダム `Edit` 列を適用→全 Undo で初期状態と一致すること、を本書のカメラ関連バリアントにも要求する（7章）。 |
| `CameraId` の `u32` 空間が枯渇する（既存カメラトラックが 42 億件、非現実的だが「壊れ方」として明記） | `try_alloc_camera_id`（3.9）が `checked_add` で溢れを検出し `None` を返す。`unwrap`/オーバーフローでのパニックも、release ビルドでのラップアラウンドによる ID 衝突も起きない。`doc 10` の `InsertCameraTrack` はこれを `DrillError` へ変換して呼び出し元へ返す（doc 10 の担当）。 |

パニック禁止経路: `catmull_rom_nonuniform` / `damped_ramp` / `Quat::*` はいずれも `unwrap`/`expect`/添字
パニックを含まない（配列アクセスはすべて固定長 `[f32;3]`/`[f32;4]` の直接インデックスで境界チェック不要）。

## 7. テスト計画

**単体**

- `catmull_rom_nonuniform`: 端点（`count == c1` / `count == c2`）で厳密に `p1`/`p2` を返す。
- 等間隔4点の手計算値で `catmull_rom_nonuniform` の結果を検証（既存 `pathing.rs`/`continuity.rs` の
  「手で計算した期待値と比較する」スタイルを踏襲）。
- `Quat::slerp(a, a, t)` が任意の `t` で `a` を返す。`Quat::slerp(a, b, 0.0) == a`、`(..., 1.0) == b`
  （符号反転を許容した近似比較）。
- `Quat::look_at` からの `rotate_vector([0,0,-1])` が `normalize(target - eye)` に一致する
  （既存 `camera.rs` の `hand_set_camera_projects_predictably` と同じ手法）。
- `CameraKeyframe::validate`: NaN、`eye == target`、FOV範囲外のそれぞれを個別に拒否することを確認。
- `CameraTrack::insert_keyframe` が同一 `count` を置き換えること、`keyframes()` が常に昇順であること。
- `CameraTrack::evaluate` がキーフレームちょうどの `count` で元の `eye`/`fov_y_rad` と一致すること
  （不変条件4）。
- `CameraBounds::clamp` が範囲内の点をそのまま返し、範囲外の点を面上に留めること。
- `CameraProgram::active_camera_at`: カット前は `None`、カットちょうどの count で新カメラに切り替わる
  境界条件（`TempoMap::bpm_at` の `boundary` テストと同じ形）。

**プロパティ**

- `evaluate_with_hint` を単調増加する count 列で呼んだ結果が、同じ count 列を`evaluate`（ヒント無し）
  で呼んだ結果と完全一致する（浮動小数ビット単位、乱数シードで複数パターン）。
- `evaluate_with_hint` をランダム（非単調）な count 列で呼んでも、直前の呼び出し順に関わらず
  `evaluate` と一致する（決定論、不変条件1）。
- `damped_follow_point` で `tau -> 0` に近づけるほど生の追従対象位置に収束する。
- `damped_follow_point` は生の対象が一定速度で動くランプ入力のとき、定常状態で
  `slope * tau` だけ遅れた位置に収束する（解析解との突き合わせ）。

**ゴールデン**

- 固定の3キーフレーム・固定 count 列に対する `CameraPose`（`eye`/`up`/`target`）の期待値を
  ハードコードし、リグレッションを検出する（`.expected` 方式は使わず、`continuity.rs` 同様に
  テスト内へインライン、少数の代表点のみ）。

**ストレス**

- 10,000件のランダム `Edit`（`InsertCameraKeyframe`/`RemoveCameraKeyframe`/`SetCameraCut`/
  `AddCameraTrack`/`RemoveCameraTrack` を含む）→ 全 Undo → 初期ドキュメントと完全一致
  （`DESIGN_GAPS.md` A-1 の受け入れ基準にカメラ系バリアントを合流させる）。

**ベンチ**（`crates/drill-core/benches/core_performance.rs` に追加）

- 1トラック・500キーフレームに対して `evaluate_with_hint` を単調 count で60,000回呼び、
  ウォームアップ後にヒープ確保がゼロであること（既存の `interpolation_reuses_output_allocation`
  と同じポインタ比較の手法を `CameraFollowCache`/`CameraEvalHint` に適用）。
- `PRODUCT_QUALITY.md` の「2026-08-09の開発機ベースライン」表に倣い、
  「1,000人 × 64セット、Follow使用時の `CameraFollowCache::refresh`」の実測ミリ秒を追記する。

## 8. 実装タスク

Codex に渡す粒度（1タスク=1〜3時間）。依存関係の無いものは並行可能。

| # | タスク | 依存 | 並行可否 |
|---|---|---|---|
| T1 | `look_at_matrix`/`perspective_matrix`/`project_with` を `Camera` から抽出し、`Camera` の既存挙動・既存テストを変えずにリファクタ。あわせて `Camera::view_matrix` を `private` から `pub` へ（doc 21 §「深度規約の落とし穴」の要求、3.1） | なし | 単独（他の全タスクの前提） |
| T2 | `Quat` 最小実装（`look_at`/`from_basis`/`from_axis_angle`/`slerp`/`rotate_vector`）＋単体テスト（3.2, 7章） | なし | T1と並行可 |
| T3 | `CameraId`/`CameraKeyframe`/`CameraTrack`（`insert_keyframe`/`remove_keyframe`/`locate_segment`）＋`catmull_rom_nonuniform`＋単体テスト（3.3, 3.4） | T1 | T2と並行可 |
| T4 | `LookMode::Explicit` の姿勢解決（`resolve_pose`/`explicit_base_target`/`segment_t`）＋`evaluate`/`evaluate_with_hint`＋ゴールデンテスト（3.5） | T2, T3 | 単独 |
| T5 | `FollowTarget`/`FollowDamping`/`CameraFollowCache`/`damped_follow_point`（閉形式ランプ応答）＋プロパティテスト（3.7） | T3 | T4と並行可（`LookMode::Follow` 分岐の配線のみ T4 完了待ち） |
| T6 | `CameraBounds`/`BoundsMode`（3.8）＋単体テスト | T3 | T2, T3 完了後は独立 |
| T7 | `CameraCut`/`CameraProgram`（`active_camera_at`/`evaluate`/`evaluate_with`/`try_alloc_camera_id`/`remove_cut`/`drain_cuts_for`/`normalize_after_load`）＋トラック削除時のカット整合＋単体テスト（3.9） | T3, T4 | 単独 |
| T8 | ショットプリセット関数群（3.10）＋既存3プリセットのラッパ化 | T3 | 単独（T1〜T7と並行可） |
| T9 | `scan_acceleration`/`AccelScratch`（3.6）＋ベンチ（doc 40 のジョブ基盤が未着手でも同期関数として実装し、後で非同期化） | T4 | T5〜T8と並行可 |
| T10 | `Document.camera_program` フィールド追加＋serde往復テスト＋schema_version への影響を doc 41 と調整（3.12） | T7 | 単独 |
| T11 | doc 10 確定済みのカメラ `Edit` 10 バリアント（`InsertCameraTrack`/`RemoveCameraTrack`/`RenameCameraTrack`/`SetCameraLook`/`SetCameraBoundsMode`/`InsertCameraKeyframe`/`RemoveCameraKeyframe`/`MoveCameraKeyframe`/`SetCameraCut`/`RemoveCameraCut`）を `Edit::apply` に配線し、doc 10 §3.3.3.3 の逆操作対応表どおりに動くことをテスト | T10、doc 10 の `Edit` 本体実装 | doc 10 待ち |
| T12 | 10,000件ランダム `Edit` ストレステストへのカメラ系バリアント合流（7章） | T11 | doc 10 待ち |

Wave分け: T1・T2 が土台（直列不要、両方並行可）。T3〜T9 が本体（T1/T2完了後に大きく並行可能）。
T10〜T12 は `Document`/`Edit` 統合で、doc 10 の `Edit` enum 着地とスケジュールを合わせる必要がある。

## 9. 未決事項

- **回転補間の粒度**: 本書は「両端キーフレームの time-linear slerp」を採用したが、Catmull-Rom と同様に
  4点（前後の姿勢も使う）で滑らかにする設計（squad 等）も選択肢としてある。前者は実装・説明が単純で
  「急な向き変更が無い限り実用上十分」という判断だが、向きが頻繁かつ急激に変わる演出（ドローン旋回等）
  では滑らかさが不足する可能性があり、実装後にプレビューで確認して要否を判断する。
- **`FollowDamping` の区分アフィン近似**: `DESIGN_GAPS.md` A-2 の `RouteTable`（曲線・ゲート）が
  将来入ると、セット内の生の追従対象位置はもはやアフィンではなくなる。そのときは
  `damped_follow_point` のセグメント内公式（3.7）を「区分アフィンの厳密解」から
  「区分ごとに数点の追加サブ区間へ分割してアフィン近似する」方向へ拡張する必要がある。
  現時点では `positions_at` が線形補間のみのため、この設計のままで正確。
- **追従対象が削除された場合の UX**: `FollowTarget::Group` が空集合に縮退したときのフォールバック
  （3.7・6章では原点を返すのみ）を、プロダクトとして「直前の位置を保持」「`Ensemble` へ自動切替」の
  どちらにするかは doc 43・製品判断待ち。
  現状の設計はどちらへも後から変更できるよう `raw_target_at_set_start` に閉じ込めてある。
- **`FollowDamping::time_constant_seconds` をキーフレーム化するか**: 現状は1トラックにつき1スカラー
  （`LookMode::Follow` に固定）。「追従を追う速さ自体をカウント上で変化させたい」という要求が
  出た場合は `LookMode::Follow` の `damping` を `Vec<(f32, FollowDamping)>` へ拡張する必要があるが、
  現行の `MEDIA_PIPELINE.md`/ヒアリング範囲では要求が無いため見送る。
- **`Edit` 統合のタイミング**: 3.12 の10バリアントは doc 10 が確定させているが、`Edit` 本体の実装
  そのものがまだ無い（本書2章、doc 10 も同様の段階）。doc 10 の着手が遅れる場合に T11 をどう
  前倒しするか（例えばカメラ側のメソッド・テストだけを doc 10 の型定義に対して先に書いておき、
  `Edit::apply` の配線だけを doc 10 待ちにする）は実装順のオーナー判断が必要。**任意クロージャで
  `CameraProgram` を書き換える暫定ラッパは doc 10 のレビューにより不採用と確定済み**（3.12）なので、
  それ以外の前倒し手段に限る。
- **カメラ衝突診断（演者・プロップとの近接）の要否**: `MEDIA_PIPELINE.md` P2 は「カメラ急加速・衝突
  診断」と併記しているが、本書は急加速のみを実装対象にした（3.6）。衝突（カメラが演者に極端に
  接近するショットの検出）は doc 13 の衝突解析基盤を再利用するのが自然かどうか、doc 13 側との
  役割分担を次の設計サイクルで確定する必要がある。

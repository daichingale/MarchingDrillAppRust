# 12. マーチングスタイルと歩幅・ドリル難易度スコア

## 1. 目的と範囲

現状 `analyze_transition(doc, i, 0.75, 1.0)`（`crates/drill-core/src/lib.rs:186-223`、呼び出しは
`crates/drill-app/src/main.rs:1011`）のように、歩幅の閾値が意味の分からない生の `f32` リテラルとして
アプリ側に直書きされている。ドリルデザイナーは歩幅を「8 to 5」「6 to 5」のような**マーチングスタイル語彙**で
考え、テンポ（BPM）との組み合わせで初めて「きつい／無理」を判断する。この文書は

1. その語彙 (`StepStyle`) をコアの型として定義し、inch・yard・meter の換算を一箇所に閉じ込める。
2. 歩幅とテンポから実際の移動速度を導き、人間の歩行運動の実測に基づいた `StrideRating` を定義する。
3. 進行方向と体の向きの差分から `MarchingTechnique`（前進・後退・スライド・ジャズ）を自動判定し、
   技法ごとの難易度係数を与える。
4. 歩幅・速度・技法・方向転換・連続移動・隣接演者との相対速度を統合した `DrillDifficulty` スコア
   （0–100、内訳付き）を、演者単位・セット単位・ショー全体単位で算出する。
5. 「最も過酷な演者トップ10」「休みなく48カウント動き続ける区間」を抽出するレポートAPIを定義する。
6. 既存 `TransitionStats` / `analyze_transition` / `pathing::transition_moves` との後方互換な統合方法を示す。

**この文書が扱わないこと**（境界線として明示する）:

- **衝突検出そのもの**（`analyze_transition` の `collisions` フィールド、O(n²)→空間ハッシュ化）は
  [DESIGN_GAPS.md](../../DESIGN_GAPS.md) A-4（doc 13 予定）の担当。本文書は「隣接演者との相対速度」
  スコアに必要な最小限の近傍探索のみを自前で定義し、衝突検査エンジンの設計そのものは行わない。
- **ルート・ゲート・ホールド**（曲線経路、出発/到着タイミング、静止カウント）は A-2（doc 11）の担当。
  本文書は現行の「セット間線形補間・到達カウントで割った歩幅」モデルの上に構築し、A-2 が入った後の
  移行点を「9. 未決事項」に明記する。
- **演者・セクションのメタデータ拡張**（`Section`、`PerformerKind` など）は A-5（doc 15）の担当。
  本文書は既存の `Performer { id, label, color }` と、分析関数が受け取る `performer_index: usize`
  （`Document::performers` と同じ並び）のみを前提にする。
- **非同期ジョブ配線**（`Job<T>`、UIスレッドからの呼び出しタイミング）は B-3（未採番、doc 40 予定）の
  担当。本文書は「1回の計算が何をどれだけ確保し、何ミリ秒かかるか」までを定義し、`Job<T>` へどう
  包むかは呼び出し側（`drill-app`）の実装タスクとして委ねる。

## 2. 現状

- `crates/drill-core/src/lib.rs:186-223` の `analyze_transition(document, set_index, collision_distance,
  max_step_per_count)` は、`collision_distance` と `max_step_per_count` を **無名の `f32`** で受け取る。
  `crates/drill-app/src/main.rs:1011` は `analyze_transition(&self.document, self.current_set, 0.75, 1.0)`
  と、単位もマーチング語彙も一切書かれていない即値を渡している。
- `crates/drill-core/src/pathing.rs` に `PerformerMove { performer_index, distance, step_size }`
  （29-51行目）、`transition_moves`（29-51行目）、`TransitionStats { max_step, mean_step,
  total_distance, longest_mover }`（54-92行目）が既にある。`step_size` は「セット始点の `counts` で
  割った、フィールド単位（yard/meter）あたりの距離」であり、閾値判定は一切持たない**素の計測値**。
  `crates/drill-app/src/main.rs:1033` はこれを毎フレーム `Vec` 確保付きで呼んでいる
  （[DESIGN_GAPS.md](../../DESIGN_GAPS.md) 既存不具合 #4）。本文書はこの `Vec` 確保問題自体は扱わないが、
  新設計はスクラッチ再利用を前提にするため同じ轍を踏まない。
- `crates/drill-core/src/tempo.rs` の `TempoMap` は `bpm_at(global_count) -> f32`
  （96-110行目、非有限・非正 BPM は `DEFAULT_BPM = 120.0` にガード済み）と `seconds_at`/`count_at` を
  持つ。歩幅とBPMを掛け合わせて速度を出すための材料は揃っている。
- `crates/drill-core/src/continuity.rs` の `horizontal_step`/`vertical_step`（54-62行目）は
  `GridConfig.horizontal_units / horizontal_steps` からステップ長を導いている。既定値
  （`horizontal_steps: 8, horizontal_units: 5.0`、`lib.rs:57-86`）は暗黙に「8 to 5」を意味するが、
  **どこにも `StepStyle` という名前が付いていない**。`HOLD_THRESHOLD_STEPS = 0.25`
  （`continuity.rs:22`）は「歩幅0.25歩未満は静止」という閾値で、本文書の連続移動検出でも同じ規約を
  再利用する。
- `StepStyle` / `StrideRating` / `MarchingTechnique` / `DrillDifficulty` に相当する型は
  リポジトリのどこにも存在しない。`Performer` は向き（facing）を持たない。
- `Document` に `style` フィールドは存在しない。`GridConfig`（`Unit::Yards`/`Unit::Meters`、
  `lib.rs:21-25`）は既にある。

## 3. 設計

新規モジュールを2つ追加する。`crates/drill-core/src/lib.rs` の `pub mod` 一覧
（6-17行目、アルファベット順）に `style`（`svg` の直前）と `difficulty`（`countsheet` と `editing` の間）
を追加する。

```
pub mod audio;
pub mod camera;
pub mod continuity;
pub mod coordinates;
pub mod countsheet;
pub mod difficulty;   // 新規
pub mod editing;
pub mod pathing;
pub mod playback;
pub mod shapes;
pub mod style;         // 新規
pub mod svg;
pub mod tempo;
pub mod video;
```

`style` はマーチング語彙（`StepStyle`・`StrideRating`・`MarchingTechnique`）を定義する。
`difficulty` は `style` と `pathing`・`tempo` を使って `DrillDifficulty` を計算する。依存方向は
`difficulty → style, pathing, tempo`（一方向、循環なし）。

### 3.1 `StepStyle` — 歩幅の名前

```rust
// crates/drill-core/src/style.rs

use crate::Unit;
use serde::{Deserialize, Serialize};

/// 1ヤード = 36インチ、1ヤード = 0.9144メートル（定義上の正確な値）。
/// 単位換算は必ずこの定数を通し、カウントごとに繰り返し掛け算しない
/// （誤差を蓄積させないため、変換は呼び出し境界で一度だけ行う）。
pub const INCHES_PER_YARD: f32 = 36.0;
pub const METERS_PER_YARD: f32 = 0.9144;

/// マーチングスタイル。全て「5ヤードにつき何歩か」で定義する
/// （Pyware・伝統的なマーチング用語と同じ規約）。
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum StepStyle {
    /// 5yd = 8歩 = 22.5インチ/歩（最も一般的なフィールドスタイル）。
    EightToFive,
    /// 5yd = 6歩 = 30.0インチ/歩（歩幅が大きい、伝統的な軍隊式に近い）。
    SixToFive,
    /// 5yd = 12歩 = 15.0インチ/歩（歩幅が小さい、密集フォーメーション向け）。
    TwelveToFive,
    /// 5yd = 16歩 = 11.25インチ/歩（インドア・小編成でよく使う極小歩幅）。
    SixteenToFive,
    /// 任意の「5ヤードあたり歩数」。0以下やNaNは `steps_per_five_yards()` で
    /// `f32::EPSILON` にガードされる。
    Custom { steps_per_five_yards: f32 },
}

impl StepStyle {
    /// 5ヤードあたりの歩数。0またはNaNは事実上「無限に細かい歩幅」を防ぐため
    /// 極小値にクランプする（ゼロ除算防止）。
    pub fn steps_per_five_yards(&self) -> f32 {
        let raw = match self {
            Self::EightToFive => 8.0,
            Self::SixToFive => 6.0,
            Self::TwelveToFive => 12.0,
            Self::SixteenToFive => 16.0,
            Self::Custom { steps_per_five_yards } => *steps_per_five_yards,
        };
        if raw.is_finite() && raw > 0.0 { raw } else { f32::EPSILON }
    }

    /// 1歩の長さ（インチ）。`180.0 = 5yd * 36in/yd`。
    pub fn step_inches(&self) -> f32 {
        180.0 / self.steps_per_five_yards()
    }

    /// 1歩の長さ（ヤード）。
    pub fn step_yards(&self) -> f32 {
        self.step_inches() / INCHES_PER_YARD
    }

    /// 1歩の長さ（メートル）。
    pub fn step_meters(&self) -> f32 {
        self.step_yards() * METERS_PER_YARD
    }

    /// `distance_yards` を移動するのに必要な歩数。距離は正数に丸める
    /// （負の距離という概念はない — 呼び出し側が符号を扱う）。
    pub fn steps_for(&self, distance_yards: f32) -> f32 {
        distance_yards.max(0.0) / self.step_yards()
    }
}

impl Default for StepStyle {
    /// 現行 `GridConfig::default()`（`horizontal_steps: 8, horizontal_units: 5.0`）
    /// が暗黙に表していたスタイルと一致させる。
    fn default() -> Self {
        Self::EightToFive
    }
}

/// フィールド単位（`Unit::Yards` または `Unit::Meters`）で表された距離を
/// ヤードへ変換する、境界での単位変換の唯一の入口。
pub fn to_yards(distance: f32, unit: Unit) -> f32 {
    match unit {
        Unit::Yards => distance,
        Unit::Meters => distance / METERS_PER_YARD,
    }
}

/// フィールド単位の距離をインチへ変換する（`to_yards` を経由する単一変換）。
pub fn to_inches(distance: f32, unit: Unit) -> f32 {
    to_yards(distance, unit) * INCHES_PER_YARD
}
```

`GridConfig` の `horizontal_steps`/`horizontal_units` は編集グリッドのスナップ解像度であり、
`StepStyle` はショー全体の宣言的なマーチングスタイルという、別の概念として扱う（両者の統合は
「9. 未決事項」）。`Document` には新しいフィールドを1つだけ追加する。

```rust
// crates/drill-core/src/lib.rs, struct Document に追加
pub struct Document {
    // ...既存フィールド...
    #[serde(default)]
    pub style: style::StepStyle,
}
```

`#[serde(default)]` により、`style` を持たない既存の v1 JSON も
`StepStyle::default() == EightToFive` として読み込める。`Document::validate` の変更は不要
（`step_inches()` は常に有限・正数を返すため、新たな検証項目を増やさない）。

### 3.2 `StrideRating` — 歩幅とテンポから速度、速度から評価

同じ歩幅でも 120bpm と 180bpm では負荷が違う。「1カウント = 1歩」というマーチングの普遍的な規約
（`continuity.rs` の `step_size_per_count` も同じ前提）から、移動速度は

```
歩/秒 = BPM / 60
速度(インチ/秒) = 歩幅(インチ) * 歩/秒
```

```rust
// crates/drill-core/src/style.rs（続き）

/// `step_inches` の歩幅を `bpm` のテンポで踏んだときの移動速度（インチ/秒）。
/// 「1カウント = 1歩」というマーチングの普遍的な規約に基づく。BPMが非有限・
/// 非正の場合は `tempo::TempoMap::bpm_at` が既に `DEFAULT_BPM` にガードして
/// いるため、ここでは追加の下限のみ掛ける。
pub fn travel_speed_in_per_sec(step_inches_per_count: f32, bpm: f32) -> f32 {
    let steps_per_sec = bpm.max(0.0) / 60.0;
    step_inches_per_count.max(0.0) * steps_per_sec
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum StrideRating {
    Comfortable,
    Aggressive,
    Impossible,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct StrideThresholds {
    /// この速度（インチ/秒）を超えると `Aggressive`。
    pub aggressive_above_in_per_sec: f32,
    /// この速度（インチ/秒）を超えると `Impossible`。
    pub impossible_above_in_per_sec: f32,
}

impl StrideThresholds {
    pub fn rate(&self, speed_in_per_sec: f32) -> StrideRating {
        if speed_in_per_sec > self.impossible_above_in_per_sec {
            StrideRating::Impossible
        } else if speed_in_per_sec > self.aggressive_above_in_per_sec {
            StrideRating::Aggressive
        } else {
            StrideRating::Comfortable
        }
    }
}

impl Default for StrideThresholds {
    /// 根拠: 成人の「快適な歩行速度」は交通工学（横断歩道の青信号時間算定など）
    /// で広く 1.4 m/s が基準値として使われる。これを `Aggressive` の下限とする。
    /// 生体力学の分野では「歩行から走行へ切り替わる自然な遷移速度」が
    /// おおむね 2.0–2.5 m/s であることが広く知られている。マーチングの歩行技術
    /// （ロールステップ、視線・楽器/プロップの水平維持）は歩行動作を前提にして
    /// いるため、この遷移速度以上は「歩いて到達不可能（走るしかない）」とみなし
    /// `Impossible` の下限に置く。中央値の 2.25 m/s を採用する。
    fn default() -> Self {
        const COMFORTABLE_WALK_MPS: f32 = 1.4;
        const WALK_RUN_TRANSITION_MPS: f32 = 2.25;
        const METERS_TO_INCHES: f32 = 39.3701;
        Self {
            aggressive_above_in_per_sec: COMFORTABLE_WALK_MPS * METERS_TO_INCHES, // ≈55.1
            impossible_above_in_per_sec: WALK_RUN_TRANSITION_MPS * METERS_TO_INCHES, // ≈88.6
        }
    }
}
```

`StrideThresholds` は速度（テンポ込み）の評価。これとは別に、「歩幅そのもの」（テンポに関係なく、
1歩の物理的な長さが体格的に無理な reach になっていないか）を独立した軸として難易度スコアに含める
（3.4節）。速度と歩幅は掛け算の関係にあり相関するが、意味が異なる — 極端に長い1歩は低速でも
股関節の可動域・重心制御の問題を起こし、極端な速度は歩幅が普通でも心肺・視覚追従の問題を起こす。
両方を別スコア軸にする理由はここにある。

### 3.3 `MarchingTechnique` — 進行方向と体の向きの差分から自動判定

体の向き（facing）は現在どのドキュメント型にも存在しない。本文書はこれを**永続化されたドキュメント
フィールドとしては提案しない**（`Document`/`Set`/`Performer` のスキーマ拡張は A-2/A-5 の担当と
衝突するため）。代わりに、分析関数の**入力パラメータ**として渡す、スパースな上書きテーブルを
`difficulty` 側で定義する（3.5節）。デフォルト（上書きなし）は「進行方向を向いて前進する」
（`ForwardMarch`、facing = 進行方向）という、実務上も最も多いケースにする。

```rust
// crates/drill-core/src/style.rs（続き）

/// フィールド上の方向を、`facing = 0` を「観客席方向 (-y)」とする弧度で表す。
/// 角度が正になるほど演者から見た右手側 (+x) へ回転する
/// （`continuity.rs` の「+x = 右」規約と整合）。
///
/// 検算: `field_angle(0.0, -1.0) == 0.0`（正面）、
/// `field_angle(1.0, 0.0) ≈ FRAC_PI_2`（右）、
/// `field_angle(0.0, 1.0) ≈ PI`（後方）、
/// `field_angle(-1.0, 0.0) ≈ -FRAC_PI_2`（左）。
pub fn field_angle(dx: f32, dy: f32) -> f32 {
    dx.atan2(-dy)
}

/// `a - b` を `(-PI, PI]` に正規化した角度差。
fn wrap_pi(mut diff: f32) -> f32 {
    use std::f32::consts::PI;
    diff %= 2.0 * PI;
    if diff > PI {
        diff -= 2.0 * PI;
    } else if diff <= -PI {
        diff += 2.0 * PI;
    }
    diff
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum MarchingTechnique {
    /// 向いている方向へそのまま進む。最も一般的で最も易しい。
    ForwardMarch,
    /// 向いている方向の逆へ進む（後ろ歩き）。進行方向が見えない。
    BackwardMarch,
    /// 向いている方向に対して右（+90°側）へ真横に進む。
    SlideRight,
    /// 向いている方向に対して左（-90°側）へ真横に進む。
    SlideLeft,
    /// 前後左右のどれにも当てはまらない斜め移動。定型技法ではなく
    /// 自由な走り（ジャズラン）として処理される、という現場慣習を反映する。
    Jazz,
}

/// 各カーディナル方向（前後左右）を中心にこの角度（度）以内なら、その技法とみなす。
/// 根拠: 実務上、45°未満のカーブは経路のドリフトとして自然に吸収されるが、
/// 90°に近い転回は明示的な「フランク」「ピボット」技法を要求する。本閾値は
/// その中間を境界に取った現場慣習的な値であり、生体力学的な実測値ではない
/// （設定可能。「9. 未決事項」参照）。
pub const CARDINAL_TOLERANCE_DEG: f32 = 30.0;

/// 進行方向 `travel_angle` と体の向き `facing_angle`（共に `field_angle` の弧度）
/// から技法を分類する。
pub fn classify_technique(travel_angle: f32, facing_angle: f32) -> MarchingTechnique {
    let deg = wrap_pi(travel_angle - facing_angle).to_degrees();
    let tol = CARDINAL_TOLERANCE_DEG;
    if deg.abs() <= tol {
        MarchingTechnique::ForwardMarch
    } else if (deg - 180.0).abs() <= tol || (deg + 180.0).abs() <= tol {
        MarchingTechnique::BackwardMarch
    } else if (deg - 90.0).abs() <= tol {
        MarchingTechnique::SlideRight
    } else if (deg + 90.0).abs() <= tol {
        MarchingTechnique::SlideLeft
    } else {
        MarchingTechnique::Jazz
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct TechniqueCoefficients {
    pub forward: f32,
    pub slide: f32,
    pub backward: f32,
    pub jazz: f32,
}

impl TechniqueCoefficients {
    pub fn get(&self, technique: MarchingTechnique) -> f32 {
        match technique {
            MarchingTechnique::ForwardMarch => self.forward,
            MarchingTechnique::SlideLeft | MarchingTechnique::SlideRight => self.slide,
            MarchingTechnique::BackwardMarch => self.backward,
            MarchingTechnique::Jazz => self.jazz,
        }
    }
}

impl Default for TechniqueCoefficients {
    /// 根拠（現場慣習。厳密な運動学研究の引用ではなく、設計上の既定値として
    /// 明示し、設定可能にする）:
    /// - `forward = 1.00`: 基準。視線も歩行方向も一致し、最も習熟が早い。
    /// - `slide = 1.25`: 真横へのシャッセ的な足運びは前進より生体力学的な
    ///   効率が落ち、かつ体幹を進行方向に対して非同期に保つ必要がある。
    /// - `backward = 1.45`: 進行方向が見えないため、周辺視野やガイド
    ///   （目印演者）に依存する。転倒リスクが高く、現場では前進より
    ///   明確に低いテンポ上限で運用されることが多い。
    /// - `jazz = 1.15`: 定型技法ではない分、足運び自体の制約は緩いが、
    ///   カウントとの同期・隊列内での位置維持の難度はやや高い。
    fn default() -> Self {
        Self { forward: 1.00, slide: 1.25, backward: 1.45, jazz: 1.15 }
    }
}
```

### 3.4 `DrillDifficulty` — 統合スコア

`crates/drill-core/src/difficulty.rs` に置く。1回のセット間遷移について、演者ごとに6要素を
0–100へ写像し、重み付き合計で最終スコアを出す。

```rust
// crates/drill-core/src/difficulty.rs

use crate::pathing::{transition_moves, PerformerMove};
use crate::style::{
    self, classify_technique, MarchingTechnique, StepStyle, StrideThresholds,
    TechniqueCoefficients,
};
use crate::tempo::TempoMap;
use crate::{Document, PerformerId, Point, Unit};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// `value` を `[comfortable, impossible]` から `[0, 100]` へ線形写像する。
/// `impossible <= comfortable` の場合は不正な設定として全域を 0 か 100 に潰す
/// （NaN/Infのしみ込みを防ぐガード）。
fn ramp(value: f32, comfortable: f32, impossible: f32) -> f32 {
    if !(value.is_finite() && comfortable.is_finite() && impossible.is_finite()) {
        return 0.0;
    }
    if impossible <= comfortable {
        return if value >= impossible { 100.0 } else { 0.0 };
    }
    (((value - comfortable) / (impossible - comfortable)) * 100.0).clamp(0.0, 100.0)
}

/// セット遷移ごとに演者の体の向きを上書きする、疎な対応表。
/// キーの無い演者は `ForwardMarch`（facing = 進行方向）として扱われる。
/// `Document` には永続化しない（3.3節）。UIから明示的にバックワード/スライドの
/// 演技を指示したい場合の一時的な分析入力として渡す。
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct FacingOverrides {
    /// `performer_index -> facing_angle`（`style::field_angle` と同じ規約の弧度）。
    pub facing: BTreeMap<usize, f32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct DifficultyWeights {
    pub stride: f32,
    pub speed: f32,
    pub technique: f32,
    pub turn: f32,
    pub endurance: f32,
    pub crowding: f32,
}

impl Default for DifficultyWeights {
    /// 合計 1.0。速度と技法を主要因とする（実際の身体的負荷と視覚/同期エラー
    /// の主因であるため）。方向転換・連続移動・混雑度は副次的なリスク要因
    /// として小さめの重みを置く。全て設定可能（「9. 未決事項」参照）。
    fn default() -> Self {
        Self { stride: 0.15, speed: 0.30, technique: 0.20, turn: 0.15, endurance: 0.10, crowding: 0.10 }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct DifficultyParams {
    pub style: StepStyle,
    pub stride_thresholds: StrideThresholds,
    pub technique_coefficients: TechniqueCoefficients,
    pub weights: DifficultyWeights,
    /// 歩幅そのもの（速度と独立な軸）の許容比率。既定 1.5 / 3.0 は、
    /// 宣言スタイルの「普通の1歩」を基準に、1.5倍まではドリルでよくある
    /// 意図的なリーチ、3.0倍（8-to-5なら約67.5インチ ≈ 1.7m）は片脚の
    /// 通常の制御可能なストライドを超え、走り幅跳びに近い跳躍になる、
    /// という現場慣習的な境界。
    pub stride_length_comfortable_multiplier: f32,
    pub stride_length_impossible_multiplier: f32,
    /// 方向転換の急峻さ（度）の境界。45°未満は経路のドリフトとして吸収可能、
    /// 135°超（ほぼ反転）は最大難度、という現場慣習値。
    pub turn_comfortable_deg: f32,
    pub turn_impossible_deg: f32,
    /// 休みなく動き続けるカウント数の境界。フレーズ構造（8/16カウント単位）の
    /// 2フレーズ分 = 32カウントまでは通常運用、96カウント（約48秒 @120bpm）
    /// を超えると視認できる疲労・技術崩れが起きる、という現場慣習値。
    pub endurance_comfortable_counts: f32,
    pub endurance_impossible_counts: f32,
    /// 近傍演者との相対速度の境界（インチ/秒の差）。0なら常に同速で混雑度なし、
    /// 60 in/s（≈1.5 m/s、快適歩行速度に近い差）を超えると、静止している
    /// 演者のすぐ隣を足早に通過するような、視覚的にも衝突リスク的にも
    /// 危険な状況とみなす。
    pub crowding_comfortable_in_per_sec: f32,
    pub crowding_impossible_in_per_sec: f32,
    /// 近傍探索の半径（フィールド単位、`Document.grid.unit` に従う）。
    pub neighbor_radius: f32,
}

impl DifficultyParams {
    /// `doc.style` と既定閾値からパラメータを組み立てる。
    pub fn from_document(doc: &Document) -> Self {
        Self {
            style: doc.style,
            stride_thresholds: StrideThresholds::default(),
            technique_coefficients: TechniqueCoefficients::default(),
            weights: DifficultyWeights::default(),
            stride_length_comfortable_multiplier: 1.5,
            stride_length_impossible_multiplier: 3.0,
            turn_comfortable_deg: 45.0,
            turn_impossible_deg: 135.0,
            endurance_comfortable_counts: 32.0,
            endurance_impossible_counts: 96.0,
            crowding_comfortable_in_per_sec: 0.0,
            crowding_impossible_in_per_sec: 60.0,
            neighbor_radius: 3.0,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct DifficultyBreakdown {
    pub stride: f32,
    pub speed: f32,
    pub technique: f32,
    pub turn: f32,
    pub endurance: f32,
    pub crowding: f32,
}

impl DifficultyBreakdown {
    pub fn weighted_score(&self, weights: &DifficultyWeights) -> f32 {
        (self.stride * weights.stride
            + self.speed * weights.speed
            + self.technique * weights.technique
            + self.turn * weights.turn
            + self.endurance * weights.endurance
            + self.crowding * weights.crowding)
            .clamp(0.0, 100.0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct MoveDifficulty {
    pub performer_index: usize,
    pub from_set: usize,
    pub to_set: usize,
    pub technique: MarchingTechnique,
    pub breakdown: DifficultyBreakdown,
    pub score: f32,
}
```

**演者ごとの継続状態**（前回の進行方向、連続移動カウント数）を遷移をまたいで持ち回るためのスクラッチ。
毎フレーム再確保しないという規約（`00-conventions.md` 不変条件3）は本来「毎フレーム走る関数」向けだが、
1,000人×63遷移というバッチ計算でも同じ考え方を踏襲し、呼び出し側が確保・再利用する。

```rust
#[derive(Debug, Default)]
pub struct DifficultyScratch {
    /// 演者ごとの直前遷移の進行方向（弧度）。最初の遷移では `None` 相当として
    /// `f32::NAN` を使わず、`has_prev: Vec<bool>` で明示的に管理する。
    prev_angle: Vec<f32>,
    has_prev: Vec<bool>,
    /// 演者ごとの「現在の連続移動カウント数」の累計（静止で0にリセット）。
    run_counts: Vec<f32>,
    /// 近傍探索用の一様グリッド（セル辺 = `neighbor_radius`）。
    /// バケット先頭 → 演者index の単方向連結リスト。
    bucket_of: Vec<i64>,
    bucket_head: BTreeMap<(i32, i32), u32>,
    bucket_next: Vec<i32>,
    /// この遷移の各演者の速度（インチ/秒）。近傍探索で読み返す。
    speeds_scratch: Vec<f32>,
    /// 出力を書き戻すための一時バッファ（呼び出し間で `Vec` 容量を再利用）。
    moves_scratch: Vec<PerformerMove>,
}

impl DifficultyScratch {
    /// 演者数が変わったとき（ドキュメントの演者追加/削除後）だけ呼ぶ。
    /// 通常の遷移計算では呼ばない。
    pub fn resize(&mut self, performer_count: usize) {
        self.prev_angle.clear();
        self.prev_angle.resize(performer_count, 0.0);
        self.has_prev.clear();
        self.has_prev.resize(performer_count, false);
        self.run_counts.clear();
        self.run_counts.resize(performer_count, 0.0);
        self.bucket_of.clear();
        self.bucket_of.resize(performer_count, -1);
        self.bucket_next.clear();
        self.bucket_next.resize(performer_count, -1);
        self.speeds_scratch.clear();
        self.speeds_scratch.resize(performer_count, 0.0);
    }
}
```

1遷移分のスコアを計算する中心関数。

```rust
/// `set_index -> set_index + 1` の遷移について、演者ごとの `MoveDifficulty` を
/// `out` に積む（`out` はクリアしない — 呼び出し側がショー全体を1本のバッファへ
/// 集約できるようにするため）。`scratch` は `resize` 済みで、直前の遷移から
/// 続けて渡されること（`prev_angle`/`run_counts` を遷移間で引き継ぐため）。
///
/// 計算量: O(n) 本体 + 近傍探索 O(n)（一様グリッド、半径内のみ比較）= O(n)。
/// 追加ヒープ確保はゼロ（`scratch` とキャパシティ十分な `out` を渡した場合）。
pub fn score_transition(
    doc: &Document,
    tempo: &TempoMap,
    set_index: usize,
    params: &DifficultyParams,
    facing: &FacingOverrides,
    scratch: &mut DifficultyScratch,
    out: &mut Vec<MoveDifficulty>,
) {
    let Some(from) = doc.sets.get(set_index) else { return };
    let Some(to) = doc.sets.get(set_index + 1) else { return };
    let unit = doc.grid.unit;
    let counts = f32::from(from.counts.max(1));
    let bpm = tempo.bpm_at(doc.global_count(set_index, 0.0));

    scratch.moves_scratch.clear();
    scratch.moves_scratch.extend(transition_moves(doc, set_index));

    // Pass 1: 速度・技法・歩幅・方向転換・連続移動を演者ごとに計算し、
    // 近傍探索で使う速度バケットも同時に埋める。
    rebuild_neighbor_grid(scratch, &from.positions, params.neighbor_radius);

    for mv in &scratch.moves_scratch {
        let i = mv.performer_index;
        let a = from.positions[i];
        let b = to.positions[i];
        let dx = b.x - a.x;
        let dy = b.y - a.y;
        let travel_angle = style::field_angle(dx, dy);
        let facing_angle = facing.facing.get(&i).copied().unwrap_or(travel_angle);
        let technique = classify_technique(travel_angle, facing_angle);

        let step_inches = style::to_inches(mv.step_size, unit);
        let speed = style::travel_speed_in_per_sec(step_inches, bpm);
        scratch.speeds_scratch[i] = speed;

        let is_hold = mv.distance < HOLD_DISTANCE_FIELD_UNITS;
        if is_hold {
            scratch.run_counts[i] = 0.0;
        } else {
            scratch.run_counts[i] += counts;
        }

        let turn_deg = if scratch.has_prev[i] {
            wrap_pi_deg(travel_angle - scratch.prev_angle[i])
        } else {
            0.0
        };
        if !is_hold {
            scratch.prev_angle[i] = travel_angle;
            scratch.has_prev[i] = true;
        }

        let nominal_step_inches = params.style.step_inches();
        let breakdown = DifficultyBreakdown {
            stride: ramp(
                step_inches,
                nominal_step_inches * params.stride_length_comfortable_multiplier,
                nominal_step_inches * params.stride_length_impossible_multiplier,
            ),
            speed: ramp(
                speed,
                params.stride_thresholds.aggressive_above_in_per_sec,
                params.stride_thresholds.impossible_above_in_per_sec,
            ),
            technique: technique_score(technique, &params.technique_coefficients),
            turn: ramp(turn_deg.abs(), params.turn_comfortable_deg, params.turn_impossible_deg),
            endurance: ramp(
                scratch.run_counts[i],
                params.endurance_comfortable_counts,
                params.endurance_impossible_counts,
            ),
            crowding: crowding_score(scratch, i, a, speed, params),
        };

        out.push(MoveDifficulty {
            performer_index: i,
            from_set: set_index,
            to_set: set_index + 1,
            technique,
            breakdown,
            score: breakdown.weighted_score(&params.weights),
        });
    }
}

/// 静止とみなす移動距離（フィールド単位）。`continuity.rs` の
/// `HOLD_THRESHOLD_STEPS = 0.25` と揃えるため、`StepStyle` の1歩の4分の1を使う。
fn hold_distance_field_units(style: &StepStyle, unit: Unit) -> f32 {
    let quarter_step_yards = style.step_yards() * 0.25;
    match unit {
        Unit::Yards => quarter_step_yards,
        Unit::Meters => quarter_step_yards * style::METERS_PER_YARD,
    }
}

fn technique_score(technique: MarchingTechnique, coeffs: &TechniqueCoefficients) -> f32 {
    let max_extra = [coeffs.slide, coeffs.backward, coeffs.jazz]
        .into_iter()
        .fold(coeffs.forward, f32::max)
        - coeffs.forward;
    if max_extra <= 0.0 {
        return 0.0;
    }
    ((coeffs.get(technique) - coeffs.forward) / max_extra * 100.0).clamp(0.0, 100.0)
}
```

近傍探索（混雑度）は一様グリッドのバケットに全演者を1回だけ登録し、半径 `neighbor_radius` の
自セル+隣接8セルだけを見る。これは [DESIGN_GAPS.md](../../DESIGN_GAPS.md) A-4 が衝突検査向けに
提案している `ScanScratch` と同じ考え方だが、**別実装**として本文書内に閉じる
（A-4 が実装された後に共通化するかどうかは「9. 未決事項」）。

```rust
fn rebuild_neighbor_grid(scratch: &mut DifficultyScratch, positions: &[Point], cell: f32) {
    let cell = cell.max(f32::EPSILON);
    scratch.bucket_head.clear();
    for (i, p) in positions.iter().enumerate() {
        let key = (
            (p.x / cell).floor() as i32,
            (p.y / cell).floor() as i32,
        );
        let head = scratch.bucket_head.get(&key).copied();
        scratch.bucket_next[i] = head.map_or(-1, |h| h as i32);
        scratch.bucket_head.insert(key, i as u32);
    }
}

/// `positions[i]` の近傍（半径 `neighbor_radius` 以内）にいる演者との速度差の
/// 最大値をランプして返す。近傍がいなければ 0（混雑リスクなし）。
fn crowding_score(
    scratch: &DifficultyScratch,
    i: usize,
    position: Point,
    my_speed: f32,
    params: &DifficultyParams,
) -> f32 {
    let cell = params.neighbor_radius.max(f32::EPSILON);
    let cx = (position.x / cell).floor() as i32;
    let cy = (position.y / cell).floor() as i32;
    let mut max_diff = 0.0f32;
    for dx in -1..=1 {
        for dy in -1..=1 {
            let Some(&head) = scratch.bucket_head.get(&(cx + dx, cy + dy)) else { continue };
            let mut cursor = head as i32;
            while cursor >= 0 {
                let j = cursor as usize;
                if j != i {
                    max_diff = max_diff.max((scratch.speeds_scratch[j] - my_speed).abs());
                }
                cursor = scratch.bucket_next[j];
            }
        }
    }
    ramp(max_diff, params.crowding_comfortable_in_per_sec, params.crowding_impossible_in_per_sec)
}
```

### 3.5 演者単位・セット単位・ショー全体の集約

```rust
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PerformerDifficulty {
    pub performer_index: usize,
    pub performer_id: PerformerId,
    /// 最も過酷だった単一遷移のスコア。
    pub peak_score: f32,
    pub peak_transition: Option<(usize, usize)>, // (from_set, to_set)
    /// 全遷移の平均スコア。
    pub mean_score: f32,
    /// 全遷移のスコア合計（累積負荷。長いショーほど大きくなる、順位付け用）。
    pub total_score: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SetDifficulty {
    pub set_index: usize,
    pub mean_score: f32,
    pub max_score: f32,
    pub hardest_performer: Option<usize>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ShowDifficulty {
    /// ショー全体の代表値（全遷移スコアの平均）。0–100。
    pub overall_score: f32,
    pub per_set: Vec<SetDifficulty>,
    pub per_performer: Vec<PerformerDifficulty>,
}

/// ショー全体（全セット遷移 × 全演者）を1回で計算する。呼び出し側は
/// UIスレッドで直接呼ばない（「5. 性能」参照）。`facing` は
/// `sets.len() - 1` 個（各遷移に対応、無ければ既定 `ForwardMarch` 扱い）。
pub fn analyze_show_difficulty(
    doc: &Document,
    params: &DifficultyParams,
    facing: &[FacingOverrides],
) -> ShowDifficulty {
    let n_performers = doc.performers.len();
    let n_transitions = doc.sets.len().saturating_sub(1);
    let mut scratch = DifficultyScratch::default();
    scratch.resize(n_performers);

    let mut per_performer_peak = vec![0.0f32; n_performers];
    let mut per_performer_peak_at = vec![None; n_performers];
    let mut per_performer_total = vec![0.0f32; n_performers];
    let mut per_performer_seen = vec![0u32; n_performers];
    let mut per_set = Vec::with_capacity(n_transitions);
    let mut moves = Vec::with_capacity(n_performers);
    let empty_facing = FacingOverrides::default();

    for set_index in 0..n_transitions {
        let facing_for_transition = facing.get(set_index).unwrap_or(&empty_facing);
        moves.clear();
        score_transition(doc, &doc.tempo, set_index, params, facing_for_transition, &mut scratch, &mut moves);

        let mut set_max = 0.0f32;
        let mut set_sum = 0.0f32;
        let mut hardest = None;
        for mv in &moves {
            set_sum += mv.score;
            if mv.score > set_max {
                set_max = mv.score;
                hardest = Some(mv.performer_index);
            }
            let i = mv.performer_index;
            per_performer_total[i] += mv.score;
            per_performer_seen[i] += 1;
            if mv.score > per_performer_peak[i] {
                per_performer_peak[i] = mv.score;
                per_performer_peak_at[i] = Some((mv.from_set, mv.to_set));
            }
        }
        per_set.push(SetDifficulty {
            set_index,
            mean_score: if moves.is_empty() { 0.0 } else { set_sum / moves.len() as f32 },
            max_score: set_max,
            hardest_performer: hardest,
        });
    }

    let per_performer = (0..n_performers)
        .map(|i| PerformerDifficulty {
            performer_index: i,
            performer_id: doc.performers[i].id,
            peak_score: per_performer_peak[i],
            peak_transition: per_performer_peak_at[i],
            mean_score: if per_performer_seen[i] == 0 {
                0.0
            } else {
                per_performer_total[i] / per_performer_seen[i] as f32
            },
            total_score: per_performer_total[i],
        })
        .collect();

    let overall_score = if per_set.is_empty() {
        0.0
    } else {
        per_set.iter().map(|s| s.mean_score).sum::<f32>() / per_set.len() as f32
    };

    ShowDifficulty { overall_score, per_set, per_performer }
}
```

### 3.6 レポート: トップ10と連続移動区間

```rust
/// `total_score`（既定）または `peak_score`（`by_peak = true`）が高い順に
/// 上位 `n` 件を返す。
pub fn hardest_performers(show: &ShowDifficulty, n: usize, by_peak: bool) -> Vec<&PerformerDifficulty> {
    let mut ranked: Vec<&PerformerDifficulty> = show.per_performer.iter().collect();
    ranked.sort_by(|a, b| {
        let (ka, kb) = if by_peak { (a.peak_score, b.peak_score) } else { (a.total_score, b.total_score) };
        kb.total_cmp(&ka)
    });
    ranked.truncate(n);
    ranked
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MovementRun {
    pub performer_index: usize,
    /// 開始セット（このセットへの到着で移動を始めた最初の遷移の始点）。
    pub start_set: usize,
    /// 終了セット（このセットで静止した、または最後の遷移の終点）。
    pub end_set: usize,
    pub total_counts: f32,
}

/// 演者ごとに、静止（`HOLD` 判定）を挟まずに `min_counts` 以上動き続けた
/// 区間を全て抽出する。休符（静止セット）で区切られる。
/// 計算量: O(演者数 × 遷移数)。`analyze_show_difficulty` と同じスキャンを
/// 再利用せず独立に計算できる（`DifficultyParams` を必要としない、
/// `StepStyle` だけに依存する軽量パス）。
pub fn continuous_movement_runs(doc: &Document, style: &StepStyle, min_counts: f32) -> Vec<MovementRun> {
    let unit = doc.grid.unit;
    let n = doc.performers.len();
    let n_transitions = doc.sets.len().saturating_sub(1);
    let mut runs = Vec::new();
    let mut run_start = vec![None::<usize>; n];
    let mut run_counts = vec![0.0f32; n];

    for set_index in 0..n_transitions {
        let hold_distance = hold_distance_field_units(style, unit);
        let moves = transition_moves(doc, set_index);
        let counts = f32::from(doc.sets[set_index].counts);
        for mv in &moves {
            let i = mv.performer_index;
            if mv.distance >= hold_distance {
                if run_start[i].is_none() {
                    run_start[i] = Some(set_index);
                }
                run_counts[i] += counts;
            } else if let Some(start) = run_start[i].take() {
                if run_counts[i] >= min_counts {
                    runs.push(MovementRun {
                        performer_index: i,
                        start_set: start,
                        end_set: set_index,
                        total_counts: run_counts[i],
                    });
                }
                run_counts[i] = 0.0;
            }
        }
    }
    // ショー末尾で静止せずに終わった区間も回収する。
    for i in 0..n {
        if let Some(start) = run_start[i] {
            if run_counts[i] >= min_counts {
                runs.push(MovementRun {
                    performer_index: i,
                    start_set: start,
                    end_set: n_transitions,
                    total_counts: run_counts[i],
                });
            }
        }
    }
    runs
}
```

### 3.7 既存 `TransitionStats` / `analyze_transition` との統合と後方互換

`analyze_transition` と `transition_stats`/`PerformerMove` は**シグネチャを変更しない**。

- `pathing::transition_moves` / `PerformerMove` / `transition_stats` は本設計の
  `score_transition` が**内部で再利用する**（3.4節）。重複実装しない。これらの型はそのまま
  「素の計測値」を返す既存の役割を維持し、`DrillDifficulty` はその上に評価を積む追加レイヤという
  位置づけにする。呼び出し側のテストや `main.rs:1033` は変更不要。
- `analyze_transition(document, set_index, collision_distance, max_step_per_count)` の
  `max_step_per_count` は**無名の float であり続けてはいけない**が、シグネチャは崩さず、
  呼び出し側が渡す値の**作り方**を変える。`crates/drill-app/src/main.rs:1011` の
  `analyze_transition(&self.document, self.current_set, 0.75, 1.0)` を、次のようなドメイン語彙
  経由の計算に置き換える（`drill-app` 側のタスク、本文書は関数を提供するのみ）。

  ```rust
  // main.rs 側（本文書が提供するAPIを使う側の例。drill-core は変更しない）
  let bpm = self.document.tempo.bpm_at(self.document.global_count(self.current_set, 0.0));
  let threshold_speed = self.document.style.step_inches()
      * drill_core::style::StrideThresholds::default().aggressive_above_in_per_sec
      / drill_core::style::StrideThresholds::default().aggressive_above_in_per_sec; // (概念のみ、実際は逆算関数を使う)
  ```

  実際には逆算用の小さなヘルパーを `style` モジュールに追加する。

  ```rust
  /// `speed_in_per_sec` の速度に達するために必要な、1カウントあたりの歩幅
  /// （フィールド単位）。`analyze_transition` の `max_step_per_count` 引数を
  /// 「歩幅の生の float」ではなく「テンポにおけるStrideRating境界」から
  /// 逆算するために使う。
  pub fn max_step_per_count_for_speed(speed_in_per_sec: f32, bpm: f32, unit: Unit) -> f32 {
      let steps_per_sec = bpm.max(1.0) / 60.0;
      let inches_per_count = if steps_per_sec > 0.0 { speed_in_per_sec / steps_per_sec } else { 0.0 };
      let yards = inches_per_count / INCHES_PER_YARD;
      match unit {
          Unit::Yards => yards,
          Unit::Meters => yards * METERS_PER_YARD,
      }
  }
  ```

  これで `main.rs:1011` は次のように書き換えられる（`drill-app` 側の実装タスク、8節参照）。

  ```rust
  let bpm = self.document.tempo.bpm_at(self.document.global_count(self.current_set, 0.0));
  let threshold = style::max_step_per_count_for_speed(
      style::StrideThresholds::default().aggressive_above_in_per_sec,
      bpm,
      self.document.grid.unit,
  );
  let analysis = analyze_transition(&self.document, self.current_set, collision_distance, threshold);
  ```

  `collision_distance` 自体（第3引数）は本文書のスコープ外（A-4/doc 13）であり、当面は既存の
  呼び出し元の値を維持する。
- `DrillDifficulty`（`score_transition`/`analyze_show_difficulty`）は**新しい追加API**であり、
  `TransitionAnalysis`/`TransitionStats` を置き換えない。両者は異なる問いに答える:
  `TransitionAnalysis`/`TransitionStats` は「何ヤード動くか」、`DrillDifficulty` は「それが
  どれだけ大変か」。

## 4. 不変条件

テストで検証できる形で記述する。

1. 任意の有効な `StepStyle`（`Custom` 含む、`steps_per_five_yards` が0以下やNaNでも）に対して
   `step_inches()` は有限かつ `> 0.0`。
2. `StepStyle::EightToFive.step_inches() == 22.5`、`SixToFive == 30.0`、`TwelveToFive == 15.0`、
   `SixteenToFive == 11.25`（誤差 `1e-4` 未満）。
3. `steps_for(0.0) == 0.0`。`distance_yards` に対して単調非減少
   （`steps_for(a) <= steps_for(b)` for `a <= b`）。
4. `StrideThresholds::rate` は速度に対して単調: `speed_a <= speed_b` ならば
   `rate(speed_a)` のランクは `rate(speed_b)` 以下（`Comfortable < Aggressive < Impossible` の順序で）。
5. `classify_technique` は `travel_angle == facing_angle` のとき常に `ForwardMarch`、
   `travel_angle == facing_angle + PI`（正規化後）のとき常に `BackwardMarch` を返す。
6. `DifficultyBreakdown::weighted_score` は常に `[0.0, 100.0]` に収まる
   （個々の要素が `ramp` により `[0, 100]` にクランプされ、`weights` の合計が1.0である限り）。
7. `analyze_show_difficulty` は**決定論的**: 同じ `(Document, DifficultyParams, facing overrides)`
   から常に同じ `ShowDifficulty` を返す（`00-conventions.md` 不変条件5）。
8. `continuous_movement_runs` が返す各演者の区間は重複しない
   （`end_set` of one run <= `start_set` of the next run for the same performer）、かつ
   各区間の `total_counts` の合計は `doc.timeline_counts()` を超えない。
9. `hold_distance_field_units` によるホールド判定は `continuity.rs::HOLD_THRESHOLD_STEPS`
   （0.25歩）と同じ演者について常に同じ真偽値を返す（`StepStyle` を `continuity.rs` が暗黙に
   使っている `GridConfig` 由来のステップ長と揃えた場合）。

## 5. 性能

基準規模: 演者1,000人 / セット64（遷移63本）。

- `score_transition` 1回: 演者ごとに定数時間の算術（歩幅・速度・技法・方向転換・持久）+
  近傍探索（3×3セルの走査、平均近傍数を仮に10人とすれば1演者あたり定数コスト）。
  合計 **O(n)**、ヒープ確保はゼロ（`scratch`/`out`/`moves_scratch` が十分な容量を持つ場合、
  `Vec::extend`/`push` は既存キャパシティ内で完了する。初回呼び出しのみ確保が起きる）。
- `analyze_show_difficulty` 全体: 63遷移 × 1,000人 = 63,000 演者-遷移。1件あたり
  近傍探索を含めても数十FLOP程度と見積もると、63,000 × 概算50ns ≈ **3ms前後**
  （実測はベンチで確認、8節タスク9）。これは **60fps・16.6ms予算の対象外**である
  （毎フレーム実行しない）。`document_revision: u64` が変化したときのみ、UIスレッド外の
  ワーカー（B-3 `Job<T>`、doc 40予定）で再計算し、結果をキャッシュして表示する
  — [DESIGN_GAPS.md](../../DESIGN_GAPS.md) A-4 が衝突走査に提案しているのと同じキャッシュ戦略を
  共有する。
- メモリ: `ShowDifficulty` は `per_performer: Vec<PerformerDifficulty>`（1,000件 × 約40バイト
  ≈ 40KB）と `per_set: Vec<SetDifficulty>`（64件、無視できる量）のみを保持する。
  遷移ごとの詳細 `MoveDifficulty`（63,000件相当）は**保持しない** — `analyze_show_difficulty` は
  各遷移の `moves` バッファを次の遷移で使い回して集約するだけで、`ShowDifficulty` には
  「どの遷移がピークだったか」という `(from_set, to_set)` の参照だけを残す。ドリルダウンUIで
  特定演者の特定遷移の内訳を見せたい場合は、その1遷移だけ `score_transition` を再実行する
  （O(n) だが1回なので数十マイクロ秒）。
- `continuous_movement_runs` は `DifficultyScratch` を使わない軽量パスで、同じ O(演者数 × 遷移数)
  だが定数係数がずっと小さい（近傍探索なし）。単独でも高速に呼べるため、UIの「48カウント区間」
  フィルタ変更のたびに再計算しても問題ない規模。
- 上限規模（演者4,000人・セット256、劣化してよいが壊れてはいけない）でも同じ O(n×m) の
  スケーリングであり、約 4,000 × 255 ≈ 1,020,000 演者-遷移 ≈ 50ms前後と見積もる。
  これも背景ジョブである限り許容範囲内（UIスレッドをブロックしないことが唯一の必須要件）。

## 6. 失敗モードと安全性

- **NaN/Inf の混入**（破損したポジション、ゼロ除算の連鎖）: `ramp` は入力が非有限なら即座に
  `0.0` を返すガードを持つ（3.4節）。`StepStyle::steps_per_five_yards` は非有限・非正を
  `f32::EPSILON` にクランプする。`TempoMap::bpm_at` は既存の `sanitize_bpm` により非有限・非正
  BPMを `DEFAULT_BPM` にガード済み（`tempo.rs:194-200`）— 本設計はこれを再利用するのみで
  新たなガードを重複させない。
- **カウント0のセット**: `score_transition` は `counts = f32::from(from.counts.max(1))` で
  ゼロ除算を避ける（`analyze_transition` の既存パターン `lib.rs:208` と同じ規約）。
- **演者数の不一致**（`from.positions.len() != to.positions.len()`）: `transition_moves` が
  `zip` で短い方に合わせるため（`pathing.rs:37-49`）、範囲外アクセスによるパニックは起きない。
  ただし `Document::validate` を経ていない不正なドキュメントに対しては、集計結果が
  「一部の演者だけ計算された」ものになる。これは既存 `transition_moves` の挙動を変えない
  範囲であり、`Document::validate` 側の責務とする。
- **添字パニックの排除**: `score_transition` 内の `from.positions[i]` / `to.positions[i]`
  アクセスは `transition_moves` が既に両方の `positions` に存在する index のみを
  `PerformerMove` として返すため安全（`zip` により短い方の長さで打ち切られる）。新規に書く
  コードで `.get()` を経ない直接添字は、この呼び出し順序が保たれる限りにおいてのみ許可する
  — レビュー時に `score_transition` のシグネチャ・呼び出し順が変わらないことを確認する。
- **信頼できない `FacingOverrides`**: 弧度に非有限値が入っていても `classify_technique` は
  `wrap_pi` 経由で `NaN` を伝播させるだけで panic はしない（`NaN.abs() <= tol` は false になり
  `Jazz` に落ちる）。1,000件の巨大な `BTreeMap` を渡されても、参照は `performer_index` の
  範囲チェック（`get`）のみで、範囲外キーは単に無視される。
- **極端な `DifficultyParams`**（`comfortable > impossible` など矛盾した設定）: `ramp` が
  `impossible <= comfortable` を検出して全域 0/100 に潰すため、逆転した閾値でも無限大や
  NaN を生成しない（不変条件6を参照）。
- **巨大規模でのハングではなく遅延として現れる劣化**: 4,000人×256セットでも O(n×m) の
  スケーリングを維持するため、フリーズではなく「ジョブの完了が遅くなる」形で劣化する
  （5節）。UIスレッドで直接呼ばれない限り、この遅延はユーザー操作をブロックしない。

## 7. テスト計画

**単体テスト**（`style.rs`）

- `StepStyle` 4種の `step_inches()` が既知の値（22.5/30.0/15.0/11.25）と一致する。
- `Custom { steps_per_five_yards: 0.0 }` や `NaN` でも `step_inches()` が有限・正数。
- `steps_for` の単調性（yardsが増えれば steps も増える）と `steps_for(0.0) == 0.0`。
- `to_yards`/`to_inches` が `Unit::Yards`/`Unit::Meters` 双方で往復一致すること
  （1回の変換だけで完結し、繰り返し変換しても誤差が蓄積しないことをプロパティテストで確認: ある
  距離をメートルへ1回変換してからヤードへ1回戻した値が、元の値と `1e-4` 未満の差であること）。
- `travel_speed_in_per_sec` の手計算検証: 22.5インチ・120bpm → 45.0 in/sec、
  22.5インチ・180bpm → 67.5 in/sec（同じ歩幅でもテンポで速度が変わることの直接検証）。
- `StrideThresholds::default().rate(...)` の境界値テスト（`aggressive_above` ちょうど、
  `impossible_above` ちょうど、その前後）。
- `classify_technique` の5パターン: facing=travel（Forward）、facing+180°（Backward）、
  facing+90°（SlideRight）、facing-90°（SlideLeft）、facing+45°（Jazz、いずれの許容域にも
  入らないこと）。
- `field_angle` の4方向の検算（0°/90°/180°/-90°、doc内コメントの検算例をそのままテスト化）。

**単体テスト**（`difficulty.rs`）

- `ramp` の境界値（`value == comfortable` → 0、`value == impossible` → 100、範囲外はクランプ）。
- `technique_score` が `ForwardMarch` で常に0、係数が最大の技法で常に100になること。
- `score_transition` を2セット・2演者の手作り `Document` で実行し、`breakdown` の各要素を
  手計算した期待値と突き合わせる（`continuity.rs` のテスト群と同じ「手作りセット」パターンを踏襲）。
- `crowding_score`: 3演者（1人が高速移動、2人が近接して静止）の合成シナリオで、静止2人の
  crowding が高く、動いている本人同士の速度差が無いケースでは crowding が低いことを確認する。
- `continuous_movement_runs`: 「動く→動く→静止→動く」という4セットの演者で、最初の2遷移分が
  1区間、最後の1遷移が別区間として抽出されること。`min_counts` を跨ぐ/跨がないケース両方。
- `hardest_performers`: 既知のスコアを持つ合成 `ShowDifficulty` で、`by_peak` の有無で
  順位が変わりうるケースを検証。

**ゴールデン/プロパティテスト**

- `Document::demo(10, 10)`（既存の他モジュールと同じ固定フィクスチャ）に対する
  `analyze_show_difficulty` の `overall_score` を一度計算し、期待レンジ（例: 0–20、ほぼ静的な
  デモ隊形なので低いはず）をアサートするゴールデン相当のテスト。
- プロパティテスト: ランダムな `Document`（演者・セット数・座標をQuickCheck風に生成、ただし
  `Document::validate` を通るもののみ）に対し、`analyze_show_difficulty` の全スコアが
  `[0, 100]` に収まること、`NaN` を含まないこと。
- プロパティテスト: `analyze_show_difficulty` を同一入力で2回呼び、結果が完全一致すること
  （決定論、不変条件7）。

**ストレス/ベンチ**

- `crates/drill-core/benches/core_performance.rs`（既存ファイル）に
  `difficulty_1000_performers_64_sets` ベンチを追加し、5節の見積もり（約3ms）を実測値で
  裏付ける。1,000人×64セットの合成ドキュメントを使い、`analyze_show_difficulty` 1回の所要時間と、
  2回目以降の `DifficultyScratch` 再利用時にヒープ確保が増えないことをアロケータフック
  または `Vec` キャパシティのポインタ比較（`interpolation_reuses_output_allocation` と同じ手法、
  `lib.rs:485-493`）で検証する。
- 上限規模（4,000人×256セット）でパニックなく完走することを確認するストレステスト
  （タイミングはアサートせず、完走のみを見る）。

## 8. 実装タスク

Codexに渡す粒度（1タスク=1〜3時間）。依存関係のないものは並行可能。

| # | タスク | 依存 | 見積り | 並行可否 |
|---|---|---|---|---|
| 1 | `style.rs` 新規作成: `StepStyle`・単位換算定数・`to_yards`/`to_inches`・単体テスト | なし | 1.5h | 可 |
| 2 | `style.rs` に `StrideRating`・`StrideThresholds`・`travel_speed_in_per_sec`・単体テスト | 1 | 1h | 1と直列、他とは並行可 |
| 3 | `style.rs` に `field_angle`・`wrap_pi`・`MarchingTechnique`・`classify_technique`・`TechniqueCoefficients`・単体テスト | なし | 2h | 可（1・2と並行可） |
| 4 | `Document` に `pub style: StepStyle`（`#[serde(default)]`）を追加し、既存 v1 JSON 読み込みテストを1件追加 | 1 | 0.5h | 1完了後、他タスクと並行可 |
| 5 | `difficulty.rs` 新規作成: `ramp`・`DifficultyBreakdown`・`DifficultyWeights`・`DifficultyParams`・`FacingOverrides` | 1,2,3 | 1.5h | 1-3完了後 |
| 6 | `difficulty.rs` に `DifficultyScratch`・`rebuild_neighbor_grid`・`crowding_score` | 5 | 2h | 5完了後 |
| 7 | `difficulty.rs` に `score_transition`（`pathing::transition_moves` 再利用）・単体テスト | 6 | 2.5h | 6完了後 |
| 8 | `difficulty.rs` に `analyze_show_difficulty`・`PerformerDifficulty`/`SetDifficulty`/`ShowDifficulty` | 7 | 1.5h | 7完了後 |
| 9 | `difficulty.rs` に `hardest_performers`・`continuous_movement_runs`・単体テスト | 7（6は不要、`transition_moves`のみ使用） | 1.5h | 7と並行着手可、統合は7後 |
| 10 | `style.rs` に `max_step_per_count_for_speed`（既存 `analyze_transition` 互換ヘルパー） | 2 | 0.5h | 2完了後、他と並行可 |
| 11 | `drill-app`: `main.rs:1011` の `analyze_transition` 呼び出しをタスク10のヘルパー経由に置き換え | 10 | 1h | drill-core側完了後 |
| 12 | ゴールデン/プロパティテスト一式（7節） | 8,9 | 2h | 8,9完了後 |
| 13 | `benches/core_performance.rs` にベンチ追加、5節の見積もりを実測で確認 | 8 | 1.5h | 8完了後、12と並行可 |

Wave構成: {1,3} → {2,4,10} → {5} → {6} → {7,9} → {8,11} → {12,13}。
`main.rs` 側の変更（タスク11）は [DESIGN_GAPS.md](../../DESIGN_GAPS.md) Wave 0/1 の
`analyze_transition` まわりの既存修正と同じファイルを触るため、実装順序の調整が必要
（コンフリクト回避のため、Wave 0 の `Edit`/`SetId` 導入が先に main.rs を書き換える場合は
タスク11をその後に回す）。

## 9. 未決事項

- **`GridConfig` と `StepStyle` の統合方針**: 現状 `GridConfig.horizontal_steps`/
  `horizontal_units` は編集グリッドのスナップ解像度であり、`Document.style` はショー全体の
  宣言的マーチングスタイルとして分離して定義した。両者が食い違う設定（例: グリッドは8-to-5相当の
  解像度なのに `style = SixToFive`）を許容するのか、`replace_grid` のように連動させるのかは
  製品判断が必要。座標系・記譜規約を扱う doc 16 との調整が必要。
- **`FacingOverrides` の永続化**: 本文書ではあえて `Document` に保存しないパラメータとして
  設計した。ディレクターが「このセクションはここでバックワードマーチ」と明示的に指定し、
  それをファイルに保存したいという要求が出た場合、A-2（doc 11）の `RouteTable` と同じ
  「疎な上書きテーブルをセットごとに持つ」パターンで正式にスキーマ化すべきだが、その所有権は
  doc 11 側に置くのが自然か、本doc側に置くのが自然かは未決定。
- **`aggressive_above`/`impossible_above`（速度）以外の全ての既定閾値は現場慣習値**:
  歩幅の1.5倍/3.0倍、方向転換45°/135°、連続移動32/96カウント、混雑度60in/secは、
  生体力学の実測ではなく設計上の仮置きである。ベータテストで実際のドリルデザイナーからの
  フィードバックを得て調整することを前提とし、全て `DifficultyParams` のフィールドとして
  設定可能にしてある。どの値を「製品既定値」として固定するかはリリース前に要検証。
- **`DifficultyWeights` はドキュメントに保存するか、アプリ設定か**: 同じショーファイルでも
  評価者によって重みを変えたいという需要と、「同じ `(Document, config)` から同じ結果」という
  決定論の不変条件（5番）を両立させるには、重みを `Document` の一部にせず「解析設定」として
  UI側（doc 43）に持たせる方が自然に見えるが、複数人でスコアを共有・比較する際に設定もセットで
  共有する必要が生じる。UI/永続化の設計判断としてdoc 43と調整が必要。
- **近傍探索グリッドと A-4 `ScanScratch` の重複**: 本文書の `rebuild_neighbor_grid`/
  `crowding_score` は、[DESIGN_GAPS.md](../../DESIGN_GAPS.md) A-4 が提案する衝突検査用
  `ScanScratch` と同種の一様グリッドを独立に実装している。A-4（doc 13）が実装された後、
  空間ハッシュ部分を共通クレート内関数として統合するかどうかは、doc 13 の実装内容を見てから
  判断する。
- **`MarchingTechnique::Jazz` の閾値 `CARDINAL_TOLERANCE_DEG = 30.0`**: 前後左右それぞれ
  ±30°の窓を「定型技法」、残り120°（4つの30°の隙間）を「ジャズ」とする設計は本文書独自の
  現場慣習的判断であり、実際のドリルデザイナーのレビューを受けていない。妥当性の検証が必要。
- **後方互換ヘルパー `max_step_per_count_for_speed` を使った `main.rs` 移行の完全性**:
  `analyze_transition` の `collision_distance` 引数（第3引数）は本文書の範囲外のまま残るため、
  A-4（doc 13）が衝突検査を再設計した際に、`analyze_transition` 自体を非推奨にして
  `score_transition`/`analyze_show_difficulty` へ完全移行するのか、`collisions` 用途だけ
  `analyze_transition` を残すのかは doc 13 の判断に委ねる。

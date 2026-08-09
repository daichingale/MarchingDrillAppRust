# 31. 動画書き出しパイプライン

## 1. 目的と範囲

`MEDIA_PIPELINE.md` の P0「2Dフィールドを決定論的にRGBAフレームへ描画し、背景workerでFFmpegへpipeし、
H.264 MP4へ出力し、内部音声をmuxし、ffprobeで検証する」を実装可能な粒度まで確定する。
あわせて P1（書き出しQueue・GPUエンコーダ検出）と P2（中断再開・3D Real View）への接続点を定義する。

この文書が決めること:

- 書き出しジョブの型・スナップショット境界・進捗・キャンセル・失敗分類
- 有理数フレーム時刻からカウントへの変換規則
- `DisplayList → RGBA` の CPU ラスタライザの選定根拠と日本語テキストの扱い
- CPU 経路と GPU 経路の二系統、および**どちらを既定にするかの結論**
- FFmpeg の起動・パイプ・進捗解析・プロセス回収・ライセンス判断
- 音声 PCM の受け取り方と音ズレを起こさない開始位置の決め方
- ffprobe による出力後検証の合格条件
- 失敗分類と software fallback、事前検査、部分ファイル処理

この文書が扱わないこと（境界で参照するのみ）:

| 事項 | 担当文書 |
|---|---|
| `DisplayList` / `DrawCmd` / `Scene` の定義と `build()` | 20-display-list |
| wgpu によるオフスクリーン描画の実装 | 21-gpu-renderer |
| 3D スタジアム形状・Real View の見た目 | 22-stadium-3d |
| `CameraShot` / count 単位キーフレーム | 23-camera |
| デコード・`PeakPyramid`・cpal 出力・`SyncAnchor` | 30-audio-engine |
| `Job<T>` / スレッドプール / 進捗チャネルの基盤 | 40-jobs |
| `DrillError` / `Locale` | 42-errors-i18n |
| `.drillproj` コンテナと相対アセットパス | 41-persistence |
| 配布形態・同梱物・価格 | 53-productization |

本書は上記のうち **40-jobs の `Job<T>`**、**20-display-list の `DisplayList`**、**30-audio-engine の PCM 供給**、
**42 の `DrillError`** を前提として設計する。それらが未確定の間は本書に示すアダプタ trait で受ける。

## 2. 現状

### 2.1 有るもの

| 位置 | 内容 |
|---|---|
| [video.rs:44-57](../../crates/drill-core/src/video.rs) | `VideoExportConfig`（解像度・fps・codec・container・backend・rate control・音声）。`u32` の `fps` のみで有理数フレームレートは表現できない |
| [video.rs:66-101](../../crates/drill-core/src/video.rs) | `ExportPreset` 4種からの構成生成 |
| [video.rs:103-129](../../crates/drill-core/src/video.rs) | `validate()`。偶数解像度・fps範囲・container×codec整合・quality上限・bitrate>0 を検査 |
| [video.rs:139-141](../../crates/drill-core/src/video.rs) | `frame_count(duration_seconds: f32) -> u64` = `ceil(d * fps)` |
| [video.rs:143-155](../../crates/drill-core/src/video.rs) | `estimated_megabytes()` |
| [video.rs:157-215](../../crates/drill-core/src/video.rs) | `ffmpeg_args()`。`-f rawvideo -pixel_format rgba -video_size WxH -framerate F -i pipe:0`、音声第2入力、`-map`、`-c:v`、`-crf`/`-b:v`、`-pix_fmt yuv420p`、`+faststart`、`-progress pipe:1 -nostats`、出力パス |
| [video.rs:218-231](../../crates/drill-core/src/video.rs) | `video_encoder()` — codec×backend から encoder 名（`libx264` / `h264_nvenc` / `h264_qsv` / `h264_amf` / `libx265` / `libsvtav1` / `libvpx-vp9`） |
| [tempo.rs:115-170](../../crates/drill-core/src/tempo.rs) | `seconds_at(count) -> f32` / `count_at(seconds) -> f32`。区分定数BPMの積分と逆写像。往復整合はテスト済み |
| [playback.rs:44-81](../../crates/drill-core/src/playback.rs) | `advance()`。決定論的・確保なし。**再生専用**で書き出しには使わない（後述 3.3） |
| [audio.rs:24-45](../../crates/drill-core/src/audio.rs) | `AudioTrack`（`offset_seconds` / `gain_db` / `muted` / `trim_*` / `fade_*`）。非破壊モデルのみ |
| [audio.rs:86-96](../../crates/drill-core/src/audio.rs) | `count_to_audio_time()` / `audio_time_to_count()` |
| [main.rs:1196-1276](../../crates/drill-app/src/main.rs) | 動画設定UI一式。プリセット・解像度・fps・品質・詳細設定・フレーム数と推定容量表示・`validate()` 表示 |
| [main.rs:1267-1273](../../crates/drill-app/src/main.rs) | FFmpeg 検出。`std::process::Command::new("ffmpeg").arg("-version").output()` を **UIスレッドで同期実行**している |
| [main.rs:1275](../../crates/drill-app/src/main.rs) | `ui.add_enabled(false, egui::Button::new("動画を書き出す（オフラインレンダラー接続後）"))` — ボタンは無効固定 |
| [assets/NotoSansJP.ttf](../../assets/) | 9,589,900 バイト。`OFL-OFL-NotoSansJP.txt`（SIL Open Font License 1.1）同梱済み |

### 2.2 無いもの

- **フレームを生成する実体が一切無い。** `DisplayList` も `drill-render` クレートも存在しない（`crates/` は `drill-core` と `drill-app` の2つのみ、[Cargo.toml](../../Cargo.toml) の `members`）。
- **`drill-export` クレートが無い。** 00-conventions のクレート表にある `drill-export` は未作成。
- **ラスタライザが無い。** `drill-app` の依存は `eframe` / `rfd` のみ（[drill-app/Cargo.toml](../../crates/drill-app/Cargo.toml)）。CPU ラスタライザのクレートは入っていない。
- **ジョブ基盤が無い。** `Job<T>` は `DESIGN_GAPS.md` B-3 の提案どまりで、コードには存在しない。
- **プロセス起動・パイプ・進捗解析・ffprobe 検証のコードが無い。** `ffmpeg_args()` は文字列を作るだけで誰も呼んでいない（`main.rs` からの呼び出しは grep で 0 件）。
- **音声 PCM が無い。** `drill-core::audio` は同期モデルのみで、`drill-core` の依存は `serde` / `serde_json` だけ（[drill-core/Cargo.toml](../../crates/drill-core/Cargo.toml)）。デコーダが無いので mux する PCM を作れない。
- **フレームレートが整数のみ。** `fps: u32` は 29.97 / 59.94 を表現できない。
- **`Document` にスナップショット機構が無い。** `Set { name, counts, positions }`（[lib.rs:233-238](../../crates/drill-core/src/lib.rs)）と `Document`（[lib.rs:241-252](../../crates/drill-core/src/lib.rs)）は `Clone` だが、書き出し中の編集から隔離する仕組みは無い。

### 2.3 現状コードの是正が必要な点

| # | 位置 | 問題 |
|---|---|---|
| V-1 | [video.rs:103](../../crates/drill-core/src/video.rs) | `validate()` が `Result<(), String>` で、しかも**日本語文字列リテラルが `drill-core` に埋まっている**。00-conventions「`drill-core` の中に日本語文字列リテラルを置かない」「`Result<_, String>` を新規に作らない」に違反。`VideoConfigError` + `Locale` へ移す |
| V-2 | [video.rs:191](../../crates/drill-core/src/video.rs) | `-shortest` を付けている。映像長が「先に終わった方」に依存するため、音声が1サンプル短いだけで出力尺が変わる。決定論を壊すので削除し `-frames:v` で確定させる（3.7） |
| V-3 | [video.rs:169](../../crates/drill-core/src/video.rs) | `-pixel_format rgba`。ラスタライザ（tiny-skia）の Pixmap は**乗算済みアルファ**、ffmpeg の `rgba` は**ストレートアルファ**。α<255 の画素があると色がずれる。加えて 25% 無駄な帯域を流す。`rgb24` へ変更する（3.4 / 5.3） |
| V-4 | [video.rs:139](../../crates/drill-core/src/video.rs) | `frame_count` が `f32` 演算。480秒×60fps 付近で丸めが不安定。有理数で計算する（3.3） |
| V-5 | [video.rs:211](../../crates/drill-core/src/video.rs) | 出力パスを素の文字列でそのまま渡している。`-` 始まりのファイル名や `concat:` 等のプロトコル解釈を許す。`file:` プロトコル明示が必要（6.4） |
| V-6 | [main.rs:1268](../../crates/drill-app/src/main.rs) | FFmpeg 検出を UI スレッドで同期実行。プロセス起動は 50〜200ms かかるので品質ゲート「UI thread で〜を行わない」に接する。ワーカーへ移す |
| V-7 | [video.rs:234](../../crates/drill-core/src/video.rs) | `quality_flag()` が `Auto` を `-crf` にしている。`Auto` が実際に NVENC へ解決された場合 `-crf` は無視され品質指定が失われる。encoder 解決後に flag を決める（3.6） |

## 3. 設計

### 3.1 クレート配置

00-conventions のクレート表に従い、新規に `drill-export` を作る。依存は上から下へのみ。

```
drill-core    (serde のみ)              VideoExportConfig / TempoMap / Document
   ▲
drill-render  (drill-core)              Scene → DisplayList          … 20-display-list
   ▲
drill-audio   (drill-core, symphonia)   RenderedAudio                … 30-audio-engine
   ▲
drill-export  (drill-render, drill-audio, tiny-skia, rustybuzz, ttf-parser, fs4)
   ▲
drill-app     (drill-export)            UI と入力変換のみ
```

`drill-export` のモジュール構成:

```
crates/drill-export/src/
  lib.rs
  video/
    mod.rs        VideoExportRequest / ExportOutcome / run_export
    clock.rs      FrameRate / frame_seconds / render_frame_count
    raster.rs     Rasterizer: DisplayList -> Pixmap -> rgb24
    text.rs       GlyphCache（NotoSansJP 固定バンドル）
    ffmpeg.rs     FfmpegInfo / locate / probe / args / spawn / ChildGuard
    progress.rs   -progress の key=value パーサ
    ffprobe.rs    ProbeResult / verify
    preflight.rs  空き容量・エンコーダ有無・上書き確認・書き込み可否
    fallback.rs   stderr 分類と software fallback 判定
    queue.rs      ExportQueue（P1）
    resume.rs     セグメント分割と再開（P2）
```

`VideoExportConfig` は**プロジェクト設定の一部**なので `drill-core::video` に残す（`serde` のみで完結する純データ）。
一方 `ffmpeg_args()` は「その環境で実際に使えるエンコーダ」を知らないと正しく組めないので `drill-export::video::ffmpeg` へ移し、
`drill-core` 側は `#[deprecated]` の薄いシムを1リリースだけ残す。

### 3.2 ジョブの型

```rust
use std::ops::Range;
use std::path::PathBuf;
use std::sync::Arc;
use drill_core::{Document, video::VideoExportConfig};

/// A snapshot-based, cancellable video export request.
///
/// Everything the worker needs is owned or `Arc`-shared here; the worker never
/// reads live application state. Submitting is the only synchronisation point.
pub struct VideoExportRequest {
    /// Immutable document snapshot. Editing during export cannot affect output.
    pub snapshot: Arc<Document>,
    /// Half-open global count range `[start, end)`.
    pub range: Range<f32>,
    pub config: VideoExportConfig,
    /// Camera, view mode and render options; see 20-display-list / 23-camera.
    pub scene: SceneConfig,
    /// Pre-rendered PCM aligned to `range.start`; see 30-audio-engine.
    pub audio: Option<RenderedAudio>,
    pub output: OutputTarget,
    /// Extra still frames appended after `range.end`, in seconds. Default 0.
    pub tail_seconds: f32,
    /// Backend choice. `Cpu` is the only bit-exact option (see 3.5).
    pub backend: RasterBackend,
}

/// Where the file goes, and how the final path is protected.
pub struct OutputTarget {
    /// Absolute, canonicalised parent directory. Must exist and be writable.
    pub directory: PathBuf,
    /// File name only, no separators, with the container extension.
    pub file_name: String,
    pub overwrite: Overwrite,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Overwrite {
    /// Fail with `OutputRefused::Exists` if the final path already exists.
    Refuse,
    /// The user was shown the existing file and confirmed replacement.
    Confirmed,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RasterBackend {
    Cpu,
    Gpu,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DeterminismClass {
    /// Same inputs → byte-identical frame stream, on any machine.
    BitExact,
    /// Same inputs → visually equivalent frames; bytes may differ per adapter.
    PerceptualOnly,
}

impl RasterBackend {
    pub fn determinism(self) -> DeterminismClass {
        match self {
            Self::Cpu => DeterminismClass::BitExact,
            Self::Gpu => DeterminismClass::PerceptualOnly,
        }
    }
}
```

`Arc<Document>` のスナップショットは submit 時に UI スレッドで1回 `Arc::new(doc.clone())` する。
基準規模（演者1,000 / セット64）で `positions` は 64 × 1,000 × 8 B = 512 KB、`performers` の `String` を含めても 1 MB 未満。
0.3 ms 程度の memcpy なので 16.6ms 予算に対して許容する（5.5）。

進捗と結果:

```rust
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ExportStage {
    Preflight,
    Rendering,
    Finalizing,   // stdin closed, ffmpeg flushing / faststart remux
    Verifying,
    Done,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct ExportProgress {
    pub stage_code: u8,          // ExportStage as u8, packed for AtomicU64 transport
    pub frames_written: u64,     // frames handed to ffmpeg
    pub frames_total: u64,
    pub encoded_us: i64,         // from ffmpeg -progress out_time_us
    pub encode_fps: f32,
    pub output_bytes: u64,
    pub used_software_fallback: bool,
}

pub struct ExportOutcome {
    pub path: PathBuf,
    pub frames: u64,
    pub duration_seconds: f64,
    pub bytes: u64,
    pub probe: ProbeResult,
    pub backend: RasterBackend,
    pub determinism: DeterminismClass,
    /// BLAKE3 of the concatenated raw frame stream. Golden-test anchor.
    pub frame_stream_hash: [u8; 32],
    pub encoder_used: String,
    pub elapsed: std::time::Duration,
    pub warnings: Vec<ExportWarning>,
}
```

失敗:

```rust
#[derive(Debug)]
pub enum ExportError {
    Config(drill_core::video::VideoConfigError),
    Range { start: f32, end: f32 },
    FfmpegNotFound,
    FfmpegRefused(FfmpegRefusal),          // untrusted / non-.exe / not a file
    FfmpegSpawn(std::io::Error),
    FfmpegExit { code: Option<i32>, stderr_tail: String },
    EncoderUnavailable { encoder: String },
    DiskSpace { needed_bytes: u64, available_bytes: u64 },
    OutputRefused(OutputRefusal),
    Audio(AudioSourceError),
    Probe(ProbeError),
    Verify(Box<VerifyReport>),
    Timeout { stage: ExportStage, waited: std::time::Duration },
    Io(std::io::Error),
    Cancelled,
}

impl ExportError {
    pub fn message(&self, locale: drill_core::Locale) -> String;
    /// UI hint: is a one-click retry with software encoding meaningful?
    pub fn suggests_software_fallback(&self) -> bool;
}
```

40-jobs の基盤へは次の形で載せる。`Job<T>` の具体形は 40 に従うが、本書はこの signature だけを要求する。

```rust
pub fn spawn_video_export(
    request: VideoExportRequest,
    ffmpeg: Arc<FfmpegInfo>,
) -> Job<Result<ExportOutcome, ExportError>>;
```

### 3.3 フレーム時刻（有理数）とカウントへの変換

```rust
/// Exact frame rate as a rational `num / den` in frames per second.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FrameRate {
    pub num: u32,
    pub den: u32,
}

impl FrameRate {
    pub const FILM: Self = Self { num: 24, den: 1 };
    pub const NTSC30: Self = Self { num: 30_000, den: 1_001 };
    pub const NTSC60: Self = Self { num: 60_000, den: 1_001 };

    pub fn integer(fps: u32) -> Self {
        Self { num: fps, den: 1 }
    }

    pub fn is_valid(self) -> bool {
        self.num >= 1 && self.den >= 1 && self.num as u64 <= 240 * self.den as u64
    }

    pub fn as_f64(self) -> f64 {
        self.num as f64 / self.den as f64
    }
}

/// Presentation time of frame `index`, in seconds from the start of the range.
///
/// A pure function of `index`: there is no running accumulator, so error does
/// not grow with frame count. `index * den` is exact in `f64` for every index
/// below 2^53 / den, i.e. far beyond any reachable export length.
#[inline]
pub fn frame_seconds(rate: FrameRate, index: u64) -> f64 {
    (index as f64 * rate.den as f64) / rate.num as f64
}

/// Number of frames covering `[0, duration)` at `rate`, plus optional tail.
///
/// Uses integer arithmetic so the result cannot wobble with `f32` rounding.
/// `duration_us` comes from `TempoMap` seconds converted once, at the boundary.
pub fn render_frame_count(rate: FrameRate, duration_us: u64, tail_us: u64) -> u64 {
    let total_us = duration_us.saturating_add(tail_us) as u128;
    // frames = ceil(total_us * num / (den * 1_000_000)), minimum 1
    let numerator = total_us * rate.num as u128;
    let denominator = rate.den as u128 * 1_000_000u128;
    (numerator.div_ceil(denominator) as u64).max(1)
}
```

**カウントへの変換**は毎フレーム次の一本道を通る。浮動小数の累加は一切しない。

```rust
pub struct FrameClock {
    rate: FrameRate,
    /// Show-relative seconds of the range start, i.e. `tempo.seconds_at(range.start)`.
    start_seconds: f64,
    total_frames: u64,
    /// Global count at which the show ends; frames past it clamp here.
    end_count: f32,
}

impl FrameClock {
    /// Global count rendered for frame `index`.
    ///
    /// `TempoMap` is `f32`-based; converting only at this boundary bounds the
    /// error at one `f32` ulp of the absolute time. At 512 s that is 6.1e-5 s,
    /// i.e. under 1/250 of a 60 fps frame. Verified by `frame_time_precision`.
    #[inline]
    pub fn count_at_frame(&self, tempo: &TempoMap, index: u64) -> f32 {
        let seconds = self.start_seconds + frame_seconds(self.rate, index);
        tempo.count_at(seconds as f32).min(self.end_count)
    }
}
```

`playback::advance` は**使わない**。あれは「経過秒を現在位置に足す」再生用の状態遷移で、
ループ折り返しと停止判定を持つ。書き出しは絶対時刻から直接引く方が単純かつ決定論的で、
かつ `range` の外へ出るケースが存在しない。この差は意図的なもので、両者が同じカウント列を出すことを
テスト `playback_and_export_agree_on_counts` で担保する（7.2）。

### 3.4 フレーム生成ループとバッファ再利用

```rust
/// One RGBA frame buffer, recycled through the pool. Never reallocated.
pub struct FrameBuffer {
    pixmap: tiny_skia::Pixmap,   // premultiplied RGBA8888, w*h*4
    packed: Vec<u8>,             // rgb24, w*h*3, written to the pipe
}

/// Per-worker scratch. Allocated once at warm-up, reused for every frame.
pub struct RenderScratch {
    display_list: drill_render::DisplayList,
    positions: Vec<drill_core::Point>,
    scan: drill_core::ScanScratch,
}

pub struct Rasterizer {
    width: u32,
    height: u32,
    glyphs: Arc<GlyphCache>,     // immutable after preflight warm-up
}

impl Rasterizer {
    /// Rasterise one display list into `frame`. No heap allocation after the
    /// first call: paths, strokes and masks all come from `scratch`/`self`.
    pub fn draw(&self, list: &DisplayList, frame: &mut FrameBuffer);

    /// Drop alpha and pack to rgb24 in place. Debug-asserts full opacity: the
    /// pixmap is premultiplied, so any alpha < 255 would ship wrong colours.
    pub fn pack_rgb24(&self, frame: &mut FrameBuffer) -> &[u8];
}
```

ワーカー構成（3スレッド種 + Nレンダラ）:

```
                 AtomicU64 next_index
                          │
    ┌─────────────────────┼─────────────────────┐
    ▼                     ▼                     ▼
render worker 0     render worker 1  …   render worker N-1
  scene_at(count) → build() → draw() → pack_rgb24()
    └──────── bounded channel (cap = 2N) of (index, FrameBuffer) ────────┐
                                                                        ▼
                                                          writer thread（順序復元）
                                                          BTreeMap<u64, FrameBuffer>
                                                          child.stdin.write_all(rgb24)
                                                          used buffers → FramePool

  progress thread : child.stdout → parse key=value → AtomicU64 へ publish
  stderr  thread  : child.stderr → 末尾 64 KiB のリングへ蓄積（必須・6.2）
```

- ワーカー数 `N = clamp(available_parallelism() - 1, 1, 6)`。UI 用に1コア残す。
  さらに `max_inflight_bytes`（既定 256 MB）から `N` を下げる: 1080p の `FrameBuffer` は 8.29 + 6.22 = 14.5 MB。
- 順序復元は `BTreeMap`。バッファ数を `2N + 2` に固定するので、writer が詰まればプールが枯れ、
  ワーカーが自然にブロックする。**バックプレッシャに追加の仕組みは要らない。**
- `frame_stream_hash` は writer スレッドが書き込み直前に BLAKE3 へ通す（順序が確定している唯一の場所）。
- キャンセルは `Arc<AtomicBool>`。レンダラは1フレームごと、writer は1書き込みごとに `load(Relaxed)` する。
  観測遅延は最大1フレーム分のレンダリング時間（≒4 ms）＋パイプ1回分。

### 3.5 CPU ラスタライザの選定

#### 候補と評価

| クレート | ライセンス | AA品質 | 速度 | 依存 | 判定 |
|---|---|---|---|---|---|
| **tiny-skia 0.12** | BSD-3-Clause | Skia の解析的AAをそのまま移植 | Skia比 x86-64 で 20–100% 遅い、AArch64 で 100–300% 遅い。それでも cairo / raqote より速いと作者が明言 | 純Rust。`tiny-skia-path` / `arrayref` / `bytemuck` / `png`(任意)。約14,000行・約200 KB | **採用** |
| raqote | MIT | Moz2D 由来。tiny-skia より品質・速度とも劣ると tiny-skia 側が比較 | tiny-skia より遅い | 純Rust | 不採用（品質・速度で下位互換） |
| cairo-rs | LGPL-2.1 / MPL-1.1 | 良好 | 良好 | **C依存**。Windows のビルド・同梱が重い | 不採用。**LGPL が販売製品の同梱義務を増やす**。C依存はCIコストとサプライチェーン面でも不利 |
| skia-safe | BSD-3-Clause | 最良（基準） | 最速 | **C++ ビルドが数GB・十数分**。独自フォントスタックを引き込む | 不採用。ビルドコストが品質に見合わない |
| vello / vello_cpu | Apache-2.0 OR MIT | 良好 | GPU前提。`vello_cpu` は若い | wgpu | 不採用（P0では時期尚早）。P2 で 3D 経路と併せて再評価 |

**結論: tiny-skia 0.12（BSD-3-Clause）を採用する。** 根拠は3点。

1. **ライセンス**: BSD-3-Clause は MIT/Apache-2.0 デュアルの本ワークスペースと衝突せず、
   販売する閉じたバイナリに静的リンクしても著作権表示のみで済む。cairo の LGPL とは義務の重さが違う。
2. **品質**: Skia の解析的アンチエイリアスの移植なので、egui（同じく高品質AA）とプレビュー／書き出しの
   見た目差が小さい。ドット1,000個の縁とヤードラインの細線が主役なので AA 品質は直接的に製品品質になる。
3. **速度と可搬性**: 純Rust・14,000行・SIMD対応（SSE2 / AVX2 / NEON / wasm-simd128）。
   C ツールチェーンを要求しないので、Windows / macOS / Linux の CI が同一手順で回る。

依存指定:

```toml
tiny-skia = { version = "0.12", default-features = false, features = ["std"] }
```

`png-format` は不要（PNG 出力は 20/50 側の担当で、動画経路では使わない）。
`simd` の扱いは 3.5.2 で決める。

#### 3.5.1 日本語テキスト

tiny-skia は**テキスト描画を持たない**（作者が最大の欠落として明記）。自前で組む。

```rust
/// Bundled-font-only text stack. System fonts are never consulted: they differ
/// per machine and would break the determinism contract.
pub struct GlyphCache {
    face: rustybuzz::Face<'static>,          // over assets/NotoSansJP.ttf
    /// key = (glyph id, size in 1/64 px, subpixel phase 0..4)
    masks: HashMap<GlyphKey, CachedGlyph>,
}

struct CachedGlyph {
    mask: tiny_skia::Mask,   // 8-bit coverage
    left: i32,
    top: i32,
}

impl GlyphCache {
    /// Load and verify the bundled font. `expected_sha256` is a compile-time
    /// constant: a swapped font would silently change every golden frame.
    pub fn load(font_bytes: &'static [u8], expected_sha256: &[u8; 32])
        -> Result<Self, TextError>;

    /// Shape with rustybuzz, then fill outlines through the same tiny-skia
    /// pipeline as every other shape, so text AA matches shape AA exactly.
    pub fn draw_text(
        &self,
        text: &str,
        origin: tiny_skia::Point,
        size_px: f32,
        anchor: Anchor,
        color: tiny_skia::Color,
        pixmap: &mut tiny_skia::Pixmap,
    );

    /// Pre-rasterise every glyph the export will need, on the submitting
    /// thread, before workers start. After this the cache is read-only and can
    /// be shared as `Arc<GlyphCache>` with no locking.
    pub fn warm_up(&mut self, texts: &[&str], sizes: &[f32]);
}
```

- **シェーピング**: `rustybuzz`（MIT、HarfBuzz v10.1.0 相当の完全移植、純Rust、C++コンパイラ不要）。
  日本語の仮名・漢字自体は複雑シェーピングを要さないが、カーニング・合字・将来の縦書き（`vert`/`vrt2`）と、
  英数字混植のときの正しい字送りに効く。決定論は「同じフォント＋同じテキスト＋同じ設定 → 同じグリフ列」で保証される。
- **アウトライン取得**: `ttf-parser`（rustybuzz が内部で使っているもの）の `OutlineBuilder` を
  `tiny_skia::PathBuilder` へ橋渡しし、`Pixmap::fill_path` で塗る。ラスタライズ経路が図形と完全に同一になるので、
  「テキストだけ AA の癖が違う」が起きない。
- **フォント**: `assets/NotoSansJP.ttf`（9.6 MB、OFL-1.1、再配布可）**のみ**。バイナリへ `include_bytes!` すると
  9.6 MB 増えるので、実行ファイル隣の `assets/` から実行時に読み、**SHA-256 を定数と照合**する。
  照合失敗は `TextError::FontMismatch` で書き出しを止める（黙って別のフォントで出さない）。
- **サブピクセル位置**: 1/4 px の4位相へ量子化してキャッシュキーに入れる。連続位置を許すとキャッシュが効かず、
  かつ丸め方が環境で揺れうる。量子化は決定論とキャッシュ効率の両方に効く。
- **キャッシュ規模**: 書き出しに出るテキストはセット名・カウント・ヤード番号・演者ラベル程度。
  基準規模で異なりグリフは高々 2,000 個、平均 24×24 px の 8bit マスクで約 1.2 MB。全ワーカーで共有する。

#### 3.5.2 SIMD と bit-exactness

tiny-skia の `simd` は**既定で有効**なフィーチャで、SSE2 / AVX2 / NEON / wasm-simd128 の経路を切り替える。
スカラ経路と SIMD 経路が bit-identical かどうかは、本設計では**仮定しない。テストで確定させる。**

- **Tier A（契約・常時強制）**: 同一ビルド・同一マシンで2回書き出すと `frame_stream_hash` が一致する。
  これは `MEDIA_PIPELINE.md` の「同じproject/configから同じフレーム列」の最低線であり、必ず満たす。
- **Tier B（CI で検証）**: x86-64（SSE2 のみ）/ x86-64 + AVX2 / aarch64 の3ランナーで同じ
  ゴールデンフレームのハッシュが一致するか。**一致すれば `simd` を有効のまま出荷する。**
- **Tier B が落ちた場合の退避**: `drill-export` に `deterministic-raster` フィーチャを置き、
  有効時は `tiny-skia` の `simd` を切る。リリースビルドではこれを有効にし、速度低下（見積り 1.5〜2.5×）を受け入れる。

この判断を先送りにしないため、7.2 のゴールデンテストを実装タスクの早い段階（T-05）に置く。

### 3.6 GPU 経路との二系統、および既定の決定

#### 矛盾の所在

`MEDIA_PIPELINE.md` の品質ゲートは「同じproject/configから同じフレーム列を生成する」と要求する。
一方 21-gpu-renderer のオフスクリーン描画は、原理的にこれを満たせない。

- ラスタライズの塗り規則（top-left rule）自体は D3D/Vulkan で規定されているが、
  **MSAA のサンプル位置はベンダ実装依存**であり、AA の結果は GPU ごとに異なる。
- シェーダの浮動小数は Vulkan/SPIR-V が **ULP 境界でしか規定していない**。
  `inversesqrt` / `pow` / 除算の精度、denormal の flush、FMA への縮約はドライバの裁量に入る。
  同一 GPU・同一ドライバなら再現するが、**別マシンでは再現しない**。
- テクスチャフィルタリングの丸め、ラインの太さ表現、深度ソートの同点順序も実装依存。

つまり「決定論」は**アプリの性質ではなくバックエンドの性質**である。ここが設計上の答えになる。

#### 結論

> **P0/P1 の 2D 書き出しは CPU（tiny-skia）を既定かつ唯一の経路とする。**
> **GPU 経路は 3D Real View（P2）専用のオプトインとし、そのとき決定論の等級を
> `BitExact` から `PerceptualOnly` へ明示的に落とす。UI はこれを文言で表示する。**

理由は4つ。

1. **決定論の契約を守れるのは CPU だけ。** 上記の通り GPU は bit-exact を約束できない。
   契約を緩める代わりに、`ExportOutcome` にバックエンド・アダプタ名・ドライバ版・`DeterminismClass` を記録し、
   「何が保証されている出力か」をファイル単位で追跡可能にする。契約を消すのではなく等級化する。
2. **GPU が無い環境でも書き出せなければならない。** CI・VM・リモートデスクトップ・古い内蔵GPUでは
   wgpu が使えるアダプタを取れないことがある。CPU 経路が既定なら、GPU 経路は純粋な加算機能になり、
   「GPU が無いと書き出せない」という製品事故が構造的に起こらない。
3. **2D では GPU が速いとは限らない。** ドリル1フレームの幾何は演者1,000個の小さな円と数百本の線しかない。
   ドローコールと状態変更の方が支配的で、しかも毎フレーム 8.29 MB のリードバックが PCIe を往復する。
   CPU ラスタライズ（見積り 4 ms/frame、6ワーカーで 300 fps）に対して優位が出にくい。
   GPU が明確に勝つのは、スタジアム形状・観客席・深度ソートを伴う 3D Real View の方である。
4. **UI スレッド汚染の回避。** GPU 経路を採る場合も、`drill-app` が持つ wgpu の `Device`/`Queue` を
   **共有してはならない**。共有するとサブミットとフェンス待ちが UI の描画キューに割り込み、
   品質ゲート6「UI スレッドでエンコード・描画を行わない」を実質的に破る。
   オフスクリーン用の `Instance`/`Device` をワーカー側で別途生成する。

GPU 経路の受け口（21-gpu-renderer が実装、本書は契約だけ定義）:

```rust
pub trait FrameSource: Send {
    /// Render one frame into `out` as rgb24, row-major, no padding.
    fn render_frame(&mut self, index: u64, out: &mut Vec<u8>) -> Result<(), ExportError>;
    fn determinism(&self) -> DeterminismClass;
    /// Identifies the renderer for reproducibility bookkeeping.
    fn fingerprint(&self) -> RendererFingerprint;
}

pub struct RendererFingerprint {
    pub backend: RasterBackend,
    pub renderer_version: u32,
    pub adapter: Option<String>,      // e.g. "NVIDIA GeForce RTX 4070 / 560.94"
}
```

GPU 実装側の注意点として本書が要求する2点:
`wgpu::COPY_BYTES_PER_ROW_ALIGNMENT`（256 B）に合わせたバッファから**パディングを剥がしてから**書くこと、
テクスチャフォーマットは `Rgba8Unorm`（sRGB 変換をシェーダで明示）とし、`Rgba8UnormSrgb` の暗黙変換に依存しないこと。

### 3.7 FFmpeg 連携

#### 3.7.1 発見と検証

```rust
pub struct FfmpegInfo {
    pub path: PathBuf,
    pub ffprobe_path: PathBuf,
    pub version: String,
    /// Encoder names reported by `ffmpeg -hide_banner -encoders`.
    pub encoders: BTreeSet<String>,
    /// `--enable-gpl` present in the build configuration.
    pub gpl_build: bool,
    pub configuration: String,
}

pub enum FfmpegSource {
    /// Explicit path from application settings (never from a project file).
    UserSetting(PathBuf),
    /// `bin/ffmpeg[.exe]` next to the running executable.
    Bundled,
    /// Resolved from PATH.
    Path,
}

pub fn locate(setting: Option<&Path>) -> Result<PathBuf, ExportError>;
pub fn probe_ffmpeg(path: &Path) -> Result<FfmpegInfo, ExportError>;
```

探索順は `UserSetting → Bundled → Path`。どの経路でも次を満たさないものは `FfmpegRefused` で拒否する。

- 実在する**通常ファイル**であること（ディレクトリ・シンボリックリンク先の不在を拒否）
- **Windows では拡張子が `.exe` であること。** `.bat` / `.cmd` は `std::process::Command` の
  Windows 実装が `cmd.exe` を経由するため、引数のクォート規則が異なり注入の余地がある（CVE-2024-24576 系）。
  受け付けない。
- **プロジェクトファイル由来のパスを絶対に受け付けない。** FFmpeg の位置はアプリ設定のみ。
  他人から受け取った `.drillproj` が任意の実行ファイルを指せてはならない（6.4）。

`probe_ffmpeg` はプロセスを2回起動する（`-version` と `-encoders`）ので **必ずワーカーで**実行し、
`(path, mtime, len)` をキーに結果をアプリ側でキャッシュする。V-6 の是正。

#### 3.7.2 エンコーダ解決と引数

```rust
pub struct ResolvedEncoder {
    pub name: String,           // "libx264" / "h264_nvenc" / "h264_mf" / ...
    pub quality_flag: &'static str,   // "-crf" | "-cq" | "-q:v" | "-global_quality"
    pub is_hardware: bool,
}

/// Pick the first available encoder for `codec` under `backend`, consulting
/// `info.encoders`. Returns `EncoderUnavailable` if none of the candidates
/// exists in this build, so we never spawn a doomed process.
pub fn resolve_encoder(
    codec: VideoCodec,
    backend: EncoderBackend,
    info: &FfmpegInfo,
) -> Result<ResolvedEncoder, ExportError>;
```

H.264 の `Auto` の優先順（ライセンス判断 3.7.6 と整合）:

```
h264_nvenc → h264_qsv → h264_amf → h264_videotoolbox → h264_mf → libopenh264 → libx264
```

`libx264` を最後に置くのは意図的である（GPL ビルドでしか存在しないため）。
`quality_flag` は**解決後の encoder 名から決める**。`libx264`/`libx265`/`libsvtav1`/`libvpx-vp9` は `-crf`、
`*_nvenc` は `-cq`、`*_qsv` は `-global_quality`、`*_amf` は `-qp_i/-qp_p`、`h264_mf` は `-quality` を持たないので
`RateControl::Bitrate` へ強制降格し `ExportWarning::RateControlDowngraded` を積む。V-7 の是正。

引数生成:

```rust
pub fn build_args(
    config: &VideoExportConfig,
    encoder: &ResolvedEncoder,
    frames: FrameSpec,
    audio: Option<&AudioInputSpec>,
    temp_output: &Path,
) -> Vec<String>;

pub struct FrameSpec {
    pub width: u32,
    pub height: u32,
    pub rate: FrameRate,
    pub total: u64,
}
```

生成される列（1080p60・H.264・音声あり・MP4 の例）:

```
-hide_banner -nostdin -loglevel error
-fflags +bitexact
-f rawvideo -pixel_format rgb24 -video_size 1920x1080 -framerate 60/1
-thread_queue_size 512 -i pipe:0
-f f32le -ar 48000 -ac 2 -thread_queue_size 512 -i file:C:\...\drillforge-audio-<nonce>.f32
-map 0:v:0 -map 1:a:0
-c:v libx264 -crf 20 -pix_fmt yuv420p
-color_primaries bt709 -color_trc bt709 -colorspace bt709
-frames:v 28800
-c:a aac -b:a 192k
-movflags +faststart
-progress pipe:1 -nostats
-y file:C:\...\show.mp4.part-1234-a1b2c3
```

現状からの変更点:

- `-pixel_format rgba` → **`rgb24`**（V-3）。乗算済みアルファ誤差を構造的に排除し、パイプ帯域を 25% 削減。
- `-shortest` → **削除**し `-frames:v <total>` を追加（V-2）。映像長を入力ではなく引数で確定させる。
  音声 PCM は `total * den * sample_rate / num` サンプルちょうどに整形するので、両ストリームの長さは設計上一致する。
- `-framerate 60/1` と有理数表記。29.97 は `30000/1001`。
- `-nostdin` を明示。`-stdin` は「標準入力を入力として使う場合は既定で off」だが、明示して意図を固定する。
- `-fflags +bitexact` でエンコーダ／ミュクサのバージョン文字列をファイルから外す。
  同一 FFmpeg 版でのファイル単位バイト再現に効く（**主契約はあくまでフレーム列ハッシュ**であり、これは付随的な保証）。
- `-thread_queue_size` を両入力に付ける。既定値だとパイプ入力で `Thread message queue blocking` の警告と
  スループット低下が出る。
- `-loglevel error`。`-progress` は別ストリームなので進捗は失わない。
- 出力は**必ず一時パス**（3.7.5）。`-y` はその一時パスに対してのみ意味を持つ。
- 全パスに `file:` プロトコルを明示（V-5 / 6.4）。

#### 3.7.3 起動とプロセス寿命

```rust
/// Owns the child and guarantees it is reaped exactly once, on every path.
pub struct ChildGuard {
    child: Option<std::process::Child>,
}

impl ChildGuard {
    /// Success path: close stdin first so ffmpeg can flush and write `moov`,
    /// then wait. Never kill on success — killing truncates the MP4 index.
    pub fn finish(mut self, timeout: Duration) -> Result<ExitStatus, ExportError>;
    /// Cancel / error path: kill, then wait. `wait` is mandatory or the child
    /// becomes a zombie on Unix and a leaked handle on Windows.
    pub fn abort(&mut self);
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        self.abort();
    }
}
```

起動:

```rust
let mut command = std::process::Command::new(&info.path);
command
    .args(&args)                       // Vec<String>: no shell string, ever
    .stdin(Stdio::piped())
    .stdout(Stdio::piped())            // -progress pipe:1
    .stderr(Stdio::piped())            // diagnostics ring buffer
    .current_dir(&output.directory);   // relative-path mistakes cannot escape

#[cfg(windows)]
{
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    command.creation_flags(CREATE_NO_WINDOW);   // no console flash per export
}
```

必ず守る3点:

1. **stderr を必ず読み続ける。** 誰も読まないと OS のパイプバッファ（64 KiB）が埋まり、
   FFmpeg が `write` でブロックして**デッドロックする**。専用スレッドで末尾 64 KiB のリングへ流し込む。
2. **`wait()` はちょうど1回。** `ChildGuard::Drop` に集約し、`?` による早期 return でも必ず回収されるようにする。
3. **成功時は kill しない。** stdin を drop → `wait()` の順。`+faststart` は `moov` を先頭へ移すために
   出力全体を書き直すので、この待ち時間は長くなりうる（720 MB で NVMe 約6秒）。
   `finish` の timeout は `max(120 s, estimated_bytes / 20 MB/s)` を既定とし、超過は `Timeout` として報告する（ハングさせない）。

**Broken pipe の扱い**: `write_all` が `ErrorKind::BrokenPipe` を返したら、それは症状であって原因ではない。
「パイプが壊れました」とユーザへ出してはならない。必ず `wait()` して stderr の末尾を読み、
`Unknown encoder 'h264_nvenc'` / `No space left on device` のような**本当の理由**に変換して `FfmpegExit` で返す。

#### 3.7.4 進捗解析

FFmpeg の `-progress` は `key=value` 行を周期的に出し、各ブロックの最後が
`progress=continue`（途中）または `progress=end`（完了）になる。

```rust
#[derive(Default, Clone, Copy)]
pub struct FfmpegProgress {
    pub frame: u64,
    pub fps: f32,
    /// Microseconds. NOTE: ffmpeg's `out_time_ms` key is misnamed — its value
    /// is microseconds (AV_TIME_BASE), identical to `out_time_us`. Both are
    /// parsed into this single field.
    pub out_time_us: i64,
    pub total_size: u64,
    pub drop_frames: u64,
    pub dup_frames: u64,
    pub ended: bool,
}

/// Parse one `key=value` line. Unknown keys are ignored; malformed values leave
/// the field untouched. Never panics, never allocates.
pub fn apply_progress_line(line: &str, out: &mut FfmpegProgress) -> bool;
```

- `out_time_ms` は**名前に反してマイクロ秒**（`AV_TIME_BASE`）。`out_time_us` と同値として扱う。
  この罠を単体テストで固定する（7.1）。
- 値が `N/A` になる行があるのでパースは失敗許容。`total_size` が `N/A` の間は前値を維持。
- 全体の進捗は `frames_written / frames_total`（自前カウンタ、レンダ側の真値）を主とし、
  ffmpeg 側の `frame` は「エンコーダがどこまで消化したか」の副表示に使う。両者の乖離はバッファ内在庫を意味する。
- **UI への通知は 10 Hz まで**に絞る。`egui::Context::request_repaint_after(100ms)` を使い、
  進捗バーのために UI フレームを焼かない（5.5）。

#### 3.7.5 一時ファイルと原子的確定

```
出力先ディレクトリ/
  show.mp4                       ← 確定後にだけ現れる
  show.mp4.part-<pid>-<nonce>    ← ffmpeg が書く先
  drillforge-audio-<nonce>.f32   ← mux 用 PCM（完了後に削除）
```

- FFmpeg は**必ず一時パスへ書く**。既存の `show.mp4` は、検証に合格するまで一切触らない。
  途中でクラッシュしても、利用者の前回の成功出力は無傷で残る（`00-conventions` のデータ喪失防止）。
- 一時ファイルは同一ディレクトリに置く（`rename` を同一ボリューム内に閉じ、原子的置換を成立させる）。
- 検証合格後に `std::fs::rename(temp, final)`。Windows の `rename` は既存ファイルがあると失敗するので、
  `Overwrite::Confirmed` のときのみ `ReplaceFileW` 相当（`fs::rename` は Windows でも上書きするが、
  読み取り専用属性で失敗しうる）を試み、失敗時は `OutputRefused::ReplaceFailed` を返して一時ファイルを残さない。
- `TempFileGuard` が `Drop` で削除する。`commit()` を呼んだ場合のみ削除しない。

### 3.8 音声の mux

30-audio-engine から受け取る形:

```rust
/// PCM already carrying trim, gain, mute and fades. No further processing here.
pub struct RenderedAudio {
    pub sample_rate: u32,
    pub channels: u16,
    /// Interleaved f32, exactly `frames * channels` samples long.
    pub samples: Arc<[f32]>,
}

pub trait AudioSource: Send + Sync {
    /// Render exactly `frames` sample-frames starting at `start_sample`, which
    /// may be negative (the show begins before the file does) or run past the
    /// end. Out-of-file regions are filled with silence, not clamped or looped.
    fn render_window(
        &self,
        start_sample: i64,
        frames: u64,
    ) -> Result<RenderedAudio, AudioSourceError>;
}
```

**開始位置の合わせ方（音ズレを起こさない唯一の手順）**:

```rust
/// Sample index in the source file that must land on video frame 0.
///
/// Rounded once, in integer sample space. Everything downstream is integers,
/// so there is no place left for a drift to accumulate.
pub fn start_sample(track: &AudioTrack, tempo: &TempoMap, start_count: f32, sample_rate: u32) -> i64 {
    let seconds = drill_core::audio::count_to_audio_time(track, tempo, start_count) as f64;
    (seconds * sample_rate as f64).round() as i64
}

/// Number of sample-frames matching exactly `total_frames` video frames.
pub fn audio_frame_count(total_frames: u64, rate: FrameRate, sample_rate: u32) -> u64 {
    let n = total_frames as u128 * rate.den as u128 * sample_rate as u128;
    let d = rate.num as u128;
    (n + d / 2).div_euclid(d) as u64      // round to nearest
}
```

規則:

- **FFmpeg には一切のオフセット引数を渡さない。** `-ss` / `-itsoffset` / `-af adelay` を使わない。
  それらはデコーダのシーク挙動やコンテナのプライミングサンプルに依存し、環境で数十 ms 動きうる。
  代わりに、**サンプル 0 が映像フレーム 0 に一致する PCM を我々が作る。**
- `start_sample < 0` の分は先頭へ無音を足す。ファイル末尾を越える分は末尾へ無音を足す。
  結果の長さは常に `audio_frame_count()` ちょうど。
- 秒ではなく**サンプル索引で丸める**（`00-conventions` 不変条件4）。丸めは上の1箇所だけ。
- PCM は生の `f32le` として一時ファイルへ書き、`-f f32le -ar <sr> -ac <ch> -i file:<tmp>` で第2入力にする。
  8分・48 kHz・ステレオで 184 MB。名前付きパイプ／FIFO を第2入力にする案は、
  Windows と Unix で実装が分岐し失敗モードが増えるので採らない。
- **リサンプルはしない。** 必要なら 30-audio-engine 側（決定論的な rubato）で済ませてから渡す。
  ffmpeg の `aresample` に任せるとリサンプラの既定値がビルド依存になる。
- **OS ループバック録音は使わない**（`MEDIA_PIPELINE.md` の明示要求）。
- 既知の残差: AAC は約1024サンプル（21 ms）のエンコーダ遅延を持ち、MP4 は `edts`（および `iTunSMPB`）で
  補正情報を書く。準拠プレイヤーは補正するが、非準拠プレイヤーでは最大 21 ms 先行して聞こえうる。
  Opus/WebM も `pre-skip` で同じ構造。これは我々の同期ではなくコンテナ規約の問題なので、
  `ExportWarning::EncoderDelayCompensationRequired` として記録するにとどめる。
- `track.muted` または `config.audio_enabled == false` のときは第2入力を組まない。
  「音声トラック無し」は仕様どおりの成功であり、検証もそれに合わせる（3.9）。

### 3.9 書き出し後検証（ffprobe）

```rust
pub struct ProbeResult {
    pub format_name: String,
    pub duration_seconds: f64,
    pub size_bytes: u64,
    pub video: VideoStreamInfo,
    pub audio: Option<AudioStreamInfo>,
}

pub struct VideoStreamInfo {
    pub codec_name: String,
    pub width: u32,
    pub height: u32,
    pub r_frame_rate: (u32, u32),
    pub avg_frame_rate: (u32, u32),
    pub nb_frames: Option<u64>,
    pub pix_fmt: String,
}

pub struct AudioStreamInfo {
    pub codec_name: String,
    pub channels: u16,
    pub sample_rate: u32,
    pub duration_seconds: f64,
}

pub fn probe(ffprobe: &Path, file: &Path) -> Result<ProbeResult, ProbeError>;

pub struct VerifyReport {
    pub checks: Vec<(VerifyCheck, bool, String)>,   // check, passed, observed
}

pub fn verify(
    probe: &ProbeResult,
    expected: &ExpectedOutput,
) -> Result<(), Box<VerifyReport>>;
```

呼び出し: `ffprobe -v error -hide_banner -print_format json -show_format -show_streams file:<path>`

**合格条件**（すべて満たすまで `Done` にしない）:

| # | 検査 | 条件 |
|---|---|---|
| 1 | 解像度 | `width == config.width && height == config.height`（完全一致） |
| 2 | フレームレート | `r_frame_rate` を**有理数のまま**約分比較して `rate` と一致（浮動小数比較にしない） |
| 3 | フレーム数 | `nb_frames == total_frames`。`nb_frames` が無いコンテナでは `|duration − total/rate| ≤ 2/fps` |
| 4 | コーデック | `codec_name` が要求 codec の族（`h264` / `hevc` / `av1` / `vp9`）に属する |
| 5 | 画素形式 | `pix_fmt == "yuv420p"`（設定どおり） |
| 6 | 音声 | `audio_enabled` かつ音源ありのとき、音声ストリームがちょうど1本、`channels ≥ 1`、`sample_rate` 一致、`|audio_duration − video_duration| ≤ 0.05 s` |
| 7 | 音声なし | `audio_enabled == false` のとき音声ストリームが 0 本 |
| 8 | コンテナ | `format_name` に期待コンテナ名を含む |
| 9 | サイズ | `size_bytes > 0` かつ推定値の 1/20 〜 20 倍の範囲（極端な破綻の検出） |
| 10 | faststart | MP4 かつ `faststart` のとき、**先頭 64 KiB を自前で読み**、box ヘッダを走査して `moov` が `mdat` より前にあること（ffprobe 不要・数ミリ秒） |

`-count_frames` は**既定で使わない**。全フレームをデコードするので 8 分 1080p60 で数分かかる。
MP4/MOV は `stsz` のサンプル数から `nb_frames` が正確に得られるのでそれで足りる。
`-count_frames` は設定の「厳密検証」チェックで opt-in にする（WebM のように `nb_frames` を持たない場合の保険）。

**検証失敗時**: 一時ファイルを**確定パスへ rename しない**。既定では一時ファイルを削除する。
設定「検証に失敗したファイルを残す」が有効なら `<final>.failed.<ext>` へ改名して残す（サポート用）。
ジョブは `Err(Verify(report))` で終わり、UI は `VerifyReport` の不一致行を表として出す。
**部分的に成功した扱いは存在しない。** 確定パスには「完全に検証済み」か「何も無い」かの2状態しかない。

### 3.10 失敗と回復

#### 3.10.1 事前検査（preflight）

```rust
pub struct PreflightReport {
    pub total_frames: u64,
    pub estimated_bytes: u64,
    pub required_bytes: u64,
    pub available_bytes: u64,
    pub encoder: ResolvedEncoder,
    pub audio_temp_bytes: u64,
    pub warnings: Vec<ExportWarning>,
}

pub fn preflight(request: &VideoExportRequest, info: &FfmpegInfo)
    -> Result<PreflightReport, ExportError>;
```

| 検査 | 失敗時 |
|---|---|
| `config.validate()` | `Config` |
| `range.end > range.start`、`total_frames ≥ 1`、`total_frames ≤ 4×3600×240` | `Range` |
| `width * height * 4` と `* total_frames` を `u64::checked_mul` で確認 | `Range`（オーバーフロー拒否） |
| `resolve_encoder()` が `info.encoders` に存在 | `EncoderUnavailable`（プロセスを起動する前に判る） |
| 出力ディレクトリが存在・書き込み可（プローブファイルを作って消す） | `OutputRefused::NotWritable` |
| 確定パスが存在し `Overwrite::Refuse` | `OutputRefused::Exists` |
| 空き容量 ≥ `estimated_bytes * 1.5 + audio_temp_bytes + 64 MB`。`fs4::available_space()`（Apache-2.0 OR MIT、薄い） | `DiskSpace { needed, available }` |
| `faststart` かつ推定 2 GB 超 → 警告（remux で同容量の一時領域が要る） | `ExportWarning::FaststartLargeFile` |
| フォント SHA-256 照合とグリフ warm-up | `Audio`/`TextError` |

#### 3.10.2 software fallback（1回だけ）

```rust
/// Classify an ffmpeg failure from its exit code and stderr tail.
pub fn classify(code: Option<i32>, stderr_tail: &str) -> FailureKind;

pub enum FailureKind {
    /// Hardware encoder could not be initialised → software retry is warranted.
    EncoderInit,
    DiskFull,
    /// Our own writer failed, or the user cancelled. Never retry.
    Local,
    Unknown,
}
```

`EncoderInit` と判定する stderr の部分文字列（テーブル駆動・単体テストで固定）:

```
"Unknown encoder"
"Cannot load nvcuda"
"OpenEncodeSessionEx failed"
"No capable devices found"
"No NVENC capable devices found"
"Error initializing output stream"
"Error while opening encoder"
"Device creation failed"
"Function not implemented"
"InitializeEncoder failed"
```

再試行の条件（**すべて**満たすときのみ、**ちょうど1回**）:

- `classify() == EncoderInit`
- `resolved.is_hardware == true`
- `attempt == 0`
- キャンセルされていない

再試行の内容: `backend = Software` へ書き換えて `resolve_encoder()` をやり直す。
要求 codec のソフトウェアエンコーダも無ければ H.264 へ降格する（`libopenh264` → `libx264` の順）。
一時ファイルを削除し、**フレーム 0 から再実行**する（部分再利用はしない。GOP 境界の整合を保証できない）。
`ExportProgress::used_software_fallback = true` を立て、完了時に
「ハードウェアエンコーダが使えなかったため CPU で書き出しました」と伝える（所要時間が伸びた理由の説明になる）。

**再試行しないもの**: 自前の書き込みエラー、`DiskFull`、キャンセル、検証失敗、`OutputRefused`。
2回目の `EncoderInit` は終端。

#### 3.10.3 中断再開（P2）

```rust
pub struct ExportFingerprint([u8; 32]);   // hash(document, config, range, scene, renderer_version)

pub struct ResumeManifest {
    pub fingerprint: ExportFingerprint,
    pub rate: FrameRate,
    pub segment_frames: u64,
    pub segments: Vec<SegmentState>,      // Pending | Done { path, bytes, sha256 }
}
```

範囲を固定長セグメント（既定 `fps * 10` フレーム）へ分け、各セグメントを同一パラメータで
`seg-NNNN.<ext>` へ書き、最後に `-f concat -safe 1 -i list.txt -c copy` で連結する。
再開時は `fingerprint` が一致するセグメントだけを再利用する。

P2 に置く理由: 連結のために **closed GOP を強制**（`-g <fps*2> -force_key_frames "expr:gte(t,n_forced*2)"`）する必要があり、
同じ CRF でも画質とサイズがわずかに悪化する。また `concat` の stream copy は
コーデックパラメータが完全一致していることを要求するため、途中で FFmpeg のバージョンが変わると再利用できない。
P0 の価値（8分で5分）に対して代償が大きい。

### 3.11 書き出しキュー（P1）

```rust
pub struct ExportQueue {
    entries: VecDeque<QueueEntry>,
    /// Exports never run concurrently: each one already saturates the machine.
    running: Option<JobHandle>,
}

pub struct QueueEntry {
    pub label: String,
    pub request: VideoExportRequest,
    pub state: QueueState,      // Waiting | Running | Done(ExportOutcome) | Failed(ExportError)
}

impl ExportQueue {
    /// Fan out one range over several configs (1080p / 4K / vertical / square).
    pub fn expand_resolutions(base: &VideoExportRequest, presets: &[VideoExportConfig]) -> Vec<QueueEntry>;
    /// Fan out one config over several camera shots (see 23-camera).
    pub fn expand_cameras(base: &VideoExportRequest, shots: &[SceneConfig]) -> Vec<QueueEntry>;
}
```

- **同時実行は1件**。書き出しは CPU とディスク帯域を使い切るので、並べても総時間は縮まず失敗率だけ上がる。
- `snapshot: Arc<Document>` はキュー内の全エントリで共有する（クローンは1回だけ）。
- 1件が失敗しても後続は続行し、最後にまとめて結果表を出す。
- キュー全体の所要時間見積りを合計して表示する（5.4 のモデルを使う）。

## 4. 不変条件

テストで検証できる形で列挙する。括弧内は 7 章の対応テスト。

1. **フレーム時刻は非累加**。`frame_seconds(rate, n)` は `n` の純関数であり、
   ループ加算で求めた値と一致しない場合はループ側が誤りである。
   28,800 フレーム分をループ加算した結果と `frame_seconds` の差が観測できること（`frame_time_no_accumulation`）。
2. **フレーム時刻の精度**。`|count_at_frame(n) − exact| < 1/(4·fps)` カウント相当。480 秒・60 fps で検証（`frame_time_precision`）。
3. **決定論（Tier A）**。同じ `(document, config, range, scene, renderer_version)` の2回の実行で
   `frame_stream_hash` が一致する（`export_is_reproducible`）。
4. **決定論（Tier B）**。CPU バックエンドでは、SSE2 / AVX2 / NEON の3ターゲットで
   ゴールデンフレームのハッシュが一致する（`golden_frames_cross_target`、CI 3ランナー）。
5. **確定パスは二値**。書き出し終了後、確定パスには「ffprobe 検証に合格したファイル」が在るか、
   何も無いかのどちらかしかない。中間状態のファイルが確定パスに残ることはない（`failed_export_leaves_no_output`）。
6. **プロセス回収**。どの終了経路（成功・失敗・キャンセル・パニック）でも子プロセスは
   ちょうど1回 `wait()` され、ゾンビも孤児も残さない（`cancel_reaps_child`）。
7. **stdout/stderr は常に排出される**。stderr を読まないことによるデッドロックが起きない
   （`ffmpeg_flooding_stderr_does_not_deadlock`、64 KiB 超の stderr を出す fake ffmpeg で検証）。
8. **フレーム順序の完全性**。ffmpeg へ渡ったフレームは index の狭義単調増加で、欠落も重複も無く、
   総数は `total_frames` に等しい（`writer_preserves_order`）。
9. **A/V 開始一致**。音声のサンプル 0 が映像フレーム 0 に対応し、
   `|audio_duration − video_duration| < 1/fps`（`audio_video_lengths_match`）。
10. **不透明性**。パイプへ書く直前のフレームは全画素 α = 255 である
    （`debug_assert` + `frame_is_fully_opaque`）。乗算済み／ストレートの取り違えを構造的に防ぐ。
11. **確保ゼロ**。warm-up 後のフレームループでヒープ確保が発生しない。
    `FrameBuffer` と `RenderScratch` のポインタ同一性で検証する
    （既存の `interpolation_reuses_output_allocation` と同じ手法、`raster_reuses_buffers`）。
12. **信頼できない入力の隔離**。プロジェクトファイル由来の文字列が、
    実行ファイルパスにも ffmpeg の引数にもならない（`project_paths_never_reach_process_args`）。
13. **UI 非関与**。書き出し中、UI スレッドは進捗の読み出し（アトミック3回と1回の整形）以外に
    何もしない。デコード・描画・エンコードを行わない（コードレビューの不変条件、および 5.5 の計測）。
14. **決定論等級の表明**。`ExportOutcome.determinism == BitExact` は
    `backend == Cpu` のときにのみ立つ（`gpu_never_claims_bit_exact`）。

## 5. 性能

### 5.1 1フレームあたりのコスト（1080p・演者1,000人・CPU 1コア）

| 工程 | 内訳 | 見積り |
|---|---|---|
| `positions_at_count` + シーン構築 | 1,000点の補間、連続配列 | 0.05 ms |
| `DisplayList::build` | 約1,200 `DrawCmd`（ドット1,000・線120・テキスト40） | 0.10 ms |
| `Pixmap` クリア | 8.29 MB の memset（実効 16 GB/s） | 0.50 ms |
| フィールド塗り + ヤードライン120本 | 全幅の AA 線 | 1.00 ms |
| ドット1,000個 | パス構築 1.5 µs × 1,000 + 被覆約150 px の blend | 1.90 ms |
| テキスト約40ラベル | キャッシュ済みマスクの blit | 0.15 ms |
| `pack_rgb24` | 8.29 MB → 6.22 MB、レーン単位 | 0.30 ms |
| **合計** | | **≈ 4.0 ms/frame** |

### 5.2 スループット

6ワーカーで理論 1,500 fps だが、1フレームあたり最低 3 回（clear / composite / pack）
8.29 MB クラスの領域を触るので約 25 MB/frame のメモリトラフィックが出る。
DDR4-3200 デュアルチャネルの実効 35〜40 GB/s では **約 300 fps が上限**。
帯域律速であってコア律速ではない、と設計上認識しておく（コアを増やしても伸びない）。

**採用値: 300 fps（1080p、6ワーカー）**。4K は画素4倍なので約 75 fps。

### 5.3 パイプ帯域

1080p60・8分 = 28,800 フレーム。

| 画素形式 | 1フレーム | 総量 |
|---|---|---|
| `rgba`（現状） | 8.29 MB | **239 GB** |
| `rgb24`（本設計） | 6.22 MB | **179 GB** |

匿名パイプの実効スループットを 2 GB/s とすると、`rgb24` で約 90 秒。エンコードと重なるが無視できない。
`rgb24` への変更だけで 60 GB・約 30 秒を節約する。これが V-3 を「色の正しさ」だけでなく
「性能」の理由でも直す根拠になる。

### 5.4 1080p60・8分ショーの所要時間見積り

28,800 フレーム。CRF 20 で約 12 Mbps → 出力約 720 MB。

| 工程 | ソフトウェア（libx264 `-preset medium`） | NVENC（Turing 以降） |
|---|---|---|
| preflight + encoder probe | 1 s | 1 s |
| フレーム生成（6ワーカー、300 fps） | 96 s | 96 s |
| パイプ転送（179 GB @ 2 GB/s） | 90 s | 90 s |
| エンコード | 110 fps → **262 s** | 700 fps → **41 s** |
| `+faststart` remux（720 MB 読み書き） | 6 s | 6 s |
| ffprobe 検証 | < 1 s | < 1 s |
| **実時間（重なりを考慮 = max(生成+転送, エンコード) + 終端）** | **≈ 270 s（4.5 分）** | **≈ 145 s（2.4 分）** |

- ソフトウェアは**エンコーダ律速**。`-preset veryfast`（約 350 fps）にすると 82 s まで落ち、
  全体は生成側律速の約 195 秒になる。プリセット `Fast` が `-preset veryfast` を選ぶべき根拠。
- NVENC は**フレーム生成とパイプ律速**。ここから先を縮めるにはラスタライザ側（3.5.2 の SIMD、5.3 の帯域）を触る。
- 4K60・8分は画素4倍で、フレーム生成 384 s、パイプ 716 GB、libx264 で 25〜30 分。
  P1 のキューはこの見積りを事前に表示して、うっかり実行を防ぐ。

**受け入れゲート**: 2026-08-09 の開発機ベースラインで、
**1080p60・8分・演者1,000人・ソフトウェアエンコードの書き出しが 6 分以内**。
`PRODUCT_QUALITY.md` の Performance 節へ追記する。

### 5.5 16.6 ms 予算のうちの取り分

**書き出しは UI スレッドの定常予算を 0 ms 消費する。** 内訳:

| 事象 | 頻度 | コスト |
|---|---|---|
| `Arc::new(document.clone())` | submit 時 1 回 | 0.3 ms（1 MB 未満の memcpy）。単発なので 1 フレームだけ 16.6 ms のうち 2% を使う |
| 進捗ポーリング | 毎 UI フレーム | アトミック 3 回 + 比較。**< 0.01 ms** |
| 進捗ラベルの整形 | 10 Hz | `format!` 1 回、**< 0.02 ms**（600 ms に 1 回相当の均し） |
| `request_repaint_after(100ms)` | 10 Hz | 実質 0 |

守るべき制約:

- レンダワーカーは `available_parallelism() - 1` 本。UI 用に 1 コア残す。
- Windows ではワーカーを `THREAD_PRIORITY_BELOW_NORMAL` にする。書き出し中も編集が引っかからない。
- **進捗更新で `request_repaint()` を毎回呼ばない。** 呼ぶと 60 Hz で UI を回し続け、
  書き出しに使えるコアを減らし、ノートPCではファンとバッテリを無駄にする。

### 5.6 メモリ

| 項目 | 1080p | 4K |
|---|---|---|
| `FrameBuffer` 1個（Pixmap 8.29 + packed 6.22） | 14.5 MB | 58 MB |
| プール `2N + 2 = 14` 個 | 203 MB | 812 MB → `max_inflight_bytes` により N を 2 へ下げ 290 MB |
| グリフキャッシュ（共有） | 1.2 MB | 2 MB |
| `DisplayList` × 6（各 1,200 cmd） | 1 MB | 1 MB |
| `Document` スナップショット | < 1 MB | < 1 MB |
| 音声 PCM 一時ファイル（ディスク） | 184 MB | 184 MB |
| stderr リング | 64 KiB | 64 KiB |

`max_inflight_bytes`（既定 256 MB）でワーカー数を自動的に絞る。RSS 上限の目安は 1080p で 400 MB。

## 6. 失敗モードと安全性

### 6.1 壊れ方の列挙

| # | 事象 | 検知 | 対処 |
|---|---|---|---|
| F-1 | FFmpeg が入っていない | `locate()` | `FfmpegNotFound`。OS 別のインストール手順と「ffmpeg を選択…」のファイルピッカーを出す（3.7.6） |
| F-2 | 指定されたエンコーダがビルドに無い | preflight の `resolve_encoder` | プロセスを起動せず `EncoderUnavailable`。代替候補を提示 |
| F-3 | ハードウェアエンコーダの初期化失敗 | stderr 分類 | software へ 1 回だけ再試行（3.10.2） |
| F-4 | 書き出し途中でディスクが埋まる | stderr `No space left on device` / 書き込みエラー | 一時ファイル削除、`DiskSpace` |
| F-5 | 書き出し前から容量不足 | preflight | 起動前に `DiskSpace{needed, available}` |
| F-6 | FFmpeg が途中で死ぬ | `write_all` の `BrokenPipe` | **原因ではなく症状**。`wait()` して stderr 末尾から実因へ変換（3.7.3） |
| F-7 | stderr を読まずデッドロック | — | stderr 専用スレッドで常時排出（不変条件7） |
| F-8 | ゾンビプロセス／ハンドルリーク | — | `ChildGuard::Drop` が kill + wait（不変条件6） |
| F-9 | `+faststart` の remux が長く、ハングに見える | `finish` の timeout | 進捗 stage を `Finalizing` にして「最終化中」と表示。timeout 超過は `Timeout` |
| F-10 | 出力が検証に落ちる | ffprobe + 自前 moov 検査 | 確定パスへ rename しない。`VerifyReport` を表で提示 |
| F-11 | 書き出し中にユーザーがドキュメントを編集 | — | `Arc<Document>` スナップショットにより影響なし（構造的に不可能） |
| F-12 | 書き出し中にキャンセル | `AtomicBool` | 1フレーム以内に停止 → kill → wait → 一時ファイル削除 |
| F-13 | 音源ファイルが消えている／壊れている | `AudioSource::render_window` | `Audio(...)`。「音声なしで続行」を選べる（`ExportWarning::AudioMissing`）。ドリル本体は書き出せる |
| F-14 | 出力先が読み取り専用／権限なし | preflight のプローブ書き込み | `OutputRefused::NotWritable` |
| F-15 | 出力先の既存ファイルを壊す | 一時ファイル + 検証後 rename | 検証に落ちれば既存ファイルは無傷 |
| F-16 | フォントが差し替わっている | SHA-256 照合 | `TextError::FontMismatch`。**黙って別フォントで出さない** |
| F-17 | 巨大な範囲指定（数時間） | preflight の `total_frames` 上限 | `Range`。見積り時間も表示して事前に止める |
| F-18 | `width * height * frames` の整数オーバーフロー | `checked_mul` | `Range` |
| F-19 | GPU 経路で bit-exact を主張してしまう | 型で防止 | `RasterBackend::determinism()` が唯一の情報源（不変条件14） |
| F-20 | パニックがワーカーで起きる | `catch_unwind` はしない | ジョブスレッドのパニックはチャネル切断として観測し `FfmpegExit` ではなく `Io`/内部エラーとして報告。`ChildGuard` は unwind 中も `Drop` で回収する |

### 6.2 信頼できない入力

書き出し経路が触る「他人由来のデータ」は次の3つ。すべて敵性入力として扱う。

- **プロジェクトファイル（`Document`）**: `title` / `Set::name` / `Performer::label` は
  **テキスト描画にしか使わない**。ファイルパスにもプロセス引数にもしない。
  描画時は 1 ラベルあたりのグリフ数に上限（既定 256）を設け、制御文字と BiDi 制御（U+202A〜U+202E, U+2066〜U+2069）を除去する。
  異常に長いラベルで書き出しが停止するのを防ぐ。
- **`AudioTrack::path`**: 30-audio-engine / 41-persistence が解決する。本書は**受け取った PCM しか触らない**。
  パス文字列が ffmpeg の引数に載ることは無い（載るのは我々が作った一時 PCM のパスだけ）。
- **FFmpeg の stdout / stderr**: 外部プロセスの出力なので信頼しない。
  `apply_progress_line` は行長上限（4 KiB）とパース失敗許容。stderr リングは固定 64 KiB。
  ffprobe の JSON は要素数・深さ上限つきでパースし、`nb_frames` 等は `u64` へ範囲検査つきで変換する。

### 6.3 パニック禁止経路

フレームループ・パーサ・プローブ処理では `unwrap` / `expect` / 添字パニック / 整数オーバーフローを作らない。

- 添字は `get()` / `get_mut()`、算術は `checked_*` / `saturating_*`。
- `as` によるキャストは、`u64 → usize` / `f64 → u64` を含めすべて範囲検査を前置する。
- `f32`/`f64` は `is_finite()` を通してから使う。NaN の `count` は preflight で `Range` として弾く。
- `tiny_skia::Pixmap::new(w, h)` は `Option` を返す。`?` で `ExportError::Range` へ変換する。

### 6.4 外部プロセス起動の安全性

- **シェル文字列を組み立てない。** `Command::args(&Vec<String>)` のみ。`sh -c` / `cmd /c` を使わない。
- **実行ファイルパスはアプリ設定・同梱・PATH のみ。** プロジェクトファイル由来のパスは決して実行しない。
  Windows では `.exe` 以外を拒否（`.bat`/`.cmd` の引数エスケープ問題を避ける）。
- **パス・トラバーサル**: `OutputTarget` は「正規化済みディレクトリ」と「セパレータを含まないファイル名」に分離して保持する。
  `file_name` に `/`、`\`、`:`、`..`、NUL、Windows 予約名（`CON`/`PRN`/`AUX`/`NUL`/`COM1..9`/`LPT1..9`）が含まれたら
  `OutputRefused::InvalidName`。連結後のパスが `directory` の下にあることを再確認する。
- **引数注入**: ffmpeg には `--` による解析終了が無い。代わりに、
  **入出力パスは必ず絶対パスで、かつ `file:` プロトコルを明示して渡す**（`file:C:\...\out.mp4`）。
  これで `-` 始まりのファイル名がフラグと解釈されることも、`concat:` / `http:` / `subfile:` のような
  プロトコル文字列として解釈されることも防げる。入力側にはさらに `-protocol_whitelist file,pipe` を付ける。
- **上書き確認**: 確定パスが既存なら UI が実ファイル（サイズ・更新日時）を提示して確認を取り、
  `Overwrite::Confirmed` を立ててから submit する。ジョブは `Refuse` のまま既存ファイルを触らない。
  `-y` は一時パスに対してのみ効く。
- **作業ディレクトリ**を出力先へ固定する（`current_dir`）。相対パスの解釈が想定外の場所へ逃げない。
- **環境変数**を継承したまま起動する（`FFREPORT` などが設定されていると挙動が変わりうる）ので、
  `FFREPORT` は明示的に `env_remove` する。

### 6.5 FFmpeg のライセンス判断（同梱するか否か）

事実関係（一次情報で確認）:

- FFmpeg 本体の既定は **LGPL-2.1 以降**。ただし任意の構成要素に **GPL-2.0 以降**のものがあり、
  それを有効にすると **FFmpeg 全体に GPL が及ぶ**。`--enable-gpl` がその切り替えである。
- **`libx264` / `libx265` は GPL**。つまり「libx264 入りの ffmpeg バイナリ」は GPL バイナリである。
- `--enable-nonfree` を付けたビルドは**再配布そのものが禁止**（有償・無償を問わない）。
- LGPL 遵守で配布する場合の義務: 動的リンク、その正確なビルドの対応ソースの提供、
  ダウンロードページとアプリ内での表示、EULA での明示、ライブラリ名の難読化・改名の禁止。
- **特許は別問題**。FFmpeg の legal ページ自身が、H.264 / MPEG-4 等は特許で覆われうると述べている。
  ライセンスが LGPL でも、H.264 エンコーダを含む有償製品の配布は特許プールとの関係が独立して発生する。
- LGPL 互換で使える H.264 エンコーダ: `libopenh264`（BSD-3、Cisco）、`h264_mf`（Windows Media Foundation、OS 同梱）、
  `h264_nvenc`（`nv-codec-headers` は MIT）、`h264_qsv`、`h264_amf`、`h264_videotoolbox`。

**結論（v1 の方針。9 章で法務確認のうえ確定する）:**

> **v1 では FFmpeg を同梱しない。** 利用者がインストールした FFmpeg を検出して使う。
> 未検出時は OS 別の導入手順（winget / choco / scoop、Homebrew、apt）と
> 「ffmpeg の場所を指定…」を出す誘導ダイアログを表示する。

理由:

1. **GPL / LGPL の論点を製品から切り離せる。** 別プロセスの CLI を叩くだけの「隔たりのある通信」は
   一般に単独著作物として扱われるが、GPL バイナリを自社インストーラで同梱すると
   その主張は弱くなる。同梱しなければ、この論点自体が発生しない。
2. **`libx264` に依存しない設計にできる。** 3.7.2 のエンコーダ優先順は、
   OS 同梱の `h264_mf` や BSD の `libopenh264`、ハードウェアエンコーダを先に選ぶ。
   利用者の ffmpeg が GPL ビルドでも LGPL ビルドでも、どちらでも動く。
   GPL ビルドを検出したら `FfmpegInfo.gpl_build` を立て、情報として表示するにとどめる。
3. **H.264 の特許問題を先送りにできない形で可視化できる。** 有償製品として H.264 エンコーダを
   配布するかどうかは、ライセンスとは独立した意思決定である。同梱しなければ、この決定を
   （少なくとも v1 では）回避できる。

**将来同梱する場合の必須条件**（53-productization と共同で満たす）:

- `--enable-gpl` を**付けない** LGPL-2.1 ビルドであること
- 動的リンクで、そのビルドの対応ソース一式を配布と同じ場所で提供すること
- アプリ内に「オープンソースライセンス」画面を置き、LGPL 全文と FFmpeg の帰属を掲載すること
- ライブラリ名を改名・難読化しないこと
- **その前に H.264 の特許ライセンスの結論を出すこと**

## 7. テスト計画

### 7.1 単体テスト（外部プロセス不要）

| テスト | 内容 |
|---|---|
| `frame_rate_validation` | `FrameRate::is_valid`。0 分母、0 分子、241 fps を拒否 |
| `frame_seconds_is_exact` | 60/1 で `frame_seconds(28800) == 480.0` 完全一致。30000/1001 で 1001 フレームがちょうど 1001/29.97 秒 |
| `frame_time_no_accumulation` | ループ加算した 28,800 フレームの合計と `frame_seconds` の差が 1e-4 秒を超えることを示し、なぜ純関数でなければならないかを固定 |
| `frame_time_precision` | 480 秒地点で `count_at_frame` の誤差が 1/(4·fps) カウント未満 |
| `render_frame_count_edges` | duration 0 → 1、1 フレームちょうど、端数、`u128` 経由でのオーバーフロー無し |
| `audio_frame_count_matches_video` | 全 fps × 44.1/48/96 kHz の組で `|音声秒 − 映像秒| < 1/fps` |
| `start_sample_rounds_once` | 負のオフセット、ファイル末尾越え、丸めが 1 箇所しかないこと |
| `progress_line_parsing` | 実際の `-progress` 出力ブロックをフィクスチャにして全キー。**`out_time_ms` がマイクロ秒である**ことを明示的に固定 |
| `progress_line_is_robust` | `N/A`、空行、4 KiB 超の行、非 UTF-8 バイト、未知キー → パニックしない |
| `classify_stderr_table` | 3.10.2 の全パターン + 誤検知しない文字列（`"Unknown encoder"` を含むファイル名など） |
| `encoder_resolution_order` | `Auto` の優先順。`encoders` に無いものを飛ばす。`h264_mf` で rate control が降格する |
| `quality_flag_per_encoder` | V-7 の回帰。`Auto` が NVENC に解決されたら `-cq` になる |
| `output_name_rejects_traversal` | `..`、セパレータ、`:`、NUL、`CON`、`-x.mp4`、超長名 |
| `ffmpeg_path_rejects_bat` | Windows で `.bat` / `.cmd` / ディレクトリを拒否 |
| `ffprobe_json_parsing` | 実出力のフィクスチャ。`nb_frames` 欠落、`r_frame_rate` が `0/0`、巨大値、壊れた JSON |
| `verify_predicate_table` | 10 項目それぞれについて合格1件・不合格1件 |
| `moov_before_mdat` | faststart 済み／未済の先頭 64 KiB フィクスチャ 2 件 |

### 7.2 ゴールデンテスト

- `golden_frames`: コミット済みフィクスチャ（演者 24 人・3 セット・可変BPM・日本語セット名を含む）を
  640×360 で 8 フレーム（範囲の 0%, 12.5%, …, 87.5%）ラスタライズし、
  各フレームの BLAKE3 を 16 進で `tests/golden/frames_640x360.expected` と文字列比較する。追加依存なし。
- `golden_ffmpeg_args`: 6 通りの構成（H264/MP4 音声あり・なし、H265/MOV、VP9/WebM、NVENC、ビットレート指定）の
  引数配列を1行1引数のテキストとして `.expected` 比較する。引数の変更が必ずレビューに乗る。
- `export_is_reproducible`（Tier A）: 同一入力で 2 回 `run_export` し `frame_stream_hash` の一致を確認。
- `golden_frames_cross_target`（Tier B）: 同じ `.expected` を x86-64（`-C target-feature=-avx2`）/
  x86-64+AVX2 / aarch64 の 3 ランナーで実行。3.5.2 の判断材料。**T-05 で先に実施する。**
- `playback_and_export_agree_on_counts`: `playback::advance` を 1/fps 秒刻みで回した列と
  `FrameClock::count_at_frame` の列が 1e-3 カウント以内で一致。
- `svg_and_raster_agree`: 同じ `DisplayList` から SVG と RGBA を作り、要素数と代表点の座標が一致することを確認
  （`DESIGN_GAPS.md` B-1 の「同じドキュメント座標を参照する」の実効検査）。

### 7.3 property テスト

追加依存なしの手書き擬似乱数（xorshift64、固定シード）で回す。

- 任意の `(num, den, duration)` について `frame_seconds(total−1) < duration ≤ frame_seconds(total)`。
- 任意の `n < m` について `count_at_frame(n) ≤ count_at_frame(m)`（単調非減少）。
- 任意の `TempoMap` と `start_count` について `audio_frame_count` と `total_frames` の秒数差が `1/fps` 未満。
- 任意のファイル名バイト列について `OutputTarget::new` は panic しない（受理か `InvalidName`）。

### 7.4 fake ffmpeg による統合テスト（CI で常時実行）

`tests/bin/fake_ffmpeg.rs` を workspace のテスト用バイナリとして置く。
stdin を読み捨て、指定に従って `-progress` 行を吐き、指定の終了コードで終わる。
これで**外部依存なしに**状態機械の全分岐を CI で回せる。

| テスト | fake の挙動 |
|---|---|
| `happy_path` | 全フレーム読み、`progress=end`、exit 0 |
| `cancel_reaps_child` | 無限に読む → 途中でキャンセル → kill + wait、一時ファイル無し、確定パス無し |
| `ffmpeg_dies_early` | 5 フレーム読んで exit 1 + stderr に `Unknown encoder 'h264_nvenc'` → `EncoderUnavailable` 相当へ分類され software 再試行が 1 回だけ起きる |
| `ffmpeg_flooding_stderr_does_not_deadlock` | stderr へ 4 MB 吐きながら stdin を読む → 完走する |
| `ffmpeg_hangs_at_finalize` | stdin を読み切ってから終了しない → `Timeout { stage: Finalizing }` |
| `disk_full_midway` | stderr に `No space left on device` → 再試行しない、一時ファイル削除 |
| `writer_preserves_order` | fake が受信バイト列のハッシュを stdout へ出し、期待ハッシュと一致 |
| `failed_export_leaves_no_output` | 上記の全失敗系で確定パスが存在しない |

### 7.5 実 FFmpeg 統合テスト（`#[ignore]`、CI では ffmpeg 導入済みジョブで実行）

- 320×240@30、2 秒、演者 20 人 → ffprobe で 10 項目すべて合格。
- 上記 + 1 kHz サイン波の PCM → 音声ストレーム 1 本、尺一致 50 ms 以内。
  さらに出力から音声を抜き、先頭サンプルの位相で開始位置ズレが 1 サンプル以内であることを確認（音ズレの実測）。
- `-c:v` に存在しないエンコーダを強制 → `EncoderUnavailable` に分類され、software へ 1 回だけ再試行して成功。
- WebM/VP9 で `nb_frames` 欠落経路の検証。
- 出力先を読み取り専用ディレクトリに → preflight で止まる。

### 7.6 ストレス・ベンチ

| 項目 | 内容 |
|---|---|
| `stress_1000_performers_1000_frames` | 1080p・1,000 人・1,000 フレーム。warm-up 後の確保ゼロ（ポインタ同一性）と RSS < 400 MB |
| `stress_two_hour_range` | 4 時間相当の範囲で `total_frames` 上限に当たり、事前に拒否される |
| `bench_raster_1080p_1000_performers` | criterion。1フレームの実測。5.1 の 4.0 ms が守られているか |
| `bench_display_list_build` | 20-display-list と共有 |
| `bench_pack_rgb24` | 8.29 MB → 6.22 MB。0.3 ms 目標 |
| `bench_glyph_cache_hit` | キャッシュヒット時のテキスト blit |

### 7.7 セキュリティテスト

- `project_paths_never_reach_process_args`: `title` / `Set::name` / `audio.path` に
  `"; rm -rf /"`, `"-i /etc/passwd"`, `"concat:a|b"`, `"..\\..\\Windows\\System32\\calc.exe"` を入れた
  フィクスチャで書き出し、生成された引数配列にそれらの文字列が一切現れないことを確認。
- `output_path_stays_in_directory`: 正規化後のパスが指定ディレクトリ配下であること。
- `long_label_is_bounded`: 100 万文字のセット名でフレーム生成時間が線形に膨らまない（グリフ数上限）。
- fuzz（P1、cargo-fuzz）: `apply_progress_line`、`parse_ffprobe_json`、`OutputTarget::new`。

## 8. 実装タスク

1タスク = 1〜3時間相当。`[dep: …]` は前提タスク。同じ Wave 内は並行可能。

### Wave A — 土台（40-jobs と 20-display-list を待たずに着手できる）

| ID | 内容 | 依存 | 見積 |
|---|---|---|---|
| T-01 | `drill-export` クレート新設。`Cargo.toml`（tiny-skia 0.12 `default-features=false, features=["std"]` / rustybuzz / ttf-parser / fs4）、workspace members へ追加、空モジュール骨格 | — | 1 h |
| T-02 | `clock.rs`: `FrameRate` / `frame_seconds` / `render_frame_count` / `FrameClock` + 7.1 の時刻系単体テスト全部 | T-01 | 3 h |
| T-03 | `progress.rs`: `FfmpegProgress` / `apply_progress_line` + フィクスチャ単体テスト（`out_time_ms`=µs 固定を含む） | T-01 | 2 h |
| T-04 | `ffprobe.rs`: `ProbeResult` の JSON パース（serde_json、上限つき）、`verify()` の 10 項目、`moov_before_mdat` 自前検査 + テスト | T-01 | 3 h |
| T-05 | `text.rs`: `GlyphCache`（フォント SHA-256 照合 / rustybuzz shaping / ttf-parser → tiny-skia path / マスクキャッシュ / 1/4px 量子化） | T-01 | 3 h |
| T-06 | **Tier B 実験**: 単純図形＋テキストのゴールデンフレームを SSE2 / AVX2 / NEON で比較し、tiny-skia の `simd` を有効のまま出荷できるかを判定。結果を 9 章へ反映 | T-05 | 2 h |
| T-07 | `drill-core::video` の是正: `VideoConfigError` + `Locale` 化（V-1）、`FrameRate` 対応、`frame_count` の有理数化（V-4）、`ffmpeg_args` の `#[deprecated]` 化 | T-02 | 2 h |

### Wave B — プロセス層（Wave A と並行可）

| ID | 内容 | 依存 | 見積 |
|---|---|---|---|
| T-08 | `ffmpeg.rs` 前半: `locate()` / `probe_ffmpeg()` / `FfmpegInfo` / パス拒否規則（`.exe` 限定・通常ファイル・プロジェクト由来禁止）+ テスト | T-01 | 3 h |
| T-09 | `ffmpeg.rs` 後半: `resolve_encoder()` の優先順テーブル、`build_args()`（`rgb24` / `-frames:v` / `file:` / `+bitexact` / `-thread_queue_size`）+ `golden_ffmpeg_args` | T-08, T-07 | 3 h |
| T-10 | `ChildGuard` / spawn（`CREATE_NO_WINDOW`・3 パイプ）・stderr リングスレッド・`finish` の timeout | T-08 | 3 h |
| T-11 | `tests/bin/fake_ffmpeg.rs` と 7.4 の統合テスト 8 本 | T-10, T-03 | 3 h |
| T-12 | `preflight.rs`: 全検査 + `fs4` による空き容量 + `PreflightReport` | T-09 | 2 h |
| T-13 | `fallback.rs`: `classify()` テーブルと 1 回限りの再試行制御 + テスト | T-10 | 2 h |
| T-14 | `OutputTarget` / `TempFileGuard` / 検証後の原子的 rename + 7.7 のパステスト | T-01 | 2 h |

### Wave C — 描画とパイプライン（20-display-list 完了後）

| ID | 内容 | 依存 | 見積 |
|---|---|---|---|
| T-15 | `raster.rs`: `DisplayList → Pixmap`（`DrawCmd` 全種）+ `pack_rgb24` + 不透明性 `debug_assert` | 20-display-list, T-05 | 3 h |
| T-16 | `FrameBuffer` / `FramePool` / `RenderScratch` と確保ゼロ検証テスト | T-15 | 2 h |
| T-17 | レンダワーカー N 本 + 有界チャネル + writer の順序復元 + `frame_stream_hash` | T-16, T-10 | 3 h |
| T-18 | `run_export` の全体結線（preflight → spawn → 3 スレッド → finish → probe → verify → rename）+ `ExportOutcome` | T-17, T-12, T-13, T-14, T-04 | 3 h |
| T-19 | ゴールデンテスト `golden_frames` と `export_is_reproducible`、`playback_and_export_agree_on_counts` | T-18 | 2 h |
| T-20 | ベンチ 7.6 一式と 5.1 / 5.4 の実測、`PRODUCT_QUALITY.md` へ書き出しゲート追記 | T-18 | 3 h |

### Wave D — 音声とUI（30-audio-engine / 40-jobs 完了後）

| ID | 内容 | 依存 | 見積 |
|---|---|---|---|
| T-21 | `AudioSource` 実装の接続、`start_sample` / `audio_frame_count`、f32le 一時ファイル書き出しと削除 | 30-audio-engine, T-18 | 3 h |
| T-22 | 音声ありの検証（7.5 の位相一致テストを含む） | T-21 | 2 h |
| T-23 | `spawn_video_export` を `Job<T>` へ載せる。進捗の 10 Hz 制限、キャンセル配線 | 40-jobs, T-18 | 2 h |
| T-24 | `drill-app` の書き出しUI: 無効ボタン（main.rs:1275）を実装へ差し替え、進捗・キャンセル・上書き確認・FFmpeg 未検出時の誘導ダイアログ、検証失敗の表表示。FFmpeg 検出をワーカーへ（V-6） | T-23 | 3 h |

### Wave E — P1 / P2

| ID | 内容 | 依存 | 見積 |
|---|---|---|---|
| T-25 | `queue.rs`: `ExportQueue`、解像度／カメラのファンアウト、所要時間見積り表示 | T-24, 23-camera | 3 h |
| T-26 | GPU 経路: `FrameSource` trait 実装を 21-gpu-renderer 側へ接続、`PerceptualOnly` の UI 表示、専用 `Device` 生成 | 21-gpu-renderer, T-18 | 3 h |
| T-27 | `resume.rs`: セグメント分割・マニフェスト・concat 再開（P2） | T-25 | 3 h |
| T-28 | fuzz ターゲット 3 本（P1） | T-11 | 2 h |

クリティカルパス: `T-01 → T-02 → T-07 → T-09 → T-10 → T-17 → T-18 → T-23 → T-24`。
Wave A と Wave B はほぼ完全に並行でき、Wave C は 20-display-list の完成が律速になる。

## 9. 未決事項

| # | 事項 | 決めるために必要なもの | 暫定 |
|---|---|---|---|
| Q-1 | **FFmpeg の同梱可否**。同梱しないなら導入ハードルが上がり、同梱するなら LGPL 遵守の実務（対応ソース公開・動的リンク・ライセンス画面）と H.264 特許の判断が要る | 法務確認。販売形態（買い切り／サブスク）と対象地域。53-productization の配布方式 | **v1 は同梱しない**（6.5）。誘導ダイアログで代替 |
| Q-2 | **H.264 の特許ライセンス**。有償製品として H.264 エンコード機能を提供することの扱い。AV1（ロイヤリティフリー志向）を既定コーデックにする選択肢もある | 法務確認。AV1 の再生互換性の実態調査（教育現場のPC・タブレット） | H.264 を既定のまま。AV1 は選択肢として提供 |
| Q-3 | **tiny-skia の SIMD が bit-exact か**。Tier B の結果次第で `deterministic-raster` フィーチャを既定にするか決まる | T-06 の実測（3ターゲット） | `simd` 有効のまま出荷、T-06 で確定 |
| Q-4 | **色管理**。現状は sRGB の値をそのまま BT.709 としてタグ付けする。厳密には転送関数が異なる。P2 の「色管理」に含めるか、P0 で正しく変換するか | プレビュー（egui）との見た目差の実測。表示側の実態調査 | BT.709 タグ付けのみ。差分を計測して P2 で判断 |
| Q-5 | **NTSC フレームレート（29.97 / 59.94）の需要**。`FrameRate` は有理数で対応済みだが、UI に出すかは別 | 想定ユーザー（日本の学校・団体）の納品先要件のヒアリング | UI には出さない。型だけ用意 |
| Q-6 | **アルファ付き書き出し**（ProRes 4444 / QT RLE）。実写のスタジアム映像へ重ねる用途。Pyware には無いので差別化になりうるが、`pack_rgb24` 前提を崩す | 需要調査。ProRes エンコーダの可用性（`prores_ks` は LGPL 内） | P2 で再検討 |
| Q-7 | **`max_inflight_bytes` の既定値 256 MB**。4K ではワーカー数が 2 まで落ちて遅くなる | T-20 のベンチで 4K の実測 | 256 MB。メモリ 16 GB 以上なら 768 MB へ自動引き上げを検討 |
| Q-8 | **試用版の透かし**。書き出しに透かしを入れるか、尺を制限するか、機能自体を無効にするか。`DrawCmd` 層で入れるとゴールデンテストが分岐する | 53-productization の価格・試用方針 | 未定。ラスタライザ層ではなく `Scene` 層で入れる想定にしておく |
| Q-9 | **GPU 3D 書き出しの決定論**。`PerceptualOnly` を恒久とするか、CPU での 3D ラスタライズ（極端に遅い）を「検証用モード」として用意するか | 22-stadium-3d の複雑度と、実際に決定論を要求される場面があるかの確認 | `PerceptualOnly` 恒久。UI で明示 |

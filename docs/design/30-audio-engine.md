# 30. 音声エンジン（デコード・再生・波形・同期）

## 1. 目的と範囲

### 解決すること

DrillForge には現在**音を鳴らす経路が一切無い**。`drill-core/src/audio.rs` はカウントと秒の写像だけを持ち、
実際のデコード・出力・波形・クリックは未実装である。本書は新クレート `drill-audio` を定義し、次を成立させる。

1. 参照音源のデコード（ワーカースレッド、進捗・キャンセル付き）
2. デコード済み PCM の常駐形式と上限
3. ズーム段階別ピーク（`PeakPyramid`）による O(画素) の波形描画データ供給
4. cpal による実再生（リングバッファ、リアルタイム安全なコールバック）
5. **時刻の正**の一元化（音声があるときはサンプル索引、無いときは単調時計）
6. クリック（メトロノーム）とカウントイン
7. 単一 `offset_seconds` から `Vec<SyncAnchor>` への同期アンカー拡張
8. BPM / ダウンビート / トランジェントの自動解析
9. 出力レイテンシの取得と利用者による手動較正
10. 非破壊調整（gain / mute / trim / fade）の再生・書き出しへの適用点
11. デバイス切断・サンプルレート変更からの回復

### 扱わないこと

| 事項 | 担当文書 |
|---|---|
| 波形・アンカーの**描画**（DisplayList 生成、GPU） | 20 / 21 |
| Audio Workspace の画面構成・操作系 | 43 |
| 動画への音声 mux、FFmpeg 引数 | 31 |
| `Job<T>` / 進捗 / キャンセルの**基盤そのもの** | 40 |
| `DrillError` の定義と `Locale` によるメッセージ解決 | 42 |
| プロジェクトコンテナ・相対パス解決・`AssetState::Missing` | 41 |
| タイムストレッチ（ピッチ保持の速度変更）、MIDI/DAW 連携、録音入力 | 範囲外（§9 に記録） |

本書は `drill-audio` の**内部設計**と、`drill-core` に必要な**最小の追加**（同期アンカーの型と写像）を定める。
`drill-core` には音声デバイス依存を一切入れない（`00-conventions.md` クレート境界）。

---

## 2. 現状

リポジトリの実コードで確認した内容のみを書く。

### 2.1 有るもの

| 位置 | 内容 |
|---|---|
| `crates/drill-core/src/audio.rs:23-45` | `AudioTrack`。`path` / `duration_seconds` / **`offset_seconds`（スカラー1個）** / `gain_db` / `muted` / `trim_start_seconds` / `trim_end_seconds` / `fade_in_seconds` / `fade_out_seconds` |
| `audio.rs:47-64` | `gain_linear()`（mute で 0、`-96..=24 dB` クランプ）、`effective_duration()` |
| `audio.rs:65-79` | `validate()`。戻り値が `Result<(), String>` で、**日本語文字列リテラルが `drill-core` に埋まっている** |
| `audio.rs:86-96` | `count_to_audio_time` / `audio_time_to_count`。`offset + tempo.seconds_at(count)` の単純加算 |
| `audio.rs:104-126` | `click_track` / `downbeats`。**秒の `Vec<f32>` を返すだけで発音しない** |
| `audio.rs:130-133` | `is_within_audio` |
| `crates/drill-core/src/tempo.rs:115-170` | `TempoMap::seconds_at` / `count_at`。**すべて `f32`**、区分定数 BPM を線形積分。O(区間数) |
| `crates/drill-core/src/playback.rs:44-81` | `advance(current_count, elapsed_seconds, speed, range, looping, tempo)`。**壁時計の `dt` 駆動**。確保なし・決定論 |
| `crates/drill-app/src/main.rs:488-509` | 唯一の再生経路。egui の `dt` を `advance_playback` に渡し `seek_global` する。`request_repaint_after(16ms)` |
| `main.rs:1343-1428` | 音源 UI。ファイル選択（`wav/mp3/ogg/flac` フィルタ）、offset・長さ・gain・mute・trim・fade の DragValue |
| `main.rs:1404-1414` | 選択時に `duration_seconds: 0.0` で `AudioTrack` を作る。**デコードしないので本当の長さを知らず、利用者が手入力する（`main.rs:1366-1373`）** |
| `main.rs:1417-1427` | 現在カウントに対応する音源秒を文字列表示するだけ |
| `crates/drill-core/src/video.rs:159-190` | FFmpeg 引数生成。音声は `audio_path: Option<&str>` を第2入力にするだけで、**trim/gain/fade は反映されない** |

### 2.2 無いもの

- `drill-audio` クレート。`Cargo.toml:2` の `members` は `crates/drill-core` と `crates/drill-app` の2つだけ。
- 音声依存クレート。`crates/drill-core/Cargo.toml:7-9` の依存は `serde` / `serde_json` のみ。
  ワークスペース `Cargo.toml:10-13` の `[workspace.dependencies]` も同じ2つだけ。
- デコード、PCM 常駐、リサンプル、出力ストリーム、リングバッファ。
- 波形・ピーク・LOD。`drill-core` 全体に "peak" / "waveform" に相当する型は無い。
- クリックの**発音**、カウントイン。
- 複数同期アンカー。`AudioTrack` にあるのは `offset_seconds` ただ1つ。
- BPM 推定・ダウンビート候補・トランジェント検出。
- レイテンシ較正、A/V オフセット。
- 非同期ジョブ基盤（`DESIGN_GAPS.md` B-3、本書は 40 の存在を前提に書く）。
- `DrillError`。`drill-core` 内に該当する enum は存在せず、`validate()` は `String` を返す（42 の担当）。

### 2.3 現状から導かれる制約

- `main.rs:488-509` の再生は egui のフレーム間隔に律速され、音とは無関係に進む。
  音を足す際、**この経路を残したまま音声を並走させると必ずズレる**。時刻の正を一本化する置換が要る（§3.6）。
- `TempoMap` が `f32` 秒であることは、長尺でサンプル精度を割る（§3.12、§9）。

---

## 3. 設計

### 3.1 クレート構成と依存の選定（2026-08-09 時点で実地確認）

#### 3.1.1 採用クレート

| クレート | 版 | ライセンス | 直近リリース | 用途 |
|---|---|---|---|---|
| `symphonia` | 0.6.0 | **MPL-2.0** | 2026-05-15 | コンテナ解析とデコード |
| `cpal` | 0.18.1 | Apache-2.0 | 2026-06-07 | 出力ストリーム（WASAPI / CoreAudio / ALSA） |
| `rubato` | 4.0.0 | MIT OR Apache-2.0 | 2026-07-09 | サンプルレート変換・可変速 |
| `rtrb` | 0.3.4 | MIT OR Apache-2.0 | 2026-04-26 | リアルタイム安全な SPSC リングバッファ |
| `realfft` | 3.5.0 | MIT | 2025-06-12 | 解析用 FFT（`rubato` が既に依存） |

`drill-audio` は `drill-core` にのみ依存する。`drill-render` / `drill-app` を知らない。

```toml
# crates/drill-audio/Cargo.toml
[package]
name = "drill-audio"
version.workspace = true
edition.workspace = true
license.workspace = true

[dependencies]
drill-core = { path = "../drill-core" }
serde.workspace = true
cpal = "0.18.1"
rtrb = "0.3.4"
rubato = "4.0.0"
realfft = "3.5.0"

[dependencies.symphonia]
version = "0.6.0"
default-features = false
# 0.6.0 の default は ["opt-simd", "all-meta", "adpcm", "flac", "mkv", "ogg",
# "pcm", "vorbis", "wav"]。mp3 は default に入っていないため明示的に足す。
features = ["opt-simd", "all-meta", "adpcm", "flac", "mkv", "ogg", "pcm", "vorbis", "wav", "mp3"]

[features]
default = []
# 既定で無効。§3.1.4 のライセンス判断を読んでから有効化すること。
aac = ["symphonia/aac", "symphonia/isomp4", "symphonia/alac"]
```

#### 3.1.2 ライセンス上の注意

**symphonia は MPL-2.0** であり、ワークスペースの `MIT OR Apache-2.0`（`Cargo.toml:8`）とは異なる。
MPL-2.0 はファイル単位の弱いコピーレフトで、§3.3「Larger Works」により
**MPL のファイル群を含む大きな著作物を別の条件（商用クローズド）で頒布してよい**。DrillForge は販売可能。
ただし次を守る。

- symphonia を**フォークして木内で改変しない**。改変した場合、改変したファイルは MPL-2.0 で公開義務が生じる。
  必要な修正は上流へ PR を出し、暫定は `[patch]` ではなくバージョン固定で回避する。
- 配布物に MPL-2.0 全文と、symphonia のソース入手先（crates.io / GitHub）を記した
  サードパーティ通知（`THIRD-PARTY-NOTICES.md`、53 の担当）を同梱する。
- `cargo deny` にライセンス許可リスト（`MIT`, `Apache-2.0`, `MPL-2.0`, `Unicode-3.0`, `BSD-3-Clause`）を設定し、
  CI で検査する（50 と連携）。

#### 3.1.3 対応フォーマット（symphonia 0.6.0 の feature に基づく）

| 拡張子 | コーデック / コンテナ | 0.6.0 での扱い | DrillForge の方針 |
|---|---|---|---|
| `.wav` | PCM / ADPCM in RIFF | `pcm` `adpcm` `wav`（既定で有効） | 対応 |
| `.flac` | FLAC | `flac`（既定で有効） | 対応 |
| `.ogg` | Vorbis in OGG | `vorbis` `ogg`（既定で有効） | 対応 |
| `.mkv` / `.webm` | 各種 in Matroska | `mkv`（既定で有効） | 対応（音声トラックのみ抽出） |
| `.mp3` | MPEG-1/2 Layer III | `mp3`（**既定では無効**、明示的に有効化する） | **対応**（§3.1.4） |
| `.m4a` / `.aac` | AAC-LC in ISO-MP4 / ADTS | `aac` + `isomp4`（既定で無効） | **既定で無効**（§3.1.4） |
| `.aiff` | PCM in AIFF | `aiff`（既定で無効） | 有効化しない（需要が低い。FFmpeg 経路で足りる） |
| `.opus` | Opus | **0.6.0 には feature が無い** | 非対応。FFmpeg 経路 |
| `.wma` ほか | — | 非対応 | FFmpeg 経路 |

#### 3.1.4 MP3 と AAC の判断

**MP3 は有効化する。** Fraunhofer IIS は 2017-04-23 に MP3 の特許ライセンスプログラムを終了しており、
プログラムに含まれていた最後の特許が満了したことが理由と説明されている。以後、MP3 の実装・利用に
ロイヤリティは発生しない。参照音源として MP3 は事実上必須であり、これを外す理由が無い。

**AAC / ALAC / ISO-MP4 は既定で無効にする。** AAC は Via Licensing Alliance（Via LA）の
特許プールが現役で、**エンコーダ／デコーダ実装の頒布**に対して 1 ユニットあたり概ね US$0.10〜0.98 の
ロイヤリティが設定されている（ビットストリームの配信自体には課金されない）。
販売する製品に AAC デコーダを同梱することは、この課金対象に該当し得る。法務判断が付くまで同梱しない。

代替経路（`.m4a` / `.aac` / `.opus` / その他）:

- **FFmpeg 経由の事前変換**。動画書き出し（31）で FFmpeg は**利用者が用意する外部プロセス**として
  既に前提になっている（`video.rs:159-190`、`MEDIA_PIPELINE.md`）。同じ FFmpeg を使い、
  `ffmpeg -i <src> -vn -acodec pcm_s16le -ar 48000 -ac 2 <tmp>.wav` で一時 WAV に変換してから symphonia で読む。
  引数は配列で渡し、シェル文字列を組み立てない（`00-conventions.md` プロセス分離）。
  FFmpeg のライセンス（LGPL/GPL）は**別プロセス**なので DrillForge のバイナリに波及しない。
- 変換結果はプロジェクトコンテナ（41）の `assets/` にキャッシュし、次回以降は再変換しない。
- FFmpeg が無い場合は「この形式には FFmpeg が必要です／WAV か FLAC に変換してください」を提示する（42 の文言）。

将来 `aac` feature を有効化する判断をしたときは、ビルド時 feature の切り替えだけで済む構造にしてある。

#### 3.1.5 rubato / rtrb / realfft

- **rubato 4.0.0**：`Async`（可変比・sinc/多項式）、`Fft`（固定比・帯域制限）、`Slip` を提供する。
  `Resampler::process_into_buffer` は「事前確保した出力バッファへ書き込み、確保もブロックし得る操作も行わない」
  と明記されており、非 RT のミキサスレッドで使うのに適する。4.0 は `audioadapter` を採用しており、
  インターリーブのスライスをそのまま入出力にできる（3.x までの `Vec<Vec<f32>>` 平面バッファが不要）。
  リリースが 2026-07 と新しいため、`mod resample` の内部トレイト `Resample` で隔離し、
  差し替え可能にする。**最頻ケース（48 kHz の音源を 48 kHz のデバイスで等速再生）では
  リサンプラを一切構築しない**ので、rubato の不安定さが常用経路に載らない。
- **rtrb 0.3.4**：wait-free / lock-free の SPSC リングバッファ。`write_chunk_uninit` / `read_chunk` で
  スライスを直接得られるため、コールバック側で確保もコピー先の一時オブジェクトも不要。
  自前 `unsafe` のリングを書くより検証済みの実装を使う。
- **realfft 3.5.0**：解析専用。rubato が既に依存しているので実効的な依存増加はゼロ。

### 3.2 モジュール構成

```
crates/drill-audio/src/
  lib.rs        公開 API の再輸出、AudioError
  asset.rs      AudioAsset, AudioSamples, DecodeLimits
  decode.rs     decode_file, ProgressSink, SourceInfo
  peaks.rs      Peak, PeakPyramid
  mix.rs        MixSettings, FrameMap, MixPipeline, mix_block（決定論・純関数）
  click.rs      ClickSettings, ClickVoices, ClickSchedule
  ring.rs       Block, BlockProducer, BlockConsumer（rtrb の薄いラッパ）
  clock.rs      ClockShared, PlaybackClock（seqlock 公開）
  engine.rs     Transport, Command, EngineEvent, ミキサスレッド
  device.rs     OutputStream, DeviceSupervisor, DeviceFault
  resample.rs   Resample トレイトと rubato 実装
  analysis.rs   OnsetEnvelope, estimate_tempo, estimate_downbeats, detect_transients
  calibrate.rs  LatencyCalibration, TapCalibrator
```

`drill-core` 側の追加は `crates/drill-core/src/audio.rs` の中だけ（§3.12）。

### 3.3 `AudioAsset`：常駐 PCM の形式と上限

#### 3.3.1 メモリ量の計算

基準：**10 分・ステレオ・48 kHz** = 600 × 48 000 = **28 800 000 フレーム** = 57 600 000 サンプル。

| 表現 | 1 サンプル | 10 分ステレオ 48 kHz | 30 分（上限） |
|---|---|---|---|
| `f32` | 4 B | **230.4 MB**（219.7 MiB） | 691.2 MB |
| `i16` | 2 B | **115.2 MB**（109.9 MiB） | 345.6 MB |

`DESIGN_GAPS.md` B-2 の素案は `pcm: Arc<[f32]>` だが、**`i16` に変更する**。根拠：

1. 参照音源は同期の基準であってマスタリング対象ではない。16 bit（SNR 96 dB）で十分。
   ミックス先の出力も多くのデバイスで 16〜24 bit。
2. メモリが半分になる。10 分で 230 MB は、1,000 人 × 64 セットのドキュメント本体
   （`PRODUCT_QUALITY.md` の基準規模）と GPU リソースに並んで載せると無視できない。
3. 波形は `PeakPyramid`（§3.4）が供給し、解析は別途デシメート済み信号を使うので、
   生 PCM への精密アクセスが要るのは**再生の逐次読み出しだけ**。
4. `i16 → f32` 変換は乗算1回。ミキサスレッドで行っても RT 予算に影響しない（§5）。

浮動小数音源（32-bit float WAV など）が ±1.0 を超える場合の扱い：デコード時に絶対値ピークを測り、
`peak_scale = 1.0 / peak.max(1.0)` を掛けてから量子化し、`AudioAsset::peak_scale` に `peak.max(1.0)` を保持する。
再生時のゲインに掛け戻すのでクリップも情報欠落も起きず、決定論。

**ストリーミングは採らない。** 30 分・345.6 MB を上限とすれば常駐で足りる。
ストリーミングは (a) 圧縮形式のフレーム精度シークが不正確、(b) シーク直後の空白、
(c) ディスクI/O をリアルタイム経路に持ち込む、という三重の複雑さを持ち込む。
ドリル制作の音源は数分〜10 分であり、常駐が正しい。上限超過は明示的にエラーにする（§6）。

```rust
// asset.rs

/// Decoded, resident PCM for one reference track.
///
/// Samples are interleaved 16-bit, downmixed to at most 2 channels at decode
/// time. Multiply by [`peak_scale`](Self::peak_scale) on playback to restore
/// the original amplitude of float sources that exceeded full scale.
#[derive(Clone, Debug)]
pub struct AudioAsset {
    samples: Arc<[i16]>,
    frames: u64,
    sample_rate: u32,
    channels: u8,
    peak_scale: f32,
}

impl AudioAsset {
    pub fn frames(&self) -> u64 { self.frames }
    pub fn sample_rate(&self) -> u32 { self.sample_rate }
    pub fn channels(&self) -> u8 { self.channels }
    pub fn peak_scale(&self) -> f32 { self.peak_scale }

    /// Exact duration derived from the frame count, never from a container header.
    pub fn duration_seconds(&self) -> f64 {
        self.frames as f64 / self.sample_rate as f64
    }

    /// Interleaved samples for frames `[start, start + len)`, truncated at the
    /// end of the asset. Returns an empty slice when `start >= frames`.
    /// Never panics.
    pub fn frames_range(&self, start: u64, len: usize) -> &[i16] {
        let ch = self.channels as u64;
        let begin = start.min(self.frames).saturating_mul(ch) as usize;
        let end = start
            .saturating_add(len as u64)
            .min(self.frames)
            .saturating_mul(ch) as usize;
        // begin <= end <= samples.len() holds by construction.
        &self.samples[begin..end]
    }

    pub fn resident_bytes(&self) -> u64 {
        self.samples.len() as u64 * 2
    }
}
```

多チャンネル音源は**デコード時にステレオへダウンミックス**する（5.1 の参照音源に意味は無く、
メモリと mix 経路の複雑さだけが増える）。ダウンミックス係数は ITU-R BS.775 の一般的な係数を使い、
`SourceInfo::source_channels` に元のチャンネル数を残す。

### 3.4 デコードのワーカー化

#### 3.4.1 ジョブ基盤との接続

40 のジョブ基盤（`Job<T>` / `progress: AtomicU32` 0..=10_000 / `cancel: AtomicBool`）を前提とするが、
`drill-audio` はそのクレートに依存しない。必要な操作は2つだけなのでトレイトで受ける。

```rust
// decode.rs

/// Implemented by the job runtime (40-jobs.md). Keeping this a trait means
/// `drill-audio` does not depend on the job crate and can be tested with a
/// no-op sink.
pub trait ProgressSink: Send + Sync {
    /// 0..=10_000. Called at most once per ~100 ms of decoded audio.
    fn set_progress(&self, permille_x10: u32);
    /// Polled at every packet boundary; must be cheap (a relaxed atomic load).
    fn is_cancelled(&self) -> bool;
}

/// A sink that never reports and never cancels. For tests and benches.
pub struct NullProgress;
impl ProgressSink for NullProgress {
    fn set_progress(&self, _: u32) {}
    fn is_cancelled(&self) -> bool { false }
}
```

#### 3.4.2 上限

```rust
#[derive(Clone, Copy, Debug)]
pub struct DecodeLimits {
    pub max_file_bytes: u64,
    pub max_duration_seconds: f64,
    pub min_sample_rate: u32,
    pub max_sample_rate: u32,
    pub max_source_channels: u16,
    pub max_consecutive_decode_errors: u32,
    pub max_decode_error_ratio: f32,
    /// Upper bound on a single reservation, so a lying header cannot make us
    /// ask the allocator for gigabytes in one call.
    pub max_reserve_bytes: usize,
}

impl Default for DecodeLimits {
    fn default() -> Self {
        Self {
            max_file_bytes: 1 << 30,          // 1 GiB
            max_duration_seconds: 1_800.0,    // 30 min
            min_sample_rate: 4_000,
            max_sample_rate: 384_000,
            max_source_channels: 8,
            max_consecutive_decode_errors: 64,
            max_decode_error_ratio: 0.05,
            max_reserve_bytes: 64 << 20,      // 64 MiB per grow step
        }
    }
}
```

#### 3.4.3 デコード関数

```rust
#[derive(Clone, Debug)]
pub struct SourceInfo {
    pub codec: &'static str,
    pub container: &'static str,
    pub source_sample_rate: u32,
    pub source_channels: u16,
    /// What the container claimed. May be absent or wrong; never trusted for
    /// allocation or for `AudioTrack::duration_seconds`.
    pub declared_duration_seconds: Option<f64>,
    pub decoded_frames: u64,
    pub recovered_errors: u32,
}

pub struct DecodeOutput {
    pub asset: AudioAsset,
    pub peaks: PeakPyramid,
    pub onsets: OnsetEnvelope,
    pub info: SourceInfo,
}

/// Decode `path` into a resident asset, its peak pyramid and its onset
/// envelope. Runs on a worker thread; never called from the UI thread.
///
/// Progress is reported as 0..=10_000 against the *declared* duration when
/// available, otherwise against bytes consumed.
pub fn decode_file(
    path: &Path,
    limits: DecodeLimits,
    progress: &dyn ProgressSink,
) -> Result<DecodeOutput, AudioError>;
```

手順（すべて `unwrap` / 添字パニックなし）:

1. `std::fs::metadata` でサイズを確認。`> max_file_bytes` なら `AudioError::FileTooLarge`。
2. `File::open` → `symphonia_core::io::MediaSourceStream::new`。
   `Hint` に拡張子を**ヒントとしてのみ**渡す。0.6.0 の `Probe` はスコアリング方式で誤判定を減らすため、
   拡張子偽装は自動的に無害化される（判定は中身が決める）。
3. `symphonia::default::get_probe()`（`&'static Probe` を返す）で `FormatReader` を得る。
   失敗は `AudioError::UnsupportedFormat`。
4. 音声トラックを選ぶ（複数あれば最初の音声トラック）。`AudioSpec` から sample_rate / channels を取り、
   範囲外なら `SampleRateOutOfRange` / `TooManyChannels`。
5. `symphonia::default::get_codecs()` でデコーダを構築。失敗は `UnsupportedCodec`。
6. 出力バッファは `declared_duration` から必要量を見積もるが、**1回の `try_reserve` は
   `max_reserve_bytes` を超えない**。以後は伸長しながら進める。`try_reserve` の失敗は
   `AudioError::OutOfMemory`（`reserve` は使わない＝OOM でアボートしない）。
7. ループ：`FormatReader::next_packet() -> Result<Option<Packet>>`。
   `Ok(None)` が正常終了（0.6.0 で EOF は `Ok(None)` になった）。
   - `Ok(Some(packet))` → `decode`。得た `GenericAudioBufferRef` を
     `i16` インターリーブへ変換しつつ、絶対値ピークと解析用デシメート信号を同じパスで更新する
     （**1パス**。PCM を後からもう一度舐めない）。
   - `Err(Error::DecodeError(_))` → そのパケットを捨てて継続。連続エラー数と総エラー率を数え、
     `max_consecutive_decode_errors` か `max_decode_error_ratio` を超えたら `AudioError::Corrupt`。
   - `Err(Error::ResetRequired)` → デコーダを作り直して継続。
   - `Err(Error::IoError(e))` で `e.kind() == UnexpectedEof` → 正常終了扱い。
   - それ以外の `Err` → `AudioError::Io`。
8. 各パケットごとに `progress.is_cancelled()` を見る。true なら `AudioError::Cancelled`。
   進捗は 100 ms 相当ごとに `set_progress`（毎パケット呼ぶとアトミックストアが無駄）。
9. デコード済みフレーム数が `max_duration_seconds * sample_rate` を超えたら
   `AudioError::TooLong`。**ヘッダの申告値ではなく実デコード量で打ち切る**（デコード爆弾対策）。
10. `PeakPyramid::build`（§3.4）と `analysis::finish_envelope`（§3.9）を実行して返す。

`decode_file` の呼び出し側（ジョブ基盤）は `std::panic::catch_unwind` で包む。
デコーダの想定外パニックがアプリを落とさないようにする（41 のクラッシュ方針と整合）。

#### 3.4.4 ピークとエンベロープの逐次構築

`PeakPyramid` の L0 と `OnsetEnvelope` の入力デシメーションは、デコードループ内で
**フレームが確定するたびに**進める。デコード完了後に 28.8 M フレームをもう一度走査しない。
上位レベルは L0 から作る（下位の 1/4 のコスト）。

### 3.5 `PeakPyramid`

```rust
// peaks.rs

/// Frames per bucket at each level: 256, 1024, 4096, 16384.
pub const PEAK_LEVEL_SHIFTS: [u32; 4] = [8, 10, 12, 14];
pub const PEAK_LEVELS: usize = PEAK_LEVEL_SHIFTS.len();

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Peak {
    pub min: i16,
    pub max: i16,
}

impl Peak {
    pub const SILENT: Peak = Peak { min: 0, max: 0 };
    #[inline]
    pub fn merge(self, other: Peak) -> Peak {
        Peak { min: self.min.min(other.min), max: self.max.max(other.max) }
    }
}

/// Per-zoom-level min/max envelopes. Makes waveform drawing O(pixels).
pub struct PeakPyramid {
    channels: u8,
    frames: u64,
    /// `levels[l][bucket * channels + channel]`.
    levels: [Vec<Peak>; PEAK_LEVELS],
}

impl PeakPyramid {
    pub fn build(asset: &AudioAsset) -> Self;

    /// Rebuild in place, reusing the existing allocations. Used when a track is
    /// replaced without tearing the whole engine down.
    pub fn build_into(asset: &AudioAsset, out: &mut Self);

    pub fn channels(&self) -> u8 { self.channels }
    pub fn frames(&self) -> u64 { self.frames }

    /// Fill `out` with `out_buckets * channels` peaks covering frames
    /// `[start, end)`, in bucket-major, channel-minor order.
    ///
    /// Picks the coarsest level whose bucket width still gives at least one
    /// source bucket per output bucket, so at most 4 source buckets are read
    /// per output bucket. When the span is narrower than one L0 bucket the raw
    /// PCM in `asset` is scanned instead, which is also at most 256 frames per
    /// output bucket and shrinks as the zoom deepens.
    ///
    /// `out` is cleared and refilled; its capacity is reused. O(out_buckets).
    pub fn range(
        &self,
        asset: &AudioAsset,
        start: u64,
        end: u64,
        out_buckets: usize,
        out: &mut Vec<Peak>,
    );

    /// Coarsest level index whose bucket width is <= `frames_per_bucket`,
    /// or `None` when even level 0 is too coarse.
    fn level_for(&self, frames_per_bucket: u64) -> Option<usize> {
        let mut chosen = None;
        for (i, shift) in PEAK_LEVEL_SHIFTS.iter().enumerate() {
            if (1u64 << shift) <= frames_per_bucket {
                chosen = Some(i);
            } else {
                break;
            }
        }
        chosen
    }

    pub fn resident_bytes(&self) -> u64 {
        self.levels.iter().map(|l| (l.len() * size_of::<Peak>()) as u64).sum()
    }
}
```

**`DESIGN_GAPS.md` B-2 の素案からの差分と根拠**

| 素案 | 本設計 | 根拠 |
|---|---|---|
| `Vec<(f32, f32)>` | `Vec<Peak>`（`i16` 2つ） | メモリ半減。`f32` 化は描画側（20/21）が正規化するときに1回行えばよい |
| `range(&self, ...)` | `range(&self, asset, ...)` | L0（256 フレーム）より深いズームを生 PCM から供給するため。これが無いと最大ズームで波形が階段になる |
| `levels: Vec<Vec<(f32,f32)>>` | `[Vec<Peak>; 4]` | 段数を型で固定し、`level_for` の分岐を静的にする |

**段数と幅の根拠**：段は 256 / 1024 / 4096 / 16384 フレームの4段（比は4）。
比を4にすると、選んだ段で1出力バケットあたり読む元バケットは**必ず4未満**になる
（4以上なら1段上を選べたはず）。比を2にすると段数が倍になりメモリが増え、比を8にすると
最悪読み取り数が8になる。4が下限コストと段数の均衡点。

**メモリ量**：L0 バケット数 = frames / 256。総バケット数は等比級数で
`L0 × (1 + 1/4 + 1/16 + 1/64) = L0 × 1.328`。
10 分ステレオ 48 kHz なら L0 = 112 500、総計 ≈ 149 400 バケット × 2 ch × 4 B ≈ **1.2 MB**。
30 分上限でも 3.6 MB。PCM（115 MB）に対して 1 % で、無視できる。

**`range` の計算量**：出力バケット数を P とすると、段選択が O(1)、各出力バケットで
元バケットを 1〜3 個読むので **O(P)**。1920 px × 2 ch なら最大 15 360 回の `Peak::merge`。
確保は `out` の初回伸長のみ（以後は容量再利用、`clear()` してから push）。

**構築の計算量**：L0 が N フレームの1パス、上位が N/4 + N/16 + N/64。合計 1.328 N。
10 分ステレオで約 7 700 万回の比較。

### 3.6 再生：スレッド構成とリングバッファ

#### 3.6.1 三層構成

```
[UI スレッド]                [ミキサスレッド（非RT）]        [cpal 出力コールバック（RT）]
Transport::tick()       ->   Command を try_recv           ->  Block を read_chunk
  Command を push            AudioAsset を読む                 出力バッファへコピー
  PlaybackClock を読む       gain/fade/trim/click/resample     （必要なら形式変換）
  EngineEvent を drain       Block を write_chunk_uninit       ClockShared を seqlock 更新
                             （rtrb, 32 ブロック）
```

**出力コールバックは「リングから読んでコピーし、アトミックを3回書く」以外を一切しない。**
デコード、ミックス、リサンプル、クリック合成、テンポ計算はすべてミキサスレッドにある。

#### 3.6.2 ブロックとリング

```rust
// ring.rs

pub const BLOCK_FRAMES: usize = 256;
pub const MAX_CHANNELS: usize = 2;
pub const BLOCK_SAMPLES: usize = BLOCK_FRAMES * MAX_CHANNELS;
pub const RING_BLOCKS: usize = 32;   // 8192 frames = 170 ms @ 48 kHz

/// One unit of already-mixed audio at the engine rate.
#[derive(Clone, Copy)]
pub struct Block {
    pub samples: [f32; BLOCK_SAMPLES],
    pub frames: u32,
    pub channels: u8,
    /// Timeline sample index (engine rate) of frame 0. Negative during count-in.
    pub timeline: i64,
    /// Bumped by the mixer on every seek / range change. The callback drops
    /// blocks whose epoch is older than the one the mixer last published, so a
    /// seek costs one device period, not the whole ring depth.
    pub epoch: u32,
}

pub type BlockProducer = rtrb::Producer<Block>;
pub type BlockConsumer = rtrb::Consumer<Block>;

pub fn block_ring() -> (BlockProducer, BlockConsumer) {
    rtrb::RingBuffer::new(RING_BLOCKS)
}
```

`Block` は 2 072 B。リング全体で 66 KB を**ストリーム構築時に1回だけ**確保する。
`write_chunk_uninit` を使えばミキサ側もスタック上の一時 `Block` を経由せず直接書ける。

リング深度の選定：170 ms はスケジューラのジッタ（Windows で 10〜30 ms、負荷時 50 ms 超）に対して
十分な余裕がある。深いことによるシーク遅延は epoch 破棄で回避するので、深さのデメリットが無い。

#### 3.6.3 出力コールバックの規律と時間予算

```rust
// device.rs（構築部の要点）

let config: cpal::StreamConfig = /* 選定済み */;
let engine_rate = config.sample_rate.0;
let out_channels = config.channels as usize;
let shared = Arc::clone(&clock_shared);
let origin = *PROCESS_ORIGIN;   // Instant, set once at startup

let stream = device.build_output_stream::<f32, _, _>(
    config,
    move |out: &mut [f32], info: &cpal::OutputCallbackInfo| {
        // 1) latency in frames, from the driver's own timestamps
        let ts = info.timestamp();
        let latency_frames = ts
            .playback
            .duration_since(&ts.callback)
            .map(|d| (d.as_secs_f64() * engine_rate as f64) as i64)
            .unwrap_or(0);

        // 2) drain the ring into `out` (memcpy only)
        let (written, first_timeline, epoch) = pull_blocks(&mut consumer, &mut partial, out, out_channels);

        // 3) underrun -> silence the tail, count it, do NOT log
        if written < out.len() {
            out[written..].fill(0.0);
            shared.underruns.fetch_add(1, Ordering::Relaxed);
        }

        // 4) publish position via seqlock (3 relaxed/release stores)
        let heard = first_timeline - latency_frames - shared.user_latency_frames.load(Ordering::Relaxed);
        let now_ns = origin.elapsed().as_nanos() as u64;
        publish_position(&shared, heard, now_ns, epoch);
    },
    move |err: cpal::Error| {
        // Also realtime-ish on some backends: store a code, never allocate.
        FAULT.store(fault_code(err.kind()), Ordering::Release);
    },
    None,
)?;
```

**コールバック内の禁止事項**（違反は設計違反として却下する）

| 禁止 | 理由 | 代替 |
|---|---|---|
| ヒープ確保・解放（`Vec`, `Box`, `String`, `format!`） | アロケータがロックを取り得る | すべて事前確保。`Block` は `Copy` の固定長配列 |
| `Mutex` / `RwLock` / `Condvar` | 優先度逆転でグリッチ | `rtrb` と `Atomic*` のみ |
| ログ出力・`println!` | I/O とロック | アンダーラン等はアトミックカウンタ。UI 側が読み取って通知 |
| `panic!` / `unwrap` / 添字 | UB かプロセス終了 | 添字は `get`/スライス長で保証。除算前にゼロ判定 |
| ファイル・ソケット・システムコール | ブロック | なし |
| `Arc::clone` / `Arc::drop` | 参照カウントの競合と解放 | コールバックが持つ `Arc` はクロージャに move 済みで、内部では複製しない |
| 三角関数・`exp` | 遅い（数十〜数百 ns/回） | クリックは事前レンダ済みテーブル（§3.8） |

**時間予算の算出**

| サンプルレート | バッファ | 1コールバックの周期 | 予算（5 %） | 実作業（見積） |
|---|---|---|---|---|
| 48 kHz | 512 frames | **10.67 ms** | 0.53 ms | memcpy 4 KB + 3 store ≈ **2〜4 µs**（0.04 %） |
| 48 kHz | 256 frames | 5.33 ms | 0.27 ms | ≈ 2 µs |
| 96 kHz | 128 frames | **1.33 ms** | 0.067 ms | ≈ 1.5 µs（0.11 %） |
| 192 kHz | 64 frames | 0.33 ms | 0.017 ms | ≈ 1 µs（0.3 %） |

最悪ケース（192 kHz / 64 frames）でも予算の 1/3 未満。
**コールバック内で許すのは「1サンプルあたり定数回の算術と代入」まで**と定める。
デバイスの標本形式が `f32` でないとき（`I16` / `U16` / `I24`）は
`build_output_stream_raw` を使い、コールバック内で 1 サンプル 1 乗算 1 変換の変換ループを回す。
これは上の規律に収まる（+1 µs 程度）。

#### 3.6.4 ミキサスレッド

固定周期ではなく「リングに空きがある限り埋める」駆動。空きが無くなったら
`std::thread::park_timeout(block_period / 2)` で寝る。優先度は上げない（RT ではないので不要）。

```rust
// engine.rs（ループの骨格）

fn mixer_loop(mut ctx: MixerContext) {
    while !ctx.stop.load(Ordering::Relaxed) {
        ctx.apply_commands();          // rtrb consumer, Copy commands only
        ctx.apply_updates();           // mpsc try_recv: Arc<AudioAsset> / Arc<ClickSchedule> swaps
        let mut produced = 0usize;
        while let Ok(mut chunk) = ctx.producer.write_chunk_uninit(1) {
            let block = ctx.render_block();   // no allocation
            chunk.fill_from_iter(std::iter::once(block));
            produced += 1;
            if produced >= RING_BLOCKS / 2 { break; }  // yield to check commands
        }
        if produced == 0 {
            std::thread::park_timeout(ctx.half_block_period);
        }
    }
}
```

- **UI → ミキサ**：`rtrb::Producer<Command>`（容量 64）。`Command` は `Copy` の POD で、
  `Box` も `String` も `Arc` も持たない。満杯なら UI 側は次フレームに再送する（落とさない）。
- **UI → ミキサ（重い物）**：`std::sync::mpsc::Sender<MixerUpdate>` で `Arc<AudioAsset>` /
  `Arc<ClickSchedule>` / `Arc<ClickVoices>` / `Arc<FrameMap>` を渡す。確保は UI 側で終わっており、
  ミキサは `try_recv` して `Arc` を差し替えるだけ。**ミキサは `TempoMap` も `Document` も触らない。**
- **ミキサ → UI**：`rtrb::Producer<EngineEvent>`（容量 64、`Copy` の POD）。満杯なら
  最古を捨てて `dropped_events` カウンタを増やす。

```rust
#[derive(Clone, Copy, Debug)]
pub enum Command {
    Play,
    Pause,
    Stop,
    Seek { timeline: i64, epoch: u32 },
    SetRange { start: i64, end: i64, looping: bool, epoch: u32 },
    SetSpeed(f32),
    SetMix(MixSettings),
    SetClick(ClickSettings),
    StartCountIn { counts: u16, epoch: u32 },
}

#[derive(Clone, Copy, Debug)]
pub enum EngineEvent {
    ReachedEnd { timeline: i64 },
    Looped { timeline: i64 },
    Underrun { total: u32 },
    DeviceFault(DeviceFault),
    EngineRateChanged { rate: u32 },
    /// The anchor map disagrees with the tempo map, so playback is using
    /// interpolated (non band-limited) resampling for this span.
    DegradedResample,
}
```

### 3.7 時刻の正

#### 3.7.1 原則

- **音源が読み込まれ、出力ストリームが動いているとき**：時間の正は
  出力コールバックが数えた**エンジンレートのサンプル索引**（`timeline: i64`）。
- **音源が無い／デバイスが無い／一時停止中**：単調時計（`std::time::Instant`）。
- **どちらの場合も UI が読むのは `PlaybackClock` ただ1つ。** 書き手が誰かは内部事情。

符号付き `i64` を使う理由：カウントイン（カウント 0 より前）と負のアンカーオフセットで
タイムライン索引が負になる。`AtomicU64` では表現できない（`DESIGN_GAPS.md` 素案からの差分）。
2時間再生は 48 kHz で 3.456 億フレーム、`i64` の上限 9.22×10^18 に対して 10 桁の余裕がある。

#### 3.7.2 seqlock による公開

位置とその観測時刻の2語を、UI が破れなく読むための seqlock。ロックではなく
「奇数のときは書き込み中」を示すカウンタで、書き手（RT コールバック）は待たない。

```rust
// clock.rs

pub struct ClockShared {
    /// Odd while a write is in progress.
    seq: AtomicU32,
    /// Timeline sample index (engine rate) being heard at `instant_ns`.
    position: AtomicI64,
    /// Nanoseconds since process start, from the same callback.
    instant_ns: AtomicU64,
    /// Seek generation. UI ignores positions from an older epoch than the
    /// seek it just issued.
    epoch: AtomicU32,
    /// Written only while the stream is stopped.
    engine_rate: AtomicU32,
    /// Driver-reported output latency, EMA, microseconds.
    output_latency_us: AtomicU32,
    /// User calibration, in engine frames. Signed.
    user_latency_frames: AtomicI64,
    state: AtomicU8,
    underruns: AtomicU32,
    dropped_events: AtomicU32,
}

#[derive(Clone, Copy, Debug)]
pub struct ClockSample {
    pub position: i64,
    pub instant_ns: u64,
    pub epoch: u32,
}

/// Called from the realtime callback. Three stores, no branches on shared state.
#[inline]
pub(crate) fn publish_position(s: &ClockShared, position: i64, instant_ns: u64, epoch: u32) {
    let begin = s.seq.load(Ordering::Relaxed).wrapping_add(1);
    s.seq.store(begin, Ordering::Release);          // now odd
    s.position.store(position, Ordering::Relaxed);
    s.instant_ns.store(instant_ns, Ordering::Relaxed);
    s.epoch.store(epoch, Ordering::Relaxed);
    s.seq.store(begin.wrapping_add(1), Ordering::Release);  // now even
}

#[derive(Clone)]
pub struct PlaybackClock {
    shared: Arc<ClockShared>,
    origin: Instant,
}

impl PlaybackClock {
    /// Torn-read-free snapshot. Spins at most a handful of times in practice;
    /// gives up after 8 attempts and returns the last consistent value.
    pub fn sample(&self) -> ClockSample { /* seqlock read loop */ }

    pub fn engine_rate(&self) -> u32 {
        self.shared.engine_rate.load(Ordering::Relaxed).max(1)
    }

    /// Position in show-relative seconds, extrapolated from the last callback
    /// so the playhead moves smoothly between callbacks. The extrapolation is
    /// clamped to 2 callback periods so a stalled stream freezes rather than
    /// running away.
    pub fn position_seconds(&self, now: Instant, speed: f32) -> f64 { /* ... */ }

    pub fn position_count(&self, now: Instant, speed: f32, tempo: &TempoMap) -> f32 {
        tempo.count_at(self.position_seconds(now, speed) as f32)
    }

    pub fn output_latency_seconds(&self) -> f32 {
        self.shared.output_latency_us.load(Ordering::Relaxed) as f32 * 1e-6
    }
}
```

補間の必要性：512 frames @ 48 kHz ではコールバックが 10.67 ms に1回しか来ないため、
`position` をそのまま読むと再生ヘッドが 60 fps で見て階段状に動く。
`instant_ns` からの外挿でこれを消す。外挿量の上限は 2 周期（21 ms）。

#### 3.7.3 `playback.rs::advance` との統合

現状 `main.rs:488-509` は毎フレーム `advance(global, dt, speed, range, looping, tempo)` を呼び、
戻り値でカウントを更新している。これを次のように置換する。

```rust
// drill-app 側（43 の担当範囲だが、置換の形だけここで示す）
self.transport.tick(now, &self.document.tempo, &mut self.engine_events);
let global = self.transport.clock().position_count(now, self.speed, &self.document.tempo);
self.seek_global(global);
```

- `drill_core::playback::advance` は**削除しない**。`Transport` の単調時計経路（音源なし・
  デバイスなし）が内部でこれを呼ぶ。純関数・確定論・確保なしという性質はそのまま活きる。
- 音声駆動経路では、範囲・ループの折り返しは**ミキサスレッドが `i64` サンプルで行う**。

```rust
// engine.rs
/// Integer twin of `drill_core::playback::advance`, working in timeline
/// samples instead of counts. Exact: no floating point accumulation.
pub fn wrap_timeline(timeline: i64, start: i64, end: i64, looping: bool) -> WrapResult {
    if end <= start { return WrapResult::Stopped(start); }
    if timeline < end { return WrapResult::Running(timeline); }
    if !looping { return WrapResult::Stopped(end); }
    let span = end - start;
    WrapResult::Looped(start + (timeline - start).rem_euclid(span))
}
```

`advance` と `wrap_timeline` が一致することは property test で保証する（§7）。
これが**単一の時間モデルを2実装で持つことの唯一の許容点**であり、テストで縛る。

### 3.8 クリック（メトロノーム）とカウントイン

#### 3.8.1 合成方式

指数減衰する正弦の一発音を**事前レンダしたテーブル**として持ち、ミキサは加算するだけにする。
コールバックはもちろん、ミキサスレッドでも `sin` / `exp` を毎サンプル呼ばない。

```
v[n] = sin(2π f n / rate) · exp(−n / (τ · rate)) · w[n]
```

- `f` = `accent_hz`（ダウンビート、既定 1600 Hz）または `beat_hz`（それ以外の拍、既定 1000 Hz）
- `τ` = `decay_seconds`（既定 0.030 s）
- 長さ = `ceil(4 τ · rate)`（時定数4本 = −34.7 dB）。48 kHz で 5 760 サンプル
- `w[n]` = 末尾 32 サンプルの raised-cosine フェード。テーブルが厳密に 0 で終わるようにし、
  クリック自体がクリックノイズを出さないようにする

```rust
// click.rs

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ClickSettings {
    pub enabled: bool,
    pub accent_hz: f32,
    pub beat_hz: f32,
    pub decay_seconds: f32,
    pub gain: f32,
    pub beats_per_measure: u16,
    pub count_in_counts: u16,
    /// Mute the reference track during count-in so the clicks are audible.
    pub mute_track_during_count_in: bool,
}

impl Default for ClickSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            accent_hz: 1_600.0,
            beat_hz: 1_000.0,
            decay_seconds: 0.030,
            gain: 0.5,
            beats_per_measure: 4,
            count_in_counts: 8,
            mute_track_during_count_in: true,
        }
    }
}

/// Pre-rendered one-shots for one (settings, engine_rate) pair. Built on the
/// control thread; the mixer only reads.
pub struct ClickVoices {
    accent: Box<[f32]>,
    beat: Box<[f32]>,
    rate: u32,
}

impl ClickVoices {
    pub fn render(settings: &ClickSettings, engine_rate: u32) -> Self;
    #[inline]
    pub fn voice(&self, accent: bool) -> &[f32] {
        if accent { &self.accent } else { &self.beat }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct ClickEvent {
    /// Timeline sample index at the engine rate. Negative during count-in.
    pub timeline: i64,
    pub accent: bool,
}

/// Sorted by `timeline`. Rebuilt on the control thread whenever the tempo map,
/// the time signature or the playback range changes; handed to the mixer as an
/// `Arc` swap.
pub struct ClickSchedule {
    events: Vec<ClickEvent>,
}

impl ClickSchedule {
    /// Build from the count timeline. Reuses `out`'s allocation.
    pub fn build_into(
        tempo: &TempoMap,
        total_counts: u32,
        beats_per_measure: u16,
        engine_rate: u32,
        out: &mut ClickSchedule,
    );

    /// Prepend `count_in_counts` clicks before count 0, spaced at the tempo in
    /// effect at `range_start`. They land on negative timeline indices.
    pub fn prepend_count_in(&mut self, tempo: &TempoMap, range_start: f32, counts: u16, engine_rate: u32);

    /// Index of the first event with `timeline >= from`. O(log n).
    pub fn lower_bound(&self, from: i64) -> usize;
    pub fn events(&self) -> &[ClickEvent];
}
```

拍とダウンビートの区別は `count % beats_per_measure == 0`（`beats_per_measure` は 1 未満に落とさない）。
`drill-core` の既存 `audio::downbeats`（`audio.rs:117-126`）と同じ規則で、
`ClickSchedule::build_into` はそれを整数化したもの。両者の一致をテストで縛る。

#### 3.8.2 ミキサでの加算

各ブロックで `[timeline_start, timeline_start + frames)` に**発音開始点が入る**イベントに加え、
**前のブロックから鳴り続けている**イベントも足す必要がある。ミキサは `active_voices`
（固定長 8 スロットの `[(usize /*schedule idx*/, u32 /*offset*/); 8]`）を持ち回る。
8 スロットを超える同時発音は最古を打ち切る（240 BPM でも減衰 30 ms なら同時発音は 1〜2）。
確保はゼロ。

#### 3.8.3 カウントイン

- `Command::StartCountIn { counts, epoch }` を受けると、ミキサは `timeline` を
  `range_start_timeline - count_in_samples` にセットし、epoch を上げる。
- タイムラインが負の区間では `mute_track_during_count_in` が真なら音源をミュートし、クリックのみ鳴らす。
- UI の再生ヘッドは負のタイムラインでは `range.start` に固定表示する（カウント 0 より前は存在しない）。
  `EngineEvent` は出さず、`PlaybackClock::sample().position < 0` を UI が見れば済む。

### 3.9 同期アンカー

#### 3.9.1 永続フィールドの型の裁定（v2 スキーマ、10 §3.8.1 への回答）

10 §3.8.1 は `TempoChange` を `{ count: f64, bpm: f64 }` に確定させ、
`AudioTrack` の秒と `SyncAnchor` の型は本書の所有として裁定を待っている
（10 §3.8.1 の切り分け表、および 10 §9 U17）。**以下が本書の裁定であり、v2 に含める。**

##### `SyncAnchor { count: f64, seconds: f64 }` — 両フィールドとも f64

`count` については 10 §3.8.1 の議論がそのまま当てはまる（グローバルカウント軸上の位置、
`f32` は 960 カウントで 1.46 サンプル、`MAX_TIMELINE_COUNTS` で 750 サンプルに量子化）。
さらに 10 は、非整数アンカーが自然発生する根拠として**本書の自動 BPM 推定とアンカー吸着**を
名指ししている（10 §3.8.1 の (2)）。実際 §3.10 の `snap_to_transient` は
11.6 ms 刻みのオンセット包絡から任意の実数秒を返すので、`count` は整数に乗らない。

`seconds` は**より切実**である。これはファイル時間軸上の位置で、そのままサンプル索引になる。
`f32` の ulp は時刻 t で `2^-24 · 2^ceil(log2 t)`:

| ファイル位置 | f32 ulp | 48 kHz 換算 |
|---|---|---|
| 60 s | 3.8e-6 s | 0.18 サンプル |
| 600 s（10 分） | 6.1e-5 s | **2.93 サンプル** |
| 1,800 s（30 分上限） | 1.22e-4 s | **5.86 サンプル** |

`TempoChange.count` の 1.46 サンプルより悪い。しかもこの誤差は
**§3.9.3 の整数写像の入口で入る**：`FrameMap::bake_into` は `seconds` を Q32.32 の
ファイルフレームへ焼くので、`f32` で受けた時点で 3〜6 サンプルの量子化が確定し、
そのあといくら整数演算で厳密に進めても意味がない。不変条件 2「2 時間再生後の累積誤差は
厳密に 0 サンプル」は、写像の入力が量子化されていないことを前提にしている。

加えて §3.9.2 の照合規則は `implied_bpm = 60 · Δcount / Δseconds` を
`TEMPO_MISMATCH_TOLERANCE = 0.5 %` と比べる。分母が 3〜6 サンプル刻みだと、
短い区間（16 カウント = 8 s）で相対誤差 1.5e-5 が乗る。0.5 % に対しては無害だが、
**誤差を持ち込む理由が無い**。

コスト：`MAX_SYNC_ANCHORS = 4_096` として最大 64 KB（`f32` なら 32 KB）。
現実のショーはアンカー 10〜60 個なので 160 B → 320 B。10 §3.8.1 と同じ結論：
**倒すコストが実質ゼロなので、迷う側へ倒す。**

##### `AudioTrack` の残りの秒フィールド

| v1 フィールド | v2 の型 | 理由 |
|---|---|---|
| `offset_seconds: f32` | **削除**（`anchors` へ吸収） | 下記 |
| `duration_seconds: f32` | **f64**（変更） | ファイル時間軸上の**位置**（終端）。`is_within_audio` の比較境界であり、`frames / sample_rate` の実測値を保持する。10 分で 2.93 サンプルの量子化を持ち込む理由が無い |
| `trim_start_seconds: f32` | **f64**（変更） | 同上。`MixSettings.trim_start_frame: u64` へ焼かれる位置。トリム端が数サンプルずれるのは、まさにその端でクリックノイズが出る条件 |
| `trim_end_seconds: f32` | **f64**（変更） | 同上 |
| `fade_in_seconds: f32` | **f32**（据置） | **位置ではなく長さ**。0〜30 s に制限され、30 s での ulp は 1.9e-6 s = 0.09 サンプル。フェード長が 1/10 サンプル違っても観測できない。10 が `Gate` を据え置いたのと同じ切り分け |
| `fade_out_seconds: f32` | **f32**（据置） | 同上 |
| `gain_db: f32` | **f32**（据置） | レベル。`-96..=24` に制限され ulp は 1e-6 dB |
| `muted: bool` / `path: String` | 据置 | — |

**`offset_seconds` を v2 で削除する理由。** 残すと「オフセットとアンカー 0 番のどちらが勝つか」が
永続形式の中に二重に存在することになり、片方だけ更新するバグが**無音のズレ**として現れる。
音のズレはクラッシュと違って気付かれにくく、書き出した動画で初めて発覚する。
v2 は移行関数を通す破壊的更新なので、消すならここしかない。

##### v1 → v2 の写像（10 の migration 表へ）

```
v1: audio.offset_seconds: f32 (JSON number)
v2: audio.anchors: [ { "count": 0.0, "seconds": <v1 offset_seconds> } ]
```

- **分岐なし。** `offset_seconds` が 0.0 でもアンカーを 1 個生成する
  （0 個と 1 個 ⟨0,0⟩ は §3.9.1 の規則で意味が同じだが、分岐を消すほうが移行を検証しやすく、
  「1 アンカー = 旧 `offset_seconds` 挙動」の等価性テスト U-3 がそのまま効く）。
- `duration_seconds` / `trim_*_seconds` の f32→f64 は **10 §3.8.1 の `TempoChange` と同じく
  写像コード不要**。JSON に浮動小数の幅は無く、`serde_json` は数値を `f64` で保持する。
  同じ注意も引き継ぐ：v1 往復のゴールデン期待値は「widening 後の f32」ではなく
  **「JSON テキストの数値」**（10 §7.5 の `v1_tempo_widens_from_json_text_not_from_f32_bits` と同型）。
- `audio` が `None` の v1 文書には何も起きない。

##### 検証規則（10 の V 表へ 1 項目）

`AudioTrack::validate()` に次を加える。

- `anchors.len() <= MAX_SYNC_ANCHORS`（4,096）
- 各 `count` / `seconds` が有限
- `count` が非負・厳密昇順（重複なし）・`<= MAX_TIMELINE_COUNTS`
- `seconds` が非負・**厳密昇順**
- `duration_seconds` / `trim_*_seconds` が有限かつ非負

`seconds` の厳密昇順は `count` とは独立に要る。両方昇順でないと区間の傾きが負になり、
**音源が逆再生される**写像ができてしまう。`seconds` が `duration_seconds` を超えることは
**エラーにしない**（アセットが `Missing` で長さが未知の状態でも文書を開けるため。
`PRODUCT_QUALITY.md`「音声・画像が欠落してもドリル本体を開ける」）。範囲外は UI の警告に留める。

#### 3.9.2 型（`drill-core::audio` に置く）

アンカーは `Document` にシリアライズされ、`drill-core` が検証する対象なので、
**型と写像は `drill-core` 側**に置く（`drill-audio` に置くと `Document` が音声クレートに依存してしまう）。
デコード・再生・波形・解析だけが `drill-audio`。

```rust
// crates/drill-core/src/audio.rs への追加（v2）

pub const MAX_SYNC_ANCHORS: usize = 4_096;

/// One point where a global count is pinned to a position in the audio file.
///
/// Both fields are f64: they are positions on a time axis, and f32 quantizes
/// them to 1.46 samples (count, at 960 counts) and 2.93 samples (seconds, at
/// 600 s) at 48 kHz. See section 3.9.1 and design 10 section 3.8.1.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct SyncAnchor {
    /// Global count on the show timeline.
    pub count: f64,
    /// Seconds from the physical start of the audio file.
    pub seconds: f64,
}

/// Piecewise-linear map between the count timeline and positions in the file.
///
/// - 0 anchors: identity against the tempo map (playback starts at file 0 s).
/// - 1 anchor: pure translation. Exactly reproduces the v1
///   `AudioTrack::offset_seconds` behaviour.
/// - n anchors: linear between neighbours; outside the outermost anchors the
///   adjacent segment's slope is extended.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct AnchorMap {
    anchors: Vec<SyncAnchor>,
}

/// Relative BPM deviation above which an anchor segment is reported as
/// disagreeing with the tempo map. 0.5 % is ~1.8 s of drift over a 6 min show.
pub const TEMPO_MISMATCH_TOLERANCE: f64 = 0.005;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TempoMismatch {
    pub segment: usize,
    pub start_count: f64,
    pub end_count: f64,
    pub anchor_bpm: f64,
    pub tempo_bpm: f64,
    /// Accumulated drift in seconds across this segment.
    pub drift_seconds: f64,
}

impl AnchorMap {
    /// Sorted ascending by count; a later duplicate count replaces the earlier.
    /// Non-finite values are rejected.
    pub fn from_anchors(anchors: impl IntoIterator<Item = SyncAnchor>) -> Self;

    /// Migration path for v1 documents, which carried a single scalar offset.
    /// Always yields exactly one anchor, including for `0.0`.
    pub fn from_offset(offset_seconds: f64) -> Self {
        Self::from_anchors([SyncAnchor { count: 0.0, seconds: offset_seconds }])
    }

    pub fn anchors(&self) -> &[SyncAnchor];
    pub fn is_empty(&self) -> bool;

    /// Insert or replace the anchor at `count`. Returns the resulting index.
    pub fn set(&mut self, anchor: SyncAnchor) -> usize;
    pub fn remove(&mut self, index: usize);

    /// Global count -> seconds into the file. O(log n).
    /// Uses `TempoMap::seconds_at_f64` (design 11) end to end; no f32 appears.
    pub fn file_seconds_at(&self, count: f64, tempo: &TempoMap) -> f64;

    /// Seconds into the file -> global count. Exact inverse of
    /// [`file_seconds_at`](Self::file_seconds_at) for finite inputs. O(log n).
    pub fn count_at(&self, file_seconds: f64, tempo: &TempoMap) -> f64;

    /// BPM implied by the span between anchor `i` and `i + 1`:
    /// `60 * (count[i+1] - count[i]) / (seconds[i+1] - seconds[i])`.
    /// `None` for the last anchor or a degenerate span.
    pub fn implied_bpm(&self, i: usize) -> Option<f64>;

    /// Segments whose implied BPM differs from the tempo map by more than
    /// [`TEMPO_MISMATCH_TOLERANCE`]. Reuses `out`'s allocation.
    pub fn mismatches(&self, tempo: &TempoMap, out: &mut Vec<TempoMismatch>);

    /// Tempo changes that make a tempo map agree with these anchors.
    /// Feeds the "テンポマップへ反映" command (an `Edit`, see 10).
    pub fn to_tempo_changes(&self, out: &mut Vec<TempoChange>);
}
```

#### 3.9.2 アンカーとテンポマップが食い違うときの判断

これが本節の核心である。カウント→実時間の写像は、視覚（ドリルの動き）と聴覚（音源）で
**同一でなければならない**。ところが現状、視覚は `TempoMap` を、音源はアンカーを使う。
2つが食い違うと必ずズレる。取り得る策は3つある。

| 案 | 内容 | 判定 |
|---|---|---|
| (a) アンカーを唯一の正とし、`TempoMap` はその表示に降格 | 実装は単純 | **却下**。テンポ編集（10/11）とカウントシート（17）が `TempoMap` を書く。降格させると編集モデルが二重になる |
| (b) `TempoMap` を正とし、差分を**音源の時間伸縮**で吸収 | 見た目は常に合う | **却下**。音楽の再生速度が区間ごとに変わり、ピッチが揺れる。参照音源として使い物にならない |
| (c) アンカーは**提案**。食い違いを検出して `TempoMap` への反映を促す | 正が1つのまま | **採用** |

**採用する規則**

1. **再生と書き出しの写像はアンカーが決める**（`AnchorMap::file_seconds_at`）。
   ただしアンカーが `TempoMap` と一致していれば、これは `TempoMap` と同じ答えになる。
2. アンカーを編集するたびに `mismatches()` を評価する。1件でもあれば
   - 非モーダルの警告を出す（「アンカーとテンポマップが N 箇所で不一致。最大 X 秒ずれます」）
   - **「テンポマップへ反映」** の1操作を提示する。これは `to_tempo_changes()` の結果を
     `Edit` コマンド（10）として適用する＝Undo 可能・決定論。
3. 反映すると全区間の写像の傾きが厳密に 1.0 になり、音源は**一切リサンプルされない**。
   これが通常状態である。
4. 未反映のまま再生した場合、傾き ≠ 1 の区間だけ線形補間で読む（帯域制限なし）。
   ミキサは `EngineEvent::DegradedResample` を出し、UI は「プレビュー品質」と表示する。
   **音を勝手に伸縮して黙っている、ということはしない。**
5. **動画書き出し（31）は未反映状態を拒否する。** 書き出し前検査に
   「アンカーとテンポマップの整合」を1項目として加える。書き出した動画で音と絵がズレるのが
   最悪の失敗であり、そこは水際で止める。

この規則により、「アンカーは音楽から真のテンポを読み取るための入力装置」「`TempoMap` は
それを受け取った唯一の真実」という役割分担になる。§3.10 の自動解析はこの入力を自動化する。

#### 3.9.3 `FrameMap`：ミキサが使う整数写像

ミキサスレッドは `TempoMap` も `AnchorMap` も持たない（`Document` に触らせないため）。
制御スレッドが両者から**エンジンレートの整数写像**を焼き、`Arc` で渡す。

```rust
// mix.rs

/// Timeline frame -> file frame, piecewise linear, in 32.32 fixed point so
/// that stepping never accumulates floating point error.
#[derive(Clone, Copy, Debug)]
pub struct FrameSegment {
    pub timeline_start: i64,
    /// File frame at `timeline_start`, Q32.32.
    pub file_start_q32: i64,
    /// File frames advanced per timeline frame, Q32.32. `1 << 32` means 1:1.
    pub step_q32: i64,
}

#[derive(Clone, Debug, Default)]
pub struct FrameMap {
    segments: Vec<FrameSegment>,
    engine_rate: u32,
    asset_rate: u32,
}

impl FrameMap {
    /// Bake `AnchorMap` + `TempoMap` into integer segments at `engine_rate`.
    /// Runs on the control thread; reuses `out`'s allocation.
    pub fn bake_into(
        anchors: &AnchorMap,
        tempo: &TempoMap,
        total_counts: u32,
        asset_rate: u32,
        engine_rate: u32,
        out: &mut FrameMap,
    );

    /// Segment covering `timeline`, or the nearest one when outside. O(log n).
    pub fn segment_at(&self, timeline: i64) -> FrameSegment;

    /// True when every segment steps exactly 1:1, i.e. no interpolation is
    /// needed anywhere. The normal, reconciled state.
    pub fn is_unit_step(&self) -> bool;
}
```

`step_q32` は「エンジンレート1フレームあたり進む**エンジンレート換算の**ファイル位置」であり、
音源レート→エンジンレートの変換（44.1 k → 48 k など）は §3.11 の帯域制限リサンプラが別に受け持つ。
`FrameMap` が扱うのは**アンカーによる平行移動と傾き**だけ。反映済みなら `step_q32 == 1 << 32`。

区間内は加算だけで進む（整数加算は誤差ゼロ）。ブロック先頭では
`file_q32 = file_start_q32 + (timeline - timeline_start) * step_q32` を1回だけ乗算する。
乗数は区間長で頭打ちなので `i64` はあふれない（30 分 × 48 kHz = 8.64×10^7、
`step_q32 ≈ 4.29×10^9` で積は 3.7×10^17 < 9.22×10^18）。

### 3.10 自動解析

すべてデコードジョブの中で、**解析専用のデシメート信号**に対して行う。生 PCM は再走査しない。

- 解析レート `ANALYSIS_RATE = 22_050 Hz`（モノラル）。デコードループ内で
  多相 FIR（33 タップ）で間引きながら作る。10 分で 13 230 000 サンプル、53 MB → いや、
  `f32` で 52.9 MB。これは解析中だけの一時バッファで、`OnsetEnvelope` を作ったら解放する。
- STFT：`N = 1024`（46.4 ms）、`hop = 256`（**11.6 ms**）、Hann 窓、`realfft`。
  10 分で 51 680 フレーム。

```rust
// analysis.rs

pub const ANALYSIS_RATE: u32 = 22_050;
pub const ANALYSIS_WINDOW: usize = 1024;
pub const ANALYSIS_HOP: usize = 256;

/// Onset strength over time. One value per hop.
#[derive(Clone, Debug)]
pub struct OnsetEnvelope {
    /// Half-wave rectified log-magnitude spectral flux, normalised to [0, 1].
    pub flux: Vec<f32>,
    pub hop_seconds: f32,
}

impl OnsetEnvelope {
    pub fn seconds_at(&self, index: usize) -> f32 { index as f32 * self.hop_seconds }
    pub fn resident_bytes(&self) -> u64 { (self.flux.len() * 4) as u64 }
}

#[derive(Clone, Copy, Debug)]
pub struct TempoCandidate {
    pub bpm: f32,
    /// 0..=1, relative to the best candidate.
    pub confidence: f32,
}

#[derive(Clone, Copy, Debug)]
pub struct DownbeatCandidate {
    pub file_seconds: f32,
    pub bpm: f32,
    pub beats_per_measure: u16,
    pub confidence: f32,
}

/// Top candidates in descending confidence. Reuses `out`. Never returns a
/// single answer: octave errors are the dominant failure mode and the UI must
/// let the user pick. Search range 50..=260 BPM.
pub fn estimate_tempo(env: &OnsetEnvelope, out: &mut Vec<TempoCandidate>);

/// Bar-phase candidates for `beats_per_measure` in {2, 3, 4, 6}, given a BPM.
pub fn estimate_downbeats(env: &OnsetEnvelope, bpm: f32, out: &mut Vec<DownbeatCandidate>);

/// Onset times in file seconds, by adaptive-threshold peak picking.
pub fn detect_transients(env: &OnsetEnvelope, out: &mut Vec<f32>);

/// Nearest transient within `tolerance_seconds`, for anchor snapping.
/// `transients` must be sorted ascending. O(log n).
pub fn snap_to_transient(transients: &[f32], seconds: f32, tolerance_seconds: f32) -> Option<f32>;
```

#### アルゴリズムの選定と精度の期待値

| 課題 | 手法 | 選定理由 | 期待精度 |
|---|---|---|---|
| オンセット強度 | 対数振幅スペクトルの半波整流フラックス + 周波数方向 max フィルタ（SuperFlux 系のビブラート抑制） | 実装が単純で、マーチングバンドの録音（打楽器が明瞭）に強い。機械学習不要＝決定論 | — |
| BPM 推定 | オンセット包絡の自己相関 × 対数正規テンポ事前分布（中心 120 BPM、σ ≈ 1.0 オクターブ） | 事前分布でオクターブ誤り（半分/倍）を減らす。Ellis のビートトラッカと同系 | テンポ一定の録音で**±0.5 %**。オクターブ誤りが最大の失敗要因なので**上位3候補を返す** |
| ダウンビート | BPM 確定後、拍位相を櫛形フィルタで走査し、`beats_per_measure ∈ {2,3,4,6}` それぞれで小節位相をスコアリング | 追加コストが小さく、4/4 の明快な曲では十分 | 明快な 4/4 で**上位1候補が 70〜85 %**。ゆえに**自動適用せず候補提示のみ** |
| トランジェント | 包絡の局所ピーク + 移動中央値 × 係数の適応閾値 | アンカーのドラッグ吸着（±60 ms）に使う。誤検出はユーザーが見て捨てられる | 打点で 90 % 以上、レガートで低下 |

**自動適用は一切しない。** 解析結果は「候補」であり、
`AnchorMap::set` を呼ぶのは常に利用者の操作（`Edit` コマンド）である。
理由：誤ったアンカーが自動で入ると、テンポマップまで自動で書き換わり、
ドリル全体のタイミングが壊れる。取り消せるとしても、そこまで自動化する価値が無い。

計算コスト：51 680 回の 1024 点実 FFT。`realfft` の 1024 点は概ね 5〜10 µs なので **0.26〜0.52 s**。
自己相関は包絡（51 680 点）に対してラグ 50〜260 BPM 分（≈ 100 ラグ）で 5×10^6 積和 ≈ 5 ms。
デコード（数秒）に対して支配的でない。

### 3.11 ミックスと非破壊調整の適用点

#### 3.11.1 パイプライン

```
AudioAsset (i16, asset_rate)
  ├─ read       FrameMap で読み位置決定 → i16→f32 → × peak_scale
  ├─ trim gate  [trim_start_frame, trim_end_frame) の外は無音（フレームを詰めない）
  ├─ fade       トリム範囲の端から cos S カーブ
  ├─ gain       AudioTrack::gain_linear()（mute は 0）
  ├─ rate       rubato::Fft  asset_rate -> engine_rate   ← 等しければ構築もしない
  ├─ warp       rubato::Async  speed / 非 1.0 の step_q32 ← 1.0 なら構築もしない
  ├─ click      ClickVoices を加算（engine_rate、ショー時間基準）
  └─ clamp      [-1.0, 1.0]
```

順序の根拠：trim / fade / gain は**ファイル時間**で定義されるのでリサンプル前。
click は**ショー時間**で定義されるのでリサンプル後。この順序を崩すとフェードの長さが速度で変わる。

**trim は「範囲外を無音にする」であって「切り詰めて前に詰める」ではない。**
カウントとファイル位置の対応はアンカーが決めており、トリムでファイルが縮むと
その対応が壊れるため。`AudioTrack::effective_duration()`（`audio.rs:56-63`）は
UI の情報表示用の値として残す。

#### 3.11.2 決定論と書き出しの共有

```rust
/// Deterministic block mixer. The realtime mixer thread and the offline
/// exporter (31-video-export.md) call this same function, so the audio muxed
/// into an exported video is sample-identical to what was monitored.
/// Allocation-free: everything lives in `pipe`.
pub fn mix_block(
    asset: Option<&AudioAsset>,
    map: &FrameMap,
    mix: MixSettings,
    clicks: &ClickSchedule,
    voices: &ClickVoices,
    timeline_start: i64,
    frames: usize,
    out_channels: u8,
    out: &mut [f32],
    pipe: &mut MixPipeline,
) -> MixStats;

#[derive(Clone, Copy, Debug, Default)]
pub struct MixStats {
    pub peak: f32,
    pub clipped_samples: u32,
    pub clicks_fired: u32,
    pub degraded_resample: bool,
}

/// POD, `Copy`, sendable through the command ring without allocation.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MixSettings {
    /// `AudioTrack::gain_linear() * AudioAsset::peak_scale()`.
    pub track_gain: f32,
    pub click_gain: f32,
    /// File frames. `trim_end_frame == 0` means "physical end of file".
    pub trim_start_frame: u64,
    pub trim_end_frame: u64,
    pub fade_in_frames: u32,
    pub fade_out_frames: u32,
    pub speed: f32,
    pub track_muted: bool,
    pub click_enabled: bool,
}

impl MixSettings {
    /// Derive from the document model. The only place `AudioTrack` is read by
    /// the audio engine; everything downstream sees plain numbers.
    pub fn from_track(track: &AudioTrack, asset: &AudioAsset, speed: f32) -> Self;
}
```

`mix_block` を純関数にすることで、次が同時に得られる。

- **決定論**（`00-conventions.md` 不変条件 5）：同じ入力から同じサンプル列。
- **モニタと書き出しの一致**：`video.rs:159-190` が FFmpeg に生パスを渡す現状では
  trim/gain/fade が無視される。書き出し側は `mix_block` で生成した WAV を
  第2入力に渡す（31 の担当）。
- **テスト容易性**：デバイス無しでゴールデン比較できる。

### 3.12 `drill-core` への変更（最小）

本書が要求する `drill-core` の変更は次だけ。実装は該当文書の担当に従う。

1. `crates/drill-core/src/audio.rs` に `SyncAnchor` / `AnchorMap` / `TempoMismatch` /
   `TEMPO_MISMATCH_TOLERANCE` を追加（§3.9.1）。
2. `AudioTrack` に `#[serde(default)] pub anchors: AnchorMap` を追加。
   `offset_seconds` は**残す**（既存ファイルの読み込みのため）。読み込み時に
   `anchors.is_empty() && offset_seconds != 0.0` なら `AnchorMap::from_offset` へ移行する
   （移行処理は 41 の担当）。`count_to_audio_time` / `audio_time_to_count`
   （`audio.rs:86-96`）は `anchors` 経由の実装に差し替え、1アンカー時に現在と同一の結果になることを
   既存テスト（`audio.rs:162-184`）で確認する。
3. **`TempoMap` に `f64` の写像を追加する**（10 または 11 の担当への依頼）。

```rust
// crates/drill-core/src/tempo.rs に追加してほしいシグネチャ
impl TempoMap {
    pub fn seconds_at_f64(&self, global_count: f64) -> f64;
    pub fn count_at_f64(&self, seconds: f64) -> f64;
}
```

理由：`f32` の秒は、時刻 t での分解能が `2^-24 · 2^ceil(log2 t)`。
48 kHz の 1 サンプル（20.8 µs）を割るのは t ≈ 350 s 以降であり、
**6 分を超えるショーでは `seconds_at` がサンプル精度を保てない**。
`MEDIA_PIPELINE.md` の品質ゲート「音声時刻は sample index」を守るには、
サンプル索引へ焼く経路（`FrameMap::bake_into`、`ClickSchedule::build_into`）が `f64` を通る必要がある。
既存の `f32` API は UI 表示用に残してよい。

4. `AudioTrack::validate()` の戻り値を `Result<(), DrillError>` にし、
   日本語リテラル（`audio.rs:66,70,73,76`）を `Locale` 経由に移す（**42 の担当**、本書は要求のみ）。

---

## 4. 不変条件

テストで検証できる形で書く。括弧内は §7 の対応項目。

**時刻**

1. 音源があり出力ストリームが動いているとき、`PlaybackClock` が返す位置は
   **出力コールバックが数えたサンプル索引にのみ由来する**。壁時計はコールバック間の外挿にしか使わず、
   外挿量は 2 バッファ周期以下。(T-4)
2. タイムライン位置は `i64` のサンプル索引で進む。浮動小数の累加で時間を進めない。
   2 時間再生後の累積誤差は**厳密に 0 サンプル**。(T-5)
3. `wrap_timeline` と `drill_core::playback::advance` は、同じ範囲・ループ設定に対して
   ±1 サンプル以内で一致する。(P-1)
4. `AnchorMap::file_seconds_at` と `count_at` は互いの逆写像（有限入力・1e-4 以内）。
   アンカーが1個のとき、結果は既存の `count_to_audio_time` と一致する。(P-2, U-3)
5. `FrameMap` の区間内進行は整数加算のみで、ドリフトを生じない。(U-6)

**リアルタイム安全性**

6. 出力コールバックは、確保・解放・ロック取得・システムコール・ログ・`panic` を行わない。(R-1)
7. 出力コールバックの実行時間は、そのバッファ周期の 5 % 未満。(B-3)
8. ミキサスレッドは、定常状態で1バイトもヒープを確保しない。(R-2)
9. UI スレッドはデコード・FFT・ピーク構築・リサンプルを行わない。(R-3)

**メモリ**

10. 常駐 PCM は `i16` で、`frames × channels × 2` バイト。上限 30 分・ステレオ = 345.6 MB。(U-1)
11. `PeakPyramid` の常駐量は PCM の 1.05 % 未満。(U-4)
12. 2 時間の連続再生で、RSS の増加は暖機後 2 MiB 未満。(S-2)
13. `Block` リング・クリックテーブル・ミックス中間バッファはストリーム構築時に確保され、
    再生中に伸長しない。(R-2)

**波形**

14. `PeakPyramid::range` の計算量は `O(out_buckets)`。出力バケット1個あたりの元バケット読み取りは
    4 未満（生 PCM 経路では 256 フレーム未満で、ズームが深いほど減る）。(P-3, B-2)
15. `range` の結果は、同じ `(start, end, out_buckets)` に対して常に同じ。段の選択が決定論。(U-5)

**決定論と一致**

16. `mix_block` は同じ入力から同じ出力を返す。モニタで聞こえた音と、書き出された音声が
    サンプル単位で一致する。(G-1)
17. `ClickSchedule` のイベント時刻は、`drill_core::audio::click_track` /
    `downbeats` が返す秒を `engine_rate` で丸めたものと一致する。(U-7)

**安全性**

18. `decode_file` はいかなる入力に対してもパニックしない。返るのは値か `AudioError`。(F-1, F-2)
19. デコードは `max_duration_seconds` / `max_file_bytes` を**実測値**で超えた時点で停止する。
    コンテナの申告値を信用しない。(F-3)
20. デバイスが消えても、UI は固まらず、ドキュメントは失われず、単調時計へ縮退して再生を続けられる。(F-5)

---

## 5. 性能

`PRODUCT_QUALITY.md` の基準規模（演者 1 000 / セット 64 / 総カウント 2 048）と
16.6 ms のフレーム予算に対する本設計の取り分。

### 5.1 UI スレッドの取り分：**0.30 ms**（16.6 ms の 1.8 %）

| 処理 | 頻度 | 想定コスト |
|---|---|---|
| `PlaybackClock::sample()`（seqlock 読み） | 1 回/フレーム | < 0.1 µs |
| `TempoMap::count_at`（O(テンポ区間数)、64 セットで ≤ 64） | 1 回/フレーム | < 1 µs |
| `Transport::tick`：コマンド push とイベント drain | 1 回/フレーム | < 0.5 µs |
| `PeakPyramid::range` 1920 px × 2 ch | 波形が見えているフレームのみ | **20〜40 µs** |
| `AnchorMap::mismatches`（アンカー ≤ 64） | 編集時のみ | < 5 µs |
| `ClickSchedule::build_into`（2 049 イベント） | テンポ/拍子変更時のみ | ≈ 30 µs |
| `FrameMap::bake_into`（アンカー数 + 1 区間） | アンカー変更時のみ | < 5 µs |
| 合計（定常・波形表示中） | | **≈ 45 µs = 0.045 ms** |

予算 0.30 ms に対して 7 分の 1。波形の**描画**（DisplayList 生成と GPU 投入）は 20/21 の取り分で、
本書はそのための min/max 配列を供給するところまで。

### 5.2 リアルタイムコールバック

§3.6.3 の表の通り。最悪（192 kHz / 64 frames、周期 0.33 ms）でも約 1 µs、予算の 0.3 %。

### 5.3 ミキサスレッド（別コア、UI 予算外）

1 秒分（48 000 フレーム × 2 ch）あたり：

| 段 | 1 サンプルあたりの演算 | 1 秒あたり |
|---|---|---|
| read + i16→f32 + peak_scale | 1 変換 + 1 乗算 | 0.19 Mops |
| trim + fade + gain | 1 比較 + 2 乗算 | 0.29 Mops |
| rate（`rubato::Fft`、44.1→48 のみ） | ≈ 30 flop | 2.9 Mflop |
| warp（通常バイパス） | 0 | 0 |
| click | 減衰中のみ加算 | < 0.01 Mops |
| clamp | 2 比較 | 0.19 Mops |
| 合計（リサンプル有） | | **≈ 3.6 Mflop/s** |

最近の CPU コアの数 GFLOP/s に対して 0.1 % 未満。等速・レート一致なら 0.7 Mops/s。

### 5.4 デコードジョブ（ワーカー、UI 予算外）

| 工程 | 10 分ステレオ 48 kHz |
|---|---|
| symphonia デコード（MP3、SIMD 有効） | 目標 **5 s 以内**（実測はベンチ B-1 で確定する） |
| i16 変換 + ピーク L0 + 解析デシメート（デコードと同一パス） | +10〜20 % |
| `PeakPyramid` 上位段 | ≈ 20 ms |
| STFT 51 680 フレーム（1024 点 realfft） | **0.26〜0.52 s** |
| 自己相関 BPM 推定 | ≈ 5 ms |
| 合計 | **6 s 以内**を目標とする |

進捗は 100 ms 相当ごと、キャンセル判定は毎パケット（≈ 26 ms 相当）なので、
キャンセルから停止まで最悪 30 ms。

### 5.5 メモリ（10 分ステレオ 48 kHz）

| 項目 | 量 |
|---|---|
| `AudioAsset`（i16 インターリーブ） | **115.2 MB** |
| `PeakPyramid`（4 段 × 2 ch × 4 B） | 1.2 MB |
| `OnsetEnvelope`（51 680 × 4 B） | 0.21 MB |
| `ClickSchedule`（2 049 × 16 B） | 0.033 MB |
| `ClickVoices`（2 × 5 760 × 4 B） | 0.046 MB |
| `Block` リング（32 × 2 072 B） | 0.066 MB |
| `MixPipeline` 中間バッファ（3 × 1024 frames × 2 ch × 4 B） | 0.025 MB |
| `FrameMap`（≤ 64 区間 × 24 B） | < 0.002 MB |
| **合計** | **≈ 116.8 MB** |

解析用デシメート信号（10 分で 52.9 MB）はデコードジョブ内の一時領域で、`OnsetEnvelope` 完成時に解放する。
30 分上限では PCM 345.6 MB + ピーク 3.6 MB + 包絡 0.62 MB ≈ 350 MB。

**2 時間再生でメモリが増えないこと**：全バッファは固定長で、再生中の唯一の可変長は
`Transport::tick` が `&mut Vec<EngineEvent>` に書くイベント列（毎フレーム `clear()`、容量再利用、
上限 64）。`Arc` の複製もストリーム構築時のみ。ソークテスト S-2 で担保する。

---

## 6. 失敗モードと安全性

### 6.1 信頼できない音声ファイル

他人から受け取ったプロジェクトに付いてくる音源は**敵性入力**として扱う。

| 攻撃 / 事故 | 対処 | 結果 |
|---|---|---|
| 巨大ファイル（100 GB） | 開く前に `metadata().len() > 1 GiB` を検査 | `AudioError::FileTooLarge`。読み込まない |
| 長時間（10 時間の FLAC） | **実デコードフレーム数**が `1_800 s × rate` を超えた時点で停止 | `AudioError::TooLong` |
| デコード爆弾（申告 3 秒・実体 10 時間） | 同上。申告値は進捗表示にしか使わない | 同上 |
| 偽装拡張子（`.wav` の中身が MKV） | 拡張子は `Hint` にすぎない。0.6.0 の `Probe` はスコアリングで判定 | 中身どおりに読むか `UnsupportedFormat` |
| 破損フレーム | `DecodeError` はスキップ。連続 64 回 or 総エラー率 5 % 超で中止 | `AudioError::Corrupt { decoded_frames }`。部分デコード分は破棄 |
| 異常なサンプルレート（0 / 4 GHz） | `4_000..=384_000` 外を拒否 | `SampleRateOutOfRange`。ゼロ除算が起きる箇所が消える |
| 異常なチャンネル数（256 ch） | `1..=8` 外を拒否。2 ch にダウンミックス | `TooManyChannels` |
| ヘッダが巨大な duration を申告 | 事前確保を `try_reserve` で行い、1回の要求を 64 MiB で頭打ち | `AudioError::OutOfMemory`（アボートしない） |
| NaN / Inf を含む float WAV | `i16` 量子化前に `is_finite()` で 0 に置換し、置換数を `SourceInfo` に記録 | 無音になるが落ちない |
| 整数オーバーフロー（`frames * channels`） | フレーム上限（30 分 × 384 kHz × 2 = 1.38×10^9）で `u64` に収まることを型と上限で保証。境界計算は `saturating_*` / `checked_*` | パニックしない |
| パス・トラバーサル（`../../etc/passwd`） | 41 のプロジェクトコンテナが相対パスを正規化し、`..` を拒否 | 本クレートは正規化済みの絶対パスしか受け取らない |
| デコーダ自体のパニック | ジョブ境界で `catch_unwind` | デコード失敗として通知。アプリは生存 |

**音源が欠落・破損してもドリル本体は開ける**（`PRODUCT_QUALITY.md`）。
`Document::audio` は `Option<AudioTrack>` のままで、アセットの状態は
`AssetState::{ Loading, Ready, Missing, Failed(AudioError) }`（41 の型）として別に持つ。
音が無いだけで、編集・保存・印刷・座標書き出しは全部できる。

### 6.2 音声デバイスの失敗

| 事象 | cpal 0.18 の `ErrorKind` | 対処 |
|---|---|---|
| デバイスが1つも無い | `HostUnavailable` / 列挙が空 | ストリームを開かず、**単調時計へ縮退**。再生・編集は続行。UI に非モーダル通知 |
| 再生中に USB オーディオを抜いた | `DeviceNotAvailable` | エラーコールバックは `AtomicU8` に印を置くだけ。監視スレッドがストリームを畳み、既定デバイスで再構築 |
| OS がルートを切り替えた（Bluetooth 接続など） | `DeviceChanged` | **再構築不要**（cpal が自動で再ルートしたことを示す）。エンジンレートだけ再確認 |
| ストリーム設定が無効化された | `StreamInvalidated` | 再構築する |
| 別アプリが排他で掴んでいる | `DeviceBusy` | 250 ms → 500 ms → 1 s の指数バックオフで3回再試行。だめなら縮退 |
| RT 昇格が拒否された | `RealtimeDenied` | 続行。バッファ長を1段上げ、UI に「レイテンシが増えます」を1回だけ通知 |
| アンダーラン | `Xrun` | カウンタを増やすだけ。10 秒で 5 回を超えたらリング深度を 32 → 48 ブロックへ増やして再構築（1回だけ） |
| サンプルレートが変わった | 再構築時に判明 | 現在位置を秒に直し、新レートでサンプル索引を再計算。`FrameMap` / `ClickSchedule` / `ClickVoices` を焼き直し、`EngineEvent::EngineRateChanged` |
| 権限拒否 | `PermissionDenied` | 縮退。OS の設定を促す文言（42） |

```rust
// device.rs
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeviceFault { None, Busy, Disconnected, Rerouted, Invalidated, HostGone, Denied, Backend }

/// Owns the cpal stream and rebuilds it when it faults. Runs on its own
/// thread; polls the fault flag at 20 Hz and coalesces device-change storms
/// with a 500 ms quiet period.
pub struct DeviceSupervisor { /* ... */ }
```

**再構築中も位置は失われない。** ストリームを畳む直前に
`ClockShared::position` を秒へ変換して保持し、新ストリームのレートでサンプルへ戻す。
再構築が全部失敗したら単調時計に切り替え、UI は再生を続ける（絵は動く、音が出ない）。

### 6.3 2時間再生とリーク

- 定常状態でヒープ確保が起きる箇所を**ゼロ**にする（不変条件 8, 13）。
- `EngineEvent` リングが満杯でも `Vec` を伸ばさない（最古を捨ててカウンタを増やす）。
- ログはミキサ／監視スレッドのみ、かつ同種メッセージは 10 秒に1回へレート制限する。
- `Arc<AudioAsset>` の差し替えで古いアセットが解放されるのは**制御スレッド**。
  ミキサが最後の参照を落とすと、ミキサスレッドで `dealloc` が走る（RT ではないので許容だが、
  数百 MB の解放は数 ms かかる）。差し替え時は制御スレッド側で先に `Arc` を保持し、
  ミキサから「離した」通知を受けてから落とす。

### 6.4 数値の失敗

- `engine_rate` は `max(1)` でクランプしてから除算する（ゼロ除算防止）。
- `speed` は `0.05..=4.0` にクランプ。0 は一時停止として扱い、除算に使わない。
- `AnchorMap::implied_bpm` は `seconds` 差がゼロ／負のとき `None`。
  逆写像で無限大が出ない。
- `AnchorMap::from_anchors` は非有限値のアンカーを捨てる（`00-conventions.md` NaN/Inf 拒否）。
- 出力は必ず `[-1.0, 1.0]` にクランプしてからデバイスへ渡す。
  クランプ回数は `MixStats::clipped_samples` に集計し、UI が「音量が大きすぎます」と示せるようにする。

---

## 7. テスト計画

### 単体（U）

| ID | 内容 |
|---|---|
| U-1 | `AudioAsset::frames_range` の境界：`start >= frames`、`start + len > frames`、`len == 0`。空スライスを返しパニックしない |
| U-2 | `peak_scale`：±1.5 の float WAV を読み、量子化後に元の振幅へ戻ることを 1e-3 で確認 |
| U-3 | `AnchorMap`：0 個 / 1 個 / 3 個での `file_seconds_at`。1 個のとき既存 `count_to_audio_time`（`audio.rs:86-88`）と完全一致 |
| U-4 | `PeakPyramid::resident_bytes` が PCM の 1.05 % 未満 |
| U-5 | `range` の決定論：同じ引数で 100 回呼んで同一結果 |
| U-6 | `FrameSegment` の整数進行：10^8 フレーム進めてもドリフト 0 |
| U-7 | `ClickSchedule::build_into` の時刻が `drill_core::audio::click_track` / `downbeats` と `engine_rate` の丸め以内で一致 |
| U-8 | `wrap_timeline` の境界：`end <= start`、ちょうど `end`、大きなオーバーシュート |
| U-9 | `ClickVoices::render` が末尾で厳密に 0.0、最大振幅が 1.0 以下 |
| U-10 | `MixSettings::from_track`：`muted` で `track_gain == 0.0`、`trim_end_seconds == 0.0` で末尾まで |
| U-11 | `TapCalibrator`：外れ値1件を含む 9 タップで中央値が正しい。8 タップ未満は `None` |
| U-12 | `AudioError` が `Copy` であり、生成経路にヒープ確保が無い |

### ゴールデン（G）

| ID | 内容 |
|---|---|
| G-1 | 合成した 10 秒のスイープ WAV に trim/fade/gain/click を適用した `mix_block` の出力を、リポジトリに置いた参照 WAV と**サンプル単位で**比較。プラットフォーム間で同一であること |
| G-2 | 生成済みの既知 PCM（矩形波・インパルス列）に対する `PeakPyramid::range` の出力を固定 |
| G-3 | 既知 BPM（100 / 120 / 144）で作った合成クリック列に対する `estimate_tempo` の第1候補が ±0.5 % 以内 |
| G-4 | `AnchorMap::to_tempo_changes` の結果が、意図した `TempoChange` 列と一致 |

### property（P）

| ID | 内容 |
|---|---|
| P-1 | 任意の `(count, dt, speed, range, looping, tempo)` に対し、`advance` の結果と `wrap_timeline` の結果が ±1 サンプル以内 |
| P-2 | 任意のアンカー集合とテンポマップに対し、`file_seconds_at` → `count_at` が恒等（1e-4） |
| P-3 | 任意の `(start, end, out_buckets)` に対し、`range` の各バケットが対応する生 PCM 区間の真の min/max を**包含**する（LOD は詳細を落とすが、外側へは出ない） |
| P-4 | 任意の `AudioTrack` に対し、`mix_block` の出力が常に `[-1.0, 1.0]` |
| P-5 | ランダムなバイト列を `decode_file` に食わせてもパニックしない（1 万ケース） |

### ファズ / 敵性入力（F）

| ID | 内容 |
|---|---|
| F-1 | `cargo-fuzz` で `decode_file` をファジング。コーパスは各対応形式のヘッダ断片。24 時間 |
| F-2 | 実ファイルの先頭・中間・末尾をランダムに破壊したもの 1 000 件。パニックゼロ、全件が値か `AudioError` |
| F-3 | 申告 duration が 3 秒・実体 2 時間の OGG を作り、`TooLong` で 30 分相当以内に停止すること |
| F-4 | 0 バイト、1 バイト、切り詰められたヘッダ、拡張子だけ音声のテキストファイル |
| F-5 | デバイス切断の注入（テスト用の `FakeHost` で `DeviceNotAvailable` を返す）。再構築と単調時計への縮退を確認 |

### リアルタイム安全性（R）

| ID | 内容 |
|---|---|
| R-1 | コールバック内で `#[global_allocator]` をフックした確保検出アロケータを有効にし、
1 分間の再生で確保回数が **0** であること。ロック取得も同様に検出（`parking_lot` の代わりに検出用ラッパを差す） |
| R-2 | ミキサスレッドについて同じ検査。暖機後の確保回数 0 |
| R-3 | UI スレッドで `symphonia` / `realfft` / `PeakPyramid::build` が呼ばれないことを、スレッドID を検査するデバッグアサートで担保 |
| R-4 | `Block` / `Command` / `EngineEvent` / `MixSettings` が `Copy` であることを型レベルで確認（`const _: fn() = || { fn assert_copy<T: Copy>() {} assert_copy::<Command>(); };`） |

### ストレス / ソーク（S）

| ID | 内容 |
|---|---|
| S-1 | 30 分・ステレオ 48 kHz を読み込み、メモリが 350 MB 前後に収まること |
| S-2 | **2 時間の連続再生**（10 分の音源をループ）。10 秒ごとに RSS を記録し、暖機後の増加が 2 MiB 未満 |
| S-3 | 1 分間に 600 回のランダムシーク。アンダーラン 0、位置が常に範囲内 |
| S-4 | 再生中にデバイスを 20 回抜き差し（`FakeHost`）。毎回再構築し、位置が 50 ms 以内の誤差で保たれる |
| S-5 | 再生しながら 10 分音源を 20 回差し替え。リークなし、クラッシュなし |
| S-6 | 4 000 人ドキュメント（上限規模）で波形表示しつつ再生し、フレーム時間が 16.6 ms を超えないこと |

### ベンチ（B、`criterion`）

| ID | 内容 | 目標 |
|---|---|---|
| B-1 | `decode_file`（10 分 MP3 / WAV / FLAC / OGG） | 6 s 以内 |
| B-2 | `PeakPyramid::range`（1920 px × 2 ch、各ズーム段） | 50 µs 以内 |
| B-3 | 出力コールバックの実行時間分布（実デバイスで 10 分計測、p99.9） | バッファ周期の 5 % 未満 |
| B-4 | `mix_block`（256 frames、リサンプル有/無） | リサンプル無で 5 µs 以内 |
| B-5 | `PeakPyramid::build`（10 分ステレオ） | 100 ms 以内 |
| B-6 | `estimate_tempo`（10 分） | 1 s 以内 |
| B-7 | `PlaybackClock::sample()` | 100 ns 以内 |

---

## 8. 実装タスク

1タスク = 1〜3 時間相当。`[依存]` は先行タスク。`∥` は並行可能。

### 第1波（並行可能、基盤）

| # | タスク | 依存 | 内容 |
|---|---|---|---|
| T1 | ワークスペースに `drill-audio` を追加 | — | `Cargo.toml:2` の `members` に追加。`[workspace.dependencies]` に cpal 0.18.1 / symphonia 0.6.0 / rubato 4.0.0 / rtrb 0.3.4 / realfft 3.5.0 を追加。空の `lib.rs` と `AudioError` |
| T2 | `cargo deny` のライセンス許可リスト | T1 | MPL-2.0 を含む許可リスト。CI ジョブ（50 と調整） |
| T3 ∥ | `asset.rs`：`AudioAsset` / `DecodeLimits` と U-1, U-2 | T1 | デコードなしで構築できるテスト用コンストラクタを含む |
| T4 ∥ | `peaks.rs`：`Peak` / `PeakPyramid::build` / `build_into` | T3 | U-4, G-2, B-5 |
| T5 ∥ | `peaks.rs`：`range` / `level_for` + 生 PCM 経路 | T4 | U-5, P-3, B-2 |
| T6 ∥ | `drill-core`：`SyncAnchor` / `AnchorMap` / `TempoMismatch` | — | `audio.rs` への追加のみ。U-3, P-2, G-4 |
| T7 ∥ | `ring.rs`：`Block` と rtrb ラッパ | T1 | R-4 |
| T8 ∥ | `clock.rs`：`ClockShared` / seqlock / `PlaybackClock` | T1 | U 相当のマルチスレッド読み書きテスト、B-7 |

### 第2波（デコードと解析）

| # | タスク | 依存 | 内容 |
|---|---|---|---|
| T9 | `decode.rs`：`ProgressSink` と WAV/FLAC 経路 | T3 | symphonia の probe → decode → i16 化。F-4 |
| T10 | `decode.rs`：MP3 / OGG / MKV と誤り回復 | T9 | `DecodeError` スキップ、`ResetRequired`、EOF |
| T11 | `decode.rs`：上限とキャンセル | T10 | F-3、進捗の間引き、`try_reserve` |
| T12 | デコードと L0 ピーク・解析デシメートの1パス統合 | T4, T11 | B-1 |
| T13 ∥ | `analysis.rs`：STFT とオンセット包絡 | T12 | realfft 導入、`OnsetEnvelope` |
| T14 ∥ | `analysis.rs`：`estimate_tempo`（自己相関＋事前分布） | T13 | G-3, B-6 |
| T15 ∥ | `analysis.rs`：`estimate_downbeats` / `detect_transients` / `snap_to_transient` | T14 | |
| T16 | `cargo-fuzz` ターゲットとコーパス | T11 | F-1, F-2, P-5 |

### 第3波（ミックスと再生）

| # | タスク | 依存 | 内容 |
|---|---|---|---|
| T17 | `drill-core`：`TempoMap::seconds_at_f64` / `count_at_f64`（10 の担当へ依頼） | — | §3.12-3 |
| T18 | `mix.rs`：`MixSettings` / `FrameMap::bake_into` / `segment_at` | T6, T17 | U-6, U-10 |
| T19 | `click.rs`：`ClickSettings` / `ClickVoices::render` / `ClickSchedule` | T17 | U-7, U-9 |
| T20 | `mix.rs`：`mix_block`（read / trim / fade / gain / click / clamp、リサンプル無） | T18, T19 | G-1, P-4, B-4 |
| T21 | `resample.rs`：`Resample` トレイトと rubato `Fft` / `Async` | T20 | 44.1→48 の帯域制限、可変速 |
| T22 | `device.rs`：`OutputStream::open`（形式選定、f32 / raw フォールバック） | T7, T8 | |
| T23 | 出力コールバック（リング drain、レイテンシ算出、seqlock 公開） | T22 | R-1, B-3 |
| T24 | `engine.rs`：ミキサスレッドとコマンド／更新チャネル | T20, T23 | R-2 |
| T25 | `engine.rs`：`Transport`（play / pause / seek / range / speed / tick） | T24 | |
| T26 | `wrap_timeline` とループ／範囲終端 | T25 | U-8, P-1 |
| T27 | カウントイン | T19, T26 | |
| T28 | `device.rs`：`DeviceSupervisor`（障害検出・再構築・縮退） | T23 | F-5, S-4 |
| T29 | 単調時計への縮退経路（`advance` 再利用） | T25, T28 | |

### 第4波（統合と較正）

| # | タスク | 依存 | 内容 |
|---|---|---|---|
| T30 | `calibrate.rs`：`LatencyCalibration` / `TapCalibrator` | T23 | U-11 |
| T31 | `drill-app`：`main.rs:488-509` の `advance` 直呼びを `Transport::tick` へ置換（43 と調整） | T25 | |
| T32 | `drill-app`：デコードジョブの起動と `AssetState`（40 / 41 と調整） | T11 | `duration_seconds` の手入力（`main.rs:1366-1373`）を廃止し実測値にする |
| T33 | `drill-app`：アンカー編集とテンポ不一致警告・「テンポマップへ反映」（10 の `Edit` として） | T6 | |
| T34 | FFmpeg 経由の AAC/Opus 変換とアセットキャッシュ（31 / 41 と調整） | T9 | |
| T35 | ソークとストレス | T31 | S-1〜S-6 |
| T36 | ベンチ一式 | T31 | B-1〜B-7 |
| T37 | サードパーティ通知（MPL-2.0 全文と入手先） | T2 | 53 と調整 |

**クリティカルパス**：T1 → T3 → T9 → T10 → T11 → T12 → T18 → T20 → T22 → T23 → T24 → T25 → T31。
第1波の T4〜T8、第2波の T13〜T15 は完全に並行できる。

---

## 9. 未決事項

| # | 事項 | 保留理由 | 決めるために必要な情報 |
|---|---|---|---|
| 1 | **ピッチ保持のタイムストレッチ** | 半速練習でピッチが下がることを利用者が許容するか不明。WSOLA / 位相ボコーダは実装量が大きく、本書では varispeed（テープ式）を採る | 指導者への聞き取り。「半速で音程が下がるのは困る」なら WSOLA を P2 に積む |
| 2 | **`rubato` 4.0 の安定性** | 2026-07 リリースで API（`audioadapter` 採用）が新しい。3.0 は 2026-05、2.0 は 2026-04 と改版が速い | 3 か月運用しての破壊的変更の有無。`mod resample` で隔離してあるので、必要なら 3.x か自前の多相 FIR へ退避できる |
| 3 | **`aac` feature を有効化するか** | Via LA の AAC プールはデコーダ実装の頒布に課金する。販売形態（買い切り／サブスク）と想定本数で費用が変わる | 法務判断と価格帯（53）。当面は FFmpeg 経由の変換で回避する |
| 4 | **`TempoMap` の `f64` 化の範囲** | `seconds_at_f64` / `count_at_f64` の追加だけで足りるか、内部を全部 `f64` にすべきか。後者はシリアライズ互換に影響する | 10 / 11 の担当との合意。10 分超のショーが現実的にどれだけあるか |
| 5 | **常駐上限 30 分の妥当性** | 345.6 MB。マーチングのショーは 6〜10 分だが、練習用に全曲通しの音源を入れる利用者がいるかもしれない | ベータでの実使用データ。超える場合は「先頭 30 分のみ読み込む」縮退か、ストリーミングの導入判断 |
| 6 | **`i16` 常駐の可聴影響** | 理論上は問題ないと判断したが、大音量の PA でのモニタリング時に量子化ノイズが気になるかは未検証 | ABX 聴取テスト。問題があれば `AudioSamples` を enum 化し、`f32` 常駐を上位設定で選べるようにする |
| 7 | **アンカーの吸着許容 ±60 ms** | 経験則。マーチングの拍精度（人間の同期誤差 20〜30 ms）から置いた | 実データでのトランジェント検出精度の実測 |
| 8 | **リング深度 32 ブロック（170 ms）** | Windows の負荷時ジッタを見積もった値。実測していない | 低スペック機での `Xrun` 発生率。B-3 と S-3 の実測後に調整 |
| 9 | **入力（マイク）を使った自動レイテンシ較正** | ループバック測定なら人間の反応時間が入らず正確だが、マイク権限とプライバシー（`00-conventions.md`）の扱いが増える | 手動タップ較正の実用精度。±10 ms を切れないなら検討する |
| 10 | **複数音源トラック** | 現状 `Document::audio` は `Option<AudioTrack>` 1本。曲間で音源を分ける需要があるか未確認 | 利用者への聞き取り。必要なら `Vec<AudioTrack>` へ拡張し、`mix_block` を N 入力にする（設計上は素直に伸びる） |

---

## 参照

- [symphonia crates.io](https://crates.io/crates/symphonia) — 0.6.0 / MPL-2.0 / 2026-05-15
- [symphonia v0.6.0 の Cargo.toml features](https://github.com/pdeljanov/Symphonia/blob/v0.6.0/symphonia/Cargo.toml)
- [symphonia v0.6.0 リリースノート](https://github.com/pdeljanov/Symphonia/releases/tag/v0.6.0)
- [symphonia_core::formats::FormatReader](https://docs.rs/symphonia-core/0.6.0/symphonia_core/formats/trait.FormatReader.html)
- [cpal crates.io](https://crates.io/crates/cpal) — 0.18.1 / Apache-2.0 / 2026-06-07
- [cpal DeviceTrait](https://docs.rs/cpal/0.18.1/cpal/traits/trait.DeviceTrait.html) / [ErrorKind](https://docs.rs/cpal/0.18.1/cpal/enum.ErrorKind.html)
- [rubato crates.io](https://crates.io/crates/rubato) — 4.0.0 / MIT OR Apache-2.0 / 2026-07-09
- [rtrb crates.io](https://crates.io/crates/rtrb) — 0.3.4 / MIT OR Apache-2.0 / 2026-04-26
- [realfft crates.io](https://crates.io/crates/realfft) — 3.5.0 / MIT / 2025-06-12
- [Fraunhofer IIS: mp3 ライセンスプログラム終了](https://www.iis.fraunhofer.de/en/ff/amm/lizenz/patent.html)
- [Via LA: AAC ライセンスプログラム](https://www.via-la.com/licensing-programs/aac/)

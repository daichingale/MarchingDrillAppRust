# 40. 非同期ジョブ基盤とスレッドモデル

## 1. 目的と範囲

現状、保存・書き出し・解析・割り当て最適化が全て UI スレッド同期で実行されており、演者
1,000人規模のドキュメントで操作するとフレームが数百ミリ秒〜数秒単位で止まる
（`00-conventions.md` の性能予算「入力から描画まで16.6ms」「UIスレッドでデコード・
フレーム描画・エンコード・全文書スキャンを行わない」に違反）。

この文書は、上記を解決する **汎用の非同期ジョブ基盤とスレッドモデル** を定義する。
具体的には次を定める。

- `Job<T>` という、進捗・キャンセル・結果受け渡しを持つ汎用型（ワーカースレッド↔UIスレッド間の唯一の橋）。
- UIスレッド／汎用ワーカー／音声出力／動画エンコードの4種のスレッドの責務と越えてはいけない境界。
- ジョブへ渡す `Document` のスナップショット戦略（clone vs Arc共有 vs 差分）と、実測に基づく判断。
- ジョブの一覧管理（同時実行数上限・重複起動防止・優先度）とアプリ終了時の畳み方。

この文書が **扱わないもの**（他の設計文書の担当）:

- 保存ファイル形式・原子的置換の具体的なファイルレイアウト（`41-persistence.md`）。
- `scan_transition` のアルゴリズム自体や空間ハッシュの中身（`13-collision.md`、`DESIGN_GAPS.md` A-4）。
- FFmpeg 引数生成やフレームのラスタライズ方法（`31-video-export.md`、`DESIGN_GAPS.md` B-1/B-4）。
- symphonia/cpal のデコード・再生アルゴリズム自体（`30-audio-engine.md`、`DESIGN_GAPS.md` B-2）。
- 進捗パネル・キャンセルボタンの実際の egui ウィジェット実装（`43-app-architecture.md`）。ここでは
  「43 に渡す仕様」として要件のみ書く。
- `DrillError`／`Locale` の設計そのもの（`42-error-i18n.md`、`DESIGN_GAPS.md` A-6）。本書はその
  型が未着手である前提で、境界だけを決める。

## 2. 現状

すべて `crates/drill-app/src/main.rs` の `DrillApp`（god struct、69〜98行目）に同期実装されている。

| 処理 | 場所 | 問題 |
|---|---|---|
| 保存 | `save_to`（main.rs:236-247）。呼び出し元は main.rs:424。 | `document.to_json()`（シリアライズ）と `std::fs::write` が UI スレッドで直列実行。バックアップコピーも同様。 |
| 自動保存 | `eframe::App::ui` 内、main.rs:477-487。30秒ごとに `dirty` かつ経過時間で判定し、その場で `to_json()` と `std::fs::write` を呼ぶ。 | 保存と全く同じ重さの処理が、ユーザー操作と無関係なタイミングで UI スレッドに割り込む。 |
| テキスト系書き出し（CSV/SVG/HTML/カウントシート/ドリルブック） | `export_text`（main.rs:249-260）。呼び出し元は main.rs:1287, 1301, 1308, 1312, 1316, 1323, 1339。 | `rfd::FileDialog` はブロッキング呼び出し（現状は許容）、直後の `std::fs::write` も同期。文字列生成自体（`svg::export` 等）も同期でここに含まれる。 |
| 割り当て最適化 | `auto_assign_next`（main.rs:264-289）が `pathing::optimal_assignment`（pathing.rs:126-175）を呼ぶ。呼び出し元は main.rs:1283。 | `optimal_assignment` は貪欲法 O(n²) の後、最大 `4n+8` 回の 2-opt パス（各パス O(n²)）を回す。最悪計算量は O(n³) 相当で、n=1,000 では UI スレッドで数百ミリ秒〜秒単位に達し得る。 |
| 衝突解析 | `analyze_transition`（lib.rs:186-223）が毎フレーム呼ばれる（main.rs:1011, 1033）。 | `DESIGN_GAPS.md` の既知バグ3・4。O(n²) を毎フレーム無条件実行し、`transition_moves` が毎フレーム `Vec` を確保している。**このドキュメントの対象だが、置き換え先の `scan_transition`（A-4）自体はまだ実装されていない。** 本書は「実装されたらどうジョブ化するか」の受け皿だけを用意する。 |
| 音声デコード | `crates/drill-core/src/audio.rs` は同期モデルのみで実際のデコードコードが無い（`DESIGN_GAPS.md` B-2）。 | ジョブ化する対象コードがまだ存在しない。本書はインターフェースの受け皿のみ用意する。 |
| 動画書き出し | `crates/drill-core/src/video.rs` は設定検証と FFmpeg 引数生成のみ（`ffmpeg_args`, video.rs:157-215）。実際にフレームをパイプへ流すループが無い（`DESIGN_GAPS.md` B-4）。 | 同上。 |

非同期実行の仕組みは現状ゼロ。`std::thread` / `std::sync::mpsc` の使用箇所は無く、
`Cargo.toml`（ワークスペース: `Cargo.toml:1-18`）にも `rayon` や `tokio` 等の非同期ランタイムは
一切入っていない。`drill-core/Cargo.toml` の依存は `serde`/`serde_json` のみ（`crates/drill-core/Cargo.toml:7-9`）。

`Document`（lib.rs:240-252）は既に `#[derive(Clone, ...)]` 済みであり、スナップショットに使える。
`History`（lib.rs:414-463）は `Vec<MoveCommand>` ベースで `SetId` も `Edit` enum もまだ無い
（`DESIGN_GAPS.md` A-1 が未着手）。本書のジョブ機構はこの後の `Edit` 移行と無関係に成立するように書く
（ジョブが受け取るのは「ある時点の `Document` の値」であり、それがどう変更コマンドで作られたかを問わない）。

`AudioTrack`（audio.rs:24-45）はファイルパスと数値メタデータのみを持ち、デコード済み PCM を保持しない。
`VideoExportConfig`（video.rs:44-57）も同様に軽量な設定値のみ。したがって `Document` 自体のクローンは
音声・動画データを一切含まず、後述のとおり軽量である。

`eframe::App for DrillApp` の `fn ui(&mut self, ui: &mut egui::Ui, _: &mut eframe::Frame)`
（main.rs:460-461）が唯一の毎フレームエントリポイントで、`egui::Context` は `ui.ctx()` で取得している
（既存の使用例: main.rs:508, 552, 608, 685）。`toggle_playback`（main.rs:145-160）が
`context.request_repaint_after(Duration::from_millis(16))` を明示的に呼んでいる事実から、
このアプリは **常時再描画ではなく反応的（reactive）描画** で動いていると分かる
（そうでなければ再生ループのためにわざわざ repaint 要求を書く理由がない）。これは本書の
進捗通知設計（3.5節）に直接影響する: ジョブが進んでも、ワーカー側から明示的に repaint を要求しない限り、
ユーザーが何も操作していない間は進捗バーもジョブ完了も画面に反映されない。

## 3. 設計

### 3.0 新規クレート `drill-jobs` の追加

`Job<T>` はドメイン（`drill-core`）にも UI（`drill-app`）にも依存しない、純粋なスレッド・同期プリミティブである。
これを既存クレートのどれかに埋め込むと、依存方向が崩れる（例: `drill-core` に置くと
「依存は serde/serde_json のみ」という `00-conventions.md` の明文規則に違反する。`drill-app` に置くと
`drill-audio`/`drill-export` が UI クレートに依存することになり、依存が下から上へ逆流する）。

よって新規クレート `drill-jobs` を提案する。

```
drill-jobs    汎用非同期ジョブ基盤（Job<T>・進捗・キャンセル・スレッド起動）。
              標準ライブラリのみに依存。ドメインモデルもUI/OS資源（GPU・音声デバイス・ファイルダイアログ）も知らない。
drill-core    （既存のまま）依存は serde/serde_json のみ。drill-jobs にも依存しない。
drill-render  （既存のまま）drill-core にのみ依存。
drill-audio   （既存のまま）symphonia/cpal + drill-core。デコードジョブの実行に drill-jobs を追加依存。
drill-export  （既存のまま）drill-render + drill-jobs（動画書き出しジョブの実行に使う）。
drill-app     drill-core・drill-render・drill-audio・drill-export・drill-jobs に依存。
```

`drill-core` を意図的に `drill-jobs` に依存させない。理由: `drill-core` の関数（`pathing::optimal_assignment`,
`svg::export` 等）は同期・純粋のまま保つ。「バックグラウンドスレッドで動かす」という判断は呼び出し側
（`drill-app` のジョブ本体クロージャ）の責務であり、ロジック自体が非同期を意識する必要はない。
これにより `drill-core` の単体テストは今まで通りスレッドを意識せずに書ける。

`Cargo.toml`（ワークスペース）には `members` に `"crates/drill-jobs"` を追加し、
`crates/drill-jobs/Cargo.toml` の `[dependencies]` は空（標準ライブラリのみ）にする。

### 3.1 `Job<T>` の型定義

```rust
// crates/drill-jobs/src/lib.rs
use std::panic::{self, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::Arc;
use std::thread::JoinHandle;

/// 進捗の最大値。`progress() -> u32..=PROGRESS_MAX` の分数として扱う。
pub const PROGRESS_MAX: u32 = 10_000;

/// ワーカースレッドがジョブ本体の中で進捗更新とキャンセル確認に使うハンドル。
/// `Clone` で、`Job` 自身より軽量にクロージャへキャプチャできる。
#[derive(Clone)]
pub struct ProgressHandle {
    value: Arc<AtomicU32>,
    cancel: Arc<AtomicBool>,
}

impl ProgressHandle {
    /// `fraction` を 0.0..=1.0 にクランプして進捗値として記録する。
    pub fn set(&self, fraction: f32) {
        let clamped = (fraction.clamp(0.0, 1.0) * PROGRESS_MAX as f32).round() as u32;
        self.value.store(clamped.min(PROGRESS_MAX), Ordering::Relaxed);
    }

    /// キャンセル要求を確認する。ジョブ本体はループの節目ごとにこれを呼ぶ（3.4節）。
    pub fn is_cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }
}

/// ジョブ終了時にチャンネルへ流れる終端メッセージ。
///
/// 注意: `DESIGN_GAPS.md` の初期スケッチにあった `JobMsg::Progress(u32)` はここでは採用しない。
/// 進捗の正は常に `Job::progress()`（ロックフリーな `AtomicU32` の読み取り）であり、
/// チャンネル越しに進捗を送るのは無駄なアロケーションと頻度制御の二重管理を生む
/// （9節・未決事項で再考の余地として記録する）。
pub enum JobMsg<T> {
    Done(T),
    Failed(String),
    Cancelled,
}

/// ジョブの種類。`JobManager`（3.6節）の重複起動防止・同時実行数制御の鍵として使う。
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum JobKind {
    Save,
    AutoSave,
    ExportText,
    ExportVideo,
    CollisionScan,
    AssignmentOptimize,
    AudioDecode,
}

impl JobKind {
    fn thread_name(self) -> &'static str {
        match self {
            JobKind::Save => "drill-job-save",
            JobKind::AutoSave => "drill-job-autosave",
            JobKind::ExportText => "drill-job-export-text",
            JobKind::ExportVideo => "drill-job-export-video",
            JobKind::CollisionScan => "drill-job-scan",
            JobKind::AssignmentOptimize => "drill-job-assign",
            JobKind::AudioDecode => "drill-job-audio-decode",
        }
    }
}

/// 実行中（または完了直後の未回収）のバックグラウンド作業を表す。
///
/// `T` はワーカースレッドから戻る成功値の型（例: `Result<(), String>` は使わず、
/// 成功は必ず `T`、失敗は必ず `JobMsg::Failed` に寄せる。呼び出し側が
/// `Job<Result<(), String>>` のように二重にエラー型を持たないよう徹底する）。
pub struct Job<T> {
    kind: JobKind,
    progress: Arc<AtomicU32>,
    cancel: Arc<AtomicBool>,
    rx: Receiver<JobMsg<T>>,
    finished: bool,
    // ジョブが終わったら join できるように保持する。UIスレッドはこれを
    // 明示的な `join()`（アプリ終了時のみ・4節）以外では待たない。
    handle: Option<JoinHandle<()>>,
}

impl<T: Send + 'static> Job<T> {
    /// ジョブを新しい OS スレッドで起動する。`body` は成功時に `Ok(T)`、
    /// 失敗時に **すでに人間可読な** `Err(String)` を返すこと
    /// （`DrillError` を持つ呼び出し側は `error.message(locale)` 等で文字列化してから渡す。
    /// `drill-jobs` は `drill-core`/`Locale` を知らないため、ここでは文字列を受け取るしかない。
    /// これは `00-conventions.md` の「`Result<_, String>` を新規に作らない」を破るものではない:
    /// 対象はドメインAPI（`drill-core`の公開関数）であり、`drill-jobs`はUI/OS境界の
    /// 汎用配線であって、ドメインの公開APIを新設しているわけではない）。
    pub fn spawn<F>(kind: JobKind, body: F) -> Self
    where
        F: FnOnce(&ProgressHandle) -> Result<T, String> + Send + 'static,
    {
        let progress = Arc::new(AtomicU32::new(0));
        let cancel = Arc::new(AtomicBool::new(false));
        let (tx, rx) = mpsc::channel();
        let handle_for_body = ProgressHandle {
            value: Arc::clone(&progress),
            cancel: Arc::clone(&cancel),
        };
        let handle = std::thread::Builder::new()
            .name(kind.thread_name().into())
            .spawn(move || {
                let outcome = panic::catch_unwind(AssertUnwindSafe(|| body(&handle_for_body)));
                let msg = match outcome {
                    Ok(Ok(value)) => JobMsg::Done(value),
                    Ok(Err(reason)) => JobMsg::Failed(reason),
                    // パニックの詳細はワーカースレッドの標準パニックフックが標準エラーへ
                    // 出す（`main.rs` 側で `std::panic::set_hook` を差し替えても、
                    // catch_unwind はフック自体は呼んだ後に unwind するので出力は残る）。
                    // ここでは文言を汎用化する。ローカライズは呼び出し側で行う。
                    Err(_) => JobMsg::Failed("internal-panic".into()),
                };
                // 受信側 (Job) が既に drop されていれば send は失敗するが、
                // 結果を捨てるだけでよいので無視する。
                let _ = tx.send(msg);
            })
            .expect("failed to spawn job thread");
        Self {
            kind,
            progress,
            cancel,
            rx,
            finished: false,
            handle: Some(handle),
        }
    }

    pub fn kind(&self) -> JobKind {
        self.kind
    }

    /// 0.0..=1.0 の進捗率。ロックフリーな読み取りで、毎フレーム呼んでよい。
    pub fn progress(&self) -> f32 {
        self.progress.load(Ordering::Relaxed) as f32 / PROGRESS_MAX as f32
    }

    /// キャンセルを要求する。冪等（何度呼んでもよい）。
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    /// ノンブロッキングでメッセージを取得する。UIスレッドは毎フレームこれを呼ぶ。
    /// 終端メッセージ（`Done`/`Failed`/`Cancelled`）を一度返した後は常に `None`。
    pub fn poll(&mut self) -> Option<JobMsg<T>> {
        if self.finished {
            return None;
        }
        match self.rx.try_recv() {
            Ok(msg) => {
                self.finished = true;
                Some(msg)
            }
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => {
                // 送信側スレッドが send する前に消えた（あり得るのは
                // thread::Builder::spawn 自体が失敗するケースのみで、
                // それは呼び出し元で expect により panic するため通常到達しない。
                // 防御的にエラー化しておく。
                self.finished = true;
                Some(JobMsg::Failed("worker-disconnected".into()))
            }
        }
    }

    pub fn is_finished(&self) -> bool {
        self.finished
    }
}

impl<T> Drop for Job<T> {
    fn drop(&mut self) {
        // キャンセルを要求してスレッドをデタッチする。ここで join しない
        // （UIスレッドをブロックしないため）。ワーカーは自分のペースで終了し、
        // 受信側が消えているので最終 send は黙って失敗するだけで安全。
        // アプリ終了時の明示的な join は JobManager::shutdown（4節）が別途行う。
        self.cancel.store(true, Ordering::Relaxed);
    }
}
```

`T` に `Send + 'static` を要求するだけで `Clone` は要求しない。結果は一度きり `poll()` から
ムーブで取り出す設計であり、コピーは発生しない（5節で定量化する）。

### 3.2 スレッドモデル全体図

```
┌────────────────────────────────────────────────────────────────────┐
│ UIスレッド（eframe/egui のイベントループ、DrillApp::ui が毎フレーム走る唯一の場所）│
│  - Document・History・Selection・カメラなど全ての描画対象状態を所有        │
│  - JobManager::poll_all() を毎フレーム呼び、終端メッセージだけを消費        │
│  - Job<T> へは cancel() のみ発行できる（結果を待たない・ロックしない）       │
│  越えてはいけない境界: symphonia デコード、FFmpegパイプ書き込み、           │
│  DisplayList→RGBAラスタライズ、全文書スキャン、JSON直列化+ファイル書き込み  │
└───────────────┬──────────────────────────────────────────────────────┘
                │ Job::spawn（std::thread::spawn、都度起動・使い捨て）
                ▼
┌────────────────────────────────────────────────────────────────────┐
│ 汎用ワーカー（常駐プールを持たない。ジョブ起動ごとに1スレッド、JobManagerが  │
│ 同時実行数を admission control で制限する。理由は3.3節）                  │
│  対象: Save / AutoSave / ExportText / CollisionScan / AssignmentOptimize │
│  責務: Document の**独立クローン**を受け取り、drill-core/drill-export の  │
│  純粋関数を呼んで結果を作り、ProgressHandle を更新し、Result を返すだけ。   │
│  越えてはいけない境界: 元の Document・History・Selection への書き戻し     │
│  （UIスレッドが poll() で受け取った結果を自分で適用する。ワーカーは       │
│  DrillApp を一切知らない）                                              │
└────────────────────────────────────────────────────────────────────┘

┌────────────────────────────────────────────────────────────────────┐
│ 動画エンコードスレッド（専用スレッド。汎用ワーカーの枠を消費しない）        │
│  責務: DisplayList→RGBAラスタライズをフレームごとに回し、FFmpeg子プロセスの │
│  stdin へ書き込み、`-progress pipe:1` の出力を読んで進捗を計算する。       │
│  外部プロセス（FFmpeg）を所有する唯一のスレッド。                         │
│  越えてはいけない境界: Document への書き戻し。UIスレッドとのやり取りは     │
│  Job<PathBuf>（完成ファイルパス）1本のみ。                               │
│  汎用ワーカーに入れない理由: FFmpegプロセスの生存期間はエクスポート全体に   │
│  及び（数秒〜数分）、汎用プールの短寿命ジョブと同居させると枠を長時間      │
│  専有してしまう。同時実行数は常に1（3.6節）。                            │
└────────────────────────────────────────────────────────────────────┘

┌────────────────────────────────────────────────────────────────────┐
│ 音声出力スレッド（リアルタイム優先・cpal がOS/ドライバ層で管理する         │
│ コールバックコンテキスト。drill-jobs が spawn するスレッドではない）        │
│  責務: 事前デコード済み PCM リングバッファから読み出し、出力デバイスへ渡す  │
│  だけ。`00-conventions.md` の「フレーム内ヒープ確保ゼロ」を最も厳格に      │
│  適用する場所: 確保・ロック・log を一切行わない（cpal コールバック内）。   │
│  越えてはいけない境界: デコード自体（それは別途 AudioDecode ジョブが       │
│  汎用ワーカーで行い、結果のPCMをこのリングバッファへ引き渡すのみ）。       │
│  詳細は 30-audio-engine.md の担当。本書はこのスレッドを「ジョブとして      │
│  起動しない特別枠」として境界だけ確定する。                              │
└────────────────────────────────────────────────────────────────────┘
```

### 3.3 rayon か自前実装か

`rayon` は導入しない。検討した3案:

1. **rayon のグローバルプール**（`par_iter` 等）。棄却。rayon はデータ並列のフォーク・ジョイン
   （1つの計算をコア数に分割し、全部終わるまで待つ）に最適化されており、
   「長時間実行・キャンセル可能・進捗を都度報告・結果は後で好きなタイミングで回収」という
   本書のジョブの形（ワーカー間で完全に独立し、UIスレッドは待たない）とは設計思想が異なる。
   さらに、動画エンコードスレッドや将来の音声出力スレッドのようにOSスレッド優先度や
   専有プロセス（FFmpeg子プロセス）を持たせたいケースに rayon は向かない
   （rayon のワーカーはプール共有が前提で、1本を掴んで居座る使い方は想定されていない）。
   加えて `00-conventions.md` 不変条件5「同じ入力からは常に同じ出力」があるため、
   衝突走査や割り当て最適化のようなアルゴリズム **内部** を rayon で並列化することも避ける
   （浮動小数の縮約順序が並列度やスレッドスケジューリングに依存すると、
   実行のたびに最終ビットが変わりうる。並列化するとしても決定的な reduction 順序を
   保証する追加設計が要り、それは 13-collision.md 側の判断に委ねる）。
2. **自前の常駐スレッドプール**（固定 N 本のワーカースレッドが `Arc<Mutex<mpsc::Receiver<Box<dyn FnOnce()>>>>`
   のようなキューからタスクを取り出す、いわゆる Rust Book 式のプール）。棄却（今は不要）。
   本書が扱うジョブはユーザー操作またはタイマーで**まれに**（多くて同時数個）発生する重量級の
   作業であり、`std::thread::spawn` のコスト（Windows で数十マイクロ秒程度）はジョブ自体の
   実行時間（数ミリ秒〜数分）に対して無視できる。常駐プールを導入すると、アイドル時にも
   スレッドとキュー用ロックを保持し続ける複雑さが増えるだけで、頻度的な見返りが無い。
3. **ジョブ起動ごとに使い捨てスレッドを立て、`JobManager` が同時実行数を admission control で
   制限する**（3.6節）。採用。ロックが一切登場せず（3.6節の `JobManager` はUIスレッドからしか
   触らない単純な `Vec`）、キャンセル・進捗・結果の受け渡しはジョブ1本につき
   `Arc<AtomicU32>` 1個・`Arc<AtomicBool>` 1個・`mpsc::channel` 1組で完結する。

将来ジョブの発生頻度が大きく増える（例: 1秒間に何十回もの微小ジョブを投げるようになる）場合は
案2への切り替えを検討する。9節に判断基準を記録する。

### 3.4 スナップショット戦略

`Job` へ渡すデータは **`Document` の値としての独立クローン**（`Document: Clone`、lib.rs:240 の
`#[derive(Clone, ...)]` を利用）を採用する。Arc共有・差分のいずれも採らない。

**サイズ試算**（`Point` は `f32`×2＝8B、`Vec` ハンドルは 24B、64bit環境）:

基準規模: 演者1,000人・セット64。

| 内訳 | 計算 | 概算 |
|---|---|---|
| `Set.positions`（`Vec<Point>`） | 64セット × 1,000人 × 8B | 512,000 B |
| `Set` 構造体本体＋`name`ヒープ | 64 × (56B スタック + 約10Bヒープ) | 約 4,200 B |
| `Performer`（id+label+color） | 1,000 × (約40Bスタック + 約5Bヒープ) | 約 45,000 B |
| `GridConfig`/`TempoMap`/`Option<AudioTrack>` | ハッシュ2本＋テンポ区間数個＋パス文字列 | 1,000 B未満 |
| **合計** | | **約 560 KB** |

上限規模（`00-conventions.md`「劣化してよいが壊れてはいけない」）: 演者4,000人・セット256 では
`Set.positions` だけで 256 × 4,000 × 8B ＝ 8,192,000 B ≈ 8.0 MB、総計は **約 8.3 MB**。

**クローン時間**: バイトコピー自体は 8.3MB でも現代の CPU で 1ms 未満（メモリ帯域 10GB/s 換算で
0.8ms）。支配的なのはヒープ確保回数で、上限規模では `Set`×256×2（name+positions）＋`Performer`×4,000
＝ 約4,500 回のアロケーション。1回あたり50〜100ns として **合計 0.5ms 未満**。
基準規模ではその1/8以下（1,100回強、0.1ms程度）。

**判断**: クローンは **ジョブ起動の瞬間に1回だけ、UIスレッドで同期的に**行う（毎フレームではない）。
基準規模で概算0.2ms、上限規模でも概算1.3ms 程度であり、1フレーム16.6msの予算の中で
ユーザーが「保存」や「書き出し」ボタンを押した、その1フレームだけに収まる（体感カクつきなし）。
これより高機能な戦略は現時点では正当化できない:

- **Arc共有**（`Document` 自体を `Arc` にする、または `Set`/`Vec` を永続データ構造にする）は、
  編集中の書き換えとジョブ側の読み取りを安全に両立するために `im` 等の追加依存と
  `Document` 内部表現の再設計が要る。上限規模でもクローンが1.3msに収まる現状ではその複雑さに
  見合わない。将来、演者数が万単位に伸びる、またはジョブの起動頻度が跳ね上がる場合に再検討する
  （9節）。
- **差分（dirty setsのみクローン）**は、実装が複雑になる割に、削減できるのは上限規模でも
  高々数ミリ秒の話であり、かつ「ジョブはある時点の Document 全体を決定論的に見る」という
  単純な不変条件（4節）を壊す。棄却。

**一貫性（書き出し中の編集）**: クローンは値であり、Rust の所有権によって
ジョブ側とUIスレッド側の `Document` は以後**別オブジェクト**になる（型システムが保証する。
規約や注意書きではない）。つまりジョブ実行中にユーザーが編集を続けても、実行中のジョブの
出力には一切影響しない。これは `00-conventions.md` 不変条件5「同じ (Document, config, count) からは
常に同じ出力が出る」を素直に満たす。ただし UI 上は「エクスポートは開始時点のドリルを書き出した」
ことを利用者に伝える必要がある（3.8節のUI要件）。

**Save/AutoSave 固有の注意（dirty フラグとの整合）**: クローンを取った**後**にユーザーが編集すると、
ジョブ完了時に無条件で `dirty = false` にしてはいけない（その編集が保存されていないのに
「保存済み」と表示してしまう）。これには単調増加する `document_revision: u64`
（`DESIGN_GAPS.md` A-4 が衝突走査のキャッシュ判定のために既に必要としている値と同じもの。
本書はこの値の**所有権・更新箇所の設計はしない**が、値の存在を前提にする）が要る。
ジョブ起動時に `let revision_at_launch = self.document_revision;` を記録し、
完了時に `if self.document_revision == revision_at_launch { self.dirty = false; }` とする
（変わっていれば `dirty` はそのまま `true` を維持し、次の自動保存サイクルに委ねる）。

### 3.5 キャンセルの粒度

キャンセル確認（`ProgressHandle::is_cancelled()`）自体は `Ordering::Relaxed` のアトミック読み取りで
1回あたり1ns未満であり、頻度を気にする必要はない。気にすべきは「確認しなさすぎて反応が遅い」方だけ。
目標は「キャンセルボタンを押してから概ね100ms以内に停止する」。ジョブ種別ごとの確認点:

| ジョブ | 確認点 | 根拠 |
|---|---|---|
| `ExportVideo` | 1フレームエンコードするごと（フレームあたり数ms〜十数ms） | 60fpsなら1フレーム16.7ms、余裕で100ms以内 |
| `CollisionScan`（`scan_transition`、A-4実装後） | カウントサンプル1個処理するごと | 基準規模で単発2ms未満（A-4の受け入れ基準）× サンプル数 |
| `AssignmentOptimize`（`optimal_assignment`、pathing.rs:126） | 2-optの外側パス1回終えるごと（`max_passes = 4n+8` 回のうちの1回） | n=1,000で1パスはO(n²)=100万回の距離計算、数ms程度と見積もり、1パス単位の確認で十分 |
| `AudioDecode` | symphoniaのパケット1個デコードするごと | 1パケットは数十msの音声に相当、デコード自体はそれよりずっと速い |
| `Save`／`ExportText` | 開始前と、シリアライズ完了後・ファイル書き込み前の1回のみ | `serde_json::to_string_pretty` や文字列生成自体は分割不能な単発呼び出し。基準規模で数ms〜十数ms程度と見積もられ（既存ベンチ `crates/drill-core/benches/core_performance.rs` の「100回JSON直列化」測定を参照）、途中キャンセルできなくても体感に影響しない |

**キャンセル後の後始末**:

- **Save/AutoSave**: 一時ファイル（`<path>.tmp`）へ書いてから `rename` する方式
  （`00-conventions.md`「上書きは原子的置換」）を徹底する。キャンセルされた場合、
  `rename` を呼ばずに `.tmp` を削除して終わる。**元ファイルには一切触れない。**
- **ExportText**: 出力は1回の `std::fs::write` で完結する単発ファイルであり、
  書き込みの途中でプロセスを止める手段が無い（`write_all` は分割不能）。
  キャンセルは「書き込み開始前」でのみ有効。書き込みが始まった後のキャンセルは、
  完了を待ってから成功扱いにする（`JobMsg::Done` を返す。キャンセルは無視されたことを
  ステータスに出す）か、書き込み後に生成物を削除するかのどちらか。**削除を選ぶ**
  （ユーザーの「やめたい」という意図を尊重する方を優先する）。
- **ExportVideo**: FFmpeg 子プロセスの `stdin` を `drop` して EOF を送り、最大2秒待って
  プロセス終了を確認、超えたら `Child::kill()` する。その後、出力ファイル（部分的にしか
  書かれていない）を削除する。詳細な待ち時間やリトライは `31-video-export.md` の担当だが、
  「キャンセル時は必ず部分ファイルを削除する」という契約はここで確定する。
- **AudioDecode**: デコード先バッファを破棄するだけ（ファイルI/Oが無いので後始末は不要）。

### 3.6 進捗報告と repaint

2節で確認した通り、このアプリは反応的描画である。ジョブ実行中は、ワーカー側から能動的に
`egui::Context::request_repaint_after` を呼ばない限り、進捗バーは更新されず、
完了しても次のユーザー操作までUIに反映されない。

`drill-jobs::Job<T>` 自体は `egui` に依存しない（3.0節の境界）。したがって repaint 要求は
**呼び出し側（`drill-app`）がジョブ本体クロージャの中で行う**。呼び出し側は `egui::Context`
（`Clone + Send + Sync`）をクロージャへキャプチャし、次の頻度で呼ぶ:

- 進捗を更新するたび、ただし **最大10Hz（100ms間隔）にスロットルする**
  （`ProgressHandle` を薄くラップし、直近の呼び出しからの経過を `Instant` で見てから
  `ctx.request_repaint_after(Duration::from_millis(100))` を呼ぶ。`request_repaint_after` は
  同一フレーム内に複数回呼んでも1回に丸められるため、10Hzを超えて呼んでも実害はないが、
  無駄な仮想関数呼び出しを避けるためワーカー側でも間引く）。
- ジョブ終了時（`Done`/`Failed`/`Cancelled` を送る直前）は必ず即座に
  `ctx.request_repaint()`（遅延なし）を呼ぶ。完了表示に100ms待たせない。

`JobManager::poll_all`（3.7節）はUIスレッド側から毎フレーム呼ばれるので、そちら側での
追加の repaint 要求は不要（既に描画中のフレームの中で結果を消費できる）。

### 3.7 ジョブの一覧管理

`JobManager` は `drill-app` 側に置く（ワーカーの型を知らなくてよいよう `dyn` 化する）。

```rust
// crates/drill-app/src/jobs.rs（DESIGN_GAPS.md C-1 の最終形。
// C-1未着手の間は同じ内容を main.rs 内の1モジュールとして置いてよい）
use drill_jobs::{Job, JobKind, JobMsg};
use std::any::Any;
use std::time::Instant;

/// 型消去された終端結果。呼び出し側は `kind()` を見て `downcast` する。
pub enum JobOutcome {
    Done(Box<dyn Any + Send>),
    Failed(String),
    Cancelled,
}

trait AnyJob {
    fn kind(&self) -> JobKind;
    fn label(&self) -> &str;
    fn progress(&self) -> f32;
    fn started_at(&self) -> Instant;
    fn cancel(&self);
    /// 終端メッセージを一度だけ `Some` で返す。
    fn poll(&mut self) -> Option<JobOutcome>;
}

struct TypedJob<T> {
    job: Job<T>,
    label: String,
    started_at: Instant,
}

impl<T: Send + 'static> AnyJob for TypedJob<T> {
    fn kind(&self) -> JobKind {
        self.job.kind()
    }
    fn label(&self) -> &str {
        &self.label
    }
    fn progress(&self) -> f32 {
        self.job.progress()
    }
    fn started_at(&self) -> Instant {
        self.started_at
    }
    fn cancel(&self) {
        self.job.cancel()
    }
    fn poll(&mut self) -> Option<JobOutcome> {
        match self.job.poll()? {
            JobMsg::Done(value) => Some(JobOutcome::Done(Box::new(value))),
            JobMsg::Failed(reason) => Some(JobOutcome::Failed(reason)),
            JobMsg::Cancelled => Some(JobOutcome::Cancelled),
        }
    }
}

/// 種別ごとの同時実行上限。値は初期見積もりであり、9節で再検討する。
fn max_concurrent(kind: JobKind) -> usize {
    match kind {
        JobKind::Save | JobKind::AutoSave => 1, // 保存系はどちらか1本のみ
        JobKind::ExportVideo => 1,              // FFmpeg子プロセスを専有するため
        JobKind::AudioDecode => 2,               // 再生用+ピーク生成用の同時デコードを許す
        JobKind::ExportText | JobKind::CollisionScan | JobKind::AssignmentOptimize => 2,
    }
}

#[derive(Debug)]
pub enum JobRejected {
    AlreadyRunning(JobKind),
    AtCapacity(JobKind),
}

pub struct JobManager {
    active: Vec<Box<dyn AnyJob>>,
}

impl JobManager {
    pub fn new() -> Self {
        Self { active: Vec::new() }
    }

    /// `kind` の実行可否を確認する。`Save`/`AutoSave` は互いに排他
    /// （どちらかが動いていればもう一方は起動させない: 「自動保存が二重に走らない」の実現）。
    pub fn can_start(&self, kind: JobKind) -> Result<(), JobRejected> {
        if matches!(kind, JobKind::Save | JobKind::AutoSave)
            && self
                .active
                .iter()
                .any(|j| matches!(j.kind(), JobKind::Save | JobKind::AutoSave))
        {
            return Err(JobRejected::AlreadyRunning(kind));
        }
        let running = self.active.iter().filter(|j| j.kind() == kind).count();
        if running >= max_concurrent(kind) {
            return Err(JobRejected::AtCapacity(kind));
        }
        Ok(())
    }

    /// `body` を起動して管理下に置く。`can_start` の確認込み。
    pub fn start<T, F>(&mut self, kind: JobKind, label: String, body: F) -> Result<(), JobRejected>
    where
        T: Send + 'static,
        F: FnOnce(&drill_jobs::ProgressHandle) -> Result<T, String> + Send + 'static,
    {
        self.can_start(kind)?;
        let job = Job::spawn(kind, body);
        self.active.push(Box::new(TypedJob {
            job,
            label,
            started_at: Instant::now(),
        }));
        Ok(())
    }

    /// 毎フレーム呼ぶ。終端に達したジョブを取り除きながら結果を返す。
    /// 呼び出し側（`DrillApp::ui`）は返り値を `match (kind, outcome)` して
    /// 自分の状態（`self.document`, `self.dirty`, `self.status` 等）に適用する。
    pub fn poll_all(&mut self) -> Vec<(JobKind, JobOutcome)> {
        let mut finished_indices = Vec::new();
        let mut results = Vec::new();
        for (index, job) in self.active.iter_mut().enumerate() {
            if let Some(outcome) = job.poll() {
                results.push((job.kind(), outcome));
                finished_indices.push(index);
            }
        }
        for &index in finished_indices.iter().rev() {
            self.active.remove(index);
        }
        results
    }

    /// UI表示用のスナップショット（進捗バー・ラベル）。
    pub fn active_summaries(&self) -> Vec<(JobKind, &str, f32)> {
        self.active
            .iter()
            .map(|j| (j.kind(), j.label(), j.progress()))
            .collect()
    }

    pub fn cancel_all_of(&self, kind: JobKind) {
        for job in &self.active {
            if job.kind() == kind {
                job.cancel();
            }
        }
    }

    pub fn has_active(&self, kind: JobKind) -> bool {
        self.active.iter().any(|j| j.kind() == kind)
    }

    /// アプリ終了時の畳み方（4節）。
    pub fn cancel_all(&self) {
        for job in &self.active {
            job.cancel();
        }
    }
}
```

呼び出し側の適用例（`DrillApp::ui` の冒頭、既存の自動保存ブロック main.rs:477-487 を置き換える形）:

```rust
for (kind, outcome) in self.jobs.poll_all() {
    match (kind, outcome) {
        (JobKind::Save, JobOutcome::Done(value)) => {
            let _: Box<()> = value.downcast().expect("Save job yields ()");
            if self.document_revision == self.revision_at_last_save_launch {
                self.dirty = false;
            }
            self.status = "保存しました".into();
        }
        (JobKind::Save, JobOutcome::Failed(reason)) => {
            self.status = format!("保存エラー: {reason}");
        }
        (JobKind::AssignmentOptimize, JobOutcome::Done(value)) => {
            let assignment: Box<Vec<usize>> =
                value.downcast().expect("AssignmentOptimize yields Vec<usize>");
            self.apply_assignment(*assignment); // 既存の MoveCommand 生成ロジックへ橋渡し
        }
        // 他の (kind, outcome) の組も同様に網羅する。
        _ => {}
    }
}
```

**優先度**: 明示的な横取り（実行中のジョブを止めて別のジョブを先に走らせる）は実装しない。
`AutoSave` は `can_start` に拒否されたら **キューに積まず、その回はスキップして次の30秒間隔を待つ**
（`AutoSave` はどうせ周期的に再試行されるので、キューイングの複雑さを持ち込む理由がない）。
`Save`（手動保存）が `AutoSave` の実行中に要求された場合も同様に拒否され、ステータスに
「自動保存中です。完了後にもう一度保存してください」等を表示する
（`AutoSave` は数十ms〜数百ms程度で終わる想定なので、実用上ほぼ気にならない）。

### 3.8 UI要件（`43-app-architecture.md` への引き渡し仕様）

- 画面のどこか（ステータスバー下 or 専用パネル）に **実行中ジョブ一覧** を表示する。
  各行: ラベル（例:「動画を書き出し中」）、進捗バー（`JobManager::active_summaries()` の `f32`）、
  キャンセルボタン。
- キャンセルボタン押下 → `JobManager::cancel_all_of(kind)` を呼ぶ。ボタンは
  押下直後に無効化し（二重押下防止）、`Cancelled` または `Failed` の終端メッセージを
  受け取ったら一覧から消える（`poll_all` が自動的に取り除く）。
- 完了通知はトースト的な一過性表示でよい（`self.status` への文字列代入を流用できる。
  重要度が高いもの、例えば動画書き出し完了は、ステータスバーだけでなくモーダルやトーストで
  明示するかは 43 の判断に委ねる）。
- **書き出し系ジョブ開始時**、対象が「開始時点のスナップショットを書き出す」ことを
  一言添える（例:「エクスポートを開始しました（現在の内容を書き出します。完了までの間の編集は
  含まれません）」）。3.4節の一貫性の帰結をユーザーに正しく伝えるための最低限の文言。
- 色だけに依存しない表示にする（`DESIGN_GAPS.md` C-3 と同じ方針）: 進捗中は `⏳`、成功は `✓`、
  失敗は `⚠` などの記号を文字色と併記する。

## 4. 不変条件

1. `Job::poll` は **絶対にブロックしない**（`mpsc::Receiver::try_recv` のみを使う。`recv` は禁止）。
2. ワーカースレッド内のパニックは `catch_unwind` で必ず捕捉され、`JobMsg::Failed` に変換される。
   アプリのプロセスは決して異常終了しない（テストで検証可能: 意図的にパニックする本体を
   `Job::spawn` し、`poll()` が最終的に `Some(JobMsg::Failed(_))` を返すことを確認する）。
3. `Job<T>` に渡される `Document` は呼び出し時点の値のクローンであり、以後 UI スレッド側の
   `Document` とは独立である（Rustの所有権によって型システムが保証。エイリアシングは物理的に
   起こり得ない）。
4. `JobManager` はロックを一切持たない。`active: Vec<Box<dyn AnyJob>>` は UI スレッドからのみ
   読み書きされる（ワーカースレッドは自分の `Job<T>` の `Arc<AtomicU32>`/`Arc<AtomicBool>`/
   `mpsc::Sender` にしか触れず、`JobManager` の存在を知らない）。
5. `JobManager::can_start` により、`Save` と `AutoSave` は同時に2つ以上動かない
   （どちらか一方が実行中なら他方は起動が拒否される）。
6. `Job<T>` が `poll()` で終端メッセージを一度返した後は、以後何度呼んでも `None` を返す
   （冪等）。`JobManager::poll_all` はその時点で対象を `active` から取り除く。
7. `Job<T>` を `drop` すると必ずキャンセルフラグが立つ（`Drop` 実装で保証）。UIスレッドの
   `join()` 待ちは発生しない（アプリ終了シーケンス（後述）を除く）。
8. デッドロックは構造的に起きない: システム全体でロックを使う箇所が存在しない
   （`Mutex`/`RwLock` を一切導入しない。`Arc<AtomicU32>`/`Arc<AtomicBool>` と
   `mpsc::channel`（各ジョブにつき単一方向・単一送信者・単一受信者のSPSC）のみで構成される）。
   UIスレッドは `try_recv`/アトミック読み取りしか行わないため、他スレッドの状態を待って
   止まることが型のレベルであり得ない。ワーカースレッドは他のワーカーの状態を一切参照しない
   （`JobManager` にも触れない）ため、ワーカー同士の相互待ちも構造的に発生しない。

## 5. 性能

すべて「ジョブ起動時に1回」または「毎フレームのポーリング」のどちらかであり、
再生中の描画ループそのもの（16.6ms予算）には新たな重い処理を追加しない。

| 操作 | 頻度 | 概算コスト | 16.6ms予算への影響 |
|---|---|---|---|
| `Document` クローン（基準規模 1,000人×64セット） | ジョブ起動時のみ | 約0.15〜0.2ms | ボタン押下フレームのみ、体感カクつきなし |
| `Document` クローン（上限規模 4,000人×256セット） | ジョブ起動時のみ | 約1.0〜1.3ms | 同上。上限規模でも1フレームの1/10未満 |
| `std::thread::spawn` | ジョブ起動時のみ | 数十マイクロ秒（Windows実測レンジ） | 無視できる |
| `Job::poll()`（1件あたり） | 毎フレーム × アクティブジョブ数 | アトミック読み取り1回＋`try_recv`1回、合計十〜数十ナノ秒 | 同時4ジョブでも1マイクロ秒未満 |
| `ctx.request_repaint_after` 呼び出し | ワーカー側、最大10Hzにスロットル | 呼び出し自体は軽量（内部でフラグを立てるのみ） | UIスレッドの1フレーム予算には無関係（別スレッドが呼ぶ） |
| ワーカー→UIの結果転送（`JobMsg::Done(T)` の `send`） | ジョブ完了時のみ | `T` は値としてムーブされるだけで、`T` の**ヒープ内容はコピーされない**（例: 出力CSVが仮に上限規模で約20MBの `String` でも、転送されるのは24バイトのポインタ/長さ/容量ハンドルのみ）。チャンネルのノード確保1回が実コスト | ジョブ完了フレームで1回、無視できる |

**空欄を残さない**: `optimal_assignment`（pathing.rs:126）自体の計算量（貪欲法 O(n²) + 2-opt
最大 `4n+8` パス×O(n²) ＝ 最悪 O(n³)）はこの文書のスコープ外（`pathing.rs` 自体の担当）だが、
n=1,000 でこれが仮に数百ms〜数秒かかったとしても、**UIスレッドをブロックしない**という
本書の目的は達成される。実測してキャンセル猶予（3.5節の「1パスごとに確認」で足りるか）を
検証するのはテスト計画（7節）に含める。

## 6. 失敗モードと安全性

| 失敗モード | 対処 |
|---|---|
| ワーカー内 `panic`（例: 想定外の `Document` 不変条件破りで添字パニック） | `catch_unwind` で捕捉 → `JobMsg::Failed("internal-panic")`。**プロセスは落ちない**。 |
| 上記パニックと `41-persistence.md` のクラッシュ復旧の関係 | 無関係。`std::panic::set_hook`（B-5）によるクラッシュダンプ生成は、パニックが**スレッド境界を越えて伝播した場合**（＝`catch_unwind` されなかった場合）にのみ意味を持つ。本書のジョブはワーカースレッドの最外周で必ず `catch_unwind` するため、ジョブ内のパニックは制御されたエラーとして扱われ、クラッシュダンプ経路には到達しない。ジョブの外（UIスレッド自体でのパニック）は本書の対象外で、41 の担当。 |
| ディスクフル・権限エラー（保存/書き出し） | `std::fs::write` の `Err` をそのまま `Result<T, String>` の `Err` として返す → `JobMsg::Failed`。一時ファイル方式のため元ファイルは無傷。 |
| FFmpeg子プロセスがパイプ書き込みでハング（相手がバッファを読まない） | ワーカースレッドの `stdin.write_all` がブロックする。**UIスレッドは別スレッドなので無関係だが、ジョブ自体が進捗停止したまま見える。** 対策: 進捗が一定時間（例: 30秒）更新されない場合にウォッチドッグとして `Failed` へ倒し、`Child::kill()` する仕組みが要る。具体的な実装（Windows名前付きパイプ vs 匿名パイプでのタイムアウト可否）は `31-video-export.md` の担当とし、本書は「ジョブは無限に沈黙してはいけない」という契約のみ確定する。 |
| `Job` が終端に達する前にアプリが終了しようとする | 4節「アプリ終了時の畳み方」を参照。 |
| 同種ジョブの二重起動（例: 自動保存タイマーが発火した瞬間に手動保存ボタンも押される） | `JobManager::can_start` が `AlreadyRunning` を返し、後から来た方は起動されない（3.7節）。 |
| ジョブが受け取った `Document` クローンが巨大で、上限規模を超える不正なプロジェクトファイル（敵性入力）から来ている | クローン自体はメモリ確保以外の危険は無いが、`00-conventions.md`「信頼できない入力」の要素数上限は読み込み時（`Document::from_json`／将来の `DrillError::` 系検証）で既に弾く前提。本書はその検証を通過済みの `Document` のみを扱う。 |
| `Job<T>` の `T` に巨大なアロケーションを持つ値を意図せず頻繁に `Done` で送り続ける設計ミス | 5節の通り転送自体は軽いが、UIスレッド側の適用コード（`match` 節）で不要なクローンを書かないことをレビュー観点にする（7節のテスト計画でチェック）。 |

## 7. テスト計画

**単体テスト**（`crates/drill-jobs/src/lib.rs` 内 `#[cfg(test)]`）:

- `poll` は結果が届く前は常に `None` を返し、ブロックしない（`Instant` で計測し、
  数ミリ秒以内に返ることを確認）。
- 正常終了ジョブは `poll` が最終的に `Some(JobMsg::Done(value))` を1回だけ返し、
  以後は `None`。
- 本体が `Err(String)` を返すジョブは `Some(JobMsg::Failed(_))` を1回返す。
- 本体がパニックするジョブ（`panic!("boom")`）でも `Some(JobMsg::Failed(_))` を返し、
  かつテストプロセス自体は異常終了しない（`catch_unwind` の効果を直接検証）。
- `progress()` は `ProgressHandle::set` で書いた値を 0.0..=1.0 にクランプして反映する
  （負値・1.0超え・NaN を入力するプロパティテストを含む。`NaN.clamp` の挙動に注意し、
  実装側で `is_finite` チェックを入れる）。
- `cancel()` 後、本体が `is_cancelled()` を確認するタイミングで速やかに停止することを、
  意図的にループする本体で検証する（ループの1反復ごとに `is_cancelled` を見るテスト用本体を
  使い、`cancel()` 呼び出しから `Job` が終端するまでの反復回数が閾値以下であることを確認）。

**`JobManager` の単体テスト**（`crates/drill-app` 側、または `drill-jobs` に切り出した場合はそちら）:

- `Save` 実行中に `AutoSave` を `start` すると `Err(JobRejected::AlreadyRunning)`。逆も同様。
- 同種ジョブを `max_concurrent` まで起動できて、それを超えると `Err(JobRejected::AtCapacity)`。
- `poll_all` が終端に達したジョブだけを `active` から取り除き、実行中のものは残す。

**ストレステスト**:

- 短命ジョブ（即座に `Ok(())` を返す）を100個連続で `start` し、全て `AtCapacity` に
  弾かれることなく（ただし同時実行数上限は守りつつ）最終的に全部完了することを確認する
  （キューイングはしない設計なので、上限に達した分は `Err` を受けてテスト側がリトライする
  形で検証する）。

**ベンチマーク**（`crates/drill-core/benches/` に追加。既存 `core_performance.rs` の隣に新規
ファイル、または追記）:

- `Document::demo(1000, 64)` 相当のドキュメントに対する `.clone()` のコストを計測し、
  5節の見積もり（0.2ms程度）と桁が合っているかを確認する。上限規模
  （`Document::demo` を演者4,000相当・セット256相当に拡張したもの、または手組みの
  フィクスチャ）でも同様に計測し、1.3ms程度の見積もりを検証する。
- `pathing::optimal_assignment` を n=1,000 で実行した際の1パスあたりの所要時間を計測し、
  3.5節の「1パスごとにキャンセル確認すれば100ms以内に反応する」という前提が
  成立するかを確認する（1パスが100msを超えるようなら、パス内でも確認点を増やす設計変更が
  要ることが分かる）。

**決定論の回帰テスト**（`00-conventions.md` 不変条件5との整合）:

- 同一の `Document`（同一revision）から2つの `ExportText` ジョブ（例: SVG書き出し）を
  独立に起動し、両方の出力バイト列が完全一致することを確認する。

## 8. 実装タスク

Codexに渡す単位（1タスク=1〜3時間相当）。依存関係を明示する。

1. **`drill-jobs` クレート新設**（依存なし）。`Cargo.toml` に `members` 追加、
   `ProgressHandle`/`Job<T>`/`JobMsg<T>`/`JobKind` を実装し、7節の単体テストを書く。
2. **`JobManager` 実装**（依存: 1）。`crates/drill-app/src/jobs.rs` として新規作成
   （`DESIGN_GAPS.md` C-1 のツリーを先取りする形。C-1本体の `main.rs` 分解を待たずに
   このファイルだけ先に切り出してよい。`DrillApp` に `jobs: JobManager` フィールドを追加）。
3. **保存/自動保存のジョブ化**（依存: 1, 2）。`save_to`（main.rs:236-247）と自動保存ブロック
   （main.rs:477-487）を `JobKind::Save`/`JobKind::AutoSave` に置き換える。一時ファイル＋
   `rename` の原子的置換をジョブ本体に実装する（現状の `save_to` はバックアップコピーのみで
   原子的置換をしていない点も同時に直す）。`document_revision` の参照が必要になるため、
   まだ存在しなければ `DrillApp` に最小限のカウンタ（全ての編集コマンド適用箇所でインクリメント）
   を仮実装してよい（正式な所有権は A-1/41 の決定を待つ、9節）。
4. **テキスト書き出しのジョブ化**（依存: 1, 2）。`export_text`（main.rs:249-260）と
   呼び出し元（main.rs:1287, 1301, 1308, 1312, 1316, 1323, 1339）を `JobKind::ExportText` に
   置き換える。文字列生成（`svg::export`/`countsheet::*`/`continuity::*`）は既存の同期関数を
   そのままジョブ本体クロージャに包む。
5. **割り当て最適化のジョブ化**（依存: 1, 2）。`auto_assign_next`（main.rs:264-289）を
   `JobKind::AssignmentOptimize` に置き換え、`pathing::optimal_assignment` 呼び出し前後に
   3.5節のキャンセル確認点を追加する（`optimal_assignment` のシグネチャ変更が必要になる場合は
   `pathing.rs` 側の変更として別途扱う。本タスクでは `ProgressHandle` を渡せるよう
   `optimal_assignment_cancellable(from, to, progress: &ProgressHandle) -> Option<Vec<usize>>`
   のような薄いラッパーを `drill-app` 側、または `pathing.rs` に追加する形で対応してよい）。
6. **進捗UI**（依存: 2）。3.8節の要件に沿った実行中ジョブ一覧パネルを実装する。
   詳細な見た目・配置は `43-app-architecture.md` の判断に従う。
7. **アプリ終了シーケンス**（依存: 2, 3）。eframeの終了要求フックに対して4節の
   「畳み方」を実装する。
8. **ベンチマークとキャンセル猶予の実測**（依存: 1）。7節のベンチマーク項目を
   `crates/drill-core/benches/` に追加する。
9. **（ブロック中・A-4待ち）** `scan_transition` 実装後に `JobKind::CollisionScan` を配線する。
10. **（ブロック中・B-2/B-4待ち）** `drill-audio`/`drill-export` 実装後に
    `JobKind::AudioDecode`/`JobKind::ExportVideo` を配線し、3.2節の専用スレッド（動画エンコード）
    境界を実装する。

並行可能性: 3・4・5 は 1・2 完了後に並行して着手できる（互いに独立したコード領域）。
6 は 2 のみに依存するため 3〜5 と並行できる。7 は 3 の一時ファイル方式が固まってから。
8 は 1 のみに依存するため最速で着手できる。9・10 は他の設計文書（13/30/31）が先に完了しないと
着手できないブロック済みタスク。

## 9. 未決事項

- **`JobMsg::Failed(String)` の型**: 本書では `drill-jobs` を `drill-core` に依存させない
  制約上、失敗理由を素の `String` にした（3.1節）。`42-error-i18n.md` で `DrillError`/`Locale`
  の設計が固まった後、「呼び出し側が `error.message(locale)` を呼んでから渡す」という
  現在の契約で十分か、それとも `Job<T, E>` のように失敗型もジェネリックにすべきかを
  再検討する。
- **同時実行数の初期値**（3.7節の `max_concurrent`: 一般系2、Save/AutoSave1、Video1、Audio2）は
  実測に基づかない初期見積もりである。低スペック端末での実プロファイルが取れる
  （`drill-audio`/`drill-export` 実装後）まで暫定値として扱う。
- **`document_revision: u64` の所有権**: 本書の3.4節（dirtyフラグ整合）はこの値の存在を
  前提にするが、正式な定義・更新箇所は `DESIGN_GAPS.md` A-1（`Edit` 導入）または
  A-4（衝突走査キャッシュ）、あるいは `41-persistence.md` のどれが持つべきか未確定。
  いずれかの文書と本書のインターフェース（型と更新タイミングの契約）をすり合わせる必要がある。
- **FFmpegパイプのウォッチドッグ**（6節）: Windows上での匿名パイプ／名前付きパイプの
  タイムアウト可否を検証していない。`31-video-export.md` 側の実測待ち。
- **常駐スレッドプールへの切り替え判断基準**（3.3節案2）: 「ジョブの起動頻度がどの程度
  増えたら切り替えるべきか」の具体的な閾値（例: 1分間に10回を超えたら、等）を
  数値化していない。運用実績が無い現時点では決め打ちできないため保留する。
- **Arc共有／永続データ構造への切り替え判断基準**（3.4節）: 演者数の見積もり上限
  （4,000人）を超える利用が実際に要求された場合の再検討トリガーとして記録するのみで、
  具体的な閾値や `im` クレート採用の是非は未検討。

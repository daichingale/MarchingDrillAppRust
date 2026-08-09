# 50. テスト・ベンチ・CI 戦略

## 1. 目的と範囲

### 目的

`PRODUCT_QUALITY.md` の Release gates を、**人の記憶や手作業ではなく、機械が毎回判定する形**に落とす。
販売可能な水準の製品とは「壊れていないと主張できる製品」ではなく、**「壊れていないことを再現可能な手順で示せる製品」**である。
本書はその手順一式（テスト層・期待値の管理・決定論の定義・確保計測・ベンチ・ファジング・CI・カバレッジ）を定義する。

具体的に解く問題は 4 つ。

1. Release gates 16 項目のうち、どれがどのテストで保証されるかが誰にも言えない。対応表を作る（§7）。
2. 「確保ゼロ」「決定論」「メモリ増加なし」が、現状は**測っていないのに満たしていることになっている**。測る仕組みを作る（§3.5・§3.6・§3.7）。
3. CI が無く、退行が commit 後いつ入ったか特定できない。3 OS の CI を設計する（§3.10）。
4. テストを増やすと開発が遅くなる。実行時間で層を分け、内側ループを 10 秒以内に保つ（§3.1）。

### 範囲外

- **各機能モジュール固有のテスト項目**。それは各設計文書の「§7 テスト計画」が正である。本書は**枠組みと共通基盤**（フィクスチャ、比較器、確保カウンタ、ゴールデンランナー、CI）だけを定義する。
- **信頼できない入力に対する上限値・拒否条件そのもの**。`51-security.md` が定義する。本書はそれを**破れないことを確認する側**（§3.9）。
- **配布物の署名・更新・インストーラの検証**。`53-製品化` の範囲。
- **UI の手動 QA チェックリストの中身**。`43-アプリケーション構造と UX・アクセシビリティ` が定義する。本書は「それをリリース前に必ず通す」という運用位置づけだけを与える（§3.1 の L4 層）。
- **コードの変更**。00-conventions.md の禁止事項に従い、本書は `docs/design/50-testing-ci.md` のみを作成する。

---

## 2. 現状

### 2.1 テストの所在と件数

`#[test]` は 124 件。全て `#[cfg(test)] mod tests` としてソースにインラインで置かれている。統合テスト用の `tests/` ディレクトリはどのクレートにも存在しない（`crates/drill-core/tests`・`crates/drill-app/tests` ともに不在）。

| ファイル | 総行数 | `mod tests` 開始行 | `#[test]` 件数 |
|---|---:|---:|---:|
| `crates/drill-core/src/lib.rs` | 631 | 481 | 12 |
| `crates/drill-core/src/coordinates.rs` | 322 | 184 | 14 |
| `crates/drill-core/src/editing.rs` | 359 | 158 | 16 |
| `crates/drill-core/src/shapes.rs` | 368 | 191 | 13 |
| `crates/drill-core/src/pathing.rs` | 336 | 178 | 12 |
| `crates/drill-core/src/tempo.rs` | 330 | 203 | 11 |
| `crates/drill-core/src/continuity.rs` | 300 | 187 | 10 |
| `crates/drill-core/src/audio.rs` | 255 | 136 | 8 |
| `crates/drill-core/src/camera.rs` | 372 | 236 | 7 |
| `crates/drill-core/src/svg.rs` | 401 | 325 | 7 |
| `crates/drill-core/src/playback.rs` | 181 | 92 | 5 |
| `crates/drill-core/src/video.rs` | 298 | 257 | 5 |
| `crates/drill-core/src/countsheet.rs` | 182 | 130 | 4 |
| `crates/drill-app/src/main.rs` | 1,837 | — | **0** |

`drill-app` は 1,837 行に対してテスト 0 件。`DESIGN_GAPS.md` C-1 が指摘する god struct であり、`DrillApp` を外部から構築する経路が無いため現状はヘッドレス実行もできない。

### 2.2 依存関係とビルド設定

- `Cargo.toml`（ルート）: members は `crates/drill-core` と `crates/drill-app` の 2 つ。`[profile.release]` は `lto = "thin"` / `codegen-units = 1` / `strip = "symbols"`。**テスト・ベンチ用のプロファイルは無い。`overflow-checks` の指定も無い。**
- `crates/drill-core/Cargo.toml`: 依存は `serde` / `serde_json` のみ。**`[dev-dependencies]` セクションは存在しない。**
- `crates/drill-app/Cargo.toml`: `eframe 0.35.0`（`default-features = false`, features = `wgpu` / `wayland` / `x11`）と `rfd 0.17.2`。Linux でのビルドに GTK3 と XCB/xkbcommon の開発パッケージが要る。
- `Cargo.lock`: 360 パッケージ。`proptest` / `insta` / `criterion` はいずれも**存在しない**。
- `rust-toolchain.toml` **無し**（toolchain 固定なし）。`deny.toml` **無し**。`.config/nextest.toml` **無し**。`.gitattributes` **無し**。
- `.github/` ディレクトリは**存在しない**。CI は一切無い。

### 2.3 ベンチ

`crates/drill-core/benches/core_performance.rs`（28 行、`harness = false` の自前 `main`）。

```rust
let document = Document::demo(100, 10);          // = 1,000 performers
for frame in 0..60_000 { document.positions_at(0, .., &mut positions); }
for _ in 0..100 { document.to_json() }
assert!(positions.capacity() >= 1_000);
```

- 出力は `println!` のみ。**閾値の assert が無いので、何倍遅くなっても成功する。**
- 最終行の `assert!(positions.capacity() >= 1_000)` が確保に関する唯一の検査だが、容量の下限しか見ておらず、途中で何度再確保しても通る。
- `PRODUCT_QUALITY.md` のベースライン「1,000人 × 60,000フレーム補間: 9.24ms」は 1 フレームあたり **154ns** に相当する。この作業集合は `from`(8KB) + `to`(8KB) + `out`(8KB) = 24KB で L1 に収まる。実フレームでは補間が DisplayList 構築・解析・描画とキャッシュを奪い合うため、この値は**関数単体の下限**であって、フレーム予算の根拠に使える値ではない。

### 2.4 確保ゼロ検証の現状と、その 3 つの穴

唯一の検証は `crates/drill-core/src/lib.rs:485` の `interpolation_reuses_output_allocation`。

```rust
let doc = Document::demo(10, 10);               // 100 performers
let mut output = Vec::with_capacity(100);
let pointer = output.as_ptr();
doc.positions_at(0, 0.5, &mut output);
assert_eq!(pointer, output.as_ptr());
```

穴は 3 つある。

1. **容量が最初から十分なので、そもそも再確保しようがない。** 100 要素に対して `with_capacity(100)`。この assert は「関数が `out` を縮めない」ことしか言っていない。
2. **`out` 以外の確保を一切見ていない。** 関数内部の一時 `Vec` / `format!` / `Box` / `HashMap` は全て素通りする。フレーム経路は補間だけでなく解析・DisplayList 構築・描画コマンド生成の連鎖であり、そのどこで確保が起きても検出されない。
3. **「再確保しなかった」は「確保回数 0」ではない。** アロケータが解放直後の同一アドレスを返せばポインタは一致する（ABA）。

加えて、検証対象の `positions_at` 自身に潜在バグがある。`crates/drill-core/src/lib.rs:388`:

```rust
out.clear();
out.reserve(from.len().saturating_sub(out.capacity()));
```

`clear()` 後の `len()` は 0 なので `reserve(additional)` は容量 `0 + additional` を要求する。`additional = from.len() - capacity` なので、`capacity` が正であるかぎり**要求容量は常に `from.len()` に届かない**。実際には後続の `extend` が `TrustedLen` の `size_hint` から正しく確保するため動くが、`reserve` の行は意図した働きをしていない。**現行テストはこの種の欠陥を原理的に検出できない。**

### 2.5 その他の欠落

- **ゴールデン（スナップショット）比較は 1 件も無い。** `svg.rs:329-393` の 7 件は `contains("<svg")` や `matches("<circle").count()` のような構造の部分検査で、出力全体を固定していない。文言・桁・属性順の変化は検出されない。
- **property テスト無し。** `tempo.rs` の 11 件は全て具体値のケーステスト。
- **決定論テスト無し。** 同一入力から同一出力を assert しているテストは 0 件。
- **ストレステスト無し。** 10,000 編集も 4,000 人も 2 時間再生も無い。
- **浮動小数比較の規約が無い。** 各モジュールがローカルに `approx` を定義している（例: `crates/drill-core/src/playback.rs:96` は `1e-3` 固定）。どこが完全一致でどこが ε かの方針が文書化も実装もされていない。
- **`.gitignore` が 2 行しか無く、`*.drill.json` を無視している。** v1 フィクスチャを `.drill.json` という自然な名前で置くと **git に入らない**。`DESIGN_GAPS.md` A-7 が要求する v1 フィクスチャの回帰テストは、この行のまま進めると静かに失敗する。
- **`.gitattributes` が無い。** Windows でチェックアウトするとテキストファイルが CRLF になりうる。ゴールデン期待値を導入する前に LF 固定が必要。

---

## 3. 設計

### 3.1 テストの層

7 層。**層の分割基準は「対象の粒度」ではなく「実行時間と実行頻度」**である。粒度で分けると遅いテストが内側ループに混ざる。

| 層 | 対象 | 置き場 | 外部依存 | 実行トリガ | 時間目標 |
|---|---|---|---|---|---|
| **単体** | 1 関数・1 型の入出力。境界値。エラー分岐。 | 各 `src/*.rs` の `#[cfg(test)] mod tests` | **なし** | 保存時・コミット前 | 全体 10 秒 |
| **統合** | クレートを跨ぐ経路。読込→編集→保存→再読込。ジョブ基盤。 | `crates/drill-conformance/tests/` | あり | PR | 60 秒 |
| **ゴールデン** | SVG / CSV / カウントシート / ドリルブック / DisplayList / フレームハッシュ | `crates/drill-conformance/tests/golden_*.rs` + `snapshots/` | `insta` | PR（Linux のみ） | 30 秒 |
| **property** | 代数的性質（可逆・往復・単調・置換不変） | `crates/drill-conformance/tests/prop_*.rs` | `proptest` | PR 256 ケース / nightly 65,536 ケース | PR 90 秒 |
| **決定論** | 同一入力 → 同一出力。プロセス跨ぎ・実行跨ぎ。 | `crates/drill-conformance/tests/determinism.rs` | なし | PR | 30 秒 |
| **ストレス** | 10,000 編集 / 4,000 人 / 2 時間再生 / 確保ゼロ区間 | `crates/drill-conformance/tests/stress_*.rs`, `alloc_zero.rs` | なし | 縮小版を PR / 全量を nightly | PR 120 秒 |
| **ベンチ** | 壁時計の絶対予算 / 命令数の相対退行 | `crates/drill-core/benches/`, `crates/drill-bench/benches/` | `criterion`, `gungraun` | 命令数を PR / 全量を nightly | PR 8 分 |
| **UI スモーク** | ヘッドレス egui でクラッシュせず 120 フレーム回る | `crates/drill-app/tests/` | `egui_kittest` | PR | 30 秒 |

#### 実行層（トリガ）

| 層 | トリガ | コマンド | 目標 |
|---|---|---|---|
| **L0** | エディタ保存 | `cargo check -p drill-core` | 2 秒 |
| **L1** | コミット前（手動 / pre-commit） | `cargo test -p drill-core` | **10 秒** |
| **L2** | PR / push | `ci.yml` 全ジョブ | **15 分**（並列後の実時間） |
| **L3** | nightly（毎日 03:00 UTC）| `nightly.yml` | 90 分 |
| **L4** | リリース候補 | L3 + 手動 UI チェックリスト（43）+ ffprobe 検証つき動画書き出し | 半日 |

#### 原則: `drill-core` に dev-dependency を一切足さない

00-conventions.md の「`drill-core` の依存は serde/serde_json のみ」を、**dev-dependencies にも適用する**。

理由:

- L1（`cargo test -p drill-core`）を無依存・数秒に保つ。`proptest` / `insta` / `criterion` を dev-dep に足すと、クリーンビルドで 100 以上のクレートが増え、内側ループが数分になる。
- 「core は純粋ロジック」という境界を、`cargo metadata` で機械的に検査できる形で守れる（§4 I-1）。
- 外部クレートを要するテストは、上位のテスト専用クレートに置けば何も失われない。

#### 追加するクレート（00-conventions.md のクレート境界表への追記）

```
drill-testkit      テスト補助ライブラリ。フィクスチャ生成・確保カウンタ・digest・
                   浮動小数比較器・決定論ハーネス・proptest 生成器。
                   依存: drill-core（+ feature 経由で proptest）。publish = false。
drill-conformance  テスト専用クレート。src/lib.rs は空。tests/ に横断テスト
                   （ゴールデン・property・決定論・ストレス・確保ゼロ・移行）を置く。
                   dev 依存: drill-core, drill-testkit, insta, proptest。publish = false。
drill-bench        ベンチ専用クレート。criterion と gungraun のベンチのみ。
                   dev 依存: drill-core, drill-testkit, criterion, gungraun。publish = false。
```

依存方向（上から下へ、既存の規約に反しない）:

```
drill-core ─→ drill-testkit ─┬─→ drill-conformance   (テストのみ)
                             └─→ drill-bench          (ベンチのみ)
```

`drill-core` はこれらを一切知らない。したがって **dev-dependency の循環は発生しない**。Cargo は dev-dependency の循環を許容するが、循環があると「core のテストだけ回す」が不可能になり、L1 の 10 秒目標が崩れるため、意図的に循環しない配置にしている。

3 クレートとも `publish = false` かつ製品バイナリのグラフに入らない。`cargo tree -p drill-app --edges normal` に現れないことを CI で検査する（§4 I-1）。

---

### 3.2 共通基盤 — `drill-testkit`

#### 3.2.1 フィクスチャ

**規模を 4 段に固定する。** テストごとに規模を発明すると、実行時間が予測できなくなり、ゴールデンが読めなくなる。

```rust
// crates/drill-testkit/src/fixture.rs
use drill_core::Document;

/// 4 performers / 2 sets. ゴールデンを人間が目で読める最小規模。
pub fn tiny() -> Document;

/// 16 performers / 3 sets / 変拍子テンポ 3 区間。ゴールデンの標準規模。
pub fn small() -> Document;

/// 128 performers / 8 sets。property とストレスの PR 層規模。
pub fn medium() -> Document;

/// 1,000 performers / 64 sets / 総カウント 2,048。00-conventions.md の基準規模。
pub fn baseline() -> Document;

/// 4,000 performers / 256 sets。00-conventions.md の上限規模。
pub fn maximum() -> Document;

/// v1 スキーマの生 JSON。`DESIGN_GAPS.md` A-7 の移行回帰用。
/// バイト列としてコミットし、パースを介さず配布する。
pub const V1_SAMPLE: &str = include_str!("../fixtures/v1_sample.drillproj.json");
```

**フィクスチャは生成する。ファイルにはしない**（`V1_SAMPLE` を除く）。4,000 人の JSON は数 MB になり、リポジトリを重くし diff も読めない。生成関数の決定論は §3.5 の決定論テストが保証する（`Digest` をゴールデンに固定する）。

`V1_SAMPLE` の拡張子は `.drillproj.json`。**`.gitignore` の `*.drill.json` に一致しないことが必須**であり、これは §8 の T-01 で `.gitignore` を修正して恒久化する。

#### 3.2.2 決定論ダイジェスト

出力の同一性を、巨大な文字列比較ではなく 64bit の値で表す。

```rust
// crates/drill-testkit/src/digest.rs

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// FNV-1a 64. Not cryptographic: this detects regressions, not tampering.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Digest(u64);

impl Default for Digest {
    fn default() -> Self {
        Self(FNV_OFFSET)
    }
}

impl Digest {
    pub fn bytes(mut self, data: &[u8]) -> Self {
        for &b in data {
            self.0 = (self.0 ^ u64::from(b)).wrapping_mul(FNV_PRIME);
        }
        self
    }

    pub fn u64(self, v: u64) -> Self {
        self.bytes(&v.to_le_bytes())
    }

    pub fn usize(self, v: usize) -> Self {
        self.u64(v as u64)
    }

    pub fn str(self, v: &str) -> Self {
        self.bytes(v.as_bytes()).u64(v.len() as u64)
    }

    /// `-0.0` collapses to `+0.0` and every NaN to one canonical NaN, so a
    /// digest mismatch always means a real difference rather than a bit pattern
    /// difference that no observer can see.
    pub fn f32(self, v: f32) -> Self {
        let bits = if v.is_nan() {
            0x7fc0_0000
        } else if v == 0.0 {
            0
        } else {
            v.to_bits()
        };
        self.bytes(&bits.to_le_bytes())
    }

    pub fn finish(self) -> u64 {
        self.0
    }

    pub fn hex(self) -> String {
        format!("{:016x}", self.0)
    }
}

/// Digest of a whole document: identity, order, geometry, timing.
pub fn document_digest(doc: &drill_core::Document) -> Digest;

/// `len=<n> fnv=<hex>` — the golden form for an RGBA frame or any byte buffer.
pub fn buffer_tag(data: &[u8]) -> String;
```

長さを常に混ぜる（`str` / `buffer_tag`）。FNV-1a は長さを区別しないため、末尾のゼロ埋めの差を取り落とす可能性がある。

#### 3.2.3 浮動小数比較器

`playback.rs:96` のようなローカル `approx` を全廃し、**許容誤差に名前をつける**。名前がないと、後から誰も「なぜ 1e-3 なのか」を答えられない。

```rust
// crates/drill-testkit/src/approx.rs

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tolerance {
    pub rel: f32,
    pub abs: f32,
}

impl Tolerance {
    /// Bit equality after canonicalising -0.0 and NaN. Use for values produced
    /// only by +, -, *, / and sqrt on one platform: IEEE-754 makes those
    /// correctly rounded, so any difference is a real difference.
    pub const EXACT: Self = Self { rel: 0.0, abs: 0.0 };

    /// Values that passed through sin/cos/tan/exp/ln/powf. Those are not
    /// correctly rounded and differ between platform libm implementations.
    pub const GEOMETRY: Self = Self { rel: 1e-5, abs: 1e-4 };

    /// Values accumulated over up to 256 piecewise-constant tempo segments.
    /// f32 has 24 mantissa bits; 256 sequential roundings at 2,048 counts give
    /// roughly 256 * 2^-24 ≈ 1.5e-5 relative worst case. 1e-4 leaves headroom.
    pub const TEMPO: Self = Self { rel: 1e-4, abs: 1e-3 };

    /// Rasterised pixel channel value.
    pub const PIXEL: Self = Self { rel: 0.0, abs: 2.0 };

    pub fn accepts(self, a: f32, b: f32) -> bool {
        if a == b {
            return true;
        }
        if !a.is_finite() || !b.is_finite() {
            return false;
        }
        let diff = (a - b).abs();
        diff <= self.abs || diff <= self.rel * a.abs().max(b.abs())
    }
}

#[track_caller]
pub fn assert_close(a: f32, b: f32, tol: Tolerance, what: &str);

#[track_caller]
pub fn assert_points_close(a: &[drill_core::Point], b: &[drill_core::Point], tol: Tolerance);

/// Every f32 reachable from the value must be finite. Catches NaN propagation,
/// which is the most common way a deterministic pipeline silently rots.
#[track_caller]
pub fn assert_all_finite(values: &[f32], what: &str);
```

#### 3.2.4 決定論的な擬似乱数

`rand` を入れない。ストレステストに必要なのは統計的品質ではなく**再現性**である。

```rust
// crates/drill-testkit/src/rng.rs

/// xorshift64. Deterministic across platforms and Rust versions: only integer
/// shifts and xors, no float and no libm.
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed | 1)
    }

    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    pub fn below(&mut self, n: usize) -> usize {
        if n == 0 { 0 } else { (self.next_u64() % n as u64) as usize }
    }

    /// 24 uniformly distributed mantissa bits mapped into `lo..=hi`.
    pub fn f32_in(&mut self, lo: f32, hi: f32) -> f32 {
        let unit = (self.next_u64() >> 40) as f32 / (1u32 << 24) as f32;
        lo + (hi - lo) * unit
    }
}
```

---

### 3.3 ゴールデンテスト

#### 3.3.1 insta を採用する（判断と根拠）

`DESIGN_GAPS.md` C-2 は「`.expected` ファイルをコミットして単純文字列比較で足りる（追加依存なし）」と書いている。**この方針を本設計で上書きする。** 根拠:

1. **本質は比較ではなく承認フローである。** ゴールデンテストの運用コストの大半は「意図的に変えたときにどう更新するか」に集中する。自前でやると結局 `UPDATE_GOLDEN=1` 環境変数と、書き戻しと、`.new` ファイルの管理と、差分表示を実装することになる。それは insta の劣化再実装である。
2. **差分の読みやすさが直接の品質要因。** 1,000 行の SVG が 1 文字違ったとき、`assert_eq!` の出力は読めない。insta は行単位の色付き差分を出す。
3. **CI での「新規スナップショット自動生成」を止める仕組みが標準で用意されている**（`INSTA_UPDATE=no`）。自前だと、環境変数を渡し忘れた CI が黙って期待値を書き換えて緑になる事故が起こりうる。
4. **`insta::glob!` によりフィクスチャディレクトリ一括適用が 3 行で書ける。** ファジングで見つかった入力を回帰へ昇格させる運用（§3.9）がこれに乗る。
5. **コストが `drill-conformance` に閉じる。** `drill-core` にも製品バイナリにも入らない。§3.1 の原則を破らない。

版: **insta 1.48.0**（2026-06-11 リリース。活発に保守されている）。`cargo-insta` は CI にはインストールせず、開発者ローカルのみ。

不採用にしたもの:

- **`expect-test`**: インラインに期待値を埋める方式。SVG や HTML のような大きい出力ではソースが読めなくなる。
- **完全自前**: 上記 1〜3 の理由。

#### 3.3.2 対象と形式

| # | 出力 | 生成 API | 形式 | フィクスチャ |
|---|---|---|---|---|
| G-01 | フィールド SVG | `svg::field_svg` | `assert_snapshot!`（テキスト） | `small()` |
| G-02 | セット SVG | `svg::set_svg` | `assert_snapshot!` | `small()` |
| G-03 | 座標 CSV | `coordinates::coordinates_csv` | `assert_snapshot!` | `small()` |
| G-04 | 演者シート | `coordinates::performer_sheet` | `assert_snapshot!` | `small()`, 演者 0 と最終 |
| G-05 | カウントシート | `countsheet::count_sheet_text` | `assert_snapshot!` | `small()`, 4/4 と 3/4 |
| G-06 | コンティニュイティ | `continuity::continuity_text` | `assert_snapshot!` | `small()` |
| G-07 | 座標表 HTML | `svg::coordinate_sheet_html` | `assert_snapshot!` | `small()` |
| G-08 | ドリルブック HTML | `svg::drill_book_html` | `assert_snapshot!` | `small()` |
| G-09 | DisplayList | `drill_render::build`（20） | `assert_snapshot!`（正規化デバッグ表現） | `small()`, count = 0 / 8.5 / 16 |
| G-10 | DisplayList（大規模） | 同上 | `assert_snapshot!(digest.hex())` | `baseline()`。**要約のみ**固定 |
| G-11 | 動画フレーム | `raster::draw`（31） | `assert_snapshot!(buffer_tag(&rgba))` | `small()`, フレーム 0 / 30 / 最終 |
| G-12 | FFmpeg 引数 | `video::VideoExportConfig::ffmpeg_args` | `assert_debug_snapshot!` | 全プリセット |
| G-13 | エラーメッセージ | `DrillError::message(Locale)`（42） | `assert_snapshot!` | 全 variant × Ja/En |
| G-14 | v1→v2 移行結果 | `migrate`（A-7） | `assert_snapshot!(document_digest.hex())` | `V1_SAMPLE` |

**大規模出力はダイジェストのみを固定する**（G-10）。1,000 人の DisplayList を全文コミットすると、レビュー時に誰も読まないゴミが増え、承認が形骸化する。人が読む対象は `small()` の全文、機械が守る対象は `baseline()` のダイジェスト、と役割を分ける。

#### 3.3.3 期待値ファイルの置き場と更新手順

```
crates/drill-conformance/
  Cargo.toml
  src/lib.rs                        // //! Test-only crate. 中身なし
  fixtures/
    v1_sample.drillproj.json
    regressions/                    // ファジング由来の最小化入力（§3.9）
      fuzz-0001-deep-nesting.json
  tests/
    golden_export.rs                // G-01..G-08
    golden_render.rs                // G-09..G-11
    golden_config.rs                // G-12..G-14
    snapshots/                      // insta が生成。git 管理下。
      golden_export__field_svg_small.snap
      ...
```

更新手順（**この 4 段だけを許す**）:

1. 実装を変える。`cargo test -p drill-conformance` が落ちる。`*.snap.new` が生成される。
2. `cargo insta review` で 1 件ずつ差分を見て accept / reject する。**まとめて `cargo insta accept` する運用を禁止する。**
3. `.snap` の変更を、実装の変更と**同じコミット**に含める。別コミットにすると「なぜ変わったか」が失われる。
4. PR 説明に「なぜこの出力が変わってよいか」を 1 行書く。レビュアは `.snap` の diff を読む。

CI 側の強制:

- 全ジョブで `INSTA_UPDATE=no` を設定する。新規スナップショットの自動生成が失敗になる。
- `.snap.new` が作業ツリーに残った状態でのコミットを検出するため、CI 末尾で `git diff --exit-code` と「`*.snap.new` が存在しないこと」を検査する。

#### 3.3.4 差分を読みやすく保つための出力側の要件

ゴールデンが読めるかどうかは**出力形式が決める**。以下を conformance 側の前提とし、満たさない出力はゴールデン対象にしない。

- **1 行 1 レコード。** `svg::field_svg` は現状 1 要素 1 行になっている（`svg.rs:66` 以降）ので条件を満たす。1 行に全要素を詰める出力は、1 文字違いで 1 行全体が差分になり読めない。
- **数値の桁を固定する。** `svg.rs:44` の `px()` は `{v:.2}` で丸めてから末尾ゼロを削っている。桁固定は良いが、`0.001` と `0.004` が両方 `"0"` になるため、ゴールデンは丸め後の値しか守らない。**丸め前の値は G-09/G-10 の DisplayList ダイジェストが守る。** 役割分担を明示しておく。
- **順序が入力順で決まること。** `HashMap` の反復順を出力に反映しない（§4 I-9）。
- **改行は LF 固定。** `.gitattributes` に以下を置く。

```gitattributes
* text=auto eol=lf
*.snap    text eol=lf
*.svg     text eol=lf
*.csv     text eol=lf
*.json    text eol=lf
*.png     binary
```

これが無いと、Windows でチェックアウトしたリポジトリで全ゴールデンが落ちる。

#### 3.3.5 ゴールデン単独にしない

ゴールデンは「変わったこと」しか言わない。「正しいこと」は言わない。したがって:

> **規約: ゴールデンを追加するとき、既存の意味テスト（構造検査）を消さない。**

`svg.rs:338` の `field_svg_draws_one_circle_per_performer`（`<circle` の個数 == 演者数）のような検査は、`.snap` を惰性で accept したときに**壊れたまま緑になることを防ぐ最後の砦**である。各ゴールデン対象につき、意味テストを最低 1 件併存させる。

---

### 3.4 property テスト

#### 3.4.1 proptest を採用する（判断と根拠）

**proptest 1.11.0**（2026-03-24）を採用。quickcheck ではない。

| 論点 | proptest | quickcheck | 本プロジェクトでの帰結 |
|---|---|---|---|
| 生成の単位 | `Strategy` 値ごと | 型ごとに 1 つ | **決定的。** `f32` に対して「有限のみ」と「NaN/Inf を含む敵性値」の 2 つの生成器が要る。quickcheck では newtype を 2 つ作る必要があり、`Point` / `TempoMap` / `Document` へ波及して型が爆発する。 |
| 制約 | Strategy が制約を知っており、違反値を生成も縮小もしない | 生成後に棄却するしかない | `Edit` は「その `Document` に適用可能」でなければ意味がない。棄却方式では有効率が落ちて実質テストにならない。 |
| 縮小 | 中間状態を保持した豊かな縮小 | 出力値からの無状態縮小 | 10,000 編集列の最小反例を出すのに必要。 |
| 速度 | 複合値の生成が最大 1 桁遅い | 速い | 許容。PR 層は 256 ケース、nightly 層は 65,536 ケースに `PROPTEST_CASES` で切り替える。 |
| 失敗種の永続化 | `proptest-regressions/*.txt` を自動生成 | なし | **コミットする。** 一度見つかった反例は永久に回帰テストになる。 |

#### 3.4.2 生成器

```rust
// crates/drill-testkit/src/strategy.rs   (feature = "proptest")
use proptest::prelude::*;
use drill_core::{Document, Point, tempo::TempoMap};

/// Finite values only, uniformly over `range`. The default for geometry.
pub fn finite_f32(range: std::ops::RangeInclusive<f32>) -> impl Strategy<Value = f32> {
    range
}

/// Values that any public API must survive without panicking. Weighted so that
/// the pathological cases are actually hit: a uniform f32 almost never produces
/// a NaN, so an unweighted generator would never test the guard clauses.
pub fn hostile_f32() -> impl Strategy<Value = f32> {
    prop_oneof![
        60 => (-1.0e4f32..1.0e4f32),
        6  => Just(f32::NAN),
        6  => Just(f32::INFINITY),
        6  => Just(f32::NEG_INFINITY),
        6  => Just(0.0f32),
        6  => Just(-0.0f32),
        4  => Just(f32::MAX),
        4  => Just(f32::MIN_POSITIVE),
        2  => Just(f32::EPSILON),
    ]
}

pub fn point() -> impl Strategy<Value = Point>;
pub fn hostile_point() -> impl Strategy<Value = Point>;

/// 0..=max_changes tempo changes, counts in 0..=2048, bpm in 20..=400.
pub fn tempo_map(max_changes: usize) -> impl Strategy<Value = TempoMap>;

/// A structurally valid document: `Document::validate` returns Ok.
pub fn document(
    performers: std::ops::Range<usize>,
    sets: std::ops::Range<usize>,
) -> impl Strategy<Value = Document>;

/// An `Edit` (see 10) that is applicable to `doc`. Generated against a concrete
/// document so that set ids, performer ids and indices always exist.
pub fn edit_for(doc: &Document) -> impl Strategy<Value = drill_core::Edit>;

/// A sequence of edits, each generated against the document state that the
/// previous edits produce.
pub fn edit_sequence(doc: Document, len: std::ops::Range<usize>)
    -> impl Strategy<Value = (Document, Vec<drill_core::Edit>)>;
```

`edit_sequence` が「直前の状態に対して有効な `Edit`」を生成するのが要点。独立に生成して棄却する方式だと、セット削除の後にそのセットを参照する編集が大量に無効化され、有効率が数 % に落ちる。

#### 3.4.3 性質一覧

| # | 性質 | 許容 | ケース数（PR / nightly） |
|---|---|---|---|
| **P-01** | `count_at(seconds_at(c)) ≈ c` | `TEMPO` | 256 / 65,536 |
| **P-02** | `seconds_at(count_at(s)) ≈ s` | `TEMPO` | 256 / 65,536 |
| **P-03** | `seconds_at` は単調非減少: `c1 <= c2 → seconds_at(c1) <= seconds_at(c2)` | **完全一致**（順序のみ） | 256 / 65,536 |
| **P-04** | `count_at` は単調非減少 | 完全一致 | 256 / 65,536 |
| **P-05** | `TempoMap` の全 API が `hostile_f32` に対してパニックせず、有限値を返す | `is_finite()` | 512 / 131,072 |
| **P-06** | `Edit` 可逆: `let inv = e.apply(&mut d)?; inv.apply(&mut d)?;` で `document_digest` が元に戻る | 完全一致 | 256 / 16,384 |
| **P-07** | `Edit` 二重反転: `inv.apply` の返す逆操作が、元の `e` と同じ効果を持つ | 完全一致 | 256 / 16,384 |
| **P-08** | 編集列: 任意長 `n` の適用後、逆順に全 Undo で初期状態に一致 | 完全一致 | 128 / 4,096 |
| **P-09** | `Edit` 適用後も `validate().is_ok()`、`PerformerId` は一意、全セットの `positions.len() == performers.len()` | — | 256 / 16,384 |
| **P-10** | `GridConfig::snap` は冪等: `snap(snap(p)) == snap(p)` | 完全一致 | 256 / 65,536 |
| **P-11** | 単位往復: `Unit` 変換の往復が元値に戻る | `GEOMETRY` | 256 / 65,536 |
| **P-12** | `camera::field_to_world(p, h)` は x/y を保存し、`Camera::overhead` の射影が field 座標の順序を保存する（単調） | `GEOMETRY` | 256 / 16,384 |
| **P-13** | `pathing::optimal_assignment(from, to)` は順列である（`0..n` が各 1 回） | 完全一致 | 256 / 16,384 |
| **P-14** | `assignment_cost(from, to, optimal) <= assignment_cost(from, to, identity)` | `GEOMETRY` | 256 / 16,384 |
| **P-15** | **置換不変性**: `from` と `to` を同一の置換 σ で並べ替えても、`optimal_assignment` の総コストが変わらない | `GEOMETRY` | 256 / 16,384 |
| **P-16** | JSON 往復: `from_json(to_json(d))` の `document_digest` が一致 | 完全一致 | 256 / 16,384 |
| **P-17** | `playback::advance` の結果 count は常に `[range.start, range.end]` に入り、有限 | 完全一致 | 512 / 131,072 |
| **P-18** | 任意のバイト列に対し `Document::from_json` はパニックしない（`Err` でよい） | — | 1,024 / 262,144 |
| **P-19** | `svg::xml_escape` / `html_escape` 後の文字列に生の `<` `&` が残らない（任意 Unicode 入力） | 完全一致 | 512 / 65,536 |

P-15（置換不変性）は「割り当ての置換性」の形式化である。`optimal_assignment` が入力順に依存すると、演者を並べ替えただけでドリルの経路が変わる。これは設計上あってはならない。

#### 3.4.4 P-01 の許容値の導出（なぜ `TEMPO` なのか）

`seconds_at` は区間ごとに `span * 60.0 / bpm` を f32 で累加する（`tempo.rs:135`）。f32 の相対丸め誤差は `2^-24 ≈ 5.96e-8`。加算 1 回あたり最悪でその 1 単位、区間数 `k` に対して累積は最悪 `k * 2^-24`。設計上の上限は 256 セット（00-conventions.md）なので `k <= 256`、累積相対誤差の最悪値は `256 * 5.96e-8 ≈ 1.5e-5`。往復（`seconds_at` → `count_at`）で 2 倍して `3e-5`。`rel = 1e-4` はこれに 3 倍強の余裕を持つ。

`abs = 1e-3` は count が 0 近傍のときの保険。count が 0 に近いと相対比較は無意味に厳しくなる。1e-3 count は 120BPM で 0.5ms であり、可聴・可視の閾値をはるかに下回る。

**この導出をテストのコメントに書く。** 数値だけ書かれた許容値は、後で誰かが「落ちるから緩めた」と区別できない。

---

### 3.5 決定論テスト

#### 3.5.1 決定論を 3 段に分けて定義する

「決定論」は 1 つの性質ではない。要求水準が違うものを混ぜると、クロスプラットフォームで落ちるテストを緩めた結果、同一プロセスの決定論まで守られなくなる。

| 段 | 定義 | 要求 | 破れる原因 |
|---|---|---|---|
| **D-A: 実行内** | 同一プロセス内で同じ関数を 2 回呼ぶと同じ結果 | **ビット完全一致** | 可変グローバル状態、キャッシュ、アドレス依存 |
| **D-B: 実行間・同一プラットフォーム** | 別プロセスで実行しても同じ結果 | **ビット完全一致** | `HashMap` の反復順（`RandomState` はプロセスごとに乱数化される）、ポインタ値の出力、時刻・環境変数の混入 |
| **D-C: プラットフォーム間** | Windows / macOS / Linux で同じ結果 | **ε 一致（完全一致は要求しない）** | `sin`/`cos`/`tan`/`exp`/`ln`/`powf` は IEEE-754 が正確丸めを規定しておらず、libm 実装ごとに最終 1 ulp が異なる |

D-C で完全一致を要求しない判断が本設計の要点である。`Document::demo`（`lib.rs:272` 付近の `angle.cos()` / `angle.sin()`）、`shapes::circle`、`shapes::spiral`、`editing::rotate`、`camera::project` は全て超越関数を通る。これらの出力を 3 OS でビット比較すると恒常的に落ち、テストが無視されるようになる。

代わりに:

- **ゴールデン（文字列・ダイジェスト）の厳密比較は Linux の 1 ジョブでのみ実行する**（`ci.yml` の `golden` ジョブ）。
- **Windows / macOS では ε 比較版を実行する**（`determinism_cross_platform.rs`）。同じフィクスチャから同じ量を計算し、Linux ジョブがコミットした参照値と `GEOMETRY` 許容で照合する。参照値は数値の表として `fixtures/reference_values.json` に置く（人が読める規模に絞る: 各 API について 8 点）。

#### 3.5.2 ε の切り分け表（どこを完全一致にするか）

| 対象 | 比較 | 根拠 |
|---|---|---|
| 整数・ID・要素数・順序・enum・文字列 | **完全一致** | 浮動小数を含まない。差があれば必ずバグ |
| `+ - * /` と `sqrt` のみで得た f32（線形補間、距離、重心、面積） | **完全一致**（D-A/D-B）/ **1 ulp**（D-C） | IEEE-754 754-2019 がこれらの正確丸めを規定している。x86-64 は SSE2 で x87 の余剰精度が無い |
| 超越関数を経た f32（円・螺旋・回転・射影） | `Tolerance::GEOMETRY` (`rel 1e-5`) | libm 差 |
| テンポ積分 | `Tolerance::TEMPO` (`rel 1e-4`, `abs 1e-3`) | §3.4.4 |
| 出力文字列（SVG / CSV / HTML / カウントシート） | **完全一致**（Linux ゴールデンのみ） | 上記を丸めた表現。丸め幅より libm 差が小さければプラットフォーム差は消えるが、それに依存しない |
| RGBA フレーム | **完全一致**（Linux）/ 最大チャネル差 ≤ 2 かつ差分画素率 ≤ 0.1%（他） | アンチエイリアスの丸め |
| 再生カウント列 | **完全一致**（D-A/D-B）/ `TEMPO`（D-C） | `advance` は四則演算のみ（`tempo.rs`・`playback.rs` に超越関数なし） |

#### 3.5.3 具体テスト

```rust
// crates/drill-conformance/tests/determinism.rs
use drill_testkit::{digest::{self, Digest}, fixture};

/// Frame time comes from a rational, never from accumulation. Accumulating
/// `+= 1.0/60.0` drifts and makes the sequence depend on the frame count so far.
fn simulate_counts(doc: &drill_core::Document, fps: u32, frames: u32) -> Vec<f32> {
    let range = drill_core::playback::PlaybackRange::new(
        0.0,
        doc.timeline_counts() as f32,
        doc.timeline_counts() as f32,
    );
    let mut out = Vec::with_capacity(frames as usize);
    let mut count = range.start;
    for frame in 0..frames {
        let seconds = f64::from(frame) / f64::from(fps);
        let previous = f64::from(frame.saturating_sub(1)) / f64::from(fps);
        let elapsed = (seconds - previous) as f32;
        count = drill_core::playback::advance(count, elapsed, 1.0, range, true, &doc.tempo).count();
        out.push(count);
    }
    out
}

#[test]
fn d_a_playback_is_deterministic_within_a_process() {
    let doc = fixture::medium();
    let a = simulate_counts(&doc, 60, 3_600);
    let b = simulate_counts(&doc, 60, 3_600);
    assert_eq!(a, b, "same input produced different playback counts");
}

#[test]
fn d_b_playback_digest_is_pinned() {
    let doc = fixture::medium();
    let counts = simulate_counts(&doc, 60, 3_600);
    let mut d = Digest::default();
    for c in &counts {
        d = d.f32(*c);
    }
    // Any change to tempo integration, looping or rounding shows up here.
    insta::assert_snapshot!(d.hex());
}

#[test]
fn d_b_document_generation_is_pinned() {
    // Guards the fixture generators themselves: if `baseline()` changes, every
    // other golden changes with it and we must know that it was intentional.
    insta::assert_snapshot!("tiny", digest::document_digest(&fixture::tiny()).hex());
    insta::assert_snapshot!("small", digest::document_digest(&fixture::small()).hex());
    insta::assert_snapshot!("medium", digest::document_digest(&fixture::medium()).hex());
    insta::assert_snapshot!("baseline", digest::document_digest(&fixture::baseline()).hex());
}
```

`simulate_counts` が**フレーム時刻を有理数から作り直している**点が重要。`count += delta` の累加は、同じ入力でもフレーム分割の仕方で結果が変わる。00-conventions.md の不変条件 4「浮動小数の累加で時間を進めない」をテスト側でも守る。

#### 3.5.4 D-B を守るための実装側規約

`HashMap` / `HashSet` の反復順は `RandomState` によりプロセスごとに変わる。これが出力に漏れると D-B が壊れる。

> **規約: 出力（DisplayList・SVG・CSV・カウントシート・解析結果の並び）に `HashMap` / `HashSet` の反復順を反映させない。順序が意味を持つコレクションは `Vec` または `BTreeMap` / `BTreeSet` を使う。**

これは lint で強制できる。`clippy::iter_over_hash_type`（restriction グループ）は `for` ループで `HashMap` / `HashSet` およびその `keys()` / `values()` / `iter()` を回すことを禁止する。`drill-core` / `drill-render` / `drill-export` で deny する（§3.10.4）。

---

### 3.6 ストレステスト

| # | 内容 | PR 層の規模 | nightly 層の規模 | 判定 |
|---|---|---|---|---|
| **S-01** | ランダム `Edit` → 全 Undo → 初期状態一致 | `medium()` × 10,000 編集 | `baseline()` × 10,000 編集 | `document_digest` 完全一致 |
| **S-02** | 同上 + 途中不変条件検査 | 1,000 手ごと | 100 手ごと | `validate().is_ok()`、ID 一意、`positions.len()` 整合 |
| **S-03** | Undo/Redo をランダムに交ぜる（10,000 手のうち 30% が undo/redo） | `medium()` | `baseline()` | 任意時点で `validate().is_ok()`、最終全 Undo で一致 |
| **S-04** | 上限規模ドキュメント | — | `maximum()`（4,000 人 / 256 セット） | 生成 / `validate` / JSON 往復 / `positions_at_count` / `count_sheet` / CSV が完走。JSON 往復 < 10s、常駐 < 1.5GB |
| **S-05** | 2 時間再生相当のメモリ推移 | 36,000 フレーム（10 分相当） | 432,000 フレーム（2 時間 @60fps） | ウォームアップ後、生存確保バイト数の増加 **0**、区間内の確保回数 **0** |
| **S-06** | 長時間ファジング | — | 各ターゲット 10 分 | クラッシュ 0 |

#### S-01 の具体

```rust
// crates/drill-conformance/tests/stress_edit.rs
use drill_testkit::{digest::document_digest, fixture, rng::Rng};

#[test]
fn s01_ten_thousand_edits_undo_to_the_initial_state() {
    let mut doc = if cfg!(feature = "heavy") { fixture::baseline() } else { fixture::medium() };
    let initial = document_digest(&doc);
    let mut rng = Rng::new(0x5EED_0001);
    let mut undo_stack: Vec<drill_core::Edit> = Vec::with_capacity(10_000);

    for step in 0..10_000u32 {
        let edit = drill_testkit::random_edit(&doc, &mut rng);
        let inverse = edit.apply(&mut doc).expect("generated edit must apply");
        undo_stack.push(inverse);
        if step % 1_000 == 0 {
            doc.validate().expect("invariants must hold mid-stream");
        }
    }
    while let Some(inverse) = undo_stack.pop() {
        inverse.apply(&mut doc).expect("inverse must apply");
    }
    assert_eq!(document_digest(&doc), initial, "10,000 edits did not fully undo");
}
```

`random_edit` は `drill-testkit` 側にあり、proptest とは独立（`proptest` feature を有効にしないビルドでも動く）。property テスト P-08 と役割が違う: P-08 は**最小反例を出す**ため、S-01 は**規模で殴る**ため。両方要る。

#### S-05 の測り方（RSS を使わない理由）

「常駐メモリの継続増加なし」を RSS で測ると、(a) 取得 API がプラットフォーム依存（Windows は `GetProcessMemoryInfo`、Linux は `/proc/self/statm`、macOS は `task_info`）で `drill-testkit` に OS 依存が入り、(b) アロケータのフラグメンテーションと OS のページ回収でノイズが乗り、閾値を緩めざるを得ず、(c) 結局リークを見逃す。

代わりに **アロケータの生存バイト数（alloc − dealloc）**を見る。クロスプラットフォームで、リークに対して RSS より厳密である（フラグメンテーションは見えないが、それは「継続増加」の主因ではない）。フラグメンテーション由来の RSS 増加は L4 のリリース前手動計測で別途確認する。

```rust
// crates/drill-conformance/tests/stress_memory.rs
#[global_allocator]
static ALLOC: drill_testkit::alloc::CountingAlloc = drill_testkit::alloc::CountingAlloc;

#[test]
fn s05_two_hours_of_playback_does_not_grow_the_heap() {
    let doc = drill_testkit::fixture::baseline();
    let mut scratch = FrameScratch::default();
    let mut list = drill_render::DisplayList::default();

    for frame in 0..600u32 {                              // warm-up: grow every pool
        render_frame(&doc, frame, &mut scratch, &mut list);
    }
    let frames: u32 = if cfg!(feature = "heavy") { 432_000 } else { 36_000 };
    let (_, stats) = drill_testkit::alloc::measure(|| {
        for frame in 600..600 + frames {
            render_frame(&doc, frame, &mut scratch, &mut list);
        }
    });
    assert_eq!(stats.allocs, 0, "playback allocated during steady state: {stats:?}");
    assert_eq!(stats.live_bytes, 0, "playback leaked {} bytes", stats.live_bytes);
}
```

実行時間の見積り: 1 フレーム ≈ 10µs（補間 0.15µs + DisplayList 構築の見積り）とすると 432,000 フレームで約 4.3 秒。nightly でも十分軽い。

---

### 3.7 確保ゼロの検証

#### 3.7.1 設計方針

§2.4 の 3 つの穴を全て塞ぐには、**ポインタ比較をやめて、区間内の確保「回数」を数える**しかない。カスタム `GlobalAlloc` にスレッドローカルのカウンタを持たせる。

外部クレートを使わない判断:

- **`allocation-counter` 0.8.1** — 最終リリース 2023-09-21。3 年近く更新が無い。`GlobalAlloc` の契約自体は安定しているので動くだろうが、停滞した依存を品質基盤の中核に置きたくない。
- **`assert_no_alloc`** — 既定で `abort` する。テストの失敗メッセージに「どの区間で何回確保したか」を出せず、失敗の診断に使えない。
- **自前** — 実装は 120 行程度。`GlobalAlloc` は `core` の安定トレイトで、10 年変わっていない。保守負債が小さい。かつ `drill-testkit` に置けば外部依存ゼロで済み、§3.1 の原則と整合する。

**自前を採用する。**

#### 3.7.2 実装

```rust
// crates/drill-testkit/src/alloc.rs
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

thread_local! {
    // `const { ... }` initialisers keep these as plain TLS slots with no lazy
    // initialisation and no destructor, so reading them never allocates and is
    // safe even while a thread is being torn down.
    static DEPTH:  Cell<u32> = const { Cell::new(0) };
    static ALLOCS: Cell<u64> = const { Cell::new(0) };
    static FREES:  Cell<u64> = const { Cell::new(0) };
    static LIVE:   Cell<i64> = const { Cell::new(0) };
    static PEAK:   Cell<i64> = const { Cell::new(0) };
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AllocStats {
    /// Number of allocation calls. A `realloc` counts as one allocation even if
    /// the allocator grows the block in place: the caller cannot rely on that.
    pub allocs: u64,
    pub frees: u64,
    /// Bytes allocated minus bytes freed inside the region. Non-zero means the
    /// region kept something alive; negative means it freed pre-existing memory.
    pub live_bytes: i64,
    pub peak_bytes: i64,
}

pub struct CountingAlloc;

#[inline]
fn counting() -> bool {
    DEPTH.with(|d| d.get() != 0)
}

#[inline]
fn on_alloc(size: usize) {
    if !counting() {
        return;
    }
    ALLOCS.with(|c| c.set(c.get().wrapping_add(1)));
    let delta = size as i64;
    LIVE.with(|l| {
        let now = l.get() + delta;
        l.set(now);
        PEAK.with(|p| {
            if now > p.get() {
                p.set(now);
            }
        });
    });
}

#[inline]
fn on_dealloc(size: usize) {
    if !counting() {
        return;
    }
    FREES.with(|c| c.set(c.get().wrapping_add(1)));
    LIVE.with(|l| l.set(l.get() - size as i64));
}

unsafe impl GlobalAlloc for CountingAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        on_alloc(layout.size());
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        on_alloc(layout.size());
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        on_dealloc(layout.size());
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        on_dealloc(layout.size());
        on_alloc(new_size);
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[derive(Clone, Copy)]
struct Snapshot {
    allocs: u64,
    frees: u64,
    live: i64,
    peak: i64,
}

fn snapshot() -> Snapshot {
    Snapshot {
        allocs: ALLOCS.with(Cell::get),
        frees: FREES.with(Cell::get),
        live: LIVE.with(Cell::get),
        peak: PEAK.with(Cell::get),
    }
}

/// Restores the depth even if `body` unwinds, so one failing test cannot leave
/// accounting permanently enabled for the rest of the thread.
struct DepthGuard;

impl Drop for DepthGuard {
    fn drop(&mut self) {
        DEPTH.with(|d| d.set(d.get().saturating_sub(1)));
    }
}

/// Runs `body` with allocation accounting enabled on the current thread and
/// returns the delta. Nested regions are supported; an inner region's counts are
/// also visible to the outer one.
///
/// Only allocations made on *this* thread are observed. Do not spawn threads
/// inside `body` and expect their allocations to appear.
pub fn measure<R>(body: impl FnOnce() -> R) -> (R, AllocStats) {
    let before = snapshot();
    DEPTH.with(|d| d.set(d.get() + 1));
    let guard = DepthGuard;
    let out = body();
    drop(guard);
    let after = snapshot();
    let stats = AllocStats {
        allocs: after.allocs - before.allocs,
        frees: after.frees - before.frees,
        live_bytes: after.live - before.live,
        peak_bytes: after.peak - before.live,
    };
    (out, stats)
}

#[track_caller]
pub fn assert_no_alloc<R>(what: &str, body: impl FnOnce() -> R) -> R {
    let (out, stats) = measure(body);
    assert_eq!(
        stats.allocs, 0,
        "`{what}` allocated {} time(s), {} peak bytes",
        stats.allocs, stats.peak_bytes
    );
    out
}
```

実装上の要点:

- **`const { Cell::new(0) }` 形式の TLS** を使う。通常の `thread_local!` は遅延初期化のために内部で確保しうるため、アロケータの中から触ると無限再帰する。`const` 初期化かつ `Drop` を持たない型なら、単純な TLS スロットになり確保も破棄も起きない。
- **スレッドローカルなので `cargo test` の並列実行で他テストの確保が混ざらない。** さらに `cargo nextest` はテストごとにプロセスを分けるため、グローバルアロケータの干渉が構造的に無い。
- **`#[global_allocator]` はプロセス単位**なので、宣言は `drill-conformance/tests/alloc_zero.rs` と `stress_memory.rs` の**先頭にだけ**置く。他のテストバイナリには入れないので、通常テストの速度は落ちない。
- **`realloc` は「1 解放 + 1 確保」として数える。** その場拡張できたかどうかは呼び出し側から保証されないので、保守的に数える。これが §2.4 の穴 1（`out.reserve` の欠陥）を検出できる理由である。

#### 3.7.3 確保ゼロを要求する区間

| # | 区間 | 前提となる設計文書 |
|---|---|---|
| **A-01** | `Document::positions_at_count`（`out` の容量が十分な状態で 1,024 回） | 11 |
| **A-02** | `scan_transition`（`ScanScratch` ウォームアップ後、1,000 人 × 16 カウント） | 13 |
| **A-03** | `drill_render::build`（`DisplayList` 再利用、1,000 人） | 20 |
| **A-04** | `raster::draw`（RGBA バッファ再利用、1 フレーム） | 31 |
| **A-05** | `playback::advance`（現行 API。既に確保しないはずだが未検証） | — |
| **A-06** | `TempoMap::seconds_at` / `count_at` / `bpm_at` | — |
| **A-07** | 音声出力コールバック相当（`drill-audio` の `fill_buffer`） | 30 |
| **A-08** | フレーム全経路（A-01 + A-03 + A-04 の連鎖、1,024 フレーム） | 20 / 31 |

```rust
// crates/drill-conformance/tests/alloc_zero.rs
#[global_allocator]
static ALLOC: drill_testkit::alloc::CountingAlloc = drill_testkit::alloc::CountingAlloc;

use drill_testkit::alloc::{assert_no_alloc, measure};

#[test]
fn a01_interpolation_allocates_nothing_after_warmup() {
    let doc = drill_testkit::fixture::baseline();
    let mut out = Vec::new();
    doc.positions_at_count(0, 0.0, &mut out);          // warm-up grows `out`
    assert_eq!(out.len(), 1_000);

    assert_no_alloc("positions_at_count x1024", || {
        for step in 0..1_024u32 {
            doc.positions_at_count(0, f32::from(step as u16) / 64.0, &mut out);
        }
    });
}

#[test]
fn meta_the_counter_actually_detects_an_allocation() {
    let (_, stats) = measure(|| Vec::<u8>::with_capacity(64));
    assert_eq!(stats.allocs, 1);
    assert!(stats.peak_bytes >= 64);
}

#[test]
#[should_panic(expected = "allocated 1 time(s)")]
fn meta_assert_no_alloc_fails_on_an_allocation() {
    assert_no_alloc("deliberate", || Vec::<u8>::with_capacity(8));
}
```

`meta_*` の 2 件が無いと、カウンタが壊れて常に 0 を返すようになったとき、確保ゼロテストが全て緑のまま無意味になる。**測定器の自己テストは必須**である。

#### 3.7.4 既存テストの扱い

`lib.rs:485` の `interpolation_reuses_output_allocation` は**残す**。`drill-core` の無依存テストとして、`out` を縮めない性質を守る役目がある。ただし「確保ゼロを保証している」という位置づけは A-01 へ移す。既存テストの doc comment にその旨を書く（実装タスク T-13）。

---

### 3.8 ベンチ

#### 3.8.1 3 種類のハーネスを役割で分ける

| ハーネス | 版 | 置き場 | 役割 | CI での判定 |
|---|---|---|---|---|
| **自前** (`harness = false`) | — | `crates/drill-core/benches/core_performance.rs` | **絶対予算ゲート**。依存ゼロで誰でも再現できる | ベースラインの **3.0 倍**超で失敗 |
| **criterion** | 0.8.2 | `crates/drill-bench/benches/*.rs` | 相対退行の観測・プロファイル・分布 | **落とさない**（記録のみ） |
| **gungraun**（旧 iai-callgrind） | 0.19.4 | `crates/drill-bench/benches/instructions.rs` | **CI で落とせる**退行検出 | 命令数が基準比 **+5%** で失敗（Linux のみ） |

#### 3.8.2 criterion を導入する（判断と根拠）

導入する。ただし `drill-bench` にのみ。

現行の自前ハーネス（`Instant::now()` で 1 回計測）は「一発計測」で、ウォームアップも外れ値除去も信頼区間も無い。同じマシンで 2 回走らせただけで数十 % ぶれる。退行を見つける道具としては使えない。criterion 0.8.2（2026-02-04）は、ウォームアップ・反復回数の自動決定・外れ値検出・`--save-baseline` / `--baseline` による比較を標準で持つ。

同時に、**現行の自前ベンチは削除しない**。理由:

- `PRODUCT_QUALITY.md` が `cargo bench -p drill-core --bench core_performance` を再計測コマンドとして明記している。壊すと文書が嘘になる。
- 依存ゼロなので、CI が無い環境でも、Rust があれば誰でも 1 コマンドで再現できる。
- criterion が答える問い（「前回より遅くなったか」）と、自前が答える問い（「絶対予算に収まっているか」）は別物である。

自前ベンチには**閾値 assert を追加する**（現状は `println!` のみ）。マシン差を吸収するため 3.0 倍と緩く取る。これは「桁で壊れた」を捕まえるための粗いゲートであり、細かい退行は命令数ベンチが受け持つ。

```rust
// crates/drill-core/benches/core_performance.rs （追加する構造の骨子）

/// Development machine baseline recorded 2026-08-09. See PRODUCT_QUALITY.md.
/// The gate is deliberately loose: CI runners are 2-3x slower than the machine
/// this was measured on, and this bench only has to catch order-of-magnitude
/// regressions. Fine-grained regressions are caught by the instruction-count
/// bench in drill-bench.
const BASELINE_INTERPOLATION_MS: f64 = 9.24;
const BASELINE_SERIALIZATION_MS: f64 = 28.75;
const SLACK: f64 = 3.0;

fn gate(name: &str, measured_ms: f64, baseline_ms: f64) {
    println!("{name}: {measured_ms:.2}ms (baseline {baseline_ms:.2}ms)");
    assert!(
        measured_ms <= baseline_ms * SLACK,
        "{name} regressed: {measured_ms:.2}ms > {:.2}ms",
        baseline_ms * SLACK
    );
}
```

#### 3.8.3 マシン差の吸収 — 命令数で落とす

これが CI 性能ゲートの最大の論点である。GitHub hosted runner の壁時計は同一コミットでも 20〜50% ぶれる。壁時計で CI を落とすと、偽陽性が続いてゲートが無効化される。

**結論: 壁時計で CI を落とさない。命令数で落とす。**

**gungraun 0.19.4**（2026-07-10。`iai-callgrind` から改名。最終の `iai-callgrind` は 0.16.1 / 2025-07-30）は Valgrind の Callgrind を使い、実行された命令数（Ir）を数える。命令数は同一バイナリ・同一入力なら実行環境にほぼ依存せず、±0.1% 未満で再現する。したがって **+5% の閾値で CI を落としても偽陽性が出ない**。

前提条件（守らないと命令数が再現しない）:

- **`RUSTFLAGS: -C target-cpu=x86-64-v2` を固定する。** ランナーの CPU 世代が変わると、`target-cpu=native` では自動ベクトル化の幅が変わり命令数が変わる。
- **Linux x86-64 のみ。** Valgrind は Windows で動かない。命令数ゲートは Linux 単独で十分（命令数の退行はアルゴリズムの退行であり、プラットフォーム固有ではない）。
- **基準値をリポジトリにコミットする。** `crates/drill-bench/baselines/instructions.json`。更新は PR の diff に現れる（§4 I-7）。

不採用: **CodSpeed**（`codspeed-criterion-compat`）。simulation モードで同等の効果を SaaS として得られるが、外部サービスへの依存と非公開リポジトリでの費用が発生する。gungraun で自前完結できるため現時点では不要。将来 OSS 化する場合に再検討する（§9）。

#### 3.8.4 測定対象

| ベンチ ID | 対象 | 規模 | 16.6ms 予算の想定取り分 |
|---|---|---|---|
| `bench/interp` | `positions_at_count` | 1,000 人 | 0.2ms |
| `bench/render_build` | `drill_render::build` | 1,000 人 | 2.0ms |
| `bench/scan` | `scan_transition` | 1,000 人 × 16 カウント | 2.0ms（毎フレームではない） |
| `bench/advance` | `playback::advance` | — | 0.01ms |
| `bench/tempo` | `seconds_at` / `count_at` | 256 区間 | 0.01ms |
| `bench/undo` | `Edit::apply`（移動 100 人） | — | 2.0ms（Undo/Redo 予算） |
| `bench/json_save` | `to_json` | 1,000 人 / 64 セット | 予算外（ワーカー） |
| `bench/json_load` | `from_json` + `validate` | 同上 | 予算外（ワーカー） |
| `bench/svg` | `field_svg` | 1,000 人 | 予算外（ワーカー） |
| `bench/assignment` | `optimal_assignment` | 1,000 人 | 予算外（ワーカー） |
| `bench/raster` | `raster::draw` 1 フレーム | 1080p / 1,000 人 | 予算外（ワーカー） |

「予算外（ワーカー）」の項目も測る。`PRODUCT_QUALITY.md` の「保存、自動保存、解析は描画スレッドを長時間停止させない」は**ワーカーへ退避したこと**（40 の設計）と**ワーカー側の絶対時間**の両方で保証される。後者はここで測る。

#### 3.8.5 2026-08-09 ベースラインの扱い

`PRODUCT_QUALITY.md` の 2 値（補間 9.24ms / JSON 28.75ms）は次のように位置づける。

1. **廃棄しない。** 開発機での既知の実測値であり、桁のゲート（§3.8.2 の 3.0 倍）の基準として使い続ける。
2. **フレーム予算の根拠には使わない。** §2.3 に述べたとおり、この補間ベンチの作業集合は 24KB で L1 に収まり、実フレームより楽観的である。16.6ms の配分（§3.8.4）は `bench/render_build` を含む実経路の計測で決め直す。
3. **測定条件を明文化して記録する。** 現状は数値だけで、CPU・OS・Rust 版・プロファイル・電源設定が残っていない。同じ数値を再測しても比較できない。以下を `crates/drill-bench/baselines/walltime.md` に記録する形式にする。

```
date, host_cpu, os, rustc, profile, target-cpu, bench_id, samples, mean_ms, stddev_ms
2026-08-09, (未記録), (未記録), (未記録), release, (未記録), interp_60k, 1, 9.24, -
```

「未記録」を明示的に残す。埋めたふりをしない。次回計測時にこの行を残したまま新しい行を足し、**2026-08-09 の行は「条件不明の参考値」として保持する**。

4. **命令数の基準値は別に取り直す。** 壁時計のベースラインから命令数は導出できない。§8 の T-16 で初回計測してコミットする。

---

### 3.9 ファジング

**cargo-fuzz 0.13.2**（2026-06-09）+ libFuzzer。`arbitrary 1.4.2` で構造化入力を作る。

#### 3.9.1 ターゲット

| ID | ターゲット | 入力 | 不変条件 | 前提 |
|---|---|---|---|---|
| **F-01** | `fuzz_json_load` | 任意バイト列 | `Document::from_json` がパニックしない。成功時は `validate().is_ok()` | 現状で可能 |
| **F-02** | `fuzz_migrate` | 任意バイト列 | `migrate` がパニックせず、成功時は v2 として往復する | A-7 |
| **F-03** | `fuzz_tempo_map` | `Arbitrary` な `TempoChange` 列 | 全 API が有限値を返す。パニックしない。`seconds_at` が単調 | 現状で可能 |
| **F-04** | `fuzz_escape` | 任意 UTF-8 | `xml_escape` / `html_escape` の結果に生の `<` `>` `&` `"` が残らない | 現状で可能 |
| **F-05** | `fuzz_project_container` | 任意バイト列（zip） | `.drillproj` 読込がパニックせず、パストラバーサル・zip bomb を拒否する | 41 |
| **F-06** | `fuzz_importer_*` | 任意バイト列 | インポータがパニックしない | 52 |
| **F-07** | `fuzz_audio_header` | 任意バイト列 | デコーダ前段のヘッダ検査がパニックしない | 30 |
| **F-08** | `fuzz_edit_sequence` | `Arbitrary` な `Edit` 列 | 適用可能な列は必ず全 Undo で初期状態へ戻る | A-1 |

```rust
// fuzz/fuzz_targets/fuzz_json_load.rs
#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    if let Ok(doc) = drill_core::Document::from_json(text) {
        // A document that loaded must be internally consistent, and must
        // survive a save/load round trip without changing.
        doc.validate().expect("loaded document failed validation");
        let json = doc.to_json().expect("valid document must serialise");
        let again = drill_core::Document::from_json(&json).expect("round trip must load");
        assert_eq!(again.performers.len(), doc.performers.len());
        assert_eq!(again.sets.len(), doc.sets.len());
    }
});
```

#### 3.9.2 `arbitrary` を `drill-core` に入れない

構造化ファジング（F-03 / F-08）には `Arbitrary` 実装が要るが、`drill-core` に `arbitrary` を dev-dependency として足すと §3.1 の原則を破る。**`drill-testkit` に `feature = "arbitrary"` を置き、newtype 経由で実装する。**

```rust
// crates/drill-testkit/src/arb.rs   (feature = "arbitrary")
use arbitrary::{Arbitrary, Result, Unstructured};

/// Newtype so that `drill-core` needs no `arbitrary` dependency.
pub struct ArbTempoMap(pub drill_core::tempo::TempoMap);

impl<'a> Arbitrary<'a> for ArbTempoMap {
    fn arbitrary(u: &mut Unstructured<'a>) -> Result<Self> {
        let n = u.int_in_range(0..=64usize)?;
        let mut map = drill_core::tempo::TempoMap::default();
        for _ in 0..n {
            let count = f32::from(u.int_in_range(0..=2_048u16)?);
            let bpm = f32::from(u.int_in_range(1..=1_000u16)?);
            map.set(count, bpm);
        }
        Ok(Self(map))
    }
}
```

#### 3.9.3 コーパスと成果の回収

- **コーパス（`fuzz/corpus/`）はコミットしない。** すぐに数万ファイルになりリポジトリを壊す。nightly の GitHub Actions cache に置き、次回の実行で再利用する（cache key は `fuzz-corpus-<target>-<週番号>`）。
- **クラッシュ入力（`fuzz/artifacts/`）は最小化してコミットする。** `cargo fuzz tmin` で縮小し、`crates/drill-conformance/fixtures/regressions/` へ移し、通常の回帰テストへ**昇格**させる。

```rust
// crates/drill-conformance/tests/regressions.rs
#[test]
fn every_known_bad_input_is_rejected_without_panicking() {
    insta::glob!("../fixtures/regressions/*.json", |path| {
        let raw = std::fs::read(path).expect("fixture must be readable");
        let text = String::from_utf8_lossy(&raw);
        // Must not panic. Ok or Err are both acceptable outcomes.
        let outcome = drill_core::Document::from_json(&text);
        insta::assert_snapshot!(match outcome {
            Ok(doc) => format!("ok performers={} sets={}", doc.performers.len(), doc.sets.len()),
            Err(e) => format!("err {e:?}"),
        });
    });
}
```

これでファジングの成果が**恒久的な高速テスト**に変わる。ファジング自体は nightly でしか回らないが、見つけた欠陥は毎 PR で守られる。

- **回帰フィクスチャは合成入力のみ。** 実ユーザーのファイルは、たとえ不具合再現用でも `fixtures/` に置かない（プライバシー）。再現に必要な構造だけを合成して置く。

#### 3.9.4 実行設定と 51 との連携

nightly: 各ターゲット `-max_total_time=600 -rss_limit_mb=2048 -max_len=1048576`。

`51-security.md` が定義する入力上限（サイズ・要素数・再帰深度）の定数について、本書は**それを破れないことの確認**を担う。

- 51 が公開する定数（例: `limits::MAX_PERFORMERS`、`limits::MAX_JSON_BYTES`、`limits::MAX_NESTING`）を `drill-conformance` から import し、**「上限ちょうど」「上限 +1」の表駆動テスト**を書く（B-01）。上限ちょうどは成功、上限 +1 は特定の `DrillError` variant で失敗する、を全定数について確認する。
- ファジングは「51 が想定しなかった経路」を探す側。両方要る。

---

### 3.10 CI

#### 3.10.1 ワークフロー構成

```
.github/workflows/
  ci.yml          push (master) と pull_request
  nightly.yml     schedule: "0 3 * * *" と workflow_dispatch
  release.yml     53 の担当。本書では扱わない。
```

#### 3.10.2 `ci.yml`

```yaml
name: CI

on:
  push:
    branches: [master, main]
  pull_request:

concurrency:
  group: ci-${{ github.ref }}
  cancel-in-progress: true

env:
  CARGO_TERM_COLOR: always
  CARGO_INCREMENTAL: 0
  RUSTFLAGS: "-D warnings"
  INSTA_UPDATE: "no"

jobs:
  lint:
    runs-on: ubuntu-26.04
    steps:
      - uses: actions/checkout@v6
      - uses: dtolnay/rust-toolchain@stable
        with:
          components: rustfmt, clippy
      - uses: Swatinem/rust-cache@v2
      - name: Install Linux GUI build dependencies
        run: |
          sudo apt-get update
          sudo apt-get install -y --no-install-recommends \
            libgtk-3-dev libxkbcommon-dev libxkbcommon-x11-dev libwayland-dev \
            libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev \
            libx11-dev libxrandr-dev libxi-dev libxcursor-dev libgl1-mesa-dev
      - run: cargo fmt --all --check
      - run: cargo clippy --workspace --all-targets --locked
      - name: drill-core must have no dev-dependencies
        run: |
          test "$(cargo metadata --format-version 1 --no-deps \
            | jq '[.packages[] | select(.name=="drill-core") | .dependencies[]
                  | select(.kind=="dev")] | length')" = "0"
      - name: drill-app must not depend on test crates
        run: |
          ! cargo tree -p drill-app --edges normal --prefix none \
            | grep -E '^(drill-testkit|drill-conformance|drill-bench|insta|proptest|criterion)'

  test:
    strategy:
      fail-fast: false
      matrix:
        os: [ubuntu-26.04, windows-2025]
    runs-on: ${{ matrix.os }}
    steps:
      - uses: actions/checkout@v6
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
      - uses: taiki-e/install-action@v2
        with:
          tool: cargo-nextest
      - name: Install Linux GUI build dependencies
        if: runner.os == 'Linux'
        run: |
          sudo apt-get update
          sudo apt-get install -y --no-install-recommends \
            libgtk-3-dev libxkbcommon-dev libxkbcommon-x11-dev libwayland-dev \
            libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev \
            libx11-dev libxrandr-dev libxi-dev libxcursor-dev libgl1-mesa-dev
      - run: cargo nextest run --workspace --locked --profile ci
        env:
          PROPTEST_CASES: 256
      - name: Stress layer with overflow checks
        run: cargo nextest run -p drill-conformance --locked --profile ci --cargo-profile stress -E 'test(/^s0/)'
      - name: No leftover snapshot drafts
        shell: bash
        run: |
          ! find . -name '*.snap.new' -print -quit | grep -q .
          git diff --exit-code

  golden:
    # Exact string comparison happens on one platform only: libm differences
    # make sin/cos-derived output differ by 1 ulp between operating systems.
    runs-on: ubuntu-26.04
    steps:
      - uses: actions/checkout@v6
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
      - uses: taiki-e/install-action@v2
        with:
          tool: cargo-nextest
      - run: cargo nextest run -p drill-conformance --locked --profile ci -E 'test(/^(g|d_)/)'

  deny:
    runs-on: ubuntu-26.04
    steps:
      - uses: actions/checkout@v6
      - uses: EmbarkStudios/cargo-deny-action@v2
        with:
          command: check all

  instructions:
    # Wall clock on hosted runners varies by 20-50%. Instruction counts vary by
    # less than 0.1%, so this is the only performance gate allowed to fail a PR.
    runs-on: ubuntu-26.04
    env:
      RUSTFLAGS: "-C target-cpu=x86-64-v2"
    steps:
      - uses: actions/checkout@v6
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
      - run: sudo apt-get update && sudo apt-get install -y valgrind
      - uses: taiki-e/install-action@v2
        with:
          tool: gungraun-runner
      - run: cargo bench -p drill-bench --bench instructions --locked
      - uses: actions/upload-artifact@v7
        if: always()
        with:
          name: instruction-counts
          path: target/gungraun/

  budget:
    runs-on: ubuntu-26.04
    steps:
      - uses: actions/checkout@v6
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
      - run: cargo bench -p drill-core --bench core_performance --locked
```

`ci.yml` は **macOS を走らせない**（§5 の課金分析）。macOS は nightly とリリース前に回す。

#### 3.10.3 `nightly.yml`（要点のみ）

- `test` の matrix に `macos-26` を追加。
- `PROPTEST_CASES: 65536`。
- `--features heavy` で S-01 / S-04 / S-05 を全量規模に切り替え。
- 全 criterion ベンチを実行し、`target/criterion` をアーティファクトに保存、`--save-baseline nightly-<date>`。
- cargo-fuzz を各ターゲット 10 分。コーパスを cache へ保存。
- `cargo llvm-cov nextest --workspace --json` でカバレッジを収集（§3.12）。
- `cargo deny check advisories`（新規 advisory の検出。日次で回す意味がある）。
- 失敗時は Issue を自動起票する。nightly は誰も見ないので、通知が無いと存在しないのと同じになる。

#### 3.10.4 toolchain と lint 設定

`rust-toolchain.toml`（新規）:

```toml
[toolchain]
channel = "1.98.0"     # 導入 PR で `rustc -V` の実測値に置換する
components = ["rustfmt", "clippy"]
```

版を固定する理由: clippy の新 lint が stable 更新で増え、`-D warnings` の CI が**コードを変えていないのに赤くなる**。更新は意図的な PR で行う（月次）。

`Cargo.toml`（ルート）への追加:

```toml
[workspace.lints.clippy]
# Panic-free paths. Tests and benches opt out via the cfg(test) gate in lib.rs.
unwrap_used = "deny"
expect_used = "deny"
panic = "deny"
indexing_slicing = "deny"
# Deterministic output: HashMap/HashSet iteration order is randomised per process.
iter_over_hash_type = "deny"

[profile.stress]
inherits = "release"
debug-assertions = true
overflow-checks = true
```

`profile.stress` が要点。ストレステストは最適化ビルドでないと現実的な時間で終わらないが、`release` は `overflow-checks` が off なので整数オーバーフローが黙ってラップする。00-conventions.md の安全性要件「整数オーバーフローを起こさない」を、最適化ビルドの速度を保ったまま検査するためのプロファイルである。

`crates/drill-core/src/lib.rs` 冒頭（実装タスク T-05）:

```rust
#![cfg_attr(not(test), deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
))]
```

`cfg_attr(not(test))` にするのは、テストコードで `unwrap` を禁止すると assertion が書けなくなるため。`indexing_slicing` の deny は既存コード（`lib.rs:385` の `self.sets[...]`）を落とすので、導入は段階的に行う（T-05 で `warn`、A-1 完了後に `deny` へ）。

#### 3.10.5 `deny.toml`

```toml
[graph]
all-features = true

[advisories]
version = 2
yanked = "deny"
ignore = []

[licenses]
version = 2
allow = [
  "MIT", "Apache-2.0", "Apache-2.0 WITH LLVM-exception",
  "BSD-2-Clause", "BSD-3-Clause", "ISC", "Zlib",
  "Unicode-3.0", "CC0-1.0", "MPL-2.0", "BSL-1.0",
]
confidence-threshold = 0.93

[bans]
multiple-versions = "warn"
wildcards = "deny"
deny = [
  # The shipped product must not link OpenSSL: it drags in a C build and a
  # separate CVE stream that we would have to track for every release.
  { name = "openssl-sys" },
]

[sources]
unknown-registry = "deny"
unknown-git = "deny"
allow-registry = ["https://github.com/rust-lang/crates.io-index"]
```

`MPL-2.0` を許可に入れているのはファイル単位コピーレフトで動的・静的リンクともに配布可能なため。**GPL / AGPL / LGPL は列挙しない**（allow 方式なので自動的に拒否される）。これは 53（製品化・販売）の前提であり、`deny` ジョブが製品ライセンス要件のゲートになる。

#### 3.10.6 nextest 設定

```toml
# .config/nextest.toml
[profile.ci]
retries = 0                      # A flaky test is a bug. Retrying hides it.
fail-fast = false
failure-output = "immediate-final"
status-level = "fail"
slow-timeout = { period = "60s", terminate-after = 4 }

[profile.ci.junit]
path = "junit.xml"

[[profile.ci.overrides]]
filter = 'test(/^s0/)'           # stress tests
slow-timeout = { period = "300s", terminate-after = 4 }

[[profile.ci.overrides]]
filter = 'test(/^p_/)'           # property tests
slow-timeout = { period = "180s", terminate-after = 4 }
```

`retries = 0` を明示する。CI の再試行は、テストが不安定であるという情報を消す。不安定なテストは削除するか直す。

nextest を使うもう 1 つの理由: **テストごとにプロセスを分ける**ため、`#[global_allocator]` を持つテストバイナリの計測が他テストに干渉しない。

---

### 3.11 UI スモークテスト

**`egui_kittest`**（0.36.1 / 2026-08-07。egui/eframe と版を揃える必要があるため、`eframe 0.35.0` を 0.36 系へ更新するか、`egui_kittest 0.35.x` を使う。§9 で保留）を使い、GPU 無しでヘッドレスに egui を回す。

前提: **`DESIGN_GAPS.md` C-1 の `main.rs` 分解が完了していること。** 現状 `DrillApp` は `main.rs` の中で `eframe::run_native` から構築される経路しか無く、テストから作れない。分解後に以下が成立する。

```rust
// crates/drill-app/tests/ui_smoke.rs
use drill_app::DrillApp;

#[test]
fn x01_survives_two_seconds_of_frames_without_panicking() {
    let mut harness = egui_kittest::Harness::new_state(
        |ctx, app: &mut DrillApp| app.ui(ctx),
        DrillApp::with_document(drill_testkit::fixture::small()),
    );
    for _ in 0..120 {
        harness.run();
    }
}

#[test]
fn x02_every_menu_command_is_reachable_by_name() {
    // PRODUCT_QUALITY.md: "全操作とショートカットはメニューバーから発見できる".
    // AccessKit exposes the accessible name of every widget, so the set of menu
    // item names can be compared against the command registry.
    let mut harness = egui_kittest::Harness::new_state(
        |ctx, app: &mut DrillApp| app.ui(ctx),
        DrillApp::with_document(drill_testkit::fixture::small()),
    );
    harness.run();
    let named: Vec<String> = harness.node_names_under("menubar");
    for command in drill_app::commands::ALL {
        assert!(
            named.iter().any(|n| n == command.title_en()),
            "command `{}` is not reachable from the menu bar",
            command.id()
        );
    }
}

#[test]
fn x03_warnings_are_not_colour_only() {
    // PRODUCT_QUALITY.md: "警告は色だけに依存せず文字と形でも示す".
    // Every warning widget must carry non-empty accessible text.
    ...
}
```

X-02 と X-03 は、`PRODUCT_QUALITY.md` の UX ゲートのうち**機械化できる部分**である。X-02 は「メニューバーから発見できる」を、コマンド登録簿とアクセシビリティツリーの名前集合の包含関係として検査する。X-03 は「色だけに依存しない」を、警告ウィジェットの accessible name が非空であることとして検査する。

機械化**できない**部分（初回起動から説明書なしで到達できる／100–200% スケーリングの見た目）は L4 の手動チェックリストに残す。43 の担当。

---

### 3.12 カバレッジ

#### 測る。ただし全体に数値目標を課さない。

**ツール**: `cargo-llvm-cov 0.8.7`（2026-05-13）。`cargo llvm-cov nextest` で nextest と統合できる。nightly のみ実行し、PR では出さない（遅く、差分が読めず、レビューのノイズになる）。

**全体に閾値を置かない理由**: カバレッジ率をゲートにすると、「テストの質」ではなく「実行された行数」を最適化するインセンティブが働く。assert の無いテストで数値は上がる。DrillForge で本当に守りたいのは「編集して壊れない」「壊れたファイルで落ちない」であって、行数ではない。

**例外として、パニック禁止経路にだけ閾値を置く。** ここは網羅が意味を持つ: 未実行の分岐 = 未検証のエラー処理 = 未知のクラッシュ経路である。

| 対象 | 指標 | 閾値 |
|---|---|---|
| `drill-core/src/lib.rs` の `from_json` / `validate` / `migrate` | region coverage | **90%** |
| 52 のインポータ全体 | region coverage | **90%** |
| 41 のプロジェクトコンテナ読込 | region coverage | **90%** |
| その他全て | — | 閾値なし（報告のみ） |

`--fail-under-regions` はクレート単位なので、ファイル単位のゲートは `cargo llvm-cov --json` の出力を小さなスクリプトで判定する。対象ファイルの一覧は `crates/drill-conformance/coverage_gate.toml` に列挙し、**追加は明示的な PR でのみ行う**（黙って対象から外れないようにする）。

カバレッジの本来の使い方は数値ではなく **HTML レポートを人が見て、未実行の分岐を探すこと**。nightly のアーティファクトとして 30 日保持する。

---

## 4. 不変条件

テストまたは CI ジョブで検証できる形で書く。括弧内が検証方法。

- **I-1** `drill-core` の `Cargo.toml` に `[dev-dependencies]` が存在しない。かつ `drill-app` の normal 依存グラフに `drill-testkit` / `drill-conformance` / `drill-bench` / `insta` / `proptest` / `criterion` が現れない。（`ci.yml` の `lint` ジョブ、`cargo metadata` / `cargo tree`）
- **I-2** `cargo test -p drill-core` が外部 dev-dependency 無しで完走し、10 秒以内に終わる。（L1、nextest の `slow-timeout`）
- **I-3** 文字列・バイト列の厳密ゴールデン比較は Linux の `golden` ジョブでのみ実行される。他 OS では ε 比較のみ。（ジョブ分割 + テスト名フィルタ）
- **I-4** CI がスナップショットを自動生成しない。`.snap.new` を含むコミットがマージされない。（`INSTA_UPDATE=no`、`find -name '*.snap.new'` の検査）
- **I-5** proptest の失敗種は `proptest-regressions/*.txt` にコミットされ、以後の全実行で再現される。（ファイルが git 管理下にあること、`.gitignore` に載せない）
- **I-6** 「確保ゼロ」を主張する全区間が `AllocStats::allocs == 0` を assert している。ポインタ比較で代用していない。（§3.7.3 の A-01〜A-08。`meta_*` 2 件が計測器の生存を保証）
- **I-7** 命令数の基準値がリポジトリにコミットされ、更新が PR の diff に現れる。（`crates/drill-bench/baselines/instructions.json`）
- **I-8** テキストファイルは LF で保存される。（`.gitattributes`）
- **I-9** 出力の順序に `HashMap` / `HashSet` の反復順が反映されない。（`clippy::iter_over_hash_type` を deny + D-B の digest 固定）
- **I-10** ストレス層は `overflow-checks = true` のプロファイルで実行される。（`profile.stress`、`ci.yml` の `test` ジョブ）
- **I-11** 決定論テストは「同一プロセス 2 回実行の完全一致」と「固定 digest への一致」の両方を持つ。片方だけにしない。（`determinism.rs` の D-A と D-B を対で書く）
- **I-12** 全ゴールデン対象に、意味テスト（構造検査）が最低 1 件併存する。（レビュー規約 + §3.3.5）
- **I-13** テストが実時計（`Instant::now` / `SystemTime::now`）に依存しない。ベンチを除く。（grep ベースの検査を `lint` ジョブに追加）
- **I-14** テストが乱数のシードを固定している。（`Rng::new(seed)` のみを使う。`proptest` は失敗種を永続化する）
- **I-15** `#[cfg(test)]` / `debug_assertions` 依存のコードが製品経路の挙動を変えない。（release ビルドでも conformance の主要テストが通ることを nightly で確認）
- **I-16** 回帰フィクスチャが合成入力のみで構成される。実ユーザーのファイルを含まない。（レビュー規約）

---

## 5. 性能

### 5.1 製品実行時の取り分: 0ms

本設計は製品バイナリに一切の実行時コストを加えない。テスト・ベンチ・計測用アロケータは全て `drill-testkit` / `drill-conformance` / `drill-bench` に閉じ、`drill-app` の依存グラフに現れない（I-1）。16.6ms 予算のうち本書の取り分は **0ms** である。

唯一の例外候補は `profile.stress` だが、これはテスト実行専用のプロファイルであり配布物には使わない。

### 5.2 テスト自体の実行時間予算

| 層 | 目標 | 内訳 |
|---|---|---|
| **L1** `cargo test -p drill-core` | **10 秒** | 単体 124 → 目標 300 件。1 件平均 30ms 以下 |
| **L2** PR 全体（並列後の実時間） | **15 分** | 下表 |
| **L3** nightly | **90 分** | proptest 65,536 ケース 25 分、ストレス全量 15 分、fuzz 8 ターゲット × 10 分 = 80 分（並列 4 で 20 分）、criterion 全ベンチ 15 分、カバレッジ 15 分 |

L2 のジョブ別内訳（並列実行、キャッシュヒット時）:

| ジョブ | runs-on | 実時間 | 課金倍率 | 課金分 |
|---|---|---:|---:|---:|
| `lint` | ubuntu | 4 分 | ×1 | 4 |
| `test` (ubuntu) | ubuntu | 8 分 | ×1 | 8 |
| `test` (windows) | windows | 10 分 | ×2 | 20 |
| `golden` | ubuntu | 4 分 | ×1 | 4 |
| `deny` | ubuntu | 2 分 | ×1 | 2 |
| `instructions` | ubuntu | 8 分 | ×1 | 8 |
| `budget` | ubuntu | 3 分 | ×1 | 3 |
| **合計** | | **実時間 10 分**（最長ジョブ） | | **49 分/PR** |

### 5.3 macOS を PR から外す判断

GitHub hosted runner の課金倍率は Linux ×1、Windows ×2、**macOS ×10**。`test` を 3 OS 化すると macOS だけで 8 分 × 10 = **80 分/PR** かかり、他の全ジョブ合計（49 分）を上回る。

一方 macOS 固有の退行リスクは、(a) libm 差 → ε 比較で吸収済み（§3.5.1）、(b) パス区切り・ファイルシステム → 41 の担当で、Windows でも検出できる、(c) GUI バックエンド → UI スモークで検出できるが、そもそも wgpu/Metal 固有の問題はヘッドレステストでは出ない。

したがって **macOS は nightly とリリース前（L4）に回す**。PR で落とす価値が課金に見合わない。

### 5.4 CI の実行時間が伸びたときの対処

15 分を超えたら、次の順で対処する。**自動的にテストを間引く仕組みは作らない**（何が測られなくなったか誰も知らないまま品質が落ちる）。

1. キャッシュのヒット率を確認する（`Swatinem/rust-cache` のログ）。
2. `PROPTEST_CASES` を PR 層で減らす（256 → 128）。nightly はそのまま。
3. ストレス層を PR から外し nightly へ移す。
4. 四半期レビューで層構成を見直す。

---

## 6. 失敗モードと安全性

### 6.1 ゴールデンが形骸化する

**症状**: 落ちたら `cargo insta accept` する習慣がつき、期待値が実装の写像になって何も守らなくなる。**これはゴールデンテスト最大の失敗モードである。**

対処:

- 意味テストを併存させる（§3.3.5、I-12）。ゴールデンが壊れても構造検査が残る。
- `cargo insta accept`（一括承認）を運用で禁止し、`cargo insta review`（1 件ずつ）のみを許す（§3.3.3）。
- 大規模出力は全文でなくダイジェストを固定する（G-10）。読まない全文が diff に出ると、レビューが「大きい diff は飛ばす」に退化する。
- `.snap` の変更を実装の変更と同じコミットに含める規約。

### 6.2 flaky テスト

| 原因 | 対処 |
|---|---|
| 実時計依存 | `playback::advance` が `elapsed_seconds` を引数で受ける現在の設計は正しい。製品ロジックに `Instant::now` を持ち込まない。テストでの使用も禁止（I-13） |
| 乱数 | `Rng::new(seed)` のみ。`rand` を入れない（I-14） |
| 並列実行での共有状態 | 確保カウンタはスレッドローカル。nextest はテストごとにプロセスを分ける |
| ファイルシステムの競合 | 一時ディレクトリは `CARGO_TARGET_TMPDIR` 下にテスト名で作る。`tempfile` を入れない |
| CI ランナーの負荷変動 | 壁時計で CI を落とさない（§3.8.3）。`retries = 0` で不安定を可視化する |
| プロセス間の HashMap 順序 | D-B の digest 固定が検出する。`clippy::iter_over_hash_type` が予防する |

### 6.3 テスト依存が攻撃面を増やす

`insta` / `proptest` / `criterion` / `gungraun` / `arbitrary` / `libfuzzer-sys` は全て dev-dependency で、配布バイナリに入らない（I-1）。ただし**ビルドマシンは侵害されうる**（build script や proc-macro は開発者のマシンで任意コードを実行する）。

対処:

- `cargo deny check advisories` を dev グラフにも適用する（`deny.toml` の `[graph] all-features = true`）。
- 全 CI コマンドに `--locked` を付ける。`Cargo.lock` の暗黙更新を禁止する。
- 依存追加は PR で `Cargo.lock` の diff がレビュー対象。
- `cargo vet` / `cargo-crev` による供給網監査は現時点では過剰と判断する。依存が 400 を超えるか、外部コントリビュータを受け入れる段階で再検討する（§9）。

### 6.4 ストレステストがランナー資源を超える

`maximum()`（4,000 人 / 256 セット）は座標だけで 4,000 × 256 × 8 バイト = 8.2MB、JSON 化すると数十 MB。GitHub hosted runner のメモリは 16GB（ubuntu-26.04）だが、`to_string_pretty` は文字列を丸ごとメモリに作る。

対処: **S-04 はメモリ上限を assert する側にする。** 1.5GB を超えたら失敗させ、設計を見直す合図とする。CI を通すために上限を上げない。fuzz は `-rss_limit_mb=2048` で制限する。

### 6.5 Valgrind が動かない / 命令数が再現しない

| 症状 | 原因 | 対処 |
|---|---|---|
| Valgrind が未対応命令で落ちる | ランナー CPU の AVX-512 等 | `RUSTFLAGS: -C target-cpu=x86-64-v2` で固定 |
| 命令数がコミット間で無関係に変わる | 依存の更新、rustc の更新、target-cpu の変化 | toolchain 固定（`rust-toolchain.toml`）+ `--locked` + target-cpu 固定 |
| Windows で命令数ゲートが無い | Valgrind が Windows 非対応 | Linux 単独で運用する。命令数の退行はアルゴリズムの退行でありプラットフォーム固有ではない |
| ゲートが恒常的に赤い | 閾値が厳しすぎる | +5% は経験的に安全な値。それでも赤いなら本当に退行している |

### 6.6 ファジングが本物の欠陥を見つけたとき

1. `cargo fuzz tmin <target> <artifact>` で最小化する。
2. 最小化した入力を `crates/drill-conformance/fixtures/regressions/` へ、**内容が分かる名前**で置く（例: `fuzz-0007-tempo-nan-in-count.json`）。
3. 回帰テスト（`regressions.rs`）が赤くなることを確認してから修正する。**赤くならない場合、その入力は再現していない。**
4. 修正し、緑になることを確認する。
5. 入力に実データ由来の情報が含まれていないことを確認する（I-16）。

### 6.7 CI が無い期間の扱い

CI 導入（T-18）より前に他の実装タスクを進めると、その間の退行が捕まらない。したがって **T-18（最小 CI）を最優先で入れる**。`fmt` + `clippy` + `cargo test --workspace` だけの 1 ジョブでよい。完全な設計を待たない。段階的に足す（§8 の依存関係）。

---

## 7. テスト計画

### 7.1 Release gates 対応表（`PRODUCT_QUALITY.md` 全項目）

この表が本書の主目的である。**空欄を作らない。**

#### Correctness

| ゲート | 保証するテスト | 層 | 置き場 |
|---|---|---|---|
| 全カウント、セット境界、可変BPM、部分ループで再生位置が決定論的である | **D-A/D-B**（同一入力 → 同一カウント列、digest 固定）、**P-01〜P-04**（テンポ往復・単調）、**P-17**（範囲外に出ない）、**A-05/A-06**（`advance`/`TempoMap` の確保ゼロ） | 決定論・property | `determinism.rs`, `prop_tempo.rs`, `alloc_zero.rs` |
| 保存ファイルはスキーマ検証され、未対応の将来形式や破損データを黙って開かない | **B-01**（51 の上限定数の境界表）、**G-14**（v1→v2 移行 digest）、**I-移行往復**（v1 読込 → v2 保存 → 再読込で演者数・座標・カウント一致）、**F-01/F-02**（JSON・移行ファジング）、**P-18**（任意バイト列で非パニック）、**回帰フィクスチャ**、**カバレッジ 90%**（`from_json`/`validate`/`migrate`） | 統合・fuzz・カバレッジ | `migration.rs`, `regressions.rs`, `fuzz/`, `coverage_gate.toml` |
| Undo/Redo後に演者数、ID、セット座標の不変条件が維持される | **P-06/P-07**（可逆・二重反転）、**P-08**（列の全 Undo）、**P-09**（適用後の不変条件）、**S-01/S-02/S-03**（10,000 編集ストレス）、**F-08** | property・ストレス | `prop_edit.rs`, `stress_edit.rs` |
| 座標表、ドリルブック、SVG/PDF出力は同じドキュメント座標を参照する | **G-01〜G-08**（同一フィクスチャから全出力をゴールデン化）、**I-座標一致**（CSV の座標値と `field_svg` の `<circle cx cy>` を逆変換した値が `GEOMETRY` 許容で一致することを直接 assert）、**G-09/G-10**（DisplayList を単一の中間表現とする） | ゴールデン・統合 | `golden_export.rs`, `coordinate_consistency.rs` |

「座標一致」は**ゴールデンだけでは保証されない**点に注意。ゴールデン 8 本が全て「同じ間違った座標」で固定されても緑になる。CSV と SVG の値を**直接突き合わせる**テストを別に置く。

#### Performance

| ゲート | 保証するテスト | 層 | 置き場 |
|---|---|---|---|
| 1,000人のアニメーション補間でフレーム内ヒープ再確保ゼロ | **A-01**（`positions_at_count` × 1,024、`allocs == 0`）、**A-08**（フレーム全経路）、**meta-01/02**（計測器の自己テスト） | ストレス | `alloc_zero.rs` |
| 1,000人、60fpsでUI入力から表示まで16.6ms以内を維持する | **`budget`**（自前ベンチ、ベースライン 3 倍ゲート）、**`instructions`**（命令数 +5% で失敗）、**`bench/*`**（criterion で分布を記録）、**§3.8.4 の予算配分表** | ベンチ | `core_performance.rs`, `drill-bench/` |
| 2時間連続再生で常駐メモリの継続増加がない | **S-05**（432,000 フレームで `live_bytes` 増加 0、`allocs == 0`） | ストレス（nightly 全量） | `stress_memory.rs` |
| 保存、自動保存、解析は描画スレッドを長時間停止させない | **I-ジョブ**（40 の `Job<T>` が UI スレッドを占有しないこと: `poll` が確保ゼロかつ即時復帰）、**`bench/json_save`/`bench/svg`/`bench/assignment`**（ワーカー側の絶対時間）、**X-01**（120 フレーム連続でフレーム時間が閾値を超えない） | 統合・ベンチ・UI | `jobs.rs`, `drill-bench/`, `ui_smoke.rs` |

#### Reliability

| ゲート | 保証するテスト | 層 | 置き場 |
|---|---|---|---|
| 上書き保存前バックアップ、自動保存、クラッシュ復旧候補を提供する | **I-原子的置換**（書き込み中に失敗させても元ファイルが無傷: 一時ファイルへの書き込み後に注入した失敗で rename を中断し、元ファイルの digest が不変であることを assert）、**I-自動保存**（間隔経過で保存され、候補が列挙される）、**I-復旧**（crash ファイルが復旧候補として検出される） | 統合 | `persistence.rs` |
| 音声・画像・外部ファイルが欠落してもドリル本体を開ける | **I-欠落アセット**（`.drillproj` から assets を削除して開き、`AssetState::Missing` になりつつ演者数・座標が読めること） | 統合 | `persistence.rs` |
| パニックはクラッシュレポートへ変換し、元ファイルを変更しない | **I-パニックフック**（`std::panic::catch_unwind` でフックの出力を検査し、元ファイルの digest が不変）、**F-01〜F-08**（そもそもパニックしないことをファジングで攻める）、**clippy deny**（`unwrap_used`/`expect_used`/`panic`/`indexing_slicing`）、**`profile.stress`**（整数オーバーフロー検出） | 統合・fuzz・lint | `panic_report.rs`, `fuzz/`, `ci.yml` |
| 10,000回の編集コマンドを含むストレステストを通す | **S-01/S-02/S-03** | ストレス | `stress_edit.rs` |

#### UX and accessibility

| ゲート | 保証するテスト | 層 | 置き場 |
|---|---|---|---|
| 初回起動から「セット選択 → 演者選択 → 編集 → 再生」まで説明書なしで到達できる | **X-04**（ヘッドレスで、初期状態からその 4 操作を accessible name のみで辿れることをスクリプト化）＋ **M-01 手動**（新規利用者による観察。機械化不能な部分） | UI スモーク・手動 L4 | `ui_smoke.rs`, 43 のチェックリスト |
| 常用操作はツールバー、全操作とショートカットはメニューバーから発見できる | **X-02**（コマンド登録簿 ⊆ メニュー項目名。ショートカット重複が無いことも同時に検査） | UI スモーク | `ui_smoke.rs` |
| 再生範囲、現在カウント、セット境界、警告は色だけに依存せず文字と形でも示す | **X-03**（全警告ウィジェットの accessible name が非空）、**G-警告文言**（警告文言を Locale 別にゴールデン固定） | UI スモーク・ゴールデン | `ui_smoke.rs`, `golden_config.rs` |
| Windows 100–200%スケーリング、日本語・英語UI、キーボード操作を検証する | **X-05**（`pixels_per_point` を 1.0 / 1.5 / 2.0 でヘッドレス実行し、パニックせずウィジェット矩形が重ならないこと）、**G-13**（全エラー文言 × Ja/En のゴールデン）、**X-06**（Tab 巡回で全操作可能要素に到達できること）＋ **M-02 手動**（Windows 実機での見た目） | UI スモーク・ゴールデン・手動 L4 | `ui_smoke.rs`, `golden_config.rs`, 43 のチェックリスト |

#### `MEDIA_PIPELINE.md` の品質ゲート（併せて満たす）

| ゲート | 保証するテスト |
|---|---|
| UI thread で decode / frame render / encode を行わない | **I-ジョブ**（40）、**X-01**（フレーム時間の上限） |
| 音声時刻は sample index、映像時刻は有理数 frame time から求める | **D-A/D-B**（`simulate_counts` が有理数からフレーム時刻を作る）、**P-01/P-02**、**I-フレーム時刻**（`frame/fps` と累加の結果が 432,000 フレーム後に乖離することを示す negative test で、累加を使っていないことを保証） |
| 同じ project/config から同じフレーム列を生成する | **G-11**（フレームハッシュ）、**D-A**（2 回生成して一致） |
| 1000人規模で再生中に継続的 allocation を発生させない | **A-08**、**S-05** |
| 書き出し後は ffprobe 検証に成功するまで完了扱いにしない | **I-ffprobe**（L4。実 FFmpeg を要するため CI では skip し、リリース前に実行） |

### 7.2 テスト基盤自体の検証（メタテスト）

品質基盤が壊れると、全テストが「緑のまま無意味」になる。基盤自体を検証する。

| ID | 内容 |
|---|---|
| **meta-01** | 確保カウンタが既知の 1 回の確保を 1 と数える |
| **meta-02** | `assert_no_alloc` が確保を検出して失敗する（`#[should_panic]`） |
| **meta-03** | `measure` がパニック後も `DEPTH` を戻す（`catch_unwind` で確認） |
| **meta-04** | `Tolerance::EXACT` が 1 ulp 差を弾き、`GEOMETRY` が通す |
| **meta-05** | `Tolerance::accepts` が NaN 同士を等しいと言わない |
| **meta-06** | `Digest::f32` が `-0.0` と `+0.0` を同一視し、異なる NaN を同一視する |
| **meta-07** | `Rng` が同一シードから同一列を返し、シードが違えば違う列を返す |
| **meta-08** | フィクスチャ生成器が 2 回呼んで同じ digest を返す（D-B に含む） |
| **meta-09** | ゴールデンランナーが 1 文字差で失敗する（意図的に壊した文字列で確認） |
| **meta-10** | CI の各ゲートが実際に赤くなる。導入時にドラフト PR で 1 度だけ手動確認する（fmt 違反 / clippy 違反 / ゴールデン差分 / 命令数 +10% / deny 違反の 5 つ） |

meta-10 は自動化しない。CI を書いた直後に 1 度確認し、その結果を PR に記録する。「ゲートが存在するが実は発火しない」は品質基盤で最も高くつく欠陥である。

---

## 8. 実装タスク

1 タスク = 1〜3 時間相当。`[並行]` は同じ記号を持つタスク同士が並行可能。

### Phase 0 — 土台（直列。これ抜きで他を始めると全部やり直す）

| # | タスク | 依存 | 並行 |
|---|---|---|---|
| **T-01** | `.gitignore` を修正（`*.drill.json` の無視をフィクスチャに波及させない。`/target` と `*.drill.json` を残しつつ `!crates/*/fixtures/**` を追加）。`.gitattributes` を新規作成し LF 固定。 | — | — |
| **T-02** | `rust-toolchain.toml` を追加（実測 stable を固定、rustfmt/clippy）。ルート `Cargo.toml` に `[profile.stress]` と `[workspace.lints]` を追加。 | T-01 | — |
| **T-03** | **最小 CI を入れる**。`.github/workflows/ci.yml` に `lint`（fmt + clippy）と `test`（ubuntu のみ、`cargo test --workspace`）の 2 ジョブだけ。Linux GUI 依存パッケージのインストールを含む。**これを最優先で緑にする。** | T-02 | — |
| **T-04** | `crates/drill-testkit` を新規作成。`Cargo.toml`（`publish = false`）、`digest.rs`、`approx.rs`、`rng.rs`、meta-04〜07 の自己テスト。workspace members に追加。 | T-02 | — |
| **T-05** | `drill-core` に `#![cfg_attr(not(test), warn(...))]` を追加（まず `warn`）。既存の違反件数を数え、Issue に記録する。 | T-02 | — |

### Phase 1 — 計測器（並行可能）

| # | タスク | 依存 | 並行 |
|---|---|---|---|
| **T-06** | `drill-testkit/src/alloc.rs`（`CountingAlloc` / `AllocStats` / `measure` / `assert_no_alloc`）。 | T-04 | A |
| **T-07** | `crates/drill-conformance` を新規作成。空の `src/lib.rs`、`tests/alloc_zero.rs` に meta-01〜03 と A-05（`playback::advance`）/ A-06（`TempoMap`）。現行 API で書ける確保ゼロ 2 件。 | T-06 | A |
| **T-08** | `drill-testkit/src/fixture.rs` の `tiny` / `small` / `medium` / `baseline` / `maximum`。生成のみ。`V1_SAMPLE` はまだ置かない。 | T-04 | B |
| **T-09** | `.config/nextest.toml` と、`ci.yml` の `test` ジョブを nextest 化 + windows matrix 追加。 | T-03 | C |
| **T-10** | `deny.toml` と `ci.yml` の `deny` ジョブ。既存 360 パッケージのライセンス違反を洗い出して allow リストを確定する。 | T-03 | C |

### Phase 2 — テスト層の立ち上げ（並行可能）

| # | タスク | 依存 | 並行 |
|---|---|---|---|
| **T-11** | insta を `drill-conformance` の dev-dep に追加。`golden_export.rs` に G-01〜G-08 を現行 API で作成（`small()` 使用）。`ci.yml` に `golden` ジョブ。`INSTA_UPDATE=no` を全ジョブに設定。 | T-08, T-09 | D |
| **T-12** | `determinism.rs` に D-A / D-B（再生カウント列、フィクスチャ digest）。`simulate_counts` の実装。 | T-08 | D |
| **T-13** | proptest を dev-dep に追加。`drill-testkit/src/strategy.rs`（`finite_f32` / `hostile_f32` / `point` / `tempo_map`）と `prop_tempo.rs` に P-01〜P-05、P-10〜P-12、P-17。`PROPTEST_CASES` の CI 設定。既存 `lib.rs:485` の doc comment を更新（役割の明記）。 | T-08 | E |
| **T-14** | `crates/drill-bench` を新規作成。criterion で `bench/interp` / `bench/tempo` / `bench/advance` / `bench/json_save` / `bench/json_load` / `bench/svg` / `bench/assignment`。 | T-08 | F |
| **T-15** | 既存 `core_performance.rs` に閾値 assert（3.0 倍ゲート）を追加。`baselines/walltime.md` を作成し、2026-08-09 の行を「条件不明」として記録。`ci.yml` に `budget` ジョブ。 | T-14 | F |
| **T-16** | gungraun で `instructions.rs`。Linux で初回計測し `baselines/instructions.json` をコミット。`ci.yml` に `instructions` ジョブ（`target-cpu=x86-64-v2` 固定、valgrind インストール）。 | T-14 | F |
| **T-17** | cargo-fuzz の初期化。F-01 / F-03 / F-04 を現行 API で作成。`nightly.yml` を新規作成し、fuzz + PROPTEST_CASES=65536 + macOS matrix + カバレッジ + 失敗時 Issue 起票。 | T-13 | G |

### Phase 3 — Wave 0/1 の実装に追随（他設計文書の完了が前提）

| # | タスク | 依存 | 並行 |
|---|---|---|---|
| **T-18** | `V1_SAMPLE` フィクスチャを作成し、`migration.rs`（v1 → v2 往復、G-14）。 | A-7（42/41） | H |
| **T-19** | `drill-testkit::random_edit` と `strategy::edit_for` / `edit_sequence`。`prop_edit.rs`（P-06〜P-09）と `stress_edit.rs`（S-01〜S-03）。 | A-1（10） | H |
| **T-20** | A-01（`positions_at_count`）と A-02（`scan_transition`）の確保ゼロ。`bench/scan`。 | 11, 13 | I |
| **T-21** | G-09 / G-10（DisplayList ゴールデン）、A-03、`bench/render_build`。`coordinate_consistency.rs`（CSV と SVG の座標突き合わせ）。 | 20 | I |
| **T-22** | S-05（2 時間再生メモリ）、A-08（フレーム全経路）。 | 20 | I |
| **T-23** | G-11（フレームハッシュ）、A-04、`bench/raster`。 | 31 | J |
| **T-24** | `persistence.rs`（原子的置換・自動保存・欠落アセット・パニックフック）、F-05。 | 41 | J |
| **T-25** | `coverage_gate.toml` と nightly のカバレッジゲート（`from_json`/`validate`/`migrate` の region 90%）。 | T-17, T-18 | K |
| **T-26** | `B-01`（51 の上限定数の境界表テスト）、F-06 / F-07。 | 51, 52, 30 | K |
| **T-27** | `drill-app/tests/ui_smoke.rs`（X-01〜X-06）。egui_kittest 導入と egui 版の整合。 | C-1（43） | L |
| **T-28** | L4 手動チェックリストの整備（43 と共同）と、リリース前実行手順の文書化。 | T-27 | L |
| **T-29** | meta-10（各 CI ゲートが実際に赤くなることの手動確認）と、結果の記録。 | T-11, T-16, T-10 | — |

**クリティカルパス**: T-01 → T-02 → T-03（最小 CI）→ T-04 → T-06 → T-07。ここまでで「CI があり、確保を数えられる」状態になる。以降は並行可能。

---

## 9. 未決事項

| # | 保留した点 | 決めるために必要な情報 |
|---|---|---|
| **1** | `rust-toolchain.toml` の `channel` の実値 | 導入 PR 時点の `rustc -V`。本書では `1.98.0` を仮置きしている。cargo-llvm-cov 0.8.7 が Rust 1.87 以上を要求するため、それを下回らないこと |
| **2** | 命令数ゲートを Linux 単独で運用してよいか | 3 か月運用して、Windows/macOS 固有の性能退行が実際に発生するかを観測する。発生するなら CodSpeed の walltime モードか、self-hosted runner を検討 |
| **3** | `egui_kittest` の版整合 | `eframe 0.35.0` に対し `egui_kittest` は 0.36.1 が最新。`eframe` を 0.36 系へ上げるか、`egui_kittest 0.35.x` を使うか。43（C-1 の `main.rs` 分解）の実装時に、eframe 更新の他影響と併せて判断する |
| **4** | `maximum()`（4,000 人）フィクスチャの生成コスト | 実測が必要。生成に 10 秒以上かかるなら、生成結果をバイナリ形式でキャッシュする仕組み（`OnceLock` + プロセス内共有）を足す |
| **5** | カバレッジ 90% ゲートの対象ファイル一覧の確定 | 41 / 51 / 52 の設計が確定してから。現時点では `from_json` / `validate` / `migrate` の 3 関数のみ確定 |
| **6** | 動画フレームの参照を「ハッシュのみ」で足りるとするか、参照 PNG を置くか | 現在リポジトリに Git LFS が無い。1080p の PNG は 1 枚数百 KB。ハッシュだけでは「壊れ方」が分からないので、失敗時に CI アーティファクトとして実画像を吐く方式で足りるかを 31 の実装時に判断する |
| **7** | CodSpeed の再検討条件 | リポジトリを OSS 化した場合（OSS は無償）、または gungraun の運用が破綻した場合 |
| **8** | `cargo vet` / `cargo-crev` の導入 | 依存が 400 を超える、または外部コントリビュータを受け入れる段階で再検討 |
| **9** | `clippy::indexing_slicing` を `deny` へ昇格する時期 | T-05 で計測する現行違反件数次第。`lib.rs:385` の `self.sets[...]` など既知の違反があり、A-1 の `Edit` 導入で多くが解消される見込み |
| **10** | PR での property ケース数 256 が十分か | 3 か月運用し、nightly（65,536 ケース）でのみ見つかる反例の頻度を観測する。頻度が高ければ PR 層を増やし、CI 時間予算を見直す |
| **11** | nightly 失敗時の Issue 自動起票のノイズ制御 | 同一原因で毎晩起票されると無視される。既存 Issue の再オープンで抑制するか、通知を週次サマリにするか |

# 21. GPU レンダラ（wgpu インスタンシング・2D/3D統合）

## 1. 目的と範囲

### 目的

演者ドットの描画を egui の CPU 側 shape 生成から **wgpu のインスタンシング描画**へ移す。
1,000人規模で `Painter::circle_filled` を1,000回呼ぶ現在の構造は、tessellation と頂点バッファ生成が
CPU の直列処理として `16.6ms` 予算の半分以上を占める（§5 で見積り）。これを1ドローコールへ畳む。

同時に、2D フィールドビューと 3D スタジアムビューを**同一のインスタンスデータ・同一のシェーダ**で描き、
投影行列とわずかなユニフォームだけを差し替える構造にする。現在は `draw_field` と `draw_stadium` が
別々に同じ演者を描いている（`crates/drill-app/src/main.rs:1474-1497` と `:396-406`）。

### この文書が扱うこと

- `egui_wgpu::CallbackTrait` による egui 描画順への自前パスの差し込み（実 API 準拠）
- 演者インスタンスのメモリレイアウト・バッファ更新戦略
- 記号（Circle/Square/Triangle/Diamond/Cross/Star）の単一シェーダ描画
- 2D/3D 統合、ビルボード、接地影、深度ソート
- 経路トレイルの GPU 化と LOD
- テキストの配置判断（GPU 側 vs egui 側）
- リサイズ・DPI・デバイスロスト・最小化
- wgpu が使えない環境でのフォールバック
- オフスクリーン描画と動画書き出しとの境界、および**決定論の結論**

### この文書が扱わないこと

| 事項 | 担当 |
|---|---|
| `DisplayList` の型定義・生成アルゴリズム | 20-display-list.md |
| スタジアム構造物・観客席・芝テクスチャのモデリング | 22-stadium.md |
| カメラのキーフレーム・追従・補間 | 23-camera.md |
| FFmpeg 起動・mux・進捗・キャンセル | 31-video-export.md |
| CPU ラスタライザの内部アルゴリズム | 31-video-export.md（本書は要件のみ提示） |
| 音声同期・タイムライン | 30-audio.md |

本書は `DisplayList` の**消費者**の立場で書く。20 の細部に依存せず、必要な入力を §3.9 に
「20 への要求」として明示する。

---

## 2. 現状

### 2.1 依存バージョン（`Cargo.lock` で確認）

| クレート | バージョン |
|---|---|
| `eframe` | 0.35.0 |
| `egui` / `epaint` | 0.35.0 |
| `egui-wgpu` | 0.35.0 |
| `wgpu` / `naga` | 29.0.4 |
| `winit` | 0.30.13 |
| `bytemuck` | 1.25.2（推移依存として既に存在） |

`crates/drill-app/Cargo.toml:9`:

```toml
eframe = { version = "0.35.0", default-features = false, features = ["wgpu", "wayland", "x11"] }
```

`wgpu` feature は `eframe` の `wgpu_no_default_features` + `egui-wgpu/default` を有効にし、
`egui-wgpu/default` は `wgpu/default`（`dx12` / `metal` / `gles` / `vulkan` / `wgsl` / `webgpu`）を引く。
つまり**バックエンドは既に4系統すべて有効**である（`wgpu-29.0.4/Cargo.toml:52-61` で確認）。
一方 `glow` feature は無効なので、現状 `eframe::Renderer` の候補は `Wgpu` のみ。

### 2.2 現在の描画コード

すべて `crates/drill-app/src/main.rs` の egui `Painter` 呼び出しである。GPU 固有のコードは**一行も無い**。

| 場所 | 内容 |
|---|---|
| `main.rs:1435` | `ui.allocate_painter(available, Sense::click_and_drag())` で描画領域を確保 |
| `main.rs:1440`, `main.rs:1617-1710` | `draw_field()`: 矩形・ヤードライン・ハッシュ・ヤード数字を `line_segment` / `text` で積む |
| `main.rs:1474-1497` | 2D の演者ループ。演者1人あたり `circle_filled` ×1、選択時 `circle_stroke` ×1、`text` ×1 |
| `main.rs:291-414` | `draw_stadium()`: 3D。`drill_core::camera::Camera::project` で CPU 投影し、同じく `circle_filled` |
| `main.rs:388-394` | 3D の深度ソート。`Vec<usize>` を**毎フレームヒープ確保**し、`sort_by` のクロージャ内で距離を再計算 |
| `main.rs:400` | `radius = (0.6 * focal / dist).clamp(1.5, 22.0)` — ピクセル半径のクランプ範囲はここが既定値 |
| `main.rs:34-49` | `install_fonts()`: 日本語フォントを egui に登録済み |

`main.rs:388-394` の毎フレーム `Vec` 確保は 00-conventions の不変条件3
（「毎フレーム走る関数は `&mut` の作業領域を受け取り、内部でヒープ確保しない」）に違反している。
本設計はこれも解消する。

### 2.3 まだ存在しないもの

- `crates/` には `drill-app` と `drill-core` の2つしかない。`drill-render` / `drill-export` / `drill-audio` は未作成。
- `DisplayList` は `DESIGN_GAPS.md:307-325` に案があるだけで、型は存在しない。
- 記号を表す `Symbol` enum はリポジトリのどこにも無い。`Performer` は
  `crates/drill-core/src/lib.rs:226-230` の `{ id, label, color: [u8;3] }` のみ。
- 経路トレイル（Trail）の描画コードは無い（`main.rs` に `trail` / `route` の語は出てこない）。
- `drill_core::camera::Camera` の `view_matrix()` / `perspective_matrix()` は
  `crates/drill-core/src/camera.rs:127` / `:142` で **private**。外からは `project()` 経由でしか使えない。
- ベンチは `crates/drill-core/benches/core_performance.rs` の1本のみ。GPU 側のベンチは無い。

### 2.4 実測されている数値

`PRODUCT_QUALITY.md:23-27` の開発機ベースライン、および `benches/core_performance.rs`:

`Document::demo(100, 10)` は `rows * columns = 1,000` 人。
`1,000 人 × 60,000 フレーム補間 = 9.24ms` なので、**補間は 1 フレームあたり約 0.154µs**。
補間は予算上の問題ではない。問題は補間の後段、すなわち描画コマンド生成側にある。

---

## 3. 設計

### 3.0 クレート配置（00-conventions のクレート表への追加提案）

00-conventions のクレート境界表では `drill-render` は「描画APIを知らない」、
`drill-app` は「egui/wgpu の表示と入力変換のみ」となっている。
wgpu レンダラは **`drill-export` からもヘッドレスで使いたい**（§3.8）ため、どちらにも入らない。

```
drill-core    （変更なし）
drill-render  DisplayList 中間表現。drill-core にのみ依存。描画APIを知らない。
drill-gpu     ★新規。DisplayList → wgpu 描画。依存は drill-render + wgpu + bytemuck のみ。
              egui を知らない。ウィンドウ・サーフェスを知らない。
drill-audio   （変更なし）
drill-export  drill-render に依存。drill-gpu には optional 依存（GPU 高速化パス）。
drill-app     egui/wgpu の表示と入力変換のみ。drill-gpu の薄い CallbackTrait アダプタを持つ。
```

依存は上から下へ。`drill-gpu` が egui に依存しないことが本質で、これによって
「同じレンダラをヘッドレスで使えるか」（§3.8）が構造的に保証される。

### 3.1 egui 0.35 / wgpu 29 への差し込み

#### 実 API（`egui-wgpu-0.35.0/src/renderer.rs:87-120` で確認）

```rust
pub trait CallbackTrait: Send + Sync {
    fn prepare(
        &self,
        _device: &wgpu::Device,
        _queue: &wgpu::Queue,
        _screen_descriptor: &ScreenDescriptor,
        _egui_encoder: &mut wgpu::CommandEncoder,
        _callback_resources: &mut CallbackResources,   // ← &mut はここだけ
    ) -> Vec<wgpu::CommandBuffer> { Vec::new() }

    fn finish_prepare(&self, ..., _callback_resources: &mut CallbackResources)
        -> Vec<wgpu::CommandBuffer> { Vec::new() }

    fn paint(
        &self,
        info: PaintCallbackInfo,
        render_pass: &mut wgpu::RenderPass<'static>,
        callback_resources: &CallbackResources,        // ← 不変参照
    );
}
```

`CallbackResources` は `type_map::concurrent::TypeMap`（`renderer.rs:15`）。
`RenderState::renderer: Arc<RwLock<Renderer>>`（`lib.rs:128`）の `callback_resources` フィールドに
起動時 1 回だけ我々のリソースを `insert` する。

#### 具体的手順

**起動時（`eframe::CreationContext` を受け取る所）**:

```rust
// crates/drill-app/src/gpu_bridge.rs
pub fn install(cc: &eframe::CreationContext<'_>, msaa_samples: u32)
    -> Result<(), DrillError>
{
    // eframe-0.35.0/src/epi.rs:797
    let rs = cc.wgpu_render_state().ok_or(DrillError::GpuUnavailable)?;

    let renderer = drill_gpu::GpuRenderer::new(&drill_gpu::GpuInit {
        device: &rs.device,
        queue:  &rs.queue,
        color_format: rs.target_format,          // RenderState::target_format と必ず一致させる
        depth_format: Some(wgpu::TextureFormat::Depth32Float),
        sample_count: msaa_samples.max(1),       // NativeOptions::multisampling と必ず一致させる
    })?;

    rs.renderer.write().callback_resources.insert(renderer);
    Ok(())
}
```

> **落とし穴（実 API 確認済み）**: `RenderState` は MSAA サンプル数を**公開していない**
> （`lib.rs:107-134` のフィールドは adapter / available_adapters / device / queue /
> target_format / renderer / surface_config のみ）。我々のパイプラインの
> `multisample.count` が egui の `RendererOptions::msaa_samples.max(1)`
> （`renderer.rs:396-400`）と食い違うとレンダーパス検証エラーで落ちる。
> したがって `NativeOptions::multisampling` の値を**アプリ側で保持して渡す**必要がある。

**毎フレーム（描画領域の中）**:

```rust
let (response, painter) = ui.allocate_painter(available, Sense::click_and_drag());
let rect = response.rect;

painter.add(egui_wgpu::Callback::new_paint_callback(
    rect,
    DrillCallback {
        // DisplayList から抽出済みの、Send + Sync な値だけを持たせる。
        // Arc<Frame> でよい。&Document を掴んではいけない（'static + Send + Sync が要る）。
        frame: Arc::clone(&self.gpu_frame),
    },
));
```

`Callback::new_paint_callback` は `epaint::PaintCallback` を返し、`Shape` へ変換されて
egui の描画順に**その位置で**挿入される。したがって、この呼び出しより前に積んだ
`Painter` の図形（フィールドの芝）は自前パスの下に、後に積んだもの（ラベル・マーキー・HUD）は上に来る。
**描画順の制御はこれだけで済む。** 別レンダーパスも合成も不要。

**egui 側が我々に保証すること（`renderer.rs:561-598` で確認）**:

- `paint` の直前に `render_pass.set_viewport(viewport_px.left, top, width, height, 0.0, 1.0)` が
  呼ばれる。つまり NDC の `[-1, +1]` が `rect` に対応する状態で始まる。自分で設定し直す必要はない。
- `set_scissor_rect` はクリップ矩形（親スクロール領域など）に設定済み。
- `viewport_px.width_px > 0 && height_px > 0` のときしか `paint` は呼ばれない。
  **ただし `prepare` はサイズ 0 でも呼ばれる**（`prepare` は描画パスの外で全コールバック分まとめて走る）。
- 我々の後に `needs_reset = true` が立ち、egui は自分のパイプライン・バインドグループ・
  ビューポートを復元する。**我々は egui の状態を壊してよい。**
- 逆に、**我々は入場時の状態を一切仮定してはいけない**。`paint` の中で
  `set_pipeline` / `set_bind_group` / `set_vertex_buffer` を毎回設定する。

**深度バッファ（`renderer.rs:190`, `winit.rs:671-697` で確認）**:

egui は既定で深度添付を作らない（`RendererOptions::depth_stencil_format = None`）。
3D ビューには深度が要るので `NativeOptions::depth_buffer = 32` を設定する
（`eframe-0.35.0/src/epi.rs:313`、`egui_wgpu::depth_format_from_bits` が変換）。
すると egui のレンダーパスに深度添付が付き、パス開始時に `LoadOp::Clear(1.0)`、
終了時 `StoreOp::Discard` になる。egui 自身のパイプラインは
`depth_write_enabled: false, depth_compare: Always`（`renderer.rs:364-365`）なので、
**egui の UI は深度を汚さず、我々が自由に使える**。理想的な形になっている。

**イミディエイトデータ（旧 push constants）は使わない**:

wgpu 29 では push constants が「immediates」に改名され、
`PipelineLayoutDescriptor::immediate_size` と `Features::IMMEDIATES` が必要になった
（`wgpu-29.0.4/src/api/pipeline_layout.rs:39-44`）。GL バックエンドでは非対応の可能性が高いので、
パス種別（ドット / 接地影）の切り替えは**動的オフセット付きユニフォームバッファ**で行う。
`min_uniform_buffer_offset_alignment` の既定 256 バイト境界にスロットを並べる。

### 3.2 インスタンスバッファ

```rust
// drill-gpu/src/instance.rs
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct PerformerInstance {
    /// World position. x = field x, y = height above turf, z = field y.
    /// Matches `drill_core::camera::field_to_world`.
    pub world: [f32; 3],      // offset  0, 12 bytes
    /// Symbol radius in world units.
    pub radius: f32,          // offset 12,  4 bytes
    /// Fill color, RGBA8 packed little-endian (r = bits 0..8).
    pub fill: u32,            // offset 16,  4 bytes
    /// Ring / outline color, RGBA8 packed.
    pub stroke: u32,          // offset 20,  4 bytes
    /// Bit-packed state, see `InstanceFlags`.
    pub flags: u32,           // offset 24,  4 bytes
    /// Reserved; keeps the stride a power of two. Must be zero.
    pub _pad: u32,            // offset 28,  4 bytes
}
const _: () = assert!(core::mem::size_of::<PerformerInstance>() == 32);
```

**フラグのビット割り当て**:

```rust
pub struct InstanceFlags;
impl InstanceFlags {
    pub const SYMBOL_MASK:  u32 = 0x0000_000F; // bits  0..4  : Symbol as u32 (0..=5)
    pub const SELECTED:     u32 = 0x0000_0010; // bit   4
    pub const HOVERED:      u32 = 0x0000_0020; // bit   5
    pub const WARN_MASK:    u32 = 0x0000_00C0; // bits  6..8  : 0 none / 1 info / 2 warn / 3 error
    pub const WARN_SHIFT:   u32 = 6;
    pub const GHOST:        u32 = 0x0000_0100; // bit   8     : previous/next set ghost
    pub const DIMMED:       u32 = 0x0000_0200; // bit   9     : outside the active subset
    pub const INDEX_SHIFT:  u32 = 12;          // bits 12..32 : performer index, for picking
    pub const INDEX_MASK:   u32 = 0xFFFF_F000;
}
```

インデックスに 20 ビット割くので上限 1,048,575。`MAX_PERFORMERS = 16_384`（§6）に対して十分。

**バッファサイズ**:

| 規模 | インスタンス数 | バイト数 |
|---|---|---|
| 基準（1,000人） | 1,000 | 32,000 B ≈ 31.3 KiB |
| 上限（4,000人） | 4,000 | 128,000 B = 125 KiB |
| ゴースト表示 ON（前後セット同時） | 3× | 375 KiB |
| 容量上限 `MAX_PERFORMERS` | 16,384 | 512 KiB |

wgpu の既定 `Limits::max_buffer_size` は 256 MiB。桁が3つ違う。**バッファサイズは制約にならない。**

**更新戦略: 毎フレーム全書き換え。差分更新はしない。**

```rust
// GpuRenderer::prepare() の中
let bytes: &[u8] = bytemuck::cast_slice(&frame.instances);
if self.instance_capacity < frame.instances.len() {
    self.grow_instance_buffer(device, frame.instances.len());  // ここだけが確保
}
queue.write_buffer(&self.instance_buffer, 0, bytes);
```

理由:

1. **再生中は全演者が毎フレーム動く。** 差分は常に 100% で、差分計算は純粋な損失になる。
2. 32 KB の memcpy は最近の CPU で約 3µs（10 GB/s 換算）。差分検出（1,000 要素の比較 + 範囲マージ）は
   それより確実に高くつく。125 KiB でも約 12µs。
3. `Queue::write_buffer` は wgpu 内部でステージングバッファを取り、
   フレーム間の同期は wgpu が持つ。手動のダブルバッファリングは不要かつ有害
   （二重管理でデバイスロスト復帰時のバグ源になる）。
4. 編集中（停止中）は動くのが選択中の数人だけだが、その場合も 32 KB のコピーは
   「毎フレーム走る関数のヒープ確保ゼロ」を満たす限り予算内。最適化の価値がない。

**容量成長**: `next_power_of_two(count).max(1024)`。バッファ再生成は `prepare` の中で行う
（`&mut CallbackResources` が使えるのは `prepare` / `finish_prepare` だけ）。
償却 O(1)、定常状態では 0 回。**フレーム内ヒープ確保ゼロ**（不変条件、§4-4）。

**ジオメトリは持たない**: 頂点バッファもインデックスバッファも作らない。
`@builtin(vertex_index)` から 4 頂点のトライアングルストリップを生成する。
`render_pass.draw(0..4, 0..instance_count)` の 1 ドローコールで全演者。

### 3.3 記号の描画: SDF を選定する

**決定: 解析的 SDF（Signed Distance Field）をフラグメントシェーダで評価する。テクスチャアトラスは採用しない。**

#### 比較

| 観点 | 解析的 SDF | テクスチャアトラス |
|---|---|---|
| スケール範囲 1.5px〜22px（`main.rs:400` の実クランプ） | 全域で正確 | ミップ必須。2px でぼやけ、カメラ移動でシマリング |
| アンチエイリアス | `fwidth` で 1px フェザー、egui の見た目と一致 | バイリニアに依存、太さが変わる |
| 選択リング・警告リング | `abs(d) - w` で同一シェーダ内、追加ドロー 0 | 別スプライトが要る＝ドローかアトラス枠が増える |
| バインドグループ | テクスチャ不要（ユニフォーム1つ） | テクスチャ + サンプラ |
| DPI 変更 | 無反応でよい | 再生成またはミップ選択の再調整 |
| 記号の追加 | WGSL に関数1つ | アトラス再パック + 再アップロード |
| 任意グリフ（将来のユーザー定義記号） | **不可** | 可能 |
| VRAM | 0 | 数百 KB |

#### 選定理由

決め手は**スケール範囲**である。3D スタジアムビューでは同一フレーム内に 1.5px の遠景ドットと
22px の近景ドットが同居する（`main.rs:400`）。アトラスでこれを綺麗に出すには
ミップチェーン＋トリリニアが要り、遠景は必ずぼやけ、カメラが動くと解像度段が切り替わってちらつく。
SDF は解像度非依存でこの問題が原理的に存在しない。

次点で**選択リングの吸収**が大きい。現状は選択演者ごとに `circle_stroke` を追加で呼んでいる
（`main.rs:404`, `main.rs:1488`）。SDF なら `abs(d) - ring_width` を同じフラグメントで評価するだけで、
ドロー数も頂点数も増えない。全選択（Ctrl+A で 1,000人）でもコストが変わらない。

分岐のコストは無視できる。記号の分岐は**インスタンス単位で一様**なので、
1つのクアッド（14×14px 程度）内の全フラグメントは同じ経路を通る。ワープ内発散は
クアッド境界にまたがる場合だけで、フラグメント総数に対して無視できる割合。

**任意グリフを諦める点は認識しておく。** 将来ユーザー定義の楽器記号や SVG インポート記号を
出す要件が来たら、`flags` に `ATLAS` ビットを立てて**第2のパイプライン**（MSDF アトラス）を
同じインスタンスバッファから駆動する。インスタンスレイアウトを変えずに拡張できる。§9 に記載。

#### WGSL（要点）

```wgsl
struct View {
    view_proj : mat4x4<f32>,  // column-major, world -> clip (wgpu depth range 0..1)
    viewport  : vec2<f32>,    // physical pixels
    focal_px  : f32,          // 3D: viewport.y*0.5/tan(fovy*0.5) ; 2D: pixels per world unit
    min_px    : f32,          // 1.5
    max_px    : f32,          // 22.0
    pass_kind : u32,          // 0 = dot, 1 = ground shadow
    _pad      : vec2<u32>,
};
@group(0) @binding(0) var<uniform> view : View;

struct Instance {
    @location(0) world  : vec3<f32>,
    @location(1) radius : f32,
    @location(2) fill   : u32,
    @location(3) stroke : u32,
    @location(4) flags  : u32,
};

struct VsOut {
    @builtin(position) pos : vec4<f32>,
    @location(0) local : vec2<f32>,
    @location(1) @interpolate(flat) fill   : u32,
    @location(2) @interpolate(flat) stroke : u32,
    @location(3) @interpolate(flat) flags  : u32,
};

@vertex
fn vs_main(@builtin(vertex_index) vi : u32, inst : Instance) -> VsOut {
    var world = inst.world;
    if (view.pass_kind == 1u) { world.y = 0.0; }        // ground shadow: flatten onto turf

    let clip = view.view_proj * vec4<f32>(world, 1.0);

    // clip.w == view-space distance for the perspective matrix, == 1.0 for the ortho matrix.
    var r_px = inst.radius * view.focal_px / max(clip.w, 1e-4);
    r_px = clamp(r_px, view.min_px, view.max_px);

    // Unit quad corners from the vertex index: (-1,-1) (1,-1) (-1,1) (1,1)
    let corner = vec2<f32>(f32(vi & 1u) * 2.0 - 1.0, f32(vi >> 1u) * 2.0 - 1.0);
    var offset = corner * r_px;
    if (view.pass_kind == 1u) { offset.y = offset.y * 0.35; }   // squashed ellipse

    var out : VsOut;
    // Expand in clip space: one physical pixel == 2/viewport in NDC, times w.
    out.pos    = vec4<f32>(clip.xy + offset * (2.0 / view.viewport) * clip.w, clip.z, clip.w);
    out.local  = corner;
    out.fill   = inst.fill;
    out.stroke = inst.stroke;
    out.flags  = inst.flags;
    return out;
}

fn sd_circle(p : vec2<f32>) -> f32 { return length(p) - 1.0; }

fn sd_box(p : vec2<f32>, b : vec2<f32>) -> f32 {
    let d = abs(p) - b;
    return length(max(d, vec2<f32>(0.0))) + min(max(d.x, d.y), 0.0);
}

fn sd_diamond(p : vec2<f32>) -> f32 {
    return (abs(p.x) + abs(p.y)) * 0.70710678 - 0.70710678;
}

fn sd_cross(p : vec2<f32>) -> f32 {
    let a = sd_box(p, vec2<f32>(1.0, 0.30));
    let b = sd_box(p, vec2<f32>(0.30, 1.0));
    return min(a, b);
}
// sd_triangle / sd_star5 follow the standard 2D SDF forms (half-plane intersection /
// polar fold). They are *bounded* rather than exact distance fields, which is
// acceptable here: only the sign and a locally correct gradient are needed for the
// 1px feather. Exact constants are pinned by the golden test in §7.

fn distance_to_symbol(sym : u32, p : vec2<f32>) -> f32 {
    switch sym {
        case 1u:  { return sd_box(p, vec2<f32>(0.8, 0.8)); }
        case 2u:  { return sd_triangle(p); }
        case 3u:  { return sd_diamond(p); }
        case 4u:  { return sd_cross(p); }
        case 5u:  { return sd_star5(p); }
        default:  { return sd_circle(p); }
    }
}

fn unpack_rgba(c : u32) -> vec4<f32> {
    return vec4<f32>(f32(c & 0xffu), f32((c >> 8u) & 0xffu),
                     f32((c >> 16u) & 0xffu), f32((c >> 24u) & 0xffu)) * (1.0 / 255.0);
}

@fragment
fn fs_main(in : VsOut) -> @location(0) vec4<f32> {
    let d  = distance_to_symbol(in.flags & 0xFu, in.local);
    let aa = fwidth(d);                                    // anisotropy-safe (needed for shadows)

    var color = unpack_rgba(in.fill);
    var alpha = 1.0 - smoothstep(-aa, aa, d);

    // Selection / warning ring, same fragment, no extra draw call.
    let ring_on = (in.flags & 0x30u) != 0u || (in.flags & 0xC0u) != 0u;
    if (ring_on) {
        let ring = abs(d - 0.30) - 0.14;
        let ra   = 1.0 - smoothstep(-aa, aa, ring);
        let rc   = unpack_rgba(in.stroke);
        color = mix(color, rc.rgb, ra * rc.a);
        alpha = max(alpha, ra * rc.a);
    }
    if (alpha <= 0.0) { discard; }
    return vec4<f32>(color * alpha, alpha);                // premultiplied
}
```

パイプラインは `PrimitiveTopology::TriangleStrip`, `strip_index_format: None`,
`cull_mode: None`, ブレンドは premultiplied over。頂点属性は **5 個**
（GLES 3.0 の最低保証 16 に対し余裕。ダウンレベルアダプタでも通る）。

### 3.4 2D と 3D の統合

**同一のインスタンスバッファ、同一のシェーダ、同一のドローコール。差し替えるのは `View` ユニフォームだけ。**

分岐は §3.3 のシェーダに現れている通り 1 箇所しかない: `clip.w` の意味である。

- 3D 透視: `clip.w = view-space distance`、`r_px = radius * focal_px / clip.w` → 遠いほど小さい
- 2D 正射: `clip.w = 1.0`、`focal_px = pixels per world unit` → `r_px = radius * px_per_world` で一定

つまり「投影して中心を求め、ピクセル半径でクリップ空間に広げる」という**1 本の経路**で両方が出る。
ビルボードは自動的に成立する（クアッドをスクリーン平面上で広げているので、常に視点を向く）。

#### 深度規約の落とし穴（重要）

`drill_core::camera::Camera::perspective_matrix`（`camera.rs:142-152`）は **OpenGL 規約**で、
NDC の z が `[-1, +1]` に写る。`camera.rs:106` の `if ndc[2] < -1.0 || ndc[2] > 1.0` がその証拠。
**wgpu / DirectX / Metal の NDC z は `[0, 1]`** である。この行列をそのまま渡すと
近半分が z<0 でクリップされ、演者が消える。

`drill-gpu` は自前で wgpu 規約の透視行列を持つ。右手系・視線 -Z・深度 0..1（row-major）:

```rust
/// Right-handed, looking down -Z, clip-space depth in [0, 1] (wgpu / D3D / Metal).
pub fn perspective_wgpu(fov_y: f32, aspect: f32, near: f32, far: f32) -> [[f32; 4]; 4] {
    let t = (fov_y * 0.5).tan();
    [
        [1.0 / (aspect * t), 0.0,       0.0,                  0.0],
        [0.0,                1.0 / t,   0.0,                  0.0],
        [0.0,                0.0,       far / (near - far),   far * near / (near - far)],
        [0.0,                0.0,       -1.0,                 0.0],
    ]
}
```

検算: 視空間 `z = -near` → `clip.z = 0`, `clip.w = near` → `ndc.z = 0`。
`z = -far` → `clip.z = far`, `clip.w = far` → `ndc.z = 1`。正しい。

**23-camera / drill-core への要求**: `Camera::view_matrix()`（`camera.rs:127`、現在 private）を
`pub` にすること。ビュー行列は深度規約に依存しないので、CPU 経路（`Camera::project`）と
GPU 経路が**同一のビュー行列**を共有できる。投影行列だけが規約ごとに分かれる。
これにより x/y の投影結果は両経路で一致し、§3.9 のコンフォーマンステスト（重心誤差 ≤ 0.5px）が成立する。

#### 行優先 / 列優先

`drill_core::camera` の行列は row-major `[[f32;4];4]` で `M * v` として適用する（`camera.rs:18-19`）。
WGSL の `mat4x4<f32>` は **column-major**。アップロード時に転置が必要。

```rust
/// Row-major (`drill_core::camera` convention) -> column-major (WGSL).
pub fn to_wgsl(m: &[[f32; 4]; 4]) -> [[f32; 4]; 4] {
    let mut out = [[0.0f32; 4]; 4];
    for r in 0..4 { for c in 0..4 { out[c][r] = m[r][c]; } }
    out
}
```

これは取り違えると「画面に何も出ない」で終わる典型的なバグなので、往復テストを必ず置く（§7）。

#### 接地影

同じインスタンスバッファを 2 回目のドローで消費する。`View::pass_kind = 1` にすると
シェーダが `world.y = 0` で地面に落とし、y 方向を 0.35 倍に潰した楕円にする（§3.3 の WGSL 参照）。
色は `fill` を無視して固定の黒 35% を使う（フラグメント側で `pass_kind` を見る、
または fill を上書きした 2 つ目のユニフォームスロットを使う）。

- ドット本体より**先に**描く（深度書き込みなし、深度テストあり）
- 2D モード（`view.mode == ortho`）では影パスを丸ごとスキップ
- 追加コスト: ドローコール 1、CPU コスト 0（バッファは共有）

#### 深度ソートは必要か

**結論: 深度テストは有効にし、深度書き込みは無効にし、ドットは CPU で背面から前面へ並べる。**

理由の分解:

- **深度テスト ON / 書き込み OFF**: スタジアム構造物（22 の担当）や地面に対する遮蔽は
  深度バッファで正しく処理される。これはソートでは代替できない。
- **ドット同士**: SDF の縁は 1px フェザーで半透明なので、アルファブレンドの順序が結果に効く。
  順序を無視すると、重なった演者の縁に暗い/明るいリングが出る。
  深度書き込みを ON にして不透明扱いにすればソート不要だが、縁がジャギーになる
  （MSAA 前提なら alpha-to-coverage で回避できるが、MSAA を必須にしたくない）。
- したがって **CPU で背面→前面にソートする**。ただし現在の実装（`main.rs:388-394`、
  毎フレーム `Vec` 確保 + 比較関数内で距離を再計算する `sort_by`）は置き換える。

**基数ソート（比較なし・確保なし・決定的）**:

```rust
pub struct DepthSortScratch {
    keys:    Vec<u16>,        // quantized view-space depth
    order:   Vec<u32>,
    tmp:     Vec<u32>,
    buckets: [u32; 256],
}

impl DepthSortScratch {
    /// Back-to-front order by view-space depth. Stable: equal depths keep instance order.
    /// O(n), zero allocation once warmed. Deterministic (integer keys, no float compare,
    /// no NaN hazard — non-finite depths are clamped by `quantize`).
    pub fn sort_back_to_front(
        &mut self,
        instances: &[PerformerInstance],
        view: &[[f32; 4]; 4],
        near: f32,
        far: f32,
        out: &mut Vec<u32>,
    );
}
```

2 パス（下位 8bit → 上位 8bit）のカウンティングソート。1,000 要素で約 4µs、4,000 で約 15µs。
`sort_by` + クロージャ距離再計算に対して 1 桁速く、かつ**確保ゼロ・完全決定的**。

2D モードではソートしない。演者インデックス順に描く（決定的で、重なりの見え方は
どの順でも実用上同じ）。

### 3.5 経路トレイル

#### 幾何

`PrimitiveTopology::LineStrip` は使わない。線幅がドライバ依存で 1px 固定の実装が多く、
「ズームしても 1.5px の経路線」という要件を満たせない。
**セグメントごとに 1 インスタンスのクアッド**を発行し、投影後のクリップ空間で
線分に垂直な方向へ `half_width_px` だけ広げる。

```rust
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct SegmentInstance {
    pub a: [f32; 3],          //  0..12  world start
    pub b: [f32; 3],          // 12..24  world end
    pub color: u32,           // 24..28  RGBA8
    /// bits 0..16 = half width in 1/256 physical pixels; bits 16..32 = style flags
    pub width_style: u32,     // 28..32
}
const _: () = assert!(core::mem::size_of::<SegmentInstance>() == 32);
```

キャップとジョインはフラグメント側の「線分への距離」SDF で丸く処理する。
マイター計算を持ち込まない。トレイルは `alpha = 1.0` の premultiplied で描き、
ジョインでの二重ブレンドを避ける。

#### 頂点数の見積り

| 状況 | セグメント数 | インスタンス VS 起動数 (×4) | バッファ |
|---|---|---|---|
| 選択演者のみ（既定、〜50人 × 15） | 750 | 3,000 | 24 KB |
| 現在の遷移・全 1,000人 × 16 サンプル | 15,000 | 60,000 | 480 KB |
| 現在の遷移・全 4,000人 × 16 サンプル | 60,000 | 240,000 | 1.9 MB |
| 全 64 セット × 全 1,000人（病的） | 960,000 | 3,840,000 | 30 MB |

最後のケースは**視覚的にも無意味**（画面が線で埋まる）であり、性能以前に UX として排除する。

#### LOD ポリシー（定数はコードに定義し、テストで参照する）

```rust
pub struct TrailLod {
    /// Above this, Douglas-Peucker simplification kicks in. Default 2_000.
    pub simplify_threshold: usize,
    /// Simplification tolerance, in physical pixels at the current camera scale. Default 0.5.
    pub simplify_epsilon_px: f32,
    /// Above this, routes collapse to a straight chord and short routes are dropped. Default 50_000.
    pub collapse_threshold: usize,
    /// Routes whose projected length is below this are not drawn at all. Default 4.0 px.
    pub min_route_px: f32,
    /// Hard cap. Beyond this, only selected performers get trails. Default 64_000.
    pub max_segments: usize,
}
```

- L0（≤ 2,000 セグメント）: 間引きなし。
- L1: Douglas–Peucker で ε = 0.5px 相当に簡約。**カメラのスケール段（対数 8 段に量子化）ごとに
  キャッシュ**し、毎フレームは再計算しない。カメラが同じ段に留まる限りコストはゼロ。
- L2（> 50,000）: 各経路を始点-終点の直線に潰す。投影長 < 4px の経路は描かない。
- ハードキャップ 64,000 到達時: 選択中の演者だけに限定し、ステータスバーへ
  「経路表示を選択中の演者に制限しました」と出す（`Locale` 経由、00-conventions #7）。

キャップは**必ず効く**ので、演者数やセット数がいくら増えても
トレイルパスのコストは定数で頭打ちになる。これが §5 の「4,000人でも壊れない」根拠の一部。

#### フィールド線も同じパイプラインで描ける

ヤードライン・ハッシュ（`main.rs:1617-1710` で約 120 本）は同じ `SegmentInstance` で表現できる。
第 1 段階では egui 側に残すが、**3D では CPU 投影されたフィールド線と GPU 投影された演者の間に
サブピクセルのずれが出る**（同じ行列を使っても丸めの経路が違う）。
第 2 段階でフィールド線を GPU パスへ移し、ずれを構造的に消す。§8 の T12 に含める。

### 3.6 テキスト（ドリルナンバーラベル）

**決定: テキストは egui のテキストレイヤに残す。GPU 側で文字を描かない。**

#### 判断の根拠（数値）

現状 1,000 人ぶんのラベル（`main.rs:1490-1496`）は 1〜3 文字、合計およそ 2,500 グリフ。
egui は整形済み `Galley` をフォントキャッシュに保持し、tessellation では
グリフ 1 個 = クアッド 1 個（4 頂点 / 6 インデックス）を**単一のフォントアトラス**から出す。
つまり 2,500 グリフ = 10,000 頂点 = 200 KB、egui 側でドローコール 1 回。

これに対し、現在ドットが積んでいる頂点数は桁違いに多い:

| 現状の描画物 | 1人あたり頂点数（フェザリング込み概算） | 1,000人合計 |
|---|---|---|
| `circle_filled`（32分割ファン + フェザー） | ≈ 66 | 66,000 |
| `circle_stroke`（選択時） | ≈ 66 | 最大 66,000 |
| `text`（2.5 グリフ） | ≈ 10 | 10,000 |

**テキストは全体の 7% 程度でしかない。** 支配項はドットである。
GPU テキストを自前実装するには、シェーピング（rustybuzz / cosmic-text）、
グリフアトラス管理、そして**日本語フォントのフォールバック**（`main.rs:34-49` で既に導入済み）を
すべて自前で持つ必要がある。0.5〜1.0ms のために製品リスクを負う取引として割に合わない。

#### 代わりにやること: ラベル LOD

1,000 個のラベルは**そもそも読めない**（8px フォントが重なる）。これは性能問題である前に UX 問題である。

```rust
pub struct LabelLod {
    /// Below this glyph height in physical pixels, labels are not drawn. Default 9.0.
    pub min_glyph_px: f32,
    /// Hard cap on simultaneously drawn labels. Default 250.
    pub max_labels: usize,
}
```

描画対象の優先順位: 選択中 → ホバー中 → アクティブサブセット → 残りをインデックス順。
上限 250 に達したら打ち切る。

**実測見積り**:

| 条件 | ラベル数 | グリフ数 | 頂点数 | egui tessellation 概算 |
|---|---|---|---|---|
| LOD なし・1,000人 | 1,000 | 2,500 | 10,000 | 0.5 – 1.0 ms |
| LOD なし・4,000人 | 4,000 | 10,000 | 40,000 | 2.0 – 4.0 ms |
| **LOD あり（本設計）** | **≤ 250** | **≤ 625** | **≤ 2,500** | **≈ 0.15 ms** |

`0.15ms` を予算計上する（§5）。LOD ありなら演者数が 4,000 でもラベルのコストは変わらない。

**逃げ道**（§9 に記録）: もし将来「4,000人全員のラベルを強制表示」が要件になったら、
まず `LayoutJob` を `PerformerId` でキャッシュして再シェーピングを消す（まだ egui 側）。
それでも足りない場合に初めて GPU テキストを検討する。

### 3.7 リサイズ・DPI・デバイスロスト・最小化

#### リサイズ

**我々は画面サイズのテクスチャを一切持たない。** したがってリサイズ対応は「何もしない」が正解になる。
サーフェスの再構成・深度テクスチャ・MSAA テクスチャの再生成はすべて egui-wgpu が持つ
（`egui-wgpu-0.35.0/src/winit.rs:324-380`）。

唯一の要件は、`View::viewport` を**毎フレーム `PaintCallbackInfo` から取り直す**こと。
キャッシュしてはいけない。

```rust
fn paint(&self, info: PaintCallbackInfo, pass: &mut wgpu::RenderPass<'static>,
         res: &CallbackResources) {
    let vp = info.viewport_in_pixels();   // epaint-0.35.0/src/viewport.rs:4-21
    // vp.left_px / top_px / width_px / height_px : i32, physical pixels
    ...
}
```

将来オフスクリーンのピッキングバッファを持つ場合は、`prepare` で
`screen_descriptor.size_in_pixels` の変化を検出して作り直す。`prepare` は `&mut` を持つ。

#### DPI

`PaintCallbackInfo::pixels_per_point` を使う。**本設計に出てくる "px" はすべて物理ピクセル**である。
UI が扱う論理ポイント（ドット半径の設定値など）から物理ピクセルへの変換は
`prepare` の 1 箇所だけで行う。これを分散させると 200% スケーリングで半分のサイズになる
バグが必ず出る（`PRODUCT_QUALITY.md:42` が 100–200% の検証を要求している）。

```rust
// GpuRenderer::prepare() 冒頭、唯一の変換点
let ppp = screen_descriptor.pixels_per_point;
let min_px = self.style.min_dot_points * ppp;   // 1.5pt -> px
let max_px = self.style.max_dot_points * ppp;   // 22.0pt -> px
```

#### デバイスロスト

wgpu 29 の実 API:

```rust
// wgpu-29.0.4/src/api/device.rs:591
device.set_device_lost_callback(move |reason: wgpu::DeviceLostReason, msg: String| { ... });
// wgpu-29.0.4/src/api/device.rs:418
device.on_uncaptured_error(Arc::new(move |err: wgpu::Error| { ... }));
```

両方を `install()` 時に登録する。既定の未捕捉エラーハンドラは**パニックする**ので、
上書きしないとドライバの不調がそのままクラッシュになる。00-conventions の「パニック禁止経路」に反する。

方針:

1. どちらのコールバックも `Arc<AtomicU8>` の `GpuHealth` を `Degraded` へ落とすだけにする
   （コールバックは任意のスレッドから、任意のタイミングで呼ばれうる。ロックを取らない）。
2. 次のフレームの `prepare` で `GpuHealth` を読み、`Degraded` なら
   **その場で CPU バックエンド（§3.8）へ切り替える**。以後 `Callback` を積まない。
3. 非モーダルのバナーを出す:
   「GPU が失われました。CPU 描画に切り替えました。編集内容は失われていません。」
4. デバイスの再作成はしない。デバイスは egui-wgpu/eframe が所有しており、
   我々が横から作り直すと二重管理になる。再起動で復帰する、と案内する。

**安全性の核心**: 我々の GPU 状態は `Document` + count + camera から**完全に再構築可能**な派生データであり、
真実の情報源を一切持たない。したがって**デバイスロストでユーザーのデータが失われることは構造的にありえない**。
これは §4 の不変条件 1 としてテストする。

#### 最小化・ゼロサイズ

`egui-wgpu-0.35.0/src/renderer.rs:575` により、`viewport_px.width_px > 0 && height_px > 0` の
ときしか `paint` は呼ばれない。したがって最小化中に `paint` は走らない。

**しかし `prepare` は走る。** `prepare` はレンダーパスの外で全コールバックぶんまとめて実行されるので、
ビューポートが 0 でも呼ばれる。よって `prepare` 側にガードが要る:

```rust
let [w, h] = screen_descriptor.size_in_pixels;
if w == 0 || h == 0 {
    return Vec::new();   // no upload, no division by zero in the aspect ratio
}
```

これが無いと `aspect = w / h` で 0 除算 → `inf` → 行列に `inf` → GPU へ非有限値が流れる（§6 参照）。

### 3.8 フォールバック

3 段構えにする。

#### 段 1: ハードウェアアダプタ（既定）

`WgpuSetupCreateNew::native_adapter_selector`（`egui-wgpu-0.35.0/src/setup.rs:191-199`）で
明示的に順序を決める。既定の `PowerPreference` 任せにしない。

```rust
// NativeOptions::wgpu_options.wgpu_setup を組み立てる
fn select_adapter(
    available: &[wgpu::Adapter],
    compatible: Option<&wgpu::Surface<'_>>,
) -> Result<wgpu::Adapter, String> {
    use wgpu::DeviceType::*;
    let rank = |a: &wgpu::Adapter| match a.get_info().device_type {
        DiscreteGpu   => 0,
        IntegratedGpu => 1,
        VirtualGpu    => 2,
        Cpu           => 3,   // WARP / lavapipe / SwiftShader
        Other         => 4,
    };
    available.iter()
        .filter(|a| compatible.is_none_or(|s| a.is_surface_supported(s)))
        .min_by_key(|a| rank(a)).cloned()
        .ok_or_else(|| "no compatible wgpu adapter".to_owned())
}
```

#### 段 2: ソフトウェア / 互換アダプタで動く

`Backends::GL` は既に有効（§2.1）。リモートデスクトップや仮想マシンでは、
Windows なら「Microsoft Basic Render Driver」（WARP、DX12、`DeviceType::Cpu`）が見えるのが普通で、
Linux VM では lavapipe（Vulkan、`DeviceType::Cpu`）が見える。**これらは動く。ただし遅い。**

`adapter.get_info().device_type == Cpu` を検出したら、起動時に自動で**低負荷プロファイル**へ落とす:

```rust
pub struct RenderProfile {
    pub target_fps: u32,            // Cpu adapter: 30, otherwise 60
    pub ground_shadows: bool,       // Cpu adapter: false
    pub msaa: u32,                  // Cpu adapter: 1
    pub trail_lod: TrailLod,        // Cpu adapter: max_segments = 4_000
    pub label_lod: LabelLod,        // Cpu adapter: max_labels = 120
}
```

ステータスバーに「ソフトウェア描画で動作中（表示品質を下げています）」と表示する。
黙って遅くしない。

#### 段 3: wgpu がまったく初期化できない

**`eframe` は自動フォールバックしない。** `eframe-0.35.0/src/lib.rs:314-327` は
`Renderer` の値で分岐して `run_wgpu` / `run_glow` を呼ぶだけで、失敗時の再試行は無い。
アプリ側で書く必要がある。

```rust
// crates/drill-app/src/main.rs
fn main() -> eframe::Result {
    let mut opts = native_options();
    opts.renderer = eframe::Renderer::Wgpu;
    match eframe::run_native(APP_NAME, opts.clone(), boxed_app()) {
        Err(err) => {
            log::warn!("wgpu backend failed ({err}); retrying with glow");
            opts.renderer = eframe::Renderer::Glow;
            eframe::run_native(APP_NAME, opts, boxed_app())
        }
        ok => ok,
    }
}
```

これには `crates/drill-app/Cargo.toml:9` に `glow` feature の追加が要る（現状は無い）:

```toml
eframe = { version = "0.35.0", default-features = false,
           features = ["wgpu", "glow", "wayland", "x11"] }
```

glow で動いているときに `egui_wgpu::Callback` を積むと、
`renderer.rs:562` の `downcast_ref::<Callback>()` が失敗して**黙って描画されない**（`continue`）。
したがってアプリは自分がどのバックエンドで走っているかを知らねばならない:

```rust
pub enum RenderBackend {
    /// wgpu instanced path via `egui_wgpu::CallbackTrait`.
    Gpu(drill_gpu::GpuHandle),
    /// DisplayList -> egui::Shape. Also the path used after device loss.
    EguiShapes,
}
```

`cc.wgpu_render_state()` が `None` なら `EguiShapes`。
`EguiShapes` は `DisplayList` を `egui::Shape` へ変換するだけの薄いバックエンドで、
**これは現状のコードとほぼ同じもの**である。つまり実装コストは「今あるものを DisplayList 経由に付け替える」だけ。
販売製品として必須という要求は、この二重化で満たす。

#### まとめ

| 環境 | 経路 | 想定品質 |
|---|---|---|
| 通常の PC（DX12 / Vulkan / Metal） | GPU インスタンシング | 1,000人 60fps |
| 古い GPU、GL しか無い | GPU インスタンシング（GL バックエンド） | 1,000人 60fps（属性 5 個は GLES3.0 の範囲内） |
| RDP / VM（WARP / lavapipe） | GPU インスタンシング + 低負荷プロファイル | 1,000人 30fps、影・MSAA なし |
| wgpu 初期化失敗 | glow + `EguiShapes` | 数百人で実用、1,000人は劣化 |
| デバイスロスト（実行中） | `EguiShapes` へ動的切替 | 同上。データ保護は完全 |

### 3.9 オフスクリーン描画と動画書き出しの境界

#### 同じレンダラをヘッドレスで使えるか — 使える

`drill-gpu` はサーフェスにも egui にも依存しない（§3.0）。公開 API を
「テクスチャビューへ描く」形にしておけば、ウィンドウの有無は関係なくなる。

```rust
// drill-gpu/src/lib.rs
pub struct RenderTarget<'a> {
    pub color: &'a wgpu::TextureView,
    pub depth: Option<&'a wgpu::TextureView>,
    pub viewport: ViewportPx,
    pub sample_count: u32,
}

impl GpuRenderer {
    /// Upload per-frame data. Must be called before `render`. Allocation-free once warmed.
    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        frame: &GpuFrame,
    ) -> Result<(), DrillError>;

    /// Record draw commands into an existing render pass (windowed: egui's pass).
    pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>, viewport: ViewportPx);

    /// Headless: own the pass. Used by drill-export.
    #[cfg(feature = "headless")]
    pub fn render_to_texture(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        target: &RenderTarget<'_>,
        clear: [f64; 4],
    );
}
```

ウィンドウ経路の `CallbackTrait::paint` は `draw()` を呼ぶだけの 5 行になる。
ヘッドレス経路は `render_to_texture()` を呼ぶ。**シェーダもインスタンス生成も 100% 共有される。**

読み出しは `egui-wgpu-0.35.0/src/capture.rs:143-220` と同じ手順を踏む（コードは流用しない、egui 結合のため）:
`COPY_SRC` 付きテクスチャ → `copy_texture_to_buffer`（行を `wgpu::COPY_BYTES_PER_ROW_ALIGNMENT` = 256 に整列）
→ `map_async(MapMode::Read)` → `device.poll(wgpu::PollType::Wait)` → 行のパディング除去。

#### 31-video-export.md との境界

| 事項 | 担当 |
|---|---|
| `DisplayList` → GPU テクスチャ、テクスチャ → RGBA8 バイト列 | **本書 / `drill-gpu`** |
| `DisplayList` → RGBA8（CPU ラスタライザ）| 31 |
| フレーム列の生成順序、TempoMap からの有理数フレーム時刻 | 31 |
| FFmpeg プロセス起動・pipe・mux・進捗・キャンセル・ffprobe 検証 | 31 |
| どちらのラスタライザを使うかの選択と、その記録 | 31（本書は要件を提示） |

`drill-gpu` は「テクスチャ 1 枚ぶんの RGBA8 を返す」以上のことをしない。

---

### 3.10 決定論 — 結論

これは本書で最も重要な論点なので、明確に決める。

#### 事実認識

GPU の描画結果を**環境間でビット一致させることはできない**。理由:

- WGSL/SPIR-V は `+ - * /` については IEEE-754 だが、ドライバは自由に **FMA へ融合**でき、
  融合の有無で最下位ビットが変わる。融合はコンパイラ最適化の結果なので、
  同じ GPU でもドライバ更新で変わりうる。
- `sqrt` / `inverseSqrt` / 超越関数の精度は仕様上 ULP 許容範囲でしか規定されない。
- ラスタライザのフィルルール、サブピクセル精度（多くは 8 サブビットだが規定ではない）、
  ブレンドの丸めは実装依存。
- MSAA のサンプル位置はベンダごとに異なる。

したがって「GPU で描いた動画がどの PC でもバイト一致する」という約束は**してはいけない**。
約束すればいずれ嘘になる。

#### 決定

**1. 決定論の契約を `DisplayList` の層に定義する。画素の層には定義しない。**

00-conventions の不変条件 5「同じ `(Document, config, count)` からは常に同じ出力
（DisplayList・フレーム・座標表）が出る」は、`drill-render::build()` が純粋関数であることで満たす。
ゴールデンテストは `DisplayList` を突き合わせる（`f32` のビット一致で比較できる）。
**GPU のスクリーンショットをゴールデンにしない。**

**2. 動画書き出しの既定は CPU ラスタライザとする。GPU は既定ではない。**

`drill-export` の参照実装は `DisplayList → RGBA8` の**ソフトウェアラスタライザ**（31 の担当）。
整数/固定小数のカバレッジ計算、固定のタイル走査順、FMA 非依存の演算にすれば、
**どの OS・どの CPU でもバイト一致**する。これで:

- `MEDIA_PIPELINE.md:79`「同じproject/configから同じフレーム列を生成する」を満たす
- 「GPU が無い環境でも書き出せる」を満たす（GPU 経路が存在しなくても製品が成立する）

つまり **CPU ラスタは "フォールバック" ではなく "参照実装" である。** これが二系統保守の根拠になる。
GPU 経路が壊れても製品の出力機能は死なない。

**3. GPU 書き出しは opt-in の加速オプションとし、非参照であることを明示する。**

4K・3分・60fps は 10,800 フレーム。GPU は 10〜30 倍速いので、これを禁止するのは製品として損失が大きい。
許すが、条件を付ける:

- 設定名は「GPU 高速書き出し（実験的）」。既定 OFF。
- 出力ジョブのログとサイドカーに `raster_backend: "gpu" | "cpu"` と
  `adapter: "<AdapterInfo>"` を記録する。
- **中断再開（`MEDIA_PIPELINE.md` P2）で `raster_backend` が異なる再開を拒否する。**
  1 本のファイルの中で 2 つのラスタライザのフレームが混ざると、
  切り替わり点で微細な色差が見え、しかも原因究明がほぼ不可能になる。
- 「同じ設定で 2 回書き出してもバイト一致するとは限りません」を設定画面に書く。

**4. プレビュー（画面表示）に決定論を要求しない。** プレビューは成果物ではなく view である。

**5. 二系統が乖離しないようにコンフォーマンステストで縛る。閾値は 2 段構えにする。**

```rust
pub struct ConformanceBound {
    /// Fraction of pixels allowed to differ at all. Default 0.005 (0.5%).
    pub max_differing_fraction: f64,
    /// Max per-channel absolute difference on any pixel. Default 2 (of 255).
    pub max_channel_delta: u8,
    /// Max distance between a rendered dot's centroid and the DisplayList's
    /// `Dot::center`, in physical pixels. Default 0.5. **This one is exact-ish
    /// and is what actually protects correctness.**
    pub max_centroid_error_px: f32,
}
```

測光的な閾値（`≤ 2/255` が 99.5% の画素）は緩い。厳しく縛るのは**幾何**の方である:
レンダリング結果を連結成分解析してドットの重心を求め、`DisplayList` の `Dot::center` と
0.5px 以内で一致することを要求する。**演者が違う場所に描かれるのはバグだが、
1/255 暗いのはバグではない。** テストはその区別を反映すべきである。

CI では CPU バックエンドのコンフォーマンスを常時、GPU バックエンドは
アダプタを固定した自己ホストランナ（Linux + lavapipe、Windows + WARP）でのみ回す。
再現可能なアダプタでしかゴールデンを取らない。

**6. 位置を決める計算に超越関数を使わない。**

シェーダがやるのは行列×ベクトル、加算、比較、`length`（＝ `sqrt`、SDF の**見た目**にしか効かない）だけ。
カメラの三角関数は CPU 側（`drill_core::camera`）でフレームあたり 1 回だけ評価する。
これにより CPU 経路と GPU 経路の幾何的一致が高精度で保たれ、上の 0.5px 判定が余裕を持って通る。

---

## 4. 不変条件

テストで検証できる形で列挙する。括弧内は §7 の対応項目。

1. **GPU は真実を持たない。** `GpuFrame` は `(Document, count, Camera, RenderOptions)` から
   完全に再構築できる。GPU リソースを全破棄しても文書は無傷（T-fault-1）。
2. **`paint` は GPU 状態を変更しない。** `CallbackTrait::paint` は `&CallbackResources` しか
   受け取らないので型で保証される。バッファ生成・書き込みは `prepare` のみ（コンパイル時保証）。
3. **インスタンス数 = `DisplayList` の `Dot` 数。** 不一致は `debug_assert_eq!`（T-unit-3）。
4. **定常状態でフレーム内ヒープ確保ゼロ。** バッファ再確保は容量超過時のみ、
   容量は power-of-two 成長。ソートは事前確保スクラッチ（T-stress-1、確保カウンタで検証）。
5. **同一 `DisplayList` → 同一インスタンスバイト列。** `bytemuck::cast_slice` の結果が
   バイト単位で一致する（T-unit-2）。これは GPU 非依存に検証できる決定論の実体。
6. **すべての px は物理ピクセル。** 論理ポイント→物理ピクセルの変換は
   `GpuRenderer::prepare` の冒頭 1 箇所のみ（T-dpi-1）。
7. **`paint` は入場時の GPU 状態を仮定しない。** パイプライン・バインドグループ・
   頂点バッファを毎回設定する（コードレビュー項目 + validation layer で検出）。
8. **シェーダは位置決定に超越関数を使わない。** WGSL に `sin`/`cos`/`pow`/`exp` が
   出現しないことを CI の grep で強制（T-unit-7）。
9. **描画順は決定的。** 基数ソートは stable、同深度はインスタンス索引順（T-prop-2）。
10. **非有限値は GPU に到達しない。** `prepare` で `is_finite()` 検査を通す（T-unit-6、T-fuzz-1）。
11. **インスタンス数は `MAX_PERFORMERS` を超えない。** 超過分は描画せず警告（T-sec-1）。

---

## 5. 性能

### 5.1 現状のコスト（本設計が置き換えるもの）

`main.rs:1474-1497` / `main.rs:396-406` の 1,000 人ループ:

| 項目 | 概算 |
|---|---|
| `circle_filled` ×1,000（32分割 + フェザー ≈ 66 頂点） | 66,000 頂点 |
| `circle_stroke`（全選択時） | +66,000 頂点 |
| `text` ×1,000（≈2.5 グリフ） | +10,000 頂点 |
| 合計 | **約 142,000 頂点** |
| epaint tessellation（単一スレッド、概ね 15M 頂点/秒） | **約 9.5 ms** |
| 3D の毎フレーム `Vec<usize>` 確保 + `sort_by`（距離を比較ごとに 2 回再計算） | **約 0.1 ms + 確保 1 回** |

**16.6ms のうち約 58% を演者ドットだけで消費している。** これが本書の存在理由である。
補間そのものは 0.154µs（§2.4）で無関係。

### 5.2 本設計のコスト — 1,000 人、1080p、3D、再生中

**CPU（16.6ms の取り分）**

| 項目 | 典型 | トレイル全表示時 |
|---|---|---|
| 位置補間 `positions_at`（実測値、§2.4） | 0.15 µs | 0.15 µs |
| `DisplayList` → `PerformerInstance[]`（1,000 × 約 20ns、32KB 線形書き込み） | 20 µs | 20 µs |
| 深度基数ソート（2 パス、確保なし） | 4 µs | 4 µs |
| `Queue::write_buffer` 32 KB | 10 µs | 10 µs |
| トレイル `SegmentInstance[]` 生成 | 2 µs（選択 750 本） | 360 µs（15,000 本） |
| egui: フィールド線 約 120 本 → 約 1,000 頂点 | 70 µs | 70 µs |
| egui: ラベル ≤250（§3.6） | 150 µs | 150 µs |
| egui: パネル・タイムライン・HUD | 300 µs | 300 µs |
| egui: 残り UI の tessellation + アップロード | 250 µs | 250 µs |
| コマンド記録（ドロー 3 本 + 状態設定） | 15 µs | 15 µs |
| **CPU 合計** | **≈ 0.82 ms** | **≈ 1.18 ms** |

**予算宣言: 本設計は 16.6ms のうち CPU 側 2.0ms 以内を使う。**
典型 0.82ms、最悪 1.18ms なので 1.7 倍の余裕を持つ。
残る 14.6ms は補間・解析（13-collision）・入力処理・自動保存の非同期投入・
音声スレッドとのロックフリー受け渡しに残る。

**GPU**

1080p、統合 GPU（実効フィルレート 1.5 GPix/s と保守的に仮定、SDF は約 20 ALU/フラグメント）:

| パス | フラグメント数 | 時間 |
|---|---|---|
| 接地影（1,000 × 196px クアッド、単純シェーダ） | 0.20 Mpx | 0.09 ms |
| 演者ドット（1,000 × 平均 14px 径 → 196px クアッド） | 0.20 Mpx | 0.13 ms |
| トレイル（典型 750 本 × 約 1.5×40px） | 0.05 Mpx | 0.03 ms |
| トレイル（最悪 15,000 本） | 0.90 Mpx | 0.60 ms |
| egui 自身の UI パス | — | 0.50 ms |
| **GPU 合計** | | **典型 0.75 ms / 最悪 1.32 ms** |

頂点シェーダ起動数は典型 (1,000 + 1,000 + 750) × 4 = 11,000。無視できる。
**ドローコールは 3 本**（影・ドット・トレイル）。現状の実質「1,000 個の shape」から 3 桁の削減。

### 5.3 4,000 人でも壊れない根拠

演者数に**線形に増える量**は次の 3 つしかない:

| 量 | 4,000人での値 |
|---|---|
| インスタンスバッファ | 125 KiB（`max_buffer_size` 256MiB の 0.05%） |
| `write_buffer` の memcpy | 約 35 µs |
| インスタンス生成 + 基数ソート | 80 µs + 15 µs |

**演者ごとのドローコールが無い。演者ごとのヒープ確保が無い。演者ごとの状態変更が無い。**
これが「壊れない」の構造的な理由である。CPU 合計は約 1.3ms（典型比 +0.5ms）。

線形に増えない量は、すべて**明示的な定数**で頭打ちになる:

| 量 | 上限定数 | 4,000人での挙動 |
|---|---|---|
| ラベル | `LabelLod::max_labels = 250` | 変化なし（1,000人時と同じ） |
| トレイル | `TrailLod::max_segments = 64_000` | 選択演者のみに縮退、警告表示 |
| ドット径 | `min_dot_points = 1.5` | 下限に張り付く（消えない） |
| インスタンス総数 | `MAX_PERFORMERS = 16_384` | 超過分は描画せず警告 |

GPU 側のフラグメントは 4,000 × 196 = 0.78 Mpx（+影で 1.56 Mpx）→ 統合 GPU で約 1.0 ms。
1080p 全画面塗り 1 回が 2.07 Mpx なので、**演者 4,000 人は全画面塗り 0.75 回ぶん**でしかない。

**劣化の仕方**（「劣化してよいが壊れてはいけない」への回答）:
最初にトレイルが選択限定へ落ち、次にラベルが 250 で頭打ちになり、
最後まで**演者ドットは 1 人も欠けない**。ドットが消える条件は存在しない。

ソフトウェアアダプタ（WARP、実効約 100 MPix/s）: 0.4 Mpx → 約 4ms。
30fps プロファイル（33ms 予算）で余裕がある。影を切って 0.2 Mpx にすればさらに半減。

### 5.4 メモリ

| 用途 | 1,000人 | 4,000人 |
|---|---|---|
| インスタンスバッファ（容量 = next_pow2） | 32 KiB | 128 KiB |
| セグメントバッファ（典型 / 上限） | 24 KiB / 2 MiB | 24 KiB / 2 MiB |
| ユニフォーム（256B スロット × 4） | 1 KiB | 1 KiB |
| 深度バッファ 1080p Depth32Float（egui-wgpu 所有） | 8.3 MiB | 8.3 MiB |
| CPU 側スクラッチ（instances / segments / sort） | 約 90 KiB | 約 300 KiB |
| **合計（深度を除く）** | **< 0.2 MiB** | **< 2.5 MiB** |

`PRODUCT_QUALITY.md:21`「2時間連続再生で常駐メモリの継続増加なし」は、
定常状態で新規確保が発生しないこと（不変条件 4）から従う。ストレステストで検証する（T-stress-1）。

---

## 6. 失敗モードと安全性

| # | 失敗 | 検出 | 対処 |
|---|---|---|---|
| F1 | アダプタが 1 つも取れない | `run_native` が `Err` | glow で 1 回だけ再試行（§3.8 段3）。両方失敗ならエラーダイアログを出し、自動保存データは無傷のまま終了 |
| F2 | デバイスロスト（ドライバリセット、GPU 抜去、Optimus 切替） | `set_device_lost_callback`（`wgpu/src/api/device.rs:591`） | `GpuHealth = Degraded` → 次フレームで `EguiShapes` へ切替 + バナー。**文書は派生元なので無傷** |
| F3 | 未捕捉の検証エラー / OOM | `on_uncaptured_error`（`:418`） | 既定のパニックハンドラを**必ず上書き**する。ログ化して F2 と同じ経路へ。00-conventions「パニック禁止経路」 |
| F4 | シェーダコンパイル失敗（古い GL ドライバの WGSL→GLSL 変換） | `create_render_pipeline` は非同期に失敗しうるので、`push_error_scope` / `pop_error_scope` で起動時に 1 回検証 | 失敗したら起動時点で `EguiShapes` を選ぶ。実行中に初めて気づく事態を避ける |
| F5 | **非有限座標（NaN / Inf）が GPU に届く** | `prepare` で `world`/`radius` の `is_finite()` を検査 | 非有限なら `(0,0,0)` / `radius = 0` に落とし、警告カウンタを上げる。**これは最重要**: NaN 頂点は無限大の三角形を生み、実質的にドライバをハングさせる（TDR → F2）。`DisplayList` 側の検証を信用せず**二重防御**する |
| F6 | 敵性プロジェクトファイルの巨大な演者数 | `prepare` で `instances.len() > MAX_PERFORMERS` | 先頭 `MAX_PERFORMERS` 件だけ描画し、「表示を 16,384 人に制限しました」を出す。`drill-core` の検証に依存しない二重防御 |
| F7 | ゼロサイズ / 最小化 | `screen_descriptor.size_in_pixels` に 0 | `prepare` の冒頭で早期 return。アスペクト比の 0 除算を防ぐ（§3.7） |
| F8 | MSAA サンプル数 / カラーフォーマットの不一致 | wgpu の検証層がレンダーパスで拒否 | `install()` で `RenderState::target_format` と `NativeOptions::multisampling` から構築し、値をアサート。デバッグビルドで `debug_assert_eq!` |
| F9 | ダウンレベルアダプタの制限超過（頂点属性数・ユニフォームサイズ） | `adapter.limits()` を起動時に検査 | 頂点属性は 5 個（GLES3.0 保証 16 の範囲内）、ユニフォームは 96B（保証 16KiB の範囲内）。設計上超えない。検査は回帰防止 |
| F10 | トレイルの爆発（全セット × 全演者） | `segments.len() > TrailLod::max_segments` | 選択演者のみに縮退（§3.5）。無限に確保しない |
| F11 | GPU 書き出しと CPU 書き出しのフレームが 1 ファイル内で混ざる | 再開状態の `raster_backend` を照合 | バックエンドが異なる再開を拒否（§3.10 決定 3） |
| F12 | 深度バッファ無しのビルドで 3D を描く | `depth_format: None` かつ 3D モード | パイプライン作成時に判定し、3D モードを無効化して 2D に固定（3D タブを disable + 理由をツールチップに表示） |

**信頼できない入力の扱い**: `drill-gpu` は `DisplayList` しか受け取らないので、
パス・ファイル・シリアライズ形式には触れない。攻撃面は**数値の非有限性と要素数**の 2 つだけであり、
F5 と F6 でどちらも塞ぐ。`unwrap` / `expect` / 添字アクセスは公開 API 経路に置かない
（`Result<_, DrillError>`、00-conventions #8）。

---

## 7. テスト計画

### 単体（GPU 不要）

| ID | 内容 |
|---|---|
| T-unit-1 | `size_of::<PerformerInstance>() == 32`、`size_of::<SegmentInstance>() == 32`、`bytemuck::Pod` が導出されている |
| T-unit-2 | **バイト列ゴールデン**: 固定の `DisplayList` から生成したインスタンス列を 16 進で突き合わせる。不変条件 5 の実体 |
| T-unit-3 | インスタンス数 == `DisplayList` の `Dot` 数 |
| T-unit-4 | `InstanceFlags` のパック/アンパック往復（`proptest`: symbol 0..6 × selected × warn 0..4 × index 0..16384） |
| T-unit-5 | row-major → column-major 転置の往復が恒等。および `perspective_wgpu` が `z=-near → ndc.z=0`、`z=-far → ndc.z=1` を満たす |
| T-unit-6 | 非有限座標を含む `DisplayList` を通しても、出力インスタンスの全 `f32` が `is_finite()` |
| T-unit-7 | WGSL ソースに `sin(` / `cos(` / `pow(` / `exp(` が出現しない（不変条件 8 の CI 強制） |
| T-unit-8 | `pixels_per_point` を 1.0 / 1.5 / 2.0 で変えると `min_px`/`max_px` が比例する |

### property

| ID | 内容 |
|---|---|
| T-prop-1 | 基数ソートの結果が、同じキーでの安定比較ソートと完全一致（1..4096 個のランダム深度、重複あり） |
| T-prop-2 | 同深度の要素はインスタンス索引順を保つ（stable） |
| T-prop-3 | 任意の `DisplayList` に対し `prepare` を 2 回呼んでも同じバイト列（べき等・決定的） |
| T-prop-4 | Douglas–Peucker 簡約後の全点が元の折れ線から ε 以内 |

### ゴールデン（ヘッドレス GPU）

固定アダプタ（CI: Linux + lavapipe / Windows + WARP）で 8 シーンを 512×512 に描き PNG 比較。

| ID | シーン |
|---|---|
| T-gold-1 | 記号 6 種を 1 行に、半径 4px / 12px / 22px の 3 行 |
| T-gold-2 | 選択リング・警告 3 段階・ghost・dimmed の組み合わせ |
| T-gold-3 | 2D 正射・1,000 人ブロック |
| T-gold-4 | 3D 透視・1,000 人・接地影あり |
| T-gold-5 | 深度の重なり（前後 3 人が重複） |
| T-gold-6 | トレイル（直線・曲線・自己交差） |
| T-gold-7 | 1.5px 下限に張り付いた遠景ドットが消えていない |
| T-gold-8 | DPI 2.0 で T-gold-3 と幾何が一致（サイズが 2 倍） |

判定は `ConformanceBound`（§3.10 決定 5）: 測光は `≤2/255` が 99.5% 以上、
**幾何は重心誤差 ≤ 0.5px（連結成分解析）**。

### コンフォーマンス（CPU ラスタ vs GPU ラスタ）

| ID | 内容 |
|---|---|
| T-conf-1 | T-gold-1..8 の同一 `DisplayList` を両バックエンドで描き `ConformanceBound` で比較 |
| T-conf-2 | CPU ラスタ単体の**バイト一致**（同じ入力を 2 回 → 完全一致）。決定論の本体 |
| T-conf-3 | CPU ラスタが Windows / Linux / macOS のランナー間でバイト一致（`MEDIA_PIPELINE.md:79` の直接検証） |

### ストレス

| ID | 内容 |
|---|---|
| T-stress-1 | 4,000 人 × トレイル全表示 × 1,000 フレーム連続。カスタム `GlobalAlloc` で**定常状態の確保回数 0** を検証。wgpu の検証エラー 0 |
| T-stress-2 | 2 時間ぶん（432,000 フレーム）相当をヘッドレスで回し、RSS の単調増加が無い |
| T-stress-3 | 演者数を 1 → 16,384 → 1 と往復させ、バッファの成長と再利用が正しい |
| T-stress-4 | ウィンドウを 0px と 4K の間で 1,000 回リサイズしてもクラッシュしない |

### 障害注入

| ID | 内容 |
|---|---|
| T-fault-1 | `GpuHealth` を強制的に `Degraded` にすると `EguiShapes` へ切り替わり、**その後の `Document` が編集前と完全一致**（不変条件 1） |
| T-fault-2 | `on_uncaptured_error` を人工的に発火させてもパニックしない |
| T-fault-3 | `wgpu_render_state()` が `None` を返す状況（glow 起動）で、UI が描画され操作できる |
| T-fuzz-1 | `DisplayList` のファザ（NaN / Inf / ±0 / 巨大値 / 要素数 0 / 要素数 100 万）で GPU がハングしない |
| T-sec-1 | 100 万人の `DisplayList` を渡すと 16,384 人に切り詰められ、警告が上がる |

### ベンチ（`criterion`、`crates/drill-gpu/benches/`）

| ID | 内容 | 目標 |
|---|---|---|
| T-bench-1 | `build_instances` 1,000 / 4,000 | ≤ 25µs / ≤ 100µs |
| T-bench-2 | `sort_back_to_front` 1,000 / 4,000 | ≤ 6µs / ≤ 20µs |
| T-bench-3 | `prepare` 全体（write_buffer 込み） 1,000 | ≤ 60µs |
| T-bench-4 | `build_segments` 15,000 本 | ≤ 400µs |
| T-bench-5 | GPU タイムスタンプ（`Features::TIMESTAMP_QUERY`、無ければ skip）でドットパスの GPU 時間 | 1080p 統合 GPU で ≤ 0.3ms |

既存の `crates/drill-core/benches/core_performance.rs` は変更しない。
`PRODUCT_QUALITY.md:28` の再計測コマンドに `cargo bench -p drill-gpu` を追加する。

---

## 8. 実装タスク

1 タスク = 1〜3 時間相当。`→` は依存。

### フェーズ A: 基盤（`drill-gpu` の骨格）

| ID | 内容 | 依存 | 並行 |
|---|---|---|---|
| T1 | `crates/drill-gpu` 作成。`Cargo.toml`（`wgpu 29` / `bytemuck` / `drill-render`、feature `headless`）、`lib.rs`、ワークスペースへ登録 | — | — |
| T2 | `PerformerInstance` / `SegmentInstance` / `ViewUniform` の型定義 + bytemuck + T-unit-1,4 | T1 | T3 と並行不可（T3 が型を使う） |
| T3 | `dots.wgsl` 第1版: クアッド生成 + `sd_circle` のみ。パイプライン生成、ユニフォームバインドグループ | T2 | T6, T7 と並行可 |
| T4 | 記号 SDF 6 種を追加、`flags` デコード、T-gold-1 | T3 | — |
| T5 | 選択リング・警告リング・ghost・dimmed を同シェーダで、T-gold-2 | T4 | — |
| T6 | `perspective_wgpu` / `ortho_wgpu` / `to_wgsl` 転置 + T-unit-5。`drill-core::camera::view_matrix` の `pub` 化を 23 の担当へ要求（本タスクでは代替として `drill-gpu` 内に同等実装を置き、公開され次第差し替え） | T2 | T3, T7 と並行可 |
| T7 | `DepthSortScratch` + 基数ソート + T-prop-1,2, T-bench-2 | T2 | T3, T6 と並行可 |
| T8 | `GpuRenderer::prepare` / `draw`。インスタンス生成（`DisplayList` → `PerformerInstance[]`）、容量成長、`is_finite` 検査、`MAX_PERFORMERS` クランプ。T-unit-2,3,6, T-bench-1,3 | T3, T6, T7 | — |

### フェーズ B: アプリ統合

| ID | 内容 | 依存 | 並行 |
|---|---|---|---|
| T9 | `drill-app/src/gpu_bridge.rs`: `install()` と `DrillCallback: egui_wgpu::CallbackTrait`。`main.rs:1474-1497` の 2D 演者ループを `Callback::new_paint_callback` に差し替え | T8 | — |
| T10 | `NativeOptions::depth_buffer = 32` を設定し、3D パイプラインに深度テスト（書き込み OFF）を追加。`main.rs:291-414` の `draw_stadium` の演者部分を GPU 経路へ。`main.rs:388-394` の毎フレーム `Vec` 確保を削除 | T9 | T11 と並行可 |
| T11 | 接地影パス（動的オフセットユニフォームの 2 スロット目）+ T-gold-4 | T10 | — |
| T12 | `segments.wgsl` + `SegmentInstance` パイプライン（太線 + 丸キャップ SDF）。まずフィールド線を移送し、3D の CPU/GPU 投影ずれを解消 | T3 | T13 と並行可 |
| T13 | トレイル生成 + `TrailLod`（Douglas–Peucker、スケール段キャッシュ、上限、選択限定）+ T-prop-4, T-bench-4 | T12 | — |
| T14 | ラベル LOD（egui 側、`LabelLod`、優先順位付き選抜、上限 250） | T9 | 他と並行可 |

### フェーズ C: 堅牢性

| ID | 内容 | 依存 | 並行 |
|---|---|---|---|
| T15 | `GpuHealth` / `set_device_lost_callback` / `on_uncaptured_error` / `EguiShapes` への動的切替 + バナー。T-fault-1,2 | T9 | T16 と並行可 |
| T16 | 起動時バックエンド選択: `native_adapter_selector`、`DeviceType::Cpu` 検出、`RenderProfile`、`drill-app/Cargo.toml` に `glow` feature 追加、`main()` の再試行。T-fault-3 | T9 | T15 と並行可 |
| T17 | `push_error_scope` によるパイプライン生成の起動時検証（F4） | T8 | 他と並行可 |
| T18 | `MAX_PERFORMERS` クランプと警告、`is_finite` 警告カウンタの UI 表示。T-sec-1, T-fuzz-1 | T8 | 他と並行可 |

### フェーズ D: ヘッドレスと検証

| ID | 内容 | 依存 | 並行 |
|---|---|---|---|
| T19 | `feature = "headless"`: `render_to_texture` + パディング付き読み出し（256B 整列）。31 が呼べる API を確定 | T8 | — |
| T20 | ゴールデンテスト基盤（固定アダプタ、PNG 比較、`ConformanceBound` の重心解析）+ T-gold-1..8 | T19 | — |
| T21 | CPU ラスタとのコンフォーマンス T-conf-1（CPU ラスタ本体は 31 の担当。本タスクは比較器と CI 配線） | T20 | 31 の CPU ラスタ完成待ち |
| T22 | ストレス T-stress-1..4（確保カウンタ付き `GlobalAlloc` を含む） | T19 | T20 と並行可 |
| T23 | `criterion` ベンチ + GPU タイムスタンプ T-bench-1..5。`PRODUCT_QUALITY.md` の再計測手順へ追記 | T8 | T20 と並行可 |

### クリティカルパス

`T1 → T2 → T3 → T8 → T9 → T10 → T11`（約 18 時間）。
T6/T7 は T2 の後、T12/T13 は T3 の後、T15/T16/T17/T18 は T9 の後で、いずれも並行に流せる。
T21 だけが 31（CPU ラスタ）に外部依存する。

### 他文書への要求（本設計が前提とするもの）

| 宛先 | 要求 |
|---|---|
| **20-display-list** | (a) `Dot` に `Symbol` と状態フラグ（selected / warn / ghost / dimmed）を持たせる。(b) `Trail` に `width_px: f32` と発生元の performer 索引を持たせる（LOD とカリングを再導出させないため）。(c) `points_pool` の点は **3D の高さを持てる**こと（`[f32;3]`、または `Trail` ごとの `height: f32`）。2D 専用の `Vec2` にすると 3D トレイルが描けない。(d) `DisplayList` が非有限値を含まないことを保証するか、含みうるなら明記する（`drill-gpu` は二重防御するが、責任の所在は決めたい）。 |
| **23-camera** | `drill_core::camera::Camera::view_matrix()`（`camera.rs:127`、現在 private）を `pub` にすること。投影行列は深度規約が異なるため `drill-gpu` が自前で持つが、ビュー行列は共有したい。 |
| **22-stadium** | スタジアム構造物は深度バッファへ**書き込む**こと（演者ドットは深度テストのみで書き込まない）。描画順はスタジアム → 接地影 → ドット → トレイル。 |
| **31-video-export** | CPU ラスタライザが決定論の**参照実装**であること（§3.10 決定 2）。GPU 経路は opt-in で、`raster_backend` を出力メタデータに記録し、異なるバックエンドでの中断再開を拒否すること（決定 3）。 |
| **00-conventions** | クレート表への `drill-gpu` 追加（§3.0）。 |

---

## 9. 未決事項

| # | 保留した判断 | 決めるために必要な情報 |
|---|---|---|
| 1 | **MSAA を使うか** | 4x MSAA は 1080p で約 33MB のカラー + 33MB の深度を追加し、egui のフェザリングと二重に効いてドットが太って見える可能性がある。統合 GPU・discrete GPU・WARP の 3 環境で `msaa=1` と `msaa=4` の実測比較（GPU 時間と見た目）が要る。**既定は `1` で始める**。 |
| 2 | **`Symbol` enum の置き場所** | `drill-core`（ドメインの一部）か `drill-render`（表示の一部）か。現状どちらにも無い。15-performers と 20 の担当と調整が要る。`drill-gpu` は `u32` としてしか見ないので、どちらでも実装は変わらない。 |
| 3 | **トレイルの点が `Vec2` か `Vec3` か** | 20 の決定待ち。`Vec2` になった場合、`drill-gpu` は高さ 0 を仮定する（3D で地面に貼り付いたトレイルになる。実用上は許容できるが、リフト表現などができない）。 |
| 4 | **ピッキング（クリック選択）を GPU でやるか** | 現在は CPU の線形探索（`main.rs:1499-1510`、1,000 人で毎クリック 1,000 回の距離計算）。1,000 人・2D なら十分。4,000 人・3D（透視で重なる）では要検証。候補は (a) CPU の空間索引、(b) `flags` の索引ビットを R32Uint のオフスクリーンへ描いて 1px 読み出し。(b) は読み出しの 1 フレーム遅延と、GPU 非対応環境での二重実装が要る。**実測してから決める**。 |
| 5 | **3D の演者表現を円板のままにするか** | 本設計はビルボード円板（Pyware の Real View 相当を目指すなら低ポリのヒューマンメッシュも選択肢）。22-stadium の担当範囲だが、決まればインスタンスレイアウトに向き（`heading: f32`）が 1 フィールド増える。32B → 36B になり、パディングを詰めれば 32B のままにもできる。**レイアウト確定前に決めたい**。 |
| 6 | **GPU 高速書き出しを出荷時に有効にするか** | §3.10 決定 3 では既定 OFF。実際の速度差（4K 10,800 フレームで CPU 何分 / GPU 何分）を測ってから、UI での見せ方を決める。差が 3 倍未満なら機能ごと落とす選択もある。 |
| 7 | **タイムスタンプクエリが無いアダプタでの GPU 時間計測** | `Features::TIMESTAMP_QUERY` 非対応環境（多くの GL バックエンド）で、GPU 時間をどう測るか。`Device::poll(Wait)` を挟んだ壁時計計測はパイプラインを壊すので開発ビルド限定にする、が暫定案。 |
| 8 | **フィールド線を完全に GPU へ移すか** | T12 で移す計画だが、ヤード数字テキストは egui に残るため、3D でテキストだけ CPU 投影になる。数字の位置ずれが実際に見えるかは実装後の目視が要る。見えなければ現状維持でよい。 |

# Ghost: Chat Window + Persona Packaging + Lazy Loading

## Context

Three features requested:
1. **Chat window** doesn't match CLAUDE.md spec — needs title bar (decorations), resizable, Winamp-style edge-snapping
2. **Persona packaging** — bundle `assets/persona/name/` as `.zip` with existing `config.toml` manifest defining character name, still image, loading text, and animation states
3. **Lazy loading** — show still image + loading text immediately, load animation frames in background thread

The existing `assets/persona/sasha/config.toml` already defines the manifest format:
```toml
[character]
name="Alexandra"
nick="Sasha"
animationstate=["idle","talk"]
image="./Sasha.png"
loading="Sasha is getting ready"
```

File size: PNG frame sequences will always be larger than .mp4 (no inter-frame compression). This is the trade-off for per-pixel alpha and instant random access. Accept for now.

---

## Phase 1: Fix Chat Window (simple)

### 1.1 Enable decorations
**File:** `src/windows/chat_window.rs:74`
- Change `.with_decorations(false)` → `.with_decorations(true)`

That's it. Window is already resizable (tao default), has `min_inner_size(300, 400)`, and follows the main window via the event loop. The `ExtraWindow` trait handles positioning and bring-to-front already.

**Checkpoint:** `cargo run` — chat window should have OS title bar, be draggable and resizable.

---

## Phase 2: Persona .zip Packaging

### 2A: gassetsmaker — pack & info commands

#### 2A.1 Add dependencies
**File:** `gassetsmaker/Cargo.toml`
```toml
zip = "2"
toml = "0.8"
serde = { version = "1.0", features = ["derive"] }
clap = { version = "4", features = ["derive"] }
```

#### 2A.2 Define manifest model + refactor to clap subcommands
**File:** `gassetsmaker/src/main.rs`

Define serde structs matching existing `config.toml`:
```rust
#[derive(Deserialize)]
struct CharacterManifest {
    character: CharacterInfo,
}
#[derive(Deserialize)]
struct CharacterInfo {
    name: String,
    nick: String,
    animationstate: Vec<String>,  // ["idle", "talk"]
    image: String,                // "Sasha.png"
    loading: String,              // "Sasha is getting ready"
}
```

Refactor to clap:
```rust
#[derive(Parser)]
enum Cli {
    Clean { path: PathBuf },
    Pack { path: PathBuf },
    Info { path: PathBuf },
}
```

#### 2A.3 Implement `pack` subcommand
- Read `config.toml` from persona directory
- Create `{nick}.persona.zip` with:
  - `config.toml` (manifest)
  - Still image (from `character.image`)
  - All `frame_NNNN.png` from each state directory listed in `animationstate`
- Use `CompressionMethod::Stored` (PNGs already compressed)

#### 2A.4 Implement `info` subcommand
- Open `.persona.zip`, read `config.toml`, print summary (name, states, frame counts)

continue here
|
V
### 2B: ghost-ui — load from .zip

#### 2B.1 Add dependencies
**File:** `ghost-ui/Cargo.toml`
- Add `zip = "2"`

(ghost-ui already has `toml` and `serde` from Phase 1 refactor)

#### 2B.2 Add PersonaMeta + Animation::from_frames()
**File:** `ghost-ui/src/animated_skin.rs`

```rust
/// Metadata from a persona package
#[derive(Debug, Clone)]
pub struct PersonaMeta {
    pub name: String,
    pub nick: String,
    pub still_image: Option<SkinData>,
    pub loading_text: String,
}
```

Add constructor to `Animation`:
```rust
impl Animation {
    pub fn from_frames(frames: Vec<SkinData>, fps: f32) -> Result<Self, SkinError> { ... }
}
```

#### 2B.3 Implement `AnimatedSkin::from_zip()`
**File:** `ghost-ui/src/animated_skin.rs`

- Opens zip, reads `config.toml` manifest
- Loads still image from `character.image` (fallback: first idle frame)
- For each state in `animationstate`: collects `{state}/frame_NNNN.png` entries, reads bytes, creates `SkinData`
- Returns `(AnimatedSkin, PersonaMeta)`

#### 2B.4 Export PersonaMeta
**File:** `ghost-ui/src/lib.rs`
- Add `PersonaMeta` to `animated_skin` exports

#### 2B.5 Detect .zip in main app
**File:** `src/main.rs`

Skin loading logic: if `config.skin.path` ends with `.zip`, use `AnimatedSkin::from_zip()` instead of `from_directory()`.

#### 2B.6 Create persona.toml for existing assets
The existing `assets/persona/sasha/config.toml` already has the right format — no changes needed.

**Checkpoint:** `cargo run -p gassetsmaker -- pack assets/persona/sasha/` creates `sasha.persona.zip`. Update `ui.toml` path to point at zip. `cargo run` loads from zip.

---

## Phase 3: Lazy Loading with Loading Indicator

### 3.1 Add `load_meta_from_zip()` to AnimatedSkin
**File:** `ghost-ui/src/animated_skin.rs`

Quick function that opens zip, reads ONLY manifest + still image (no frame loading):
```rust
pub fn load_meta_from_zip(path: impl AsRef<Path>) -> Result<PersonaMeta, SkinError>
```

### 3.2 Add loading state to App
**File:** `src/windows/main_window.rs`

```rust
enum SkinLoadState {
    Loading { receiver: Receiver<AnimatedSkin> },
    Ready,
    Static,  // not using animated skin
}
```

Add fields to `App`:
- `load_state: SkinLoadState`
- `still_skin: Option<Skin>` — GPU texture of still image during loading
- `needs_gpu_reinit: bool` — flag for deferred GPU init after background load
- `loading_layer: Option<Layer>` — text overlay with loading message

### 3.3 Lazy loading startup
**File:** `src/main.rs`

For `.zip` paths:
1. Call `AnimatedSkin::load_meta_from_zip()` — get still image + loading text (fast)
2. Spawn `std::thread::spawn` that calls `AnimatedSkin::from_zip()` (slow, loads all frames)
3. Pass `mpsc::Receiver` to App via `SkinLoadState::Loading`
4. Create window with still image dimensions

### 3.4 Still image display during loading
**File:** `src/windows/main_window.rs`

In `init_gpu()`: create `Skin` from `PersonaMeta.still_image` → store as `still_skin`

In `current_skin()`:
```rust
match self.load_state {
    SkinLoadState::Loading { .. } => self.still_skin.as_ref(),
    SkinLoadState::Ready => self.animated_skin.as_ref().and_then(|a| a.current_skin()),
    SkinLoadState::Static => None,
}
```

### 3.5 Loading indicator layer
**File:** `src/windows/main_window.rs`

In `App::new()`: if `SkinLoadState::Loading`, create a `Layer` with semi-transparent background and loading text from `PersonaMeta.loading_text`. Use existing `LayerConfig` with `anchor: BottomCenter`, `z_order: 100`.

Include this layer in `prepare()` and `render_layers()`.

### 3.6 Completion check + transition
**File:** `src/windows/main_window.rs`

In `update()`: `receiver.try_recv()` — if loaded skin arrives:
- `self.animated_skin = Some(loaded_skin)`
- `self.load_state = SkinLoadState::Ready`
- `self.needs_gpu_reinit = true`
- `self.loading_layer = None`

In `prepare()`: if `needs_gpu_reinit`, call `animated_skin.init_gpu(device, queue)` then clear flag.

**Checkpoint:** `cargo run` with `.zip` path — still image shows immediately with loading text, then transitions to animation.

---

## Key Files

| File | Changes |
|------|---------|
| `src/windows/chat_window.rs` | Phase 1: decorations |
| `gassetsmaker/Cargo.toml` | Phase 2A: add zip, toml, serde, clap |
| `gassetsmaker/src/main.rs` | Phase 2A: clap refactor, pack, info |
| `ghost-ui/Cargo.toml` | Phase 2B: add zip |
| `ghost-ui/src/animated_skin.rs` | Phase 2B+3: PersonaMeta, from_frames, from_zip, load_meta_from_zip |
| `ghost-ui/src/lib.rs` | Phase 2B: export PersonaMeta |
| `src/main.rs` | Phase 2B+3: zip detection, lazy loading thread |
| `src/windows/main_window.rs` | Phase 3: SkinLoadState, still_skin, loading_layer, completion check |

## Verification

1. Phase 1: `cargo run` → chat window has title bar, resizable, draggable
2. Phase 2: `cargo run -p gassetsmaker -- pack assets/persona/sasha/` → creates zip; update ui.toml path; `cargo run` loads from zip
3. Phase 3: `cargo run` with zip → still image + "Sasha is getting ready" text shows, then transitions to animation

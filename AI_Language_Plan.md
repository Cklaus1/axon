# AI Language Plan

One language. Every domain. Every platform. Tier 1 performance. Optimized for AI token density + AST editability.

## Core Design

Dense by default. Braces not whitespace. Types inferred. No null. No exceptions. Ownership not GC.
```
From Rust:      ownership/borrowing, Result<T,E>, pattern matching, traits, zero-cost abstractions
From TypeScript: structural typing, type inference, union types
From Go:        goroutines, typed channels, fast compilation
From Zig:       comptime (compile-time execution), no hidden allocations
```

## Syntax

```
// Functions — terse, last expression is return
fn add(a:i32,b:i32)->i32{a+b}
fn fetch(url:str)->Result<Response,Error>{http.get(url)?}
let f=(a,b)=>a+b

// Ownership — simplified from Rust, 2 modes
own x=Vec.new()      // owned, moveable
ref y=&x              // borrowed, read-only

// Structural typing + traits
type Handler={fn handle(req:Request)->Response}
// anything with handle method satisfies Handler, no implements keyword

// Result<T,E> + ? operator, no exceptions ever
fn process(data:Bytes)->Result<Output,Error>{
  let parsed=parse(data)?
  let validated=validate(parsed)?
  Ok(transform(validated))
}

// Pattern matching
match value{
  Some(x) if x>0 => process(x),
  Some(_) => skip(),
  None => default()
}

// Concurrency — Go-style, type-safe
spawn{fetch(url)}
let ch=Chan<Message>.new()
select{ch.recv()=>handle, timeout(5s)=>abort}

// Comptime — computed at compile time, zero runtime cost
comptime{
  let schema=derive_json_schema(MyType)
  let routes=scan_handlers("src/")
}

// No null, ever
let x:Option<i32>=map.get("key")
let y=x.unwrap_or(0)

// Algebraic data types
enum Shape{Circle{radius:f64},Rect{w:f64,h:f64},Point}

// Modules — flat
mod server
use server.{listen,Router}

// Generics with trait bounds
fn sort<T:Ord>(items:&[T])->[T]{...}

// Compile targets
#[target(native)]   // x86, ARM
#[target(wasm)]     // browser, edge
#[target(js)]       // Node/Bun interop
#[target(mobile)]   // iOS (Metal), Android (Vulkan)
```

## Type System

```
Primitives:    i8 i16 i32 i64 u8 u16 u32 u64 f32 f64 bool str char
Collections:   [T] (array/slice), Map<K,V>, Set<T>, Vec<T>
Nullable:      Option<T> (never null, always explicit)
Errors:        Result<T,E> (never exceptions, always explicit)
Functions:     fn(A,B)->C
Tuples:        (A,B,C)
Unions:        A|B|C (structural, like TypeScript)
Structs:       struct Point{x:f64,y:f64}
Enums:         enum Color{Red,Green,Blue} or ADTs with data
Traits:        trait Serialize{fn to_bytes(&self)->Bytes}
Generics:      <T:Trait+Clone> with monomorphization (zero-cost)
Inference:     let x=42 (i32 inferred), let y=fetch(url)? (type inferred from return)
```

## Token Density Comparison

```
// AI language (12 tokens)
fn process(segs:[Segment],prov:Provider)->Result<[Output],Error>{segs.map(|s|prov.gen(s)?).collect()}

// Rust (18 tokens)
fn process(segs:&[Segment],prov:&Provider)->Result<Vec<Output>,Error>{segs.iter().map(|s|prov.gen(s)).collect::<Result<Vec<_>,_>>()}

// TypeScript (22 tokens)
async function process(segs:Segment[],prov:Provider):Promise<Output[]>{return Promise.all(segs.map(s=>prov.gen(s)))}

// Go (28 tokens)
func process(segs []Segment,prov *Provider)([]Output,error){var out []Output;for _,s:=range segs{o,err:=prov.Gen(s);if err!=nil{return nil,err};out=append(out,o)};return out,nil}

Savings: 33% vs Rust, 45% vs TypeScript, 57% vs Go
```

## Compiler Architecture

Custom frontend, LLVM backend. Don't build a compiler from scratch.
```
AI language source (.ai)
    |
    v
Parser (tree-sitter grammar)                <- build this
    |
    v
AST                                          <- build this
    |
    v
Type checker + borrow checker                <- build this (hardest part)
    |
    v
LLVM IR emission                             <- build this (using LLVM C API)
    |
    v
LLVM optimization passes                     <- free (200+ passes)
    |  \  \  \
    v   v   v   v
  x86  ARM WASM NVPTX                        <- free (all LLVM targets)
              \
               v
            JS codegen                        <- build this (separate backend)
```

### Design Sources (steal designs not code)
```
rustc:          borrow checker (MIR-based), trait resolution, monomorphization, error messages
zig compiler:   comptime engine, no hidden allocations, incremental compilation
go compiler:    fast compile speed, goroutine runtime, simple modules
tsc:            structural type checking, type inference, union/intersection resolution
swiftc:         SIL (mid-level IR), protocol witness tables
```

### Build Phases
```
Phase 1 (3-6 months): Minimum viable compiler
  Parser, basic type checker, LLVM IR emission, native target only
  No borrow checker yet — simple move semantics only
  Result: compiles basic programs to native binaries

Phase 2 (3-6 months): Safety
  Borrow checker, Result<T,E> + ?, Option<T>, pattern matching, generics+traits
  Result: memory-safe with Rust-level guarantees

Phase 3 (3-6 months): Concurrency + targets
  Goroutine-style tasks, typed channels, WASM target, JS codegen
  Result: full language, all targets

Phase 4 (ongoing): Self-improving compiler
  Comptime engine, AI profiler, learned optimization passes, incremental compilation
  Result: compiler gets faster over time
```

## Self-Improving Compiler

```
1. AI writes code
2. Compiler emits via LLVM (95% optimal)
3. AI profiler finds hot path
4. AI generates optimized asm for that pattern
5. Pattern verified correct + measurable speedup
6. Pattern added to compiler as optimization pass
7. Next compilation applies it everywhere automatically
8. Original asm block deleted — compiler handles it natively
```

```
compiler/.optimizations/patterns.json:
{
  "vectorize-reduction": {
    "pattern": "loop accumulating scalar from array elements",
    "optimization": "AVX-512 gather + horizontal add",
    "speedup": "3.8x avg across 47 programs",
    "source": "discovered by AI profiler",
    "graduated_from_asm": true
  }
}
```

```
Compiler v1:    95% optimal (LLVM baseline)
Compiler v5:    98% optimal (100 learned patterns)
Compiler v20:   99.5% optimal (1000 patterns)
Asm blocks:     only for truly novel hardware not yet patterned
```

## Cross-Platform

LLVM handles CPU targets. Stdlib handles OS differences.

### Stdlib (per-platform implementations behind unified API)
```
std.fs          file I/O (open, read, write, walk, metadata)
std.net         TCP, UDP, TLS, HTTP
std.os          env vars, args, signals, processes
std.thread      spawn, join, mutex, channels
std.io          buffered read/write, stdin/stdout
std.path        cross-platform path manipulation
std.time        clocks, duration, sleep
std.crypto      hash, encrypt, random
std.async       event loop, tasks, select
```

### Platform-specific (internally):
```
                Linux           Windows         macOS
File open:      open()          CreateFile()    open()
Networking:     epoll           IOCP            kqueue
Threads:        pthreads        Win32 threads   pthreads
Paths:          /home/user      C:\Users\user   /Users/user
```

User code never sees platform differences. Stdlib abstracts it.

## UI — GPU-Rendered, Not Webview

One rendering path: code -> GPU -> pixels. Same output every platform. No DOM, no CSS, no widget toolkit.

### Stack
```
AI language UI code
    |
    v
Declarative UI tree (dense, reactive)
    |
    v
Layout engine (flexbox-like)
    |
    v
Vello + wgpu (GPU-accelerated 2D)
    |
    v
Platform window (winit)
    |         |         |         |
    v         v         v         v
  Vulkan    Metal     DX12     WebGPU
  (Linux)   (macOS)   (Win)    (Browser)
```

### UI Syntax
```
fn app(s:&AppState)->View{
  col{
    text("Users: {s.users.len()}").font(24).bold()
    for u in s.users{
      row{
        avatar(u.image,40)
        col{text(u.name).font(16) text(u.email).font(12).color(GRAY)}
        spacer()
        button("Delete").on_click(||msg(DeleteUser(u.id)))
      }.pad(8).swipe_to_delete(||msg(Delete(u.id)))
    }
    input("Search...").bind(s.query).on_change(||msg(Filter))
  }.pad(16)
}

enum Msg{DeleteUser(Id),Filter}
fn update(s:&mut AppState,m:Msg){match m{
  DeleteUser(id)=>s.users.retain(|u|u.id!=id),
  Filter=>s.filtered=s.users.filter(|u|u.name.contains(&s.query))
}}
```

### Token comparison vs HTML+CSS
```
AI language:    ~15 tokens, 1 file, 1 language, everything inline
HTML+CSS+JS:    ~40 tokens, 3 files, 2 languages, layout split from content
Savings:        62%
```

### Binary size comparison
```
AI language (wgpu+vello+winit):    ~3.5MB
Flutter (Skia+Dart runtime):       ~40MB
Tauri (system webview+Rust):       ~10MB
Electron (bundled Chromium):       ~150MB
```

## Mobile

Same GPU rendering via wgpu. Metal on iOS, Vulkan on Android.

### Platform Integration Stdlib
```
std.platform.lifecycle:     on_foreground, on_background, on_terminate
std.platform.permissions:   request(Camera|Location|Contacts|Notifications|Microphone)
std.platform.notifications: register_push, show_local
std.platform.storage:       secure_store, secure_load (keychain/keystore)
std.platform.haptics:       impact(Light|Medium|Heavy), notify(Success|Warning|Error)
std.platform.share:         share(text, url, image)
std.platform.biometrics:    authenticate(reason) (Face ID, fingerprint)
```

### Build Pipeline
```
iOS:      LLVM -> AArch64 -> .a lib + Swift wrapper (~50 lines, generated) + Metal via wgpu -> .ipa
Android:  LLVM -> AArch64/x86_64 -> .so lib + Kotlin wrapper (~50 lines, generated) + Vulkan via wgpu -> .apk
Web:      LLVM -> WASM + JS glue (generated) + WebGPU via wgpu
```

Native wrappers are ~50 lines each, auto-generated by `ai-lang build --target ios|android`. AI never sees or edits them.

## 3D — Scene Graph on wgpu

Same GPU layer as 2D UI. Add scene graph + physics.

```
2D UI:     AI language -> layout engine -> vello -> wgpu -> GPU
3D:        AI language -> scene graph -> renderer -> wgpu -> GPU
```

### 3D Syntax
```
fn scene(s:&State)->Scene3D{
  scene{
    camera([0,5,-10],[0,0,0],60)
    dir_light([0.5,-1,0.3],WHITE,1.0)
    ambient(WHITE,0.2)
    model("char.glb").pos(s.player.pos).rot(s.player.rot).animate("run",s.player.speed)
    for obj in s.objects{
      mesh(obj.geometry).material(pbr{albedo:obj.color,metallic:0.5,roughness:0.3}).pos(obj.pos).shadow(true)
    }
    heightmap("terrain.png",100,10).material(pbr{albedo_map:"grass.png",tiling:20})
    particles{count:1000,emitter:point(s.player.pos+[0,2,0]),lifetime:2.0,speed:3.0,color:gradient(YELLOW,RED),size:0.1..0.0,gravity:-2.0}
    post{bloom(0.8,0.3),tonemap(ACES),fxaa}
    physics{
      body(s.player,kinematic,capsule(0.5,1.8))
      for obj in s.objects{body(obj,dynamic,from_mesh(obj.geometry))}
      gravity:[0,-9.81,0]
    }
  }
}
```

### 3D Stdlib
```
std.render3d              Scene, Camera, Light, Mesh, Material(pbr|unlit|toon|shader), Model(GLTF/GLB), Texture, CubeMap, Heightmap
std.render3d.animation    Skeleton, Animation, AnimationState, BlendTree, Tweens
std.render3d.particles    Emitter(point|sphere|box|mesh_surface), particle properties
std.render3d.post         Bloom, SSAO, Tonemap(ACES|Reinhard|Filmic), FXAA/TAA, DOF, MotionBlur, ColorGrading
std.render3d.physics      RigidBody(static|dynamic|kinematic), Collider(sphere|box|capsule|mesh|heightfield), Joints, Raycasting (wraps Rapier)
std.render3d.spatial      Octree, BVH, Frustum culling, LOD, Instancing
```

### Design Sources
```
Bevy:       ECS architecture, wgpu rendering, asset pipeline, WASM builds (closest to AI language philosophy)
Three.js:   Scene/Camera/Mesh/Material API (proven simple), GLTF loading, post-processing
Unity:      component system, physics integration, animation state machines, particle systems
Unreal:     PBR material model, Nanite/Lumen concepts, LOD
```

## Full Domain Coverage

```
Domain:           Stdlib:                            Target:
CLI tools         std.io, std.fs, std.net, std.os    native
Backend servers   std.net, std.async, std.crypto      native
Desktop apps      std.ui                              native (wgpu)
Web apps          std.ui                              wasm (WebGPU)
Mobile apps       std.ui + std.platform               mobile (Metal/Vulkan)
3D/games/viz      std.render3d                        native/wasm/mobile
GPU compute       std.compute (WGSL shaders)          native/wasm
ML inference      std.compute + tensor ops             native (NVPTX)
```

One language. Every domain. Every platform. The AI agent never asks "which language" — always AI language, different imports.

## Performance Tiers

```
Tier 1 (native, ~C speed):     #[target(native)]     x86/ARM via LLVM
Tier 1 (WASM, near-native):    #[target(wasm)]       browser via WebGPU
Tier 1 (mobile, native):       #[target(mobile)]     iOS Metal / Android Vulkan
Tier 4 (JS interop):           #[target(js)]         Node/Bun, npm ecosystem access
Tier 1 (GPU compute):          #[target(gpu)]        CUDA/Vulkan compute shaders

No GC -> ownership (Tier 1)
No runtime overhead -> compiles to native (Tier 1)
Zero-cost abstractions -> monomorphized generics (Tier 1)
Comptime -> zero runtime cost for metaprogramming (Tier 1)
```

## ASI Design Decisions

### 1. Package Management — No Package Manager. Inline Content-Addressed Imports.
```
// Import by hash, not by name+version. Hash IS the package. Can't be poisoned.
use "sha256:a3f2b7c9d1e4..." as http     // trust THIS EXACT CODE
use "github.com/user/repo@v2.1" as utils  // resolved to hash at first import, pinned

// Compiler flattens ALL transitive deps into .deps file (checked into repo):
// .deps
// sha256:abc123  http     4.2KB   audit:pass  2026-04-07
// sha256:def456  tls      2.1KB   audit:pass  2026-04-07
// sha256:789abc  dns      0.8KB   audit:pass  2026-04-07
// Nothing hidden. Every piece of code in your binary is visible.
```
No registry, no lockfile, no version conflicts, no name squatting. Comptime resolves, downloads, caches. The import IS the dependency declaration. The compiler IS the package manager.

### 2. FFI — Automatic From C Headers
```
use ffi "libsqlite3.so"    // compiler reads .h, generates typed bindings at comptime
let db=sqlite3_open("data.db")?
```
Comptime parses C headers, generates safe wrappers. No hand-written bindings. Rust libraries consumed via `.rlib` (both target LLVM IR). AI never writes FFI code.

### 3. Strings — UTF-8 Always, One Type
```
str                    // owned UTF-8, heap, growable
&str                   // borrowed slice
"hello {name}"         // interpolation built in
r"no \escapes"         // raw string
b"raw bytes"           // [u8]
```
No String vs &str confusion. One type. Borrowing is `&str`. Compiler optimizes small strings to stack (SSO) automatically.

### 4. Error Messages — Structured for AI, Pretty for Humans
```
// AI sees (compiler API output):
{"error":"E0312","node":"#fn_process.body.stmt_3","expected":"Result<Output,Error>","found":"Option<Output>","fix":"wrap with .ok_or(Error::Missing)?"}

// Human sees (terminal):
error[E0312]: type mismatch at main.ai:12:5
  expected Result<Output,Error>, found Option<Output>
  help: use .ok_or(Error::Missing)? to convert
```
Same error, two renderers. AI gets structured errors with node IDs and suggested fixes.

### 5. Async — Green Threads Only. No async/await.
```
fn fetch(url:str)->Result<Response,Error>{http.get(url)}   // looks sync, runs on green thread
spawn{fetch(url)}                                           // explicit concurrency
let ch=Chan<i32>.new()                                      // typed channels
select{ch.recv()=>handle(it), timeout(5s)=>abort()}         // multiplexing
```
No `async fn`, no `.await`, no `Future`, no `Pin<Box<dyn Future>>`. M:N threading with work-stealing scheduler (Go model). No async coloring problem. Simpler code, fewer tokens.

### 6. Memory Layout — Comptime-Controlled
```
struct Vertex{pos:[f32;3],normal:[f32;3],uv:[f32;2]}           // compiler chooses optimal
#[layout(C)] struct FFIStruct{x:i32,y:i32}                     // C-compatible
#[layout(packed)] struct Compact{flags:u8,id:u32}               // no padding
#[layout(align(64))] struct CacheLine{data:[u8;64]}             // cache-aligned
comptime{assert(size_of(Vertex)==32,"vertex must be 32 bytes")} // compile-time verify
```

### 7. Testing — Built In, Same File, Dense
```
fn add(a:i32,b:i32)->i32{a+b}
#[test] fn test_add(){assert(add(2,3)==5)}
#[test] fn test_overflow(){assert_err(add(i32.MAX,1))}
#[test(should_fail)] fn test_bad(){assert(add(2,3)==6)}
#[test] fn test_commutative(){forall(a:i32,b:i32){assert(add(a,b)==add(b,a))}}  // property-based built in
```
`ai-lang test` runs all. No test framework. Property-based testing (`forall`) is a language feature.

### 8. Build System — The Compiler IS the Build System
```
// No Cargo.toml, no package.json, no Makefile. Source IS config.
#[target(native,wasm,mobile)]
#[optimize(speed)]
use "github.com/user/http@v2" as http
use ffi "libsqlite3.so"
fn main()->Result<(),Error>{...}
```
`ai-lang build` reads source, resolves everything. One file = one module. No build config to maintain.

### 9. Debugging — Trace-Based, Not Breakpoint-Based
```
ai-lang run --trace main.ai                  // records full execution trace
ai-lang trace --node "#fn_process"           // calls, args, returns, timing for this function
ai-lang trace --error                        // execution path leading to error
```
Output is structured PSV. AI reads trace, understands bug, fixes code. LLVM emits DWARF for humans who want GDB/LLDB. But primary flow is trace -> AI analysis -> fix.

### 10. Unsafe — Explicit, Isolated, Auditable
```
unsafe{
  let ptr=alloc(1024) as *mut u8
  ptr.write(0,42)
  ffi.call("some_c_function",ptr)
  free(ptr)
}
```
Required for: raw pointers, FFI calls, manual memory. `ai-lang audit --unsafe` lists every unsafe block. Goal: zero unsafe in application code. Unsafe lives only in stdlib and FFI wrappers. AI agents instructed never to generate unsafe unless implementing FFI.

### 11. Versioning — No Editions. Compiler Auto-Migrates.
```
ai-lang migrate    // AI reads all source, applies migration as tree transform
```
No edition flags, no backward compat hacks. Language evolves freely. Compiler includes migrator that rewrites code to latest syntax via AST transforms. Cost of migration: near zero. No reason to maintain old syntax.

### 12. Serialization — Comptime-Derived, Zero Boilerplate
```
struct User{name:str,age:i32,email:str}
// That's it. Every struct is serializable automatically.
let json=user.to_json()
let user=User.from_json(json_str)?
let bytes=user.to_msgpack()
let proto=user.to_protobuf()
```
No `#[derive(Serialize)]`. Comptime generates ser/deser for every struct. Formats: JSON, MessagePack, Protobuf, CBOR, TOML, YAML — all stdlib. Custom via trait override.

### 13. Database — Type-Safe Query Builder, Comptime-Validated
```
let db=Db.connect("postgres://localhost/mydb")?
let users=db.query(User).filter(|u|u.age>18).order_by(.name).limit(10).exec()?
let results=db.sql("SELECT * FROM users WHERE age > {min_age}")?
ai-lang db migrate    // comptime diffs struct vs table, generates ALTER statements
```
No ORM. Query builder compiles to SQL. Comptime connects to database, reads schema, validates queries. Wrong column name = compile error.

### 14. HTTP — Stdlib, Not Framework
```
let app=Router.new()
  .get("/users",list_users)
  .post("/users",create_user)
  .ws("/live",handle_ws)
  .middleware(cors())
  .middleware(auth(jwt_secret))
serve(app,":8080")?
```
Routing, middleware, websockets, JSON responses — all stdlib. No Express, no Actix. Entire server definition ~20 lines.

## Supply Chain Security

Every major supply chain attack of the last 5 years would be stopped by this model.

### Content-Addressed Imports (no name poisoning)
```
use "sha256:a3f2b7c9d1e4..." as utils
// Hash IS the identity. No name to squat, no account to hack, no version to poison.
// Code changes by one byte -> hash changes -> import fails -> compiler tells you.
```

### Capability Permissions (compile-time I/O restrictions)
```
use "sha256:abc123" as utils {allow:[pure]}                     // no I/O at all
use "sha256:def456" as http {allow:[net("*.myapi.com")]}        // specific domains only
use "sha256:789abc" as imagelib {allow:[fs(read,"./content/")]} // read only, one dir

// imagelib tries to read /etc/passwd:
// COMPILE ERROR: imagelib requested fs(read,"/etc/passwd") but only allowed fs(read,"./content/")
```
Enforced at compile time. Dependency can't even contain disallowed I/O code — compiler rejects it.

### AI Audit on Import
```
ai-lang add "github.com/user/repo@v2.1"

// Compiler:
// 1. Download SOURCE (never compiled binary)
// 2. Hash content
// 3. Flatten all transitive deps
// 4. AI security scan ALL source:
//    - Flag network calls, file access, subprocess spawning
//    - Flag obfuscated code (eval, dynamic imports, encoded strings)
//    - Flag capabilities beyond what package claims to need
//    - Flag known vulnerability patterns
// 5. Generate audit report with risk level + recommendation
```

### No Install Scripts
```
// npm: "postinstall": "node backdoor.js" runs on install
// AI language: packages are SOURCE CODE ONLY
//   No scripts, no hooks, no binaries, no native extensions
//   Compiler compiles everything from source
//   Nothing executes until YOU run YOUR binary
```

### Reproducible Builds
```
ai-lang build --verify
// Rebuild from source (all deps from hashes)
// Compare output binary hash to expected
// If mismatch: something changed, refuse to deploy
// Same source hashes -> same binary hash, always
```

### Rich Stdlib (fewer deps = smaller attack surface)
```
Eliminated external deps by putting in stdlib:
  HTTP client/server, JWT, hashing, UUID, dates, DB access, 
  websockets, JSON/YAML/protobuf, file utils, path utils,
  env vars, validation, formatting
  
Typical npm project: 500-1500 transitive deps
AI language project: 0-10 deps (most apps need nothing external)
```

### Dependency Health Monitoring
```
ai-lang deps --health
// sha256:abc123  http-client    last commit: 82 days ago       HEALTHY
// sha256:def456  image-proc     last commit: 2 years ago       STALE
// sha256:789abc  auth-helper    maintainer: 1 person           RISK (bus factor 1)
// sha256:111222  crypto-utils   known vuln: CVE-2026-1234      VULNERABLE
```

### Attacks Prevented
```
Attack                          Stopped by
event-stream (crypto stealer)   Capability: {allow:[pure]}, no network access
ua-parser-js (crypto miner)     AI audit flags obfuscated code + unexpected subprocess
colors.js (protest sabotage)    Content hash pinned, update requires re-audit
xz-utils (backdoor in build)    No install scripts, compile from source only
node-ipc (disk wiper)           Capability: no fs(write) for a parser library
```

## AI as Language Primitive

AI is infrastructure, not a plugin. Same as `std.net` for networking, `std.ai` for intelligence.

### std.ai — Core Inference
```
use std.ai
let response=ai.complete("summarize: {text}",model:auto)
let structured=ai.extract<Invoice>(document)        // returns typed struct
let embedding=ai.embed(text)                         // returns [f32;768]
let image=ai.generate_image("sunset over mountains")
let speech=ai.tts("hello world",voice:auto)
let text=ai.transcribe(audio_bytes)
```

### Model Routing — Automatic Tiered Selection
```
Tier 1 (local, <1ms):     1-3B params bundled with binary
                           classification, extraction, simple completion
                           on device, offline, free, private

Tier 2 (local, <100ms):   7-13B params if GPU available
                           summarization, code gen, structured output
                           on device, offline, free, private

Tier 3 (API, <2s):        large models (Sonnet/Opus/GPT-4) via API
                           complex reasoning, long-form generation
                           cloud, requires network, costs money

Routing: match task.complexity{
  Simple=>local_small, Medium=>if gpu(){local_medium}else{api_sonnet},
  Complex=>api_opus, CostSensitive=>local_best, Offline=>local_best
}
```

### AI Policy Per Function
```
#[ai(policy: local_only)]        // never call API, privacy-critical
#[ai(policy: prefer_local)]      // local when possible, API fallback
#[ai(policy: best_available)]    // best model regardless of cost
#[ai(policy: budget(0.01))]      // max $0.01 per call
```

## Three Code Zones: Static, Adaptive, Agent

Not all software evolves. Developer explicitly chooses per function.

### Static — Never Changes (default, most code)
```
fn calculate_tax(income:f64,rate:f64)->f64{income*rate}
// Deterministic, auditable, no AI, traditional software
```

### Adaptive — Evolves Within Constraints
```
#[adaptive(
  metric: response_quality,
  bounds: {latency:<200ms},
  approval: auto_if_improvement,
  rollback: on_regression,
  log: provenance
)]
fn generate_summary(article:str)->str{
  ai.complete("summarize: {article}",model:auto)
  // Prompt, model, parameters CAN evolve
  // Function signature, constraints CANNOT
  // Every change recorded in provenance
}
```

### Experiment — Actively Tests Alternatives
```
#[experiment(
  variants: 3,
  traffic: split_evenly,
  duration: 7d,
  decide: highest_engagement
)]
fn recommend(user:User)->[Item]{
  // System generates 3 strategies, tests them, picks winner
  // Winner graduates to adaptive zone
}
```

### Agent Module — Autonomous Reasoning
```
#[agent]
mod support{
  goal: "resolve customer issues with >90% satisfaction"
  tools: [db.query, email.send, ticket.update, escalate]
  constraints: {
    never: [delete_data, issue_refund_over(100)],
    always: [log_actions, cite_policy]
  }
  fn handle(ticket:Ticket)->Resolution{
    // Agent reasons, uses tools, constrained by rules
    // Every action logged to provenance
  }
}
```

### Code Zone Summary
```
Pure:        fn                  no AI, deterministic, most code
Enhanced:    fn + ai.complete()  uses inference, static prompts
Adaptive:    #[adaptive] fn      evolves parameters within constraints
Experiment:  #[experiment] fn    tests alternatives, picks winners
Agent:       #[agent] mod        reasons, plans, acts with tools
```

## Shared Intelligence Platform

Not just shared code — shared learnings. AI equivalent of GitHub.

### What's Shared
```
Current GitHub:    CODE        (functions, libraries, applications)
AI platform:       CODE        (same, content-addressed)
                 + MODELS      (fine-tuned for specific tasks)
                 + KNOWLEDGE   (learned facts with evidence)
                 + ADAPTATIONS (prompt improvements, routing optimizations)
```

### Registry
```
packages/                                    content-addressed code
models/
  models/invoice-extraction-v3               3B params, 97% accuracy on invoices
  models/code-review-rust-v2                 7B params, trained on 1M reviews
  models/sentiment-multilingual-v1           1B params, 40 languages
knowledge/
  knowledge/ecommerce/pricing                proven pricing strategies with p-values
  knowledge/content/thumbnails               thumbnail styles that drive CTR
  knowledge/devops/incident-response         patterns that reduce MTTR
adaptations/
  adaptations/summarize/news                 best prompts for news summarization
  adaptations/extract/receipts               best schema+prompt for receipt parsing
```

### CLI
```
ai-lang knowledge push "content-summarization"
// Shares: "extractive summaries with 3 bullets outperform abstractive
//          by 23% on engagement (n=5000, p<0.01)"

ai-lang knowledge pull "content-summarization"
// Your adaptive functions benefit from what everyone else learned

ai-lang knowledge search "image generation prompts"
// Find proven patterns with evidence and confidence levels

ai-lang models pull "invoice-extraction-v3"
// Download fine-tuned model, runs locally as Tier 1/2
```

### Trust Model
Same as packages: content-addressed, AI-audited, capability-restricted. Knowledge entries have evidence (sample size, p-value, confidence). Models have benchmark scores. Everything verifiable.

### How It Accelerates
```
Without shared intelligence:
  1000 teams * same discovery independently = massive waste
  Every team tunes prompts from scratch
  Every team fine-tunes models on own data

With shared intelligence:
  One team discovers pattern, publishes with evidence
  999 teams benefit immediately
  Knowledge compounds across ecosystem
  1000th team that joins is as smart as all 999 before it
```

## Intelligence Maturity Model

```
Level 0: Static code           traditional software, no AI
Level 1: AI-enhanced           uses inference, static prompts
Level 2: Adaptive              evolves within constraints, self-optimizes
Level 3: Agent                 reasons, plans, acts autonomously
Level 4: Shared intelligence   learns from ecosystem, benefits from collective
Level 5: Collective ASI        the ecosystem itself is intelligent
                               new software inherits all prior knowledge
                               every program improves every other program
                               convergence toward optimal for every problem
```

## Autonomous Optimization — AI-Generated Experiments Toward Goals

The developer sets the goal and constraints. The AI generates experiments, evaluates them deterministically, hill climbs toward the target. No feature flags. No A/B tests with 100K users. No manual tuning.

### The Full Loop
```
1. Developer defines: goal metric + test set + constraints + budget
2. AI analyzes current implementation, identifies improvement axes
3. AI generates experiment (code change, prompt change, parameter change)
4. System evaluates experiment against test set (deterministic, no users)
5. If better: keep + record why. If worse: revert + record why.
6. AI uses results to generate SMARTER next experiment (not random)
7. Repeat until: target met OR budget exhausted OR plateau detected
8. Best version auto-deploys. All experiments logged in provenance.
```

### Syntax
```
#[goal(
  metric: extract_accuracy,              // what to maximize
  test_set: "data/invoices_labeled.jsonl", // ground truth
  target: 0.97,                           // stop when 97% accurate
  budget: {evals:100,cost:5.00,time:1h},  // hard limits
  axes: [prompt, model, temperature, schema], // what AI can mutate
  constraints: {latency:<500ms, cost_per_call:<0.01}
)]
fn extract_invoice(doc:str)->Invoice{
  ai.extract<Invoice>(doc,prompt:PROMPT,model:auto,temperature:0.3)
}
```

### What the AI Generates as Experiments
The AI doesn't randomly mutate — it reasons about what to try:

```
// Experiment log (auto-generated):
Experiment 1: baseline
  prompt: "Extract invoice fields from this document: {doc}"
  model: local-3b, temperature: 0.3
  score: 0.82
  analysis: "Misses line items when table format varies. Prompt too generic."

Experiment 2: (AI-generated based on analysis)
  mutation: prompt -> "Extract invoice: number, date, vendor, total, and each line 
            item (description, qty, unit_price, amount) from: {doc}"
  reasoning: "Explicit field list should reduce omissions on structured data"
  score: 0.89 (+0.07)
  analysis: "Line items improved. Still fails on handwritten invoices."

Experiment 3: (AI-generated)
  mutation: model -> api-sonnet (upgrade for complex cases)
  reasoning: "Handwritten text needs stronger vision model"
  score: 0.91 (+0.02)
  analysis: "Better but cost_per_call=$0.008, close to $0.01 limit."

Experiment 4: (AI-generated)
  mutation: temperature 0.3 -> 0.1
  reasoning: "Extraction is factual, lower temperature = less hallucination"
  score: 0.93 (+0.02)
  
Experiment 5: (AI-generated)
  mutation: add few-shot examples in prompt (3 labeled examples from test set)
  reasoning: "Few-shot typically improves structured extraction 5-10%"
  score: 0.97 (+0.04)
  TARGET MET. Deploying experiment 5.
```

Key: the AI **reasons about failures** and **generates targeted fixes**, not random mutations. Each experiment is informed by why the previous one fell short.

### Three Optimization Strategies
```
#[goal(strategy: hill_climb)]
// Mutate one axis at a time, keep improvements, revert regressions.
// Best for: continuous tuning (prompt wording, temperature, thresholds)
// Evaluations: ~20-50 to converge. Speed: minutes.

#[goal(strategy: tournament)]
// Generate N completely different approaches, evaluate all, keep top K,
// breed winner traits + mutate, repeat generations.
// Best for: discrete choices (algorithm A vs B, model X vs Y)
// Evaluations: ~50-100. Speed: minutes to hours.

#[goal(strategy: bayesian)]
// Model the metric surface, pick next experiment to maximize information gain.
// Best for: expensive evaluations (each eval costs real money/time)
// Evaluations: ~10-30 (most sample-efficient). Speed: depends on eval cost.
```

### Feature Flags Are Dead
```
// Old way (LaunchDarkly):
if feature_flag("new_checkout"){
  new_checkout()     // manual flag, manual cleanup, tech debt accumulates
}else{
  old_checkout()     // lives forever, nobody deletes it
}

// AI language:
#[goal(metric:checkout_success, test_set:"data/checkout.jsonl", target:0.99)]
fn checkout(cart:Cart)->Result<Order,Error>{...}
// AI tries implementations, picks winner, loser deleted automatically
// No flag. No cleanup. No tech debt.
```

### Evaluation Hierarchy
```
Level 1: #[goal]        deterministic eval, test set, no users      (90% of decisions)
                         correctness, performance, accuracy, extraction
                         minutes to converge, costs ~$0-5

Level 2: #[simulate]    AI simulates user behavior on variants       (8% of decisions)
                         UX flows, content quality, engagement
                         hours to converge, costs ~$5-50

Level 3: #[ab_test]     real user traffic split                      (2% of decisions)
                         brand preference, pricing, visual design
                         days/weeks, needs real users
```

Most decisions don't need real users. The test set IS the ground truth.

### Deterministic Test Sets
```
// data/invoices_labeled.jsonl — the ground truth
{"input":"<pdf bytes>","expected":{"number":"INV-001","date":"2026-01-15","total":1234.56,"items":[...]}}
{"input":"<pdf bytes>","expected":{"number":"INV-002","date":"2026-02-01","total":567.89,"items":[...]}}
// 50-200 labeled examples covering normal, edge, error cases

// Metric function — YOU define what "better" means
fn extract_accuracy(input:str,output:Invoice,expected:Invoice)->f64{
  let mut s=0.0
  if output.number==expected.number{s+=0.2}
  if output.date==expected.date{s+=0.2}
  if (output.total-expected.total).abs()<0.01{s+=0.2}
  if output.items.len()==expected.items.len(){s+=0.2}
  if output.latency<500ms{s+=0.2}
  s
}
```

### Goal Composition — Multi-Function Optimization
```
// Optimize an entire pipeline, not just one function
#[goal(
  metric: pipeline_quality,
  test_set: "data/video_eval.jsonl",
  target: 0.95,
  budget: {evals:200,cost:50.00}
)]
mod video_pipeline{
  fn generate_script(topic:str)->Script{...}       // AI can mutate prompt, model
  fn generate_images(script:Script)->[Image]{...}  // AI can mutate provider, style
  fn generate_audio(script:Script)->Audio{...}      // AI can mutate voice, speed
  fn render(images:[Image],audio:Audio)->Video{...} // AI can mutate template, transitions
}
// AI hill-climbs the ENTIRE pipeline toward the goal
// May discover: "better script prompts matter more than better images"
// Focuses effort on highest-leverage axis automatically
```

### Provenance on Every Experiment
```
// Every experiment recorded — what was tried, why, what happened
ai-lang experiments --goal extract_invoice
// Experiment 1: baseline                     score:0.82
// Experiment 2: explicit field list          score:0.89  +0.07  reason:"reduce omissions"
// Experiment 3: upgrade model                score:0.91  +0.02  reason:"handwritten support"
// Experiment 4: lower temperature            score:0.93  +0.02  reason:"less hallucination"
// Experiment 5: add few-shot examples        score:0.97  +0.04  reason:"structured extraction"  TARGET MET
// Total: 5 experiments, $1.23 cost, 4.2 minutes, 0.82->0.97 improvement

ai-lang experiments --goal extract_invoice --failures
// Shows what DIDN'T work and why — prevents future regressions
```

### Shared Experiment Results
```
// Push winning strategy to shared intelligence platform
ai-lang knowledge push --goal extract_invoice
// Shares: "Few-shot with 3 examples + explicit field list + temperature 0.1
//          achieves 97% on invoice extraction. Model: sonnet for complex, 
//          local-3b for standard. Cost: $0.003/invoice avg."

// Other teams benefit:
ai-lang knowledge pull "invoice extraction"
// Get the proven strategy, skip the 5 experiments, start at 0.97
```

## Compiler vs Stdlib vs Library — What Lives Where

Rule: if it's an **invariant** (must always be true, forgetting it = bug), bake into compiler. If it's **behavior** (runs at runtime, customizable), make it stdlib.

### Compiler (language-level, compile-time enforcement)
```
Ownership/borrowing:          compiler enforces memory safety
Type system:                  compiler checks all types
Capability permissions:       compiler rejects disallowed I/O in deps
Comptime execution:           compiler runs code at compile time
#[goal] attribute:            compiler validates metric fn sigs, type-checks test set, 
                              validates constraints don't conflict
#[adaptive] attribute:        compiler ensures provenance logging can't be omitted,
                              enforces that static zones can't be mutated
#[agent] attribute:           compiler validates tool list, constraint declarations
#[ai(policy)] attribute:      compiler enforces local_only = no network calls compiled in
Content-addressed imports:    compiler resolves hashes, enforces capability permissions
Dead experiment cleanup:      compiler deletes losing variants from binary
Error messages with node IDs: compiler produces structured diagnostics
Test set schema validation:   compiler type-checks test data against function inputs
No null enforcement:          compiler rejects null, requires Option<T>
No exception enforcement:     compiler rejects throw, requires Result<T,E>
```

### Stdlib (ships with language, runtime behavior)
```
std.ai:              inference, model routing, embedding, generation
std.ai.optimize:     hill climbing, tournament, bayesian eval loops
std.ai.trace:        execution tracing, provenance logging
std.ai.experiment:   experiment generation, mutation strategies
std.net/fs/io:       I/O operations
std.ui:              GPU rendering (vello+wgpu)
std.render3d:        3D scene graph, physics
std.db:              database access, query builder
std.platform:        mobile integration
std.http:            server/client, routing, middleware, websockets
std.crypto:          hashing, encryption, JWT, UUID
std.time:            clocks, duration
std.path:            cross-platform paths
std.serial:          JSON, MessagePack, Protobuf, CBOR, TOML, YAML
```

### Compiler-Aware Stdlib (compiler + stdlib work together)
```
#[goal] + std.ai.optimize:
  Compiler:  parses attribute, validates metric types, checks test set schema
  Stdlib:    runs optimization loop, calls LLM for experiment generation, evaluates

#[adaptive] + std.ai.trace:
  Compiler:  ensures every adaptive fn logs changes (can't omit)
  Stdlib:    actually writes provenance logs, manages rollback

#[ai(policy)] + std.ai:
  Compiler:  enforces policy (local_only = no network in compiled output)
  Stdlib:    routes inference calls based on policy at runtime

#[agent] + std.ai:
  Compiler:  validates tool declarations, constraint types
  Stdlib:    runs agent reasoning loop, executes tools, logs actions

Content imports + std.security:
  Compiler:  resolves hashes, enforces capability permissions
  Stdlib:    AI audit scan, health monitoring, vulnerability checking
```

### Why This Matters
```
Library-based #[goal]:    decorator that hopes you set it up right
                          skip the metric fn? compiles fine, fails at runtime
                          wrong test set types? silent data corruption
                          forget provenance? no record, can't debug

Compiler-based #[goal]:   refuses to compile if metric fn signature wrong
                          refuses to compile if test set types don't match fn inputs
                          refuses to compile if adaptive fn missing provenance
                          optimizer sees full type graph, generates better experiments

Same reason Rust borrow checker works: it's in the compiler, not a linter.
If it were a library, people skip it. In the compiler, you can't.
```

### Error/Perf Logging — Compiler-Enforced
```
// Logging format (PSV) is stdlib convention
// But compiler enforces that certain contexts MUST log:

#[adaptive] fn:    compiler injects provenance logging (can't opt out)
#[goal] fn:        compiler injects experiment logging (can't opt out)  
#[agent] mod:      compiler injects action logging (can't opt out)
unsafe{} blocks:   compiler injects audit logging (can't opt out)

// Regular fn: logging is optional (use std.log if you want)
// Special zones: logging is mandatory (compiler inserts it)

// All logs: PSV format, structured, AI-readable, human-renderable
```

## Critical Considerations

### 1. Bootstrap Problem — Compiler First, Language Second
```
The AI language doesn't exist. LLMs aren't trained on it. Who writes the first program?

Phase 0: Compiler written in Rust (proven LLVM frontend language)
Phase 1: LLMs learn from spec + compiler test suite + stdlib source (in-context)
Phase 2: Compiler self-hosts (rewritten in AI language, compiled by Rust version)
Phase 3: LLMs trained on growing AI language corpus

The language is 80% Rust syntax. LLMs already know Rust.
The 20% new (#[goal], #[adaptive], std.ai) is learnable from spec in context.
Same way this conversation worked — no training data needed, just a good spec.

Training timeline:
  Month 1-3:   models use spec-in-context (works today, proved it)
  Month 4-6:   fine-tune on compiler test suite + stdlib (~50K lines)
  Month 7-12:  fine-tune on community code (~500K lines)
  Month 12+:   models natively know AI language from pre-training
```

### 2. AI Reproducibility — Pin Model Versions in Build
```
Problem: same code + different model version = different output
  fn summarize(text:str)->str{ai.complete("summarize: {text}")}
  Today model v1: "Brief summary..."
  Tomorrow model v2: completely different text
  Tests break. Users see different behavior. Nothing in YOUR code changed.

Solution: model pins in build manifest
  #[ai(model:"sonnet-4.6-20260401")]
  fn summarize(text:str)->str{...}

  // .build-manifest (auto-generated, checked into repo)
  // code-hash: sha256:abc123
  // model-pins: {summarize: "sonnet-4.6-20260401"}
  // stdlib-hash: sha256:def456
  // Same manifest = same behavior, always.

  // Upgrade models explicitly:
  ai-lang upgrade-models    // re-runs test set against new models
                            // only pins new version if tests still pass
```

### 3. Cost Ceiling — System-Wide Budget Governance
```
AI inference everywhere = costs spiral. Need project-level control.

// ai-lang.config
{
  "budget": {
    "monthly": 500.00,
    "per_goal_optimization": 5.00,
    "per_adaptive_call": 0.01,
    "per_agent_action": 0.05,
    "alert_at": 0.80,
    "hard_stop": true
  }
}

// Compiler enforces: every AI call has a cost path
// Runtime tracks: cumulative spend, per-function spend

ai-lang costs --month
//   extract_invoice:  $12.40 (620 calls, $0.02 avg)
//   summarize:        $3.20  (320 calls, $0.01 avg)
//   goal/optimize:    $8.50  (3 optimizations)
//   agent/support:    $45.00 (900 tickets)
//   TOTAL:            $69.10 / $500.00 budget
```

### 4. Privacy & Data Sovereignty — Sensitivity at the Type Level
```
#[sensitive(pii)] struct User{name:str,email:str,ssn:str}
#[sensitive(phi)] struct Patient{name:str,diagnosis:str}
#[sensitive(financial)] struct Transaction{amount:f64,account:str}

// Compiler enforces:
//   sensitive data NEVER sent to external AI APIs (compile error)
//   sensitive data NEVER in shared knowledge push
//   #[goal] test sets with sensitive types MUST use synthetic data
//   #[agent] tools can't exfiltrate sensitive fields

#[ai(policy:local_only)]
fn process_user(u:User)->Report{
  ai.extract<Report>(u.to_redacted())
  // .to_redacted() auto-generated by compiler, strips #[sensitive] fields
}

// Knowledge sharing respects sensitivity:
ai-lang knowledge push --goal process_user
// Shares: strategy, prompts, parameters
// Never shares: test data, PII, actual inputs

// Compliance: GDPR, HIPAA, SOC2 enforced by compiler, not by policy docs
```

### 5. Multi-Agent Coordination — Hierarchy with Arbitration
```
// Two agents, potentially conflicting goals:
#[agent] mod sales{goal:"maximize revenue"}
#[agent] mod support{goal:"maximize satisfaction"}
// Sales upsells. Support resolves fast. Conflict on same customer.

#[agent_system(
  arbitrator: business_rules,
  priority: [support, sales],          // support wins ties
  shared_state: customer_context,      // both see same data
  constraints: {
    never: [contradict_other_agent],   // can't promise what other denied
    always: [log_conflicts]
  }
)]
mod agents{
  #[agent] mod sales{...}
  #[agent] mod support{...}
}

// Compiler enforces: agents in same system MUST declare arbitration
// Runtime: conflict detected -> arbitrator resolves -> loser yields -> logged
```

### 6. Graceful Degradation — Mandatory Fallbacks
```
// Compiler enforces: every AI-enhanced function MUST have a fallback

#[ai(fallback:deterministic)]
fn summarize(text:str)->str{
  match ai.complete("summarize: {text}"){
    Ok(s)=>s,
    Err(_)=>text.sentences().take(3).join(" ")  // fallback: first 3 sentences
  }
}

// Degradation levels (automatic):
// Level 0: full AI (normal operation)
// Level 1: local models only (API down, reduced quality)
// Level 2: cached responses (model unavailable, stale but functional)
// Level 3: static fallback (no AI, deterministic behavior)

// #[adaptive] degrades to last-known-good static version
// #[agent] queues actions, processes when AI restored
// #[goal] pauses optimization, keeps current best

// Compiler refuses to compile #[adaptive] or #[goal] without fallback defined
```

### 7. Governance — Agent Deployment Pipeline
```
#[agent(
  approval: required,
  reviewers: ["security","ops"],
  sandbox_duration: 7d,
  rollback_trigger: {error_rate:>0.05, cost:>budget*1.5}
)]
mod support{...}

// Deployment stages:
ai-lang deploy --agent support --stage sandbox
  // Real traffic, actions logged but NOT executed
  
ai-lang deploy --agent support --stage shadow
  // Actions executed, human reviews outcomes for 7d
  
ai-lang deploy --agent support --promote
  // Requires reviewer approval
  // Gradual rollout: 10% -> 50% -> 100%
  // Automatic rollback on error_rate > 0.05

// Provenance logging continues forever in production
// Every agent action auditable, every decision explainable
```

### 8. Offline-First — AI Without Network
```
// Binary ships with embedded local model (1-3B params)
// Handles: classification, extraction, simple completion

// Offline behavior:
#[ai(policy:prefer_local)]
fn categorize(text:str)->Category{
  ai.classify(text,categories:CATEGORIES)
  // Online: routes to best model (maybe API)
  // Offline: uses embedded local model (lower quality but works)
}

// What works offline:
//   std.ai with local models (Tier 1-2)
//   #[adaptive] with cached parameters (no new optimization)
//   #[goal] paused (needs eval which may need API)
//   #[agent] with local model (reduced reasoning quality)

// What needs network:
//   API model calls (Tier 3)
//   Knowledge push/pull
//   AI audit on import
//   Dependency resolution (first time only, cached after)

// Compiler warns: "this function uses ai(policy:best_available), 
//                   will degrade to local model when offline"
```

## AI-First Design Properties

```
1. DENSE SYNTAX
   fn not function, let not const/var, i32 not number
   No semicolons required, no return keyword
   12 tokens vs 22 (TS) vs 28 (Go) for same logic

2. AST-CLEAN GRAMMAR
   Every construct = clear tree node
   No ambiguous parses, no preprocessor, no syntax-rewriting macros
   Comptime replaces macros with type-safe compile-time execution

3. ERROR-PROOF BY DESIGN
   No null (Option<T>), no exceptions (Result<T,E>), no data races (ownership)
   No memory leaks, no unchecked array access
   AI can't generate unsafe code — language prevents it

4. SELF-DESCRIBING
   Comptime derives JSON schemas from types
   Comptime generates API docs from function signatures
   Code IS the spec, no separate OpenAPI/docs to maintain

5. UNIVERSAL TARGET
   Write once -> native/WASM/JS/mobile/GPU
   No "rewrite in Rust for speed" or "rewrite in JS for browser"
```

---

# Axon — ASI Extensions

> Language renamed from "AI Language" to **Axon**. File extension: `.ax` (e.g. `main.ax`).
> All references to `.ai` above should be read as `.ax`.

The following sections extend the base PRD with ASI-native primitives, identified by reviewing
the PRD from an ASI perspective. Organized by implementation layer so nothing is over-built early.

## Extension Layering

```
Layer 0 (v1, always on, zero cost)
  No null, no exceptions, no races, capability permissions
  Already in base PRD. This IS the alignment foundation.

Layer 1 (v1-v2, opt-in, near-zero cost)
  Uncertain<T>, Temporal<T>, Goals as values,
  Agent metacognition, std.causal,
  #[contained], #[corrigible]

Layer 2 (v3, opt-in, AI call per audit)
  Value alignment evaluation (#[aligned(values:...)])

Layer 3 (v4, opt-in, SMT solver, critical code only)
  Formal verification (#[verify])
```

---

## Uncertain\<T\> — Uncertainty as a First-Class Type

ASI doesn't return values. It returns values *with confidence*. Every AI-enhanced function
currently pretends certainty it doesn't have. `Uncertain<T>` makes honesty a type.

```
// Without: pretends certainty
fn classify(text:str)->Category{...}

// With: honest about confidence
fn classify(text:str)->Uncertain<Category>{...}

let result=classify(text)
result.value          // Category::Spam
result.confidence     // 0.94
result.alternatives   // [(Category::Ham, 0.05)]
result.source         // Model::LocalSmall | Model::ApiSonnet | Model::Ensemble

// Uncertainty propagates through decisions
let decision=if result.confidence>0.9{act(result.value)}else{escalate(result)}

// Aggregation across multiple inferences
let consensus=Uncertain.consensus([r1,r2,r3])   // weighted vote by confidence
let ensemble=Uncertain.ensemble([r1,r2,r3])     // bayesian combination
```

### Type Definition
```
struct Uncertain<T>{
  value:        T,
  confidence:   f64,              // 0.0-1.0
  alternatives: [(T,f64)],        // other possibilities with probabilities
  interval:     Option<(T,T)>,    // credible interval for continuous types
  source:       InferenceSource,
}

enum InferenceSource{
  LocalModel(str),                // model id + version
  ApiModel(str),
  Ensemble([InferenceSource]),
  Deterministic,                  // pure computation, confidence always 1.0
}
```

### Rules
```
// Deterministic functions: confidence=1.0 implicitly — no syntax change
fn add(a:i32,b:i32)->i32{a+b}

// Compiler warning: unwrapping Uncertain<T> without checking confidence
let raw=classify(text).value   // warning: ignoring uncertainty
                               // prefer: .unwrap_or_escalate() or check first

// Uncertain inputs -> Uncertain output (confidence propagates)
fn pipeline(text:str)->Uncertain<Report>{
  let cat=classify(text)?       // Uncertain<Category>
  let summary=summarize(text)?  // Uncertain<str>
  Ok(Report{category:cat,summary:summary})
  // result.confidence = f(cat.confidence, summary.confidence)
}
```

---

## Temporal\<T\> — Time-Aware Types

ASI plans across time. Values have horizons. Knowledge decays. Decisions expire.
`Temporal<T>` makes time-awareness explicit rather than something you forget to handle.

```
fn forecast_revenue(q:Quarter)->Temporal<f64>{...}

let rev=forecast_revenue(Q2)
rev.value           // 1_200_000.0
rev.confidence      // 0.82
rev.horizon         // 90d
rev.decay           // 0.02/day   (confidence degrades as time passes)
rev.valid_until     // now + horizon
rev.at(30.days)     // recompute confidence at t+30d

// Temporal decisions
#[temporal(
  valid_for:  30d,    // re-evaluate when this expires
  horizon:    1y,     // planning looks 1 year ahead
  checkpoint: weekly  // may change course each week
)]
fn strategy(market:MarketState)->Strategy{...}

// Time-aware collections
let knowledge:Temporal<Map<str,str>>=load_knowledge()
knowledge.prune_stale(threshold:0.5)  // drop entries below confidence
knowledge.at(30.days_ago)             // what did we know then?
```

### Temporal in Agents
```
#[agent]
mod planner{
  fn plan(task:Task)->Temporal<Plan>{
    Plan{
      steps:        generate_steps(task),
      valid_for:    7d,
      checkpoint:   daily,
      invalidated_by: [market_shift,user_goal_change]
    }
  }
}
```

---

## Goals as First-Class Values

`#[goal(...)]` as attribute stays — it's still valid for simple cases.
For ASI: goals compose, conflict, decompose into sub-goals, inherit across agent hierarchies.
These operations require goals to be *values*, not decoration.

```
// Simple case: attribute still works
#[goal(metric:accuracy, target:0.97)]
fn extract_invoice(doc:str)->Invoice{...}

// ASI case: goal as value
let g:Goal=maximize(extract_accuracy)
  .subject_to(latency<500ms, cost<0.01)
  .with_budget(evals:100, cost:5.00)
  .expires(30d)

// Composition
let combined=g.and(minimize_cost).prioritize(accuracy_over_cost)

// Decomposition — agent generates sub-goals with confidence
let subgoals:Uncertain<[Goal]>=g.decompose()
// → [Goal::improve_prompt(0.91), Goal::tune_model(0.74), Goal::add_examples(0.88)]

// Conflict detection (compile-time where possible, runtime otherwise)
let conflicts=Goal.check_conflicts([sales.goal, support.goal])
match conflicts{
  Conflict::Priority(a,b)  => resolve_by_priority(a,b),
  Conflict::Incompatible(a,b) => escalate_to_human(a,b),
  Conflict::None           => proceed()
}

// Goal inheritance in agent hierarchies
#[agent(inherits_goal_from: parent_system)]
mod sub_agent{...}
```

### Goal Type
```
struct Goal{
  metric:      fn(&State)->f64,
  target:      Option<f64>,
  constraints: [Constraint],
  budget:      Budget,
  expires:     Option<Duration>,
  strategy:    OptimizationStrategy,
  priority:    u32,
}

impl Goal{
  fn and(other:Goal)->GoalSet
  fn or(other:Goal)->GoalSet
  fn prioritize(rule:PriorityRule)->Goal
  fn decompose()->Uncertain<[Goal]>
  fn conflicts_with(other:Goal)->Option<Conflict>
}
```

---

## Agent Metacognition

Agents that can't inspect their own reasoning can't catch their own failures.
Without metacognition, an agent in a reasoning loop has no way to know it's stuck.

```
#[agent]
mod planner{
  fn plan(task:Task)->Plan{
    let trace       = self.reasoning_trace()
    let uncertainty = self.uncertainty_estimate()
    let blind_spots = self.known_unknowns()
    let assumptions = self.unverified_assumptions()

    if uncertainty.overall > 0.3{
      return Plan::GatherInfo(blind_spots)        // I don't know enough yet
    }
    if assumptions.contains_high_stakes(){
      return Plan::Clarify(assumptions.highest_stakes())  // confirm before acting
    }
    if trace.detect_loop(){
      return Plan::Escalate("reasoning loop detected")
    }

    Plan::Execute(generate_steps(task))
  }
}
```

### Metacognition Trait (auto-implemented by #[agent])
```
trait Metacognitive{
  fn reasoning_trace()         -> [ReasoningStep]
  fn uncertainty_estimate()    -> Uncertain<f64>
  fn known_unknowns()          -> [BlindSpot]
  fn unverified_assumptions()  -> [Assumption]
  fn detect_loop()             -> bool
  fn confidence_calibration()  -> CalibrationReport   // am I historically over/under-confident?
}
```

---

## std.causal — Causal Reasoning

`#[goal]` optimization without causality finds correlations, not levers.
It can't distinguish "ad spend causes revenue" from "both are caused by seasonality."
`std.causal` adds do-calculus so experiments target actual causes.

```
use std.causal

let model=causal.model{
  nodes: [ad_spend, awareness, trials, retention, revenue],
  edges: [ad_spend->awareness, awareness->trials, trials->retention, retention->revenue],
  confounders: [seasonality->trials, seasonality->revenue]
}

// Observational: what correlates?
let corr=model.observe(revenue).given(ad_spend>100K)

// Interventional (do-calculus): what happens if we force a change?
let effect=model.do(ad_spend:=2x_current)   // -> Uncertain<f64>

// Counterfactual: what would have happened?
let cf=model.counterfactual(
  observed:    {revenue:1M, ad_spend:100K},
  hypothetical:{ad_spend:50K}
)
// "Had we spent 50K, revenue would have been 820K (CI: 780K-860K)"

// Integration with #[goal]: experiments target causal levers, not correlations
#[goal(
  metric:        revenue,
  causal_model:  model,     // NEW: focus on highest-leverage variables
  target:        1.2x_current,
  budget:        {evals:20}
)]
fn growth_strategy(state:MarketState)->Strategy{...}
// Without causal_model: tries ad_spend, pricing, messaging, timing, ... all of it
// With causal_model:    targets ad_spend + retention (proven causal levers)
// Result: fewer experiments, better outcomes
```

---

## Structural Alignment — Layer 1

The PRD already has alignment at Layer 0 implicitly:
- No null + no exceptions = entire bug classes eliminated by type system
- Capability permissions = what agents can touch is compiler-enforced
- Governance pipeline = approval before autonomous action

Layer 1 gives these mechanisms **named, composable identity** — explicit rather than assembled
from parts each time. Zero new overhead: these compile to the same enforcement mechanisms.

```
// #[contained] — named alias for capability permissions on agents
#[contained(
  fs:    [read("./data/"), write("./output/")],
  net:   ["*.myapi.com", "api.openai.com"],
  exec:  none,
  never: [read("/etc/"), write("~/.ssh/"), spawn_subprocess]
)]
#[agent] mod processor{...}

// #[corrigible] — named alias for governance + override hooks
#[corrigible(
  override_by: ["ops","security"],    // who can stop or redirect
  pause_on:    [novel_action_class],  // auto-pause on unseen action types
  shutdown:    graceful,             // finish current action, then halt
  heartbeat:   60s                   // must check in or auto-pause
)]
#[agent] mod autonomous_worker{...}

// Standard pattern for any production agent
#[contained(...)]
#[corrigible(...)]
#[agent] mod safe_agent{...}
```

### What Layer 1 Is Not
```
// NOT included in Layer 1:
//   Value alignment evaluation ("does this action match our values?") — Layer 2
//   Formal proofs of alignment                                        — Layer 3
//   AI judgment of ethical correctness                                — Layer 2+

// IS included in Layer 1:
//   Hard structural limits on what agents can touch   (compile-time)
//   Hard guarantee agents can be stopped/corrected    (compile-time)
//   Zero runtime overhead
//   Zero AI calls — pure constraint checking
```

---

## Formal Verification — Layer 3 (v4+, critical code only)

The type system already provides substantial proof-level guarantees for free.
Full SMT-solver verification is reserved for cryptographic primitives, financial math,
and safety-critical stdlib paths where testing isn't sufficient.

```
// Future syntax — not v1
#[verify]
fn transfer(from:Account, to:Account, amount:f64)->Result<(),Error>{...}
proof{
  // Compiler runs Z3/CVC5, rejects build if unprovable
  forall(from,to,amount where amount>0):
    transfer(from,to,amount).is_ok() -> from.balance decreases_by(amount)
  forall(from,amount where from.balance<amount):
    transfer(from,_,amount).is_err()
  no_double_spend:
    forall concurrent_transfers: sum(debits) <= initial_balance
}
```

Most code never uses `#[verify]`. The type system handles 95% of correctness guarantees at zero cost.
This layer exists for the 5% where proof matters more than shipping speed.

---

## Updated Type System

```
Primitives:    i8 i16 i32 i64 u8 u16 u32 u64 f32 f64 bool str char
Collections:   [T] (array/slice), Map<K,V>, Set<T>, Vec<T>
Nullable:      Option<T>          never null, always explicit
Errors:        Result<T,E>        never exceptions, always explicit
Functions:     fn(A,B)->C
Tuples:        (A,B,C)
Unions:        A|B|C              structural, like TypeScript
Structs:       struct Point{x:f64,y:f64}
Enums:         enum Color{Red,Green,Blue} or ADTs with data
Traits:        trait Serialize{fn to_bytes(&self)->Bytes}
Generics:      <T:Trait+Clone>    monomorphized, zero-cost
Inference:     let x=42           i32 inferred; let y=fetch(url)?  inferred from return
Uncertain:     Uncertain<T>       value + confidence + alternatives        ← NEW (Layer 1)
Temporal:      Temporal<T>        value + horizon + decay + valid_until    ← NEW (Layer 1)
Goals:         Goal               metric + constraints + budget, composable ← NEW (Layer 1)
```

## Updated Stdlib

```
std.ai              core inference, model routing, embedding, generation
std.ai.optimize     hill climbing, tournament, bayesian eval loops
std.ai.trace        execution tracing, provenance logging
std.ai.experiment   experiment generation, mutation strategies
std.causal          causal models, do-calculus, counterfactuals     ← NEW (Layer 1)
std.net/fs/io       I/O operations
std.ui              GPU rendering (vello+wgpu)
std.render3d        3D scene graph, physics
std.db              database access, query builder
std.platform        mobile integration
std.http            server/client, routing, middleware, websockets
std.crypto          hashing, encryption, JWT, UUID
std.time            clocks, duration
std.path            cross-platform paths
std.serial          JSON, MessagePack, Protobuf, CBOR, TOML, YAML
```

---

# Axon — ASI Review (2026-09-29)

> A review of this PRD from the position of its intended primary user: an AI system that
> writes, optimizes, and operates Axon programs with delegated authority. It is **appended**
> rather than edited inline because code and specs cite this file by line number
> (`infer.rs`, `builtins.rs`, `codegen/types.rs`, `governance/specs/R13/R14/R16/R7c`); inserting
> text above line 1733 would silently break those references.
>
> Every "measured" claim below was run against `target/debug/axon` at `8b0d2dc9`. The repros
> are listed so each one can become a gate that fails until the gap is closed.

## A. Drift — where this PRD no longer describes the language

The build has overtaken the PRD in some places and diverged from it in others. Treat
`AXON_REFERENCE.md` (generated, drift-gated) as authoritative for *what exists*; this table
records *which PRD statements a reader must discount*.

| PRD says | The build does | Consequence for this doc |
|---|---|---|
| `#[attr]`, `struct P{…}`, `enum E{…}`, `.ai`, `ai-lang …` | `@[attr]`, `type P = {…}`, `type E = A{…} \| B{…}`, `.ax`, `axon …` | Syntax examples above are illustrative, not parseable |
| Integers default to `i32` | Default is `i64`; checked operators panic on overflow; `wrapping_*` is the explicit opt-out | `test_overflow` (line 467) cannot type-check: `add` returns `i32`, not `Result` |
| Reproducibility = pin model versions (§Critical 2) | Pins are necessary but insufficient (providers are not bit-deterministic). Shipped answer: host journal (`AXON_RECORD`/`AXON_REPLAY`, divergence ⇒ exit 11) + `AXON_AI_REPLAY` + seeded RNG + virtual clock | Reproducibility is a property of the **environment seam**, not of the model string |
| `#[corrigible]` = "hard guarantee agents can be stopped (compile-time)" | Stopping is a **runtime** latch held by the `axon-os` supervisor, outside the agent's authority; monotonicity proved in TLA+/Coq (R32) | A compile-time stop guarantee is not possible; the design principle is *the kill switch lives outside what it kills* |
| Self-improving compiler emits AI-generated asm, "verified correct" | Passes are **data** in a closed rewrite vocabulary, admitted by a 4-gate firewall (oracle, capability monotonicity, test preservation, perf) | The asm-graduation loop (§Self-Improving Compiler) should be retired, not built: an unverifiable asm path is a TCB hole |
| `use "sha256:…" as x` — import by hash | `mod x` + `use x.{…}` resolved **ambiently** via `AXON_PATH` → `~/.axon/lib` → `<bindir>/../lib/axon`, first match wins, choice unrecorded; `axon lock`/`verify-lock` pin hashes after the fact | Content-addressed import is still a gap (see G12) |
| Per-import capabilities `{allow:[pure]}` | Per-fn `@[contained]`, effect rows, `AXON_ALLOWED_EFFECTS`, job profiles | No module-boundary ceiling yet (G12) |
| `comptime` = zero runtime cost | True under `axon build` only; the interpreter evaluates `comptime` bodies as ordinary runtime code on every pass | Performance claims hold for native only |
| Not mentioned | Effect rows + handlers, refinement types (runtime-enforced, exit 6), `@[pure]`/`@[total]`, Principals with attenuation-by-construction, profiles (`developer`→`hermetic`), approval policy vs. evidence, Risk-typed deploy pipeline, hash-chained audit ledger, attestation (R26/R31), Cortex repair loop | These are now the load-bearing design; the "ASI Extensions" section above under-describes the language |

## B. Contradictions inside the PRD

An ASI reading this PRD as a spec would exploit or trip over each of these.

1. **The flagship optimization example leaks the test set.** Experiment 5 (line 874) adds
   "3 labeled examples from test set" as few-shot prompts and then reports 0.97 *on that test
   set*. The headline result is contaminated: the optimizer was shown the answers. Cortex already
   embodies the fix (*the adjudicating check is one the generator is never shown*); `#[goal]` must
   adopt it as a language rule, not a convention. → G3.
2. **"Loser deleted automatically" vs. `rollback: on_regression` and provenance.** You cannot roll
   back to a variant you deleted, and you cannot replay or audit an experiment whose code is gone.
   Losing variants should leave the *binary*, not the *record*: keep them content-addressed in the
   provenance store.
3. **Auto-serialization of every struct vs. `#[sensitive]`.** `user.to_json()` on a
   `#[sensitive(pii)]` type is an exfiltration primitive the compiler generated for free.
   Sensitive types must not auto-derive serializers; they get `to_redacted()` only, and an explicit,
   audited `declassify` for the rest.
4. **Money is `f64`.** `#[sensitive(financial)] struct Transaction{amount:f64}` and the
   `#[verify] transfer(amount:f64)` proof example both reason about balances in binary floating
   point, where `0.1+0.2≠0.3` and "decreases_by(amount)" is not an exact statement. The build
   ships `Decimal` (i128, scale 9, explicit rounding mode on division); the PRD's money examples
   should use it, and proofs over balances should be stated over it.
5. **"AI can't generate unsafe code — language prevents it"** (line 1317) vs. the `unsafe{}` block
   (line 494). Restate honestly: `unsafe` is a *capability* — refused unless granted, like `exec` —
   not something the language makes impossible.
6. **"No registry, no package manager"** (line 414) vs. the Shared Intelligence **Registry**
   (line 750). There is a registry; the question is only what it is allowed to serve.
7. **Supply-chain rigor stops at the code boundary.** `knowledge pull` and `adaptations/` import
   prompts, routing rules, and model weights that *change program behavior* — code by another
   name — with self-reported p-values as the trust signal. Those are trivially fabricated.
   Imported knowledge must be treated as untrusted input (G1), carry replayable evidence (the
   journal plus the eval set hash), and re-validate against the importer's own sealed holdout
   before it can alter an `@[adaptive]` fn.
8. **"Deterministic eval" with a nondeterministic metric.** `extract_accuracy` scores
   `output.latency<500ms` (line 948). Latency is not a field of `Invoice`, and wall-clock latency
   is not reproducible. Metrics must be pure over `(input, output, expected)`; latency and cost
   belong in `constraints`, measured separately and reported with variance.
9. **Queued agent actions are a TOCTOU bug.** "#[agent] queues actions, processes when AI
   restored" (line 1240) executes a decision made against a world that has since changed. An
   action must carry its preconditions and re-check them at execution time, or expire (it is a
   `Temporal<Action>`). → G8.
10. **The sandbox stage observes fiction.** "Real traffic, actions logged but NOT executed"
    (line 1258) means every observation downstream of the first un-executed write is wrong, so
    the sandbox scores a trajectory that cannot happen. A sandbox needs a *simulated* world
    response for suppressed effects (a world model or a recorded journal), and must report
    which observations were simulated.

## C. Gaps — what an ASI author needs that neither the PRD nor the build provides

Ranked by how much damage the gap permits, not by how interesting the feature is.

### G1 (P0) — Untrusted data flow / prompt injection is not typed

The PRD never mentions prompt injection. It is the dominant real attack on AI-operated software,
and Axon's whole premise is AI-operated software.

**Measured:** this program type-checks clean (exit 0). Model output flows straight into a
subprocess's argv:

```axon
fn main() {
    match ai_complete("give me a shell command") {
        Ok(cmd) => { let _ = exec(cmd, []) }
        Err(e) => { println(e) }
    }
}
```

`@[contained]` and `AXON_ALLOWED_EFFECTS` bound *whether* `exec` may run. Neither constrains
*what the model gets to choose once it does*. The integrity lattice already exists in userland
(`examples/stdlib/tainted.ax`, i64-only). It needs to be a compiler-tracked property:

- **Sources** mark values `Untrusted`: `ai_complete`/`ai_extract*`, `http_*`, `read_file`,
  `read_line`, `env_var`, `host_await*`, anything `knowledge pull` delivers.
- **Sinks** refuse `Untrusted` arguments in authority-bearing positions: `exec` program and argv,
  HTTP URL and host, `write_file`/`dir_*` paths, SQL text, agent **tool selection**, and the
  *instruction* half of a prompt (data may be interpolated into a prompt; it may not become one).
- **The only lift** is a parse into a type: `parse_int`, `ai_extract::<T>` into a closed enum, a
  refinement predicate, or a declared validator fn. *Parse, don't validate*, enforced.
- **Reuse:** the E1206 flow walker already tracks confidentiality to a set of sinks. Taint is
  the same walker over the integrity axis with a different source/sink table.

**Gate:** the repro above must fail `axon check` with a new E12xx, and
`exec(allowlisted_enum_to_str(x), [])` must pass.

### G2 (P0) — `@[sensitive]` stops at files and AI, not stdout/stderr

**Measured:** `write_file(path, u.ssn)` is refused (E1206, good). `println(u.ssn)`,
`eprintln(s)`, and `"id={u.ssn}"` interpolation are all accepted (exit 0). Stdout is not
"inside the program": it lands in host journals, CI logs, the `axon-web` run pane, and agent
transcripts that are then sent to a model. Extend the sink table to every output builtin, and
treat string interpolation as propagation, not as laundering.

### G3 (P0) — Sealed evaluation (Goodhart and winner's-curse guard)

`goal { holdout: … }` exists. What an optimizer needs is a guarantee about **who has seen
what**:

| Split | Seen by | Used for |
|---|---|---|
| `train` | generator (few-shot, analysis) | proposing variants |
| `dev` | optimizer's score function | selecting among variants |
| `holdout` | nobody until approval; hash committed at goal creation | the number a human approves |

- The compiler refuses any data flow from `holdout` into a prompt, generator, or `@[adaptive]`
  parameter (G1's walker again).
- Report the **optimism gap** `dev − holdout` on every approval. A large gap is overfitting, and
  it is the single most useful number to show a human reviewer.
- With 100 evals on 50–200 noisy LLM-scored examples, the best `dev` score is biased upward.
  Keep a variant only when the delta's confidence interval excludes zero, and re-score the
  finalist on fresh draws.

### G4 (P1) — `Uncertain<T>` confidence is not calibrated

A model's self-reported 0.94 is not a probability. `if result.confidence>0.9 {act}` is only
safe if 0.9-confidence claims are right about 90% of the time. Nothing in the PRD or the build
checks that. Proposal:

- `confidence` carries its **provenance**: `SelfReported | Calibrated{set_hash, method}`.
- A `calibrate(fn, labeled_set)` step fits split-conformal thresholds on held-out data and yields
  a coverage guarantee ("the prediction set contains the truth ≥ 90% of the time").
- Extend W0701: gating an action on `SelfReported` confidence warns; under `AXON_STRICT` or
  `axon deploy`, it errors.
- `confidence_calibration()` (the PRD's metacognition trait, line 1568) becomes a real report:
  a reliability curve from the provenance log, not a promise.

### G5 (P1) — Stable node identity and typed patches as the unit of AI edit

The PRD's node IDs (`#fn_process.body.stmt_3`) are positional; inserting one statement renames
every later node. AI edits today are text replacements. Proposal:

- **Content-derived node IDs**, stable under unrelated edits.
- **`axon patch`**, taking typed edit operations (`replace_expr`, `insert_stmt`,
  `rename_symbol`, …). Each op names the hash of the node it expects to find, so an edit applies
  to exactly the node that was reviewed or refuses. No fuzzy matching.
- This also gives semantic merge for concurrent agents editing one file, and makes Cortex's
  "restore between attempts" a patch inverse rather than a file copy.

### G6 (P1) — Review authority, not text

At ASI throughput, humans cannot read ASTs. `.ax.approved` binds to a whole-file hash: strict,
but blind. A comment edit and a new `exec` are indistinguishable to the reviewer. Proposal:

- **`axon ast diff A B`** reports the **authority delta**: effects added or removed,
  `@[contained]` widening, budget changes, new sinks reached by untrusted data, and risk-tier
  change.
- The approval record stores the authority summary alongside the hash, so re-approval shows the
  delta rather than a fresh wall of code.
- **Monotonicity rule:** a self-modification (Cortex, `@[adaptive]`, `axon improve`) whose
  authority delta is non-positive may proceed under policy. Any widening requires a human
  signature. That rule is the one TCB property self-improvement must never be able to change.

### G7 (P2) — Budgets and capabilities as affine values

Axon already has `own`/`ref`. Budgets and capability grants are the resources that most need
ownership semantics, and today they are runtime integers (`principal_mint(…, grant)`). An
affine `Budget` can be **split** and **moved** into a sub-agent and **cannot be copied**, so
"two sub-agents each got the whole budget" becomes a type error. R27's coalition ceiling stays
as the runtime backstop for what cannot be proved statically.

### G8 (P1) — Reversibility belongs in the effect row

Risk derivation already consumes an *irreversibility annotation*. An annotation is declared and
therefore forgettable. Proposal: split effect rows into `Reversible` and `Irreversible`
(`http_post` to a payments host, `exec rm`, sending email), or require a declared compensating
action (a saga). Then, mechanically:

- `Irreversible` ⇒ Risk ≥ High ⇒ the full pipeline, with no override.
- `Irreversible` actions never run in the sandbox or shadow stage (B10) and always re-check
  their preconditions at execution time (B9).
- `axon replay` can show a reviewer "this run did 3 irreversible things", leading the summary.

### G9 (P3) — Typed agent protocols

`Chan<T>` types each message but not their **order**. Multi-agent arbitration (PRD §5), R33
quorum votes, and approval hand-offs are protocols ("propose → vote×N → commit | abort"). Session
types over channels would make "committed before quorum" unrepresentable rather than tested.

### G10 (P3) — Deliberation is a resource with no price

The metacognition example returns `Plan::GatherInfo` or `Plan::Clarify`, but nothing prices
thinking, asking a human, or gathering information against acting now. Proposal:
`@[anytime(deadline)]` fns that return a best-so-far `Uncertain<T>` when the deadline or budget
hits, plus a value-of-information helper that uses the calibrated uncertainty from G4. Without
calibration, this is guesswork, so G4 comes first.

### G11 (P3) — Rationalization is recorded as evidence

The experiment log's `reasoning:` and `analysis:` fields are the generator's own story about why
a change helped. That is a hypothesis, not a cause. Provenance should separate
`claimed_reason` (text) from `measured_effect` (an ablation: revert just that mutation on the
dev set). The same applies to shared knowledge: publish the ablation, not the narrative. This
is where `std.causal` and the shipped `Counterfactual` type earn their place.

### G12 (P2) — Content-addressed, capability-bounded modules

This is the PRD's own §1 promise, still open. Minimum slice:

1. Record the resolved path and hash of every module in the `run_start` record and the journal.
   Today, nothing records which file was chosen.
2. Support `use "axsha256:…"` so the hash is the import.
3. Allow a module-boundary effect ceiling (`mod http allow {Net}`), checked by the existing
   effect-row subsumption (E1310).

### G13 (P2) — Units are written but not typed

The PRD writes `500ms`, `5s`, `30d`, `$0.01`, and `budget: {cost:5.00, time:1h}` as if they
were typed. They are not. `Duration`, `Money` (a `Decimal` plus a currency), and `Tokens` with
dimensional checking would close the class of bugs where `budget_left` (micro-dollars) is
compared with a token count.

## D. Fold-in order

| Priority | Item | First slice | Acceptance gate |
|---|---|---|---|
| P0 | G1 taint | Source/sink table + walker (reuse E1206); `exec`/`http`/path sinks only | §G1 repro fails `axon check`; enum-validated form passes |
| P0 | G2 sensitive→stdout | Add output builtins + interpolation to the E1206 sink/propagation set | `println(u.ssn)` fails; `println(redact(u))` passes |
| P0 | G3 sealed holdout | `train/dev/holdout` in `goal{}`; holdout hash committed; flow refusal | Few-shot-from-holdout (B1) fails to compile; approval shows optimism gap |
| P1 | G6 authority diff | `axon ast diff` over the existing `ast review` effect/cap extraction | Adding one `exec` to an approved file reports `+Exec`, not just "hash changed" |
| P1 | G8 reversibility | `Irreversible` effect row on a closed builtin list | An irreversible call cannot be deployed below Risk High |
| P1 | G4 calibration | Confidence provenance + split-conformal `calibrate` | Gating on `SelfReported` warns; errors under `axon deploy` |
| P1 | G5 stable IDs | Content-derived node IDs in `axon parse` JSON | IDs are unchanged for untouched nodes after an unrelated insert |
| P2 | G12, G7, G13 | See above | — |
| P3 | G9, G10, G11 | See above | — |

B-items that are **doc fixes only** (apply when this PRD is next revised): B2, B4, B5, B6, B8.
B3, B7, B9, and B10 need the matching G items.

## E. Principles this review adds to the PRD

These are already true of the best parts of the build (Cortex, axon-os, the replay journal).
Stating them makes them binding on the parts that are not built yet.

1. **The check that judges a candidate is one its generator never saw.**
2. **The kill switch lives outside what it kills.**
3. **Authority only narrows under self-modification.** Widening needs a human signature.
4. **Every datum from a model or the world is untrusted until parsed into a type.**
5. **Stated confidence without calibration is not evidence.**
6. **Irreversibility is a type, not a comment.**
7. **Reproducibility is a property of the environment seam, not of the model version.**
8. **An absent gate is not a passed gate**, and a document may not imply completeness it does not have.

---

# Axon — Product Review (2026-09-29)

> The ASI Review above asks *what the language must guarantee*. This section asks *what a
> customer needs in order to adopt it*. It covers the jobs, surfaces, and operating model around
> the language that neither this PRD nor the build provides yet. Grounded against `8b0d2dc9`.
> "Absent" means it was searched for in `crates/*/src` and `scripts/` and not found; that is not
> the same as "proved impossible".

## P-A. Who is buying, and the job they hire Axon for

ROADMAP §9 defaults the first customer to *"research lab building agent products with hard
safety constraints."* Three roles meet in that account, and today the product serves only the
first:

| Persona | Job to be done | Served today? |
|---|---|---|
| **Builder** (engineer or AI author) | Write an agent, make it better, prove it is bounded | Yes: CLI, `goal`, Cortex, `axon-os explain` |
| **Approver** (safety, security, compliance lead) | Decide what may run, see what did run, stop it | Partly: `ast approve`, kill-switch, ledger. No inbox, no authority diff (G6), no report they can hand to an auditor |
| **Operator** (on-call, ops) | Keep it running, know when it misbehaves, roll back | No. `axon deploy` runs the program **once** and exits. There is no running service to operate |

**The core product gap:** Axon has an excellent *pre-flight* story (prove, approve, gate) and an
excellent *forensics* story (journal, replay, ledger), but no **in-flight** story. The PRD's
§Governance promises sandbox → shadow → 10% → 50% → 100% rollout with automatic rollback on an
error rate. None of that lifecycle exists. Acid Test 3 promises *"sees deploy + post-deploy
metrics"*. The shipped pane shows the optimizer's score trajectory, which is pre-deploy.

## P-B. Missing features, ranked by adoption impact

### Tier 1: without these, a design partner cannot run Axon in production

**PM1. Long-running deployments: versions, triggers, rollback.**
A deployment should be an object: `{program hash, approval record, grant, version}`.
- **Triggers:** schedule (cron), webhook, queue message, file arrival, manual. `host_await`
  already gives the resume substrate. What's missing is the thing that wakes the program up.
- **Commands:** `axon deploy` creates version N. `axon rollback` returns to N-1 in one step,
  with the previous approval still valid, because it binds to the program hash.
- **Staged rollout:** stages as the PRD describes (shadow, then percentage), with the promotion
  criterion written as a `Goal` so it is typed and audited like everything else.
- **Metric:** time from "approved" to "running on real traffic", and time to roll back. Both
  should be minutes.

**PM2. Live monitoring and alerting (the operator's pane).**
Per deployment: runs/hour, error and exit-code mix, spend against budget, gate refusals, kill
events, and the score of the declared `Goal` on live traffic versus its approved holdout score
(the online counterpart of G3's optimism gap). Alert on budget burn rate, divergence from the
approved score, and any `SandboxViolation` (exit 8). The R29 monitor already polls the ledger;
it trips the kill-switch but tells no human.
- **Notification sinks:** email, Slack, webhook. These are sinks, so they go through G1/G2 like
  any other output.

**PM3. Approval inbox with mid-run, human-in-the-loop actions.**
Today approval is a file written before the run. Real agents need a person *during* the run:
"refund $240? approve / deny / edit". Build it on `host_await` (the resume runtime exists):
- the program suspends on an approval request, and the request lands in an inbox (web plus
  Slack/email notification);
- the decision resumes the run and is ledgered with the approver's authenticated identity;
- requests carry a timeout with a declared default, which is always **deny** for irreversible
  actions (G8);
- **Metric:** median time-to-decision; the percentage auto-approved by policy versus sent to a
  human. The second number is what makes the product scale.

**PM4. Secrets broker.**
`env_var` is refused inside `@[contained]` (correctly: it is an ambient secret channel), so a
contained agent that calls a paid API has no sanctioned way to get the key. Needed: named
secrets granted per job in the manifest (`secrets = ["stripe_live"]`). The value is injected
at the sink (the HTTP header) and **never** materialises as an Axon `str`, so it cannot be
printed, journaled, or sent to a model. Journals record the secret *name*, not the value. This
also closes the "journals hold env-var values verbatim" hazard called out in `CLAUDE.md`.

**PM5. Connectors and tool catalog, via MCP.**
The PRD's agent example lists `tools: [db.query, email.send, ticket.update]`. The runtime has
no tool ecosystem: no MCP client in `axon-core` (the only MCP code is in the ledger and signal
side-products). Ship:
- an **MCP client** in which each MCP tool is imported with a declared effect row and
  capability, so `@[contained]` and G1 taint apply to tool calls exactly as they do to builtins;
- an **MCP server mode** (`axon serve --mcp`) that exposes approved Axon functions *as* tools to
  other agent stacks. This is the adoption wedge: teams keep their agent framework and route
  consequential actions through a bounded, audited Axon function. It matches R38 ("Axon
  Embedded") and is cheaper to build.

### Tier 2: needed to win the evaluation against alternatives

**PM6. Eval-set management.** G3 defines the discipline; the product needs the workflow:
import labelled data (CSV/JSONL), version it by hash, split it (train/dev/holdout), keep it
fresh by promoting reviewed production cases into it, and flag when production inputs drift
away from the eval distribution. Today eval data is inline literals in `goal{ holdout: … }`.

**PM7. Cost forecasting before a run, not just metering after.** The meter charges per call.
An approver should see *"this goal will cost $3–$11 (p10–p90) and at most $15, the hard
cap"* **before** approving. The approval should bind to that cap, and the Improve pane should
show $/point-of-score so the optimizer's value is legible.

**PM8. Incident reports generated from the journal.** When something goes wrong, an operator
needs one document: what ran, under whose authority, what it touched (irreversible actions
first), where it diverged from the last good run (`axon replay --diff`), and what gate would
have caught it. Every ingredient exists; this is assembly work plus a plain-English renderer.
This is the artifact a compliance buyer actually purchases.

**PM9. Compliance evidence export.** `@[sensitive]`, the hash-chained ledger, attestation, and
approval records map directly onto SOC 2 change management, the EU AI Act's record-keeping and
human-oversight obligations, and HIPAA access logging. What's missing is the mapping and an
exporter: "evidence pack for control X, date range Y", signed. The PRD's line
*"Compliance … enforced by compiler, not by policy docs"* is a sales claim with no artifact
behind it.

**PM10. Teams, identity, and multi-tenancy.** The ledger has RBAC and uid-based admin
authority. The runtime and web UI have no notion of *organization → project → member → role*.
There is no SSO and no tenant isolation of journals or provenance (`provenance.jsonl` is
per-user `$XDG_CACHE_HOME`). The Approver persona needs a real identity to sign with, and PM3
needs someone to route to.

### Tier 3: growth and retention

**PM11. Template gallery.** "Examples define the language identity" (ROADMAP §2.7). Make them a
product surface: support triage, invoice extraction, research summarizer, on-call runbook bot.
Each ships with an eval set, a grant, and an approval policy, and runs green in ten minutes.
This is the fastest route through Acid Tests 2–4 for a new user.

**PM12. Authoring-success telemetry (opt-in).** The whole thesis is that *AI writes Axon*.
Measure it: the percentage of `intent compile` outputs that type-check on the first try, the
diagnostics that most often stall a model, and the time from intent to first green run. R38
pre-registers exactly this hypothesis; make it a standing product metric. `axon-signal` already
scores AI coding sessions and could host it.

**PM13. Model and provider management.** Per organization: which providers are allowed, data
residency (EU-only routing), zero-retention flags, model pins, and fallbacks. This is the
product surface for `@[ai(policy)]`, tiers, and `AXON_AI_*`, which today are environment
variables on one machine.

**PM14. Pricing instrumentation.** ROADMAP §9's default is *"margin on user LLM spend +
per-goal-compile + per-deploy fee."* Nothing counts goal-compiles or deploys per customer yet.
Metering must exist before pricing can be tested.

## P-C. Positioning risks

1. **"New language" is the adoption tax.** The market has Python agent frameworks plus sandboxes
   (E2B-, Modal-, Firecracker-style), guardrail libraries, and eval platforms. Each covers one
   pillar well. Axon's advantage is that the pillars are *one* system: the proof, the grant,
   the journal, and the approval all refer to the same typed artifact. Lead with PM5 (MCP
   server) and R38 (embedded), so a team gets that advantage without rewriting its stack.
   Treat the language as the upgrade path, not the entry fee.
2. **The PRD sells breadth the build does not have.** GPU UI, 3D, mobile, HTTP framework, DB
   query builder, and "every domain" (§Full Domain Coverage) set expectations a design partner
   will test and find absent, which damages trust in the claims that *are* true. The
   positioning doc should say: *"Axon is for software that acts with delegated authority."*
   Present the rest as roadmap, labelled as such.
3. **"Collective ASI / shared intelligence" (§Maturity Level 5) is a liability in a safety
   pitch** until B7 is solved. A safety buyer hears "other people's prompts change my agent's
   behavior." Park it publicly; keep it as research.

## P-D. Sequencing

| Quarter-sized step | Ships | Unlocks |
|---|---|---|
| 1 | PM1 (versions + cron/webhook + rollback), PM4 (secrets), PM2 minimal (spend + errors + Slack alert) | A design partner runs one real agent in production |
| 2 | PM3 (approval inbox on `host_await`), PM5 (MCP client + server), G1 (taint) | Consequential actions with a human in the loop; plugs into existing agent stacks |
| 3 | PM6 + G3 (eval sets, sealed holdout), PM7 (cost forecast), PM8 (incident report) | The Improve loop is trustworthy and its value is legible in dollars |
| 4 | PM9 (compliance export), PM10 (orgs/SSO), PM11 (templates), PM13, PM14 | Sellable to a compliance-driven buyer; pricing testable |

**North-star metric:** *consequential actions executed per week under an approved, bounded
grant, with zero unbounded incidents.* It rises only if agents are genuinely useful
(Builder), approvals scale (Approver), and runs stay healthy (Operator). It cannot be gamed by
demos, because a demo runs once.

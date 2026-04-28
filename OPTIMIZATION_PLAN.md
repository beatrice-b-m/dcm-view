# Streaming & Rendering Optimization Plan

**Scope:** Multi-frame DICOM image streaming, cine playback, and rendering pipeline  
**Baseline branch:** `main` (commit `20f54d7`)  
**Target:** Smooth cine playback at 30+ fps, no main-thread jank on large frames, responsive scrubbing

---

## Current State Summary

| Mechanism | Status | Bottleneck |
|---|---|---|
| Display frame cache (320 MB LRU) | Operational | Blobs cached but bitmaps decoded lazily at draw time |
| Raw pixel cache (256 MB ring-buffer) | Operational | None significant |
| Directional prefetch (±48 frames) | Operational | Fires reactively after frame change; not ahead of playback |
| W-L worker offload (>300k px) | Operational | `putImageData` still blocks main thread after worker returns |
| Request cancellation / generation guards | Operational | None |
| CSS transform for pan/zoom | Operational | None |

---

## Phase 1 — Eager Bitmap Pre-decode During Prefetch

**Effort:** Low · **Risk:** Low · **Impact:** Reduces first-draw latency for cached frames

### Problem

`runDisplayPrefetch` at [ImageViewport.svelte:682](frontend/src/lib/ImageViewport.svelte#L682) caches blobs but does not decode them into `ImageBitmap` objects. When the user reaches a prefetched frame, `drawDisplayEntry` ([ImageViewport.svelte:503](frontend/src/lib/ImageViewport.svelte#L503)) still has to call `createImageBitmap`, which is async and can take 10–50 ms for large frames. This adds latency even when the frame was "ready."

### Change

After `cacheDisplayFrame` returns an entry inside `runDisplayPrefetch`, immediately kick off `createImageBitmap` on a best-effort basis so the bitmap is ready by the time the frame is displayed:

```
// inside the runDisplayPrefetch batch map, after cacheDisplayFrame:
if (typeof createImageBitmap === "function" && entry && !entry.bitmap && !entry.decodePromise) {
    entry.decodePromise = createImageBitmap(entry.blob).then((bmp) => {
        entry.bitmap = bmp;
        return bmp;
    }).catch(() => null).finally(() => {
        entry.decodePromise = null;
    });
}
```

**Files touched:** [frontend/src/lib/ImageViewport.svelte](frontend/src/lib/ImageViewport.svelte) — `runDisplayPrefetch` only

### Verification

- Open a multi-frame series (>50 frames) and scroll through it in cine mode.
- With DevTools Performance tab recording, verify that `createImageBitmap` no longer appears on the critical path of frame-change events.
- Frame draws that hit cache should complete in <5 ms (down from 10–50 ms).

### Tests

- **Unit:** Assert that after `runDisplayPrefetch` resolves, every cached `DisplayFrameCacheEntry` has a non-null `bitmap` (or a non-null `decodePromise` still in flight).
- **E2E:** Playwright: navigate to a series with 30+ frames, wait for prefetch to settle (1 s idle), then scroll forward 20 frames rapidly; assert no `loading` spinner appears.

---

## Phase 2 — Proactive Cine Playback Frame Buffer

**Effort:** Medium · **Risk:** Medium · **Impact:** Eliminates stalls during cine playback at high fps

### Problem

Prefetch is seeded reactively — it starts after the user changes frames ([ImageViewport.svelte:815](frontend/src/lib/ImageViewport.svelte#L815), [ImageViewport.svelte:844](frontend/src/lib/ImageViewport.svelte#L844)). During active cine playback (auto-advancing frames on a timer), the frame counter advances faster than the seeding heuristic can keep up, especially when `PREFETCH_RESEED_DISTANCE = 6` suppresses reseeds for short movements. At 15+ fps the prefetch queue is often empty when the player needs the next frame, causing a visible stall.

### Change

**2a. Expose a cine playback state signal**

Add a boolean reactive variable `isCinePlaying` that is `true` while the cine timer is active. (The cine control logic presumably lives in a parent component or in `ViewerToolbar.svelte` — thread the value down to `ImageViewport` as a prop.)

**2b. Proactive buffer during active playback**

When `isCinePlaying` is true, replace the reseed-suppression logic in `startDisplayPrefetch` with an unconditional forward-only prefetch rooted at `currentFrame + 1`. The buffer depth should be at least `fps * latency_estimate` frames — a starting value of `16` frames covers ~0.5 s of latency at 30 fps:

```
const CINE_LOOKAHEAD_FRAMES = 16; // tune based on profiling

function startDisplayPrefetch(...) {
    if (isCinePlaying) {
        // Always prefetch forward, no reseed suppression.
        // No need to cover backward direction during playback.
        const forwardTarget = Math.min(frameIndex + CINE_LOOKAHEAD_FRAMES, totalFrames - 1);
        ...
    } else {
        // existing bidirectional reseed logic unchanged
        ...
    }
}
```

**2c. Pre-decode bitmaps in the lookahead buffer** (extends Phase 1)

In playback mode, also eagerly decode `ImageBitmap` for the lookahead window (Phase 1 already handles this generically, but confirm it is called for the forward batch).

**Files touched:**
- [frontend/src/lib/ImageViewport.svelte](frontend/src/lib/ImageViewport.svelte) — `startDisplayPrefetch`, new `isCinePlaying` prop
- Parent component that owns the cine playback timer (confirm which file via `ViewerToolbar.svelte` or a parent `+page.svelte`)

### Verification

- DevTools Performance recording during active playback: zero gaps between successive `drawImage` calls.
- FPS counter (overlay or manual requestAnimationFrame loop): stable at target fps across a 100-frame series.
- Memory: verify display cache does not grow unboundedly (existing LRU eviction already covers this).

### Tests

- **Unit:** Mock `fetchDisplayFrameBlob` with a 20 ms artificial delay. Simulate a playback timer advancing `currentFrame` every 33 ms. Assert that by the time each frame is requested, the blob (and bitmap) are already present in the cache.
- **E2E:** Playwright: start cine playback on a 60-frame series, record frame timestamps via a test hook, assert that the 95th-percentile inter-frame gap is <50 ms.

---

## Phase 3 — OffscreenCanvas to Unblock Main Thread

**Effort:** Medium · **Risk:** Medium · **Impact:** Eliminates jank during W-L rendering of large frames (mammography, DBT)

### Problem

`ctx.putImageData(imageData, 0, 0)` at [ImageViewport.svelte:482](frontend/src/lib/ImageViewport.svelte#L482) and [ImageViewport.svelte:494](frontend/src/lib/ImageViewport.svelte#L494) runs synchronously on the main thread. For a 5000×4000 mammography frame this is ~80 MB of pixel data and can block the main thread for 20–60 ms, causing dropped frames and unresponsive UI.

The worker at [wlRenderer.worker.ts](frontend/src/lib/workers/wlRenderer.worker.ts) already does the LUT computation off-thread, but it still posts the RGBA buffer back to the main thread for the `putImageData` call.

### Change

**3a. Transfer an OffscreenCanvas to the worker**

On worker initialization, create an `OffscreenCanvas` matching the viewport's canvas dimensions and transfer it to the worker via `transferControlToOffscreen()`. The worker paints directly via its own 2D context; the main thread never calls `putImageData`.

```typescript
// ImageViewport.svelte — worker init
const offscreen = canvasEl.transferControlToOffscreen();
wlWorker.postMessage({ type: "init_canvas", canvas: offscreen }, [offscreen]);
```

```typescript
// wlRenderer.worker.ts — handle init_canvas
let offscreenCtx: OffscreenCanvasRenderingContext2D | null = null;
self.onmessage = (e) => {
    if (e.data.type === "init_canvas") {
        offscreenCtx = e.data.canvas.getContext("2d", { alpha: false });
        return;
    }
    // existing render message handling — after computing rgba, paint to offscreenCtx
    ...
};
```

**3b. Resize handling**

When the canvas dimensions change (new frame dimensions), post a `resize_canvas` message to the worker before sending the render request. The worker resizes its OffscreenCanvas accordingly.

**3c. Fallback path**

`transferControlToOffscreen` is not available in all browsers. Detect support and keep the existing `putImageData` path as the fallback:

```typescript
const canUseOffscreen = typeof OffscreenCanvas !== "undefined" && 
    typeof canvasEl.transferControlToOffscreen === "function";
```

**3d. Canvas access after transfer**

Once `transferControlToOffscreen` is called, the main thread loses direct 2D context access to the canvas. For pan/zoom, the existing CSS `transform` approach is unaffected. The annotation SVG overlay is a sibling element, also unaffected. Confirm that no other main-thread code calls `canvasEl.getContext("2d")` after transfer.

**Files touched:**
- [frontend/src/lib/workers/wlRenderer.worker.ts](frontend/src/lib/workers/wlRenderer.worker.ts) — add `init_canvas`, `resize_canvas` handlers; paint via OffscreenCanvas instead of posting RGBA back
- [frontend/src/lib/ImageViewport.svelte](frontend/src/lib/ImageViewport.svelte) — worker init, `renderDiagnosticFrame`, resize effect

### Verification

- Chrome DevTools Performance timeline: `putImageData` entry disappears from the main thread flame graph for large frames.
- `Long Tasks` (>50 ms blocks on main thread) should be absent during W-L adjustment on a 5000×4000 frame.
- UI interaction test: drag the window/level handle continuously for 2 s on a large frame; the drag must track the cursor without visible lag.

### Tests

- **Unit (worker):** Send a `render` message to the worker with a synthetic 1024×1024 raw frame; assert the worker posts back a completion acknowledgement (not an RGBA buffer) and that no RGBA buffer transfer appears in the message.
- **Integration:** Vitest with jsdom: verify that when `canUseOffscreen` is false (simulated by deleting `HTMLCanvasElement.prototype.transferControlToOffscreen`), the fallback `putImageData` path is taken without errors.
- **E2E:** Playwright on a large DICOM: assert no `Long Tasks` entries during W-L drag (using `PerformanceObserver` injection).

---

## Phase 4 — requestIdleCallback Scheduling for Prefetch Initiation

**Effort:** Low · **Risk:** Low · **Impact:** Prevents prefetch from competing with user interaction on slow hardware

### Problem

`startDisplayPrefetch` ([ImageViewport.svelte:711](frontend/src/lib/ImageViewport.svelte#L711)) and `runRawRingPrefetch` ([ImageViewport.svelte:654](frontend/src/lib/ImageViewport.svelte#L654)) fire immediately when a frame is loaded. On devices with slow CPUs (e.g., tablets), the burst of 3 concurrent prefetch requests and their associated `createImageBitmap` calls can compete with ongoing user input processing.

### Change

Wrap the `void runDisplayPrefetch(...)` and `void runRawRingPrefetch(...)` fire-and-forget calls inside a `requestIdleCallback` with a short timeout so they are deferred to browser idle time but not indefinitely blocked:

```typescript
function scheduleIdleOrImmediate(fn: () => void, timeout = 200): void {
    if (typeof requestIdleCallback === "function") {
        requestIdleCallback(fn, { timeout });
    } else {
        setTimeout(fn, 0);
    }
}
```

**Exception:** During active cine playback (`isCinePlaying === true`), skip the idle deferral — the proactive lookahead buffer from Phase 2 must fill as fast as possible.

**Files touched:** [frontend/src/lib/ImageViewport.svelte](frontend/src/lib/ImageViewport.svelte) — `startDisplayPrefetch`, `loadRawFrameAndRender`

### Verification

- On a throttled CPU (DevTools 4× slowdown): input events (mouse wheel, drag) should not show queueing delay attributed to prefetch callbacks in the Performance tab.
- Prefetch should still complete within the `timeout` window on normal hardware.

### Tests

- **Unit:** Spy on `requestIdleCallback`. Load a frame and assert that the prefetch function is not called synchronously, but is passed to `requestIdleCallback`.
- **Unit:** Assert that when `isCinePlaying` is true, `requestIdleCallback` is NOT called — prefetch fires immediately.

---

## Phase 5 — Network-Aware Adaptive Prefetch Concurrency

**Effort:** Low · **Risk:** Low · **Impact:** Prevents prefetch from saturating slow connections; speeds up on fast ones

### Problem

`PREFETCH_CONCURRENCY = 3` is a fixed constant at [ImageViewport.svelte:114](frontend/src/lib/ImageViewport.svelte#L114). On a fast local connection, 3 parallel requests under-utilizes bandwidth and slows fill of the lookahead buffer. On a slow or metered connection, 3 simultaneous large-frame fetches can congest the link and delay the current-frame request.

### Change

Use the Network Information API to derive a concurrency level at startup and on `change` events:

```typescript
function derivePrefetchConcurrency(): number {
    const conn = (navigator as any).connection ?? (navigator as any).mozConnection;
    if (!conn) return 3; // default if API unavailable
    if (conn.saveData) return 1;
    const type: string = conn.effectiveType ?? "";
    if (type === "slow-2g" || type === "2g") return 1;
    if (type === "3g") return 2;
    return 4; // 4g / wifi
}
```

Store as a reactive variable and update on `conn.addEventListener("change", ...)`. Pass the derived value into `runDisplayPrefetch` and `runRawRingPrefetch` in place of the constant.

Note: The Network Information API has limited browser support and is behind a flag in some contexts. The fallback (`return 3`) preserves existing behavior when unavailable.

**Files touched:** [frontend/src/lib/ImageViewport.svelte](frontend/src/lib/ImageViewport.svelte) — add `derivePrefetchConcurrency`, thread into prefetch functions

### Verification

- DevTools Network throttling to "Slow 3G": confirm only 1–2 parallel prefetch requests in the waterfall.
- DevTools Network throttling to "No throttling" (local): confirm 4 parallel requests.
- Slow-network test: current-frame load should not be queued behind prefetch requests.

### Tests

- **Unit:** Mock `navigator.connection` with various `effectiveType` values; assert `derivePrefetchConcurrency` returns the expected level for each.
- **Integration:** With `effectiveType: "2g"`, invoke `runDisplayPrefetch` for 10 frames; assert that no more than 1 fetch is in flight simultaneously (via a spy on `fetchDisplayFrameBlob`).

---

## Implementation Order & Dependencies

```
Phase 1 (bitmap pre-decode)
    └── Phase 2 (cine buffer) — builds on Phase 1's eager decode
            └── Phase 4 (idle scheduling) — must respect Phase 2's playback-mode exception

Phase 3 (OffscreenCanvas) — independent of all others
Phase 5 (adaptive concurrency) — independent of all others
```

Phases 3 and 5 can be developed in parallel with the Phase 1–2–4 track. Each phase is independently mergeable and testable.

---

## Cross-Cutting Concerns

### Memory

The lookahead buffer in Phase 2 and the eager bitmap decode in Phase 1 both increase the number of live `ImageBitmap` objects. The existing 320 MB `DISPLAY_CACHE_BYTE_BUDGET` LRU accounts for blob bytes but **not** for the decoded bitmap size (typically ~4× the JPEG size for a 16-bit grayscale frame). After Phase 1 and Phase 2 land, measure actual GPU memory usage and revisit whether the eviction policy should account for decoded bitmap size alongside blob bytes.

### Regression Surface

The generation-counter guards (`requestGeneration`, `wlRenderGeneration`) at [ImageViewport.svelte:92-95](frontend/src/lib/ImageViewport.svelte#L92-L95) are the primary defense against stale async results. Any new async path introduced by these phases must check the relevant generation counter before writing to reactive state or painting the canvas.

### Browser Compatibility Checklist

| Feature | Chrome | Firefox | Safari |
|---|---|---|---|
| `createImageBitmap` | ✓ | ✓ | ✓ (TP+) |
| `OffscreenCanvas` (Phase 3) | ✓ | ✓ | ✓ (16.4+) |
| `requestIdleCallback` (Phase 4) | ✓ | ✓ | ✗ — needs `setTimeout` fallback |
| Network Information API (Phase 5) | ✓ | partial | ✗ — needs fallback |

---

## Verification Harness

Before beginning any phase, establish a baseline with the following repeatable measurements:

1. **Cine stall rate** — Playwright script that plays a 60-frame series at 15 fps and records the number of frames where `loading === true` at draw time. Baseline should be 0 after Phase 2.
2. **Main-thread long-task count** — `PerformanceObserver` for `longtask` entries during 5 s of W-L dragging on a large frame. Baseline should be 0 after Phase 3.
3. **Time-to-first-frame** — Measure ms from file-open event to first canvas paint. Should not regress across any phase.
4. **Display cache fill time** — Time for `displayFrameCache.size === totalFrames` to become true after file open on a local 100-frame series. Should improve after Phase 1 and Phase 2.

Record baseline numbers before starting Phase 1 and re-run after each phase merge.

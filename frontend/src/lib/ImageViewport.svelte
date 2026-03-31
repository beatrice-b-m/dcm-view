<script lang="ts">
	import { frameUrl, type WindowMode, type FileSummary } from "../api";
	import type { ActiveTool } from "./viewerTools";

	type TransformState = { scale: number; tx: number; ty: number };
	type DragState =
		| { mode: "pan"; startX: number; startY: number; baseTx: number; baseTy: number }
		| { mode: "wl"; startX: number; startY: number; baseCenter: number; baseWidth: number }
		| { mode: "zoom_drag"; startX: number; startY: number; baseScale: number; pivotX: number; pivotY: number }
		| { mode: "scroll_drag"; startY: number; baseFrame: number }
		| null;

	let {
		files,
		activeFileIndex,
		currentFrame = $bindable(),
		windowCenter = $bindable(),
		windowWidth = $bindable(),
		activeTool,
		windowMode,
		resetCount,
		onreset,
	}: {
		files: FileSummary[];
		activeFileIndex: number;
		currentFrame: number;
		windowCenter: number | null;
		windowWidth: number | null;
		activeTool: ActiveTool;
		windowMode: WindowMode;
		resetCount: number;
		onreset?: () => void;
	} = $props();

	let transformsByFile = $state<Record<number, TransformState>>({});
	let dragState = $state<DragState>(null);
	let loading = $state(false);
	let loadError = $state<string | null>(null);
	let liveWindowCenter = $state<number | null>(null);
	let liveWindowWidth = $state<number | null>(null);
	let displaySrc = $state('');
	let viewportEl: HTMLElement | undefined = $state();
	let imgEl: HTMLImageElement | undefined = $state();
	let wheelAccum = $state(0);

	// Non-reactive: mutations must not trigger Svelte re-renders.
	// AbortController for the currently in-flight frame fetch.
	let pendingController: AbortController | null = null;
	// Client-side frame cache: frame index → blob URL.
	// Valid only for the current _frameCacheKey scope.
	let frameCache = new Map<number, string>();
	let _frameCacheKey = '';
	// requestAnimationFrame handle for W/L drag throttle.
	let wlRafId: number | null = null;

	const ZOOM_STEPS = [0.25, 0.5, 0.75, 1, 1.25, 1.5, 2, 3, 4, 6, 8];
	const activeFile = $derived(files[activeFileIndex] ?? { frame_count: 0, default_window: null });
	const activeTransform = $derived(transformsByFile[activeFileIndex] ?? { scale: 1, tx: 0, ty: 0 });
	const transformCss = $derived(
		`translate(${activeTransform.tx}px, ${activeTransform.ty}px) scale(${activeTransform.scale})`,
	);
	const displayWindowCenter = $derived(
		liveWindowCenter ?? windowCenter ?? activeFile?.default_window?.center ?? 0,
	);
	const displayWindowWidth = $derived(
		liveWindowWidth ?? windowWidth ?? activeFile?.default_window?.width ?? 1,
	);
	const zoomPercent = $derived(Math.round(activeTransform.scale * 100));
	const isDragging = $derived(dragState !== null);
	const isJpegPassthrough = $derived(
		activeFile?.transfer_syntax_uid === '1.2.840.10008.1.2.4.50' ||
		activeFile?.transfer_syntax_uid === '1.2.840.10008.1.2.4.51'
	);

	// --- Cache management ---

	function clearFrameCache() {
		for (const url of frameCache.values()) URL.revokeObjectURL(url);
		frameCache = new Map();
	}

	/**
	 * Revoke a blob URL if it is not currently held in the frame cache.
	 * Call before replacing displaySrc with a new blob URL.
	 */
	function revokeIfOrphaned(blobUrl: string) {
		if (!blobUrl.startsWith('blob:')) return;
		for (const cached of frameCache.values()) {
			if (cached === blobUrl) return; // still referenced by cache
		}
		URL.revokeObjectURL(blobUrl);
	}

	// --- Image loading ---

	/**
	 * Load a frame from `url` and update displaySrc.
	 *
	 * frameIndex: when non-null, the result is stored in frameCache (keyed by frame index) and
	 *   the cache is checked before issuing a network request. Pass null for intermediate W/L
	 *   drag values that should not pollute the cache.
	 *
	 * isDragFrame: when true, liveWindowCenter/Width are NOT cleared on successful load —
	 *   the drag is still in progress and the user still needs to see live overlay values.
	 *
	 * Any currently in-flight request is aborted before this fetch begins. This prevents
	 * race conditions where older responses overwrite newer ones.
	 */
	async function loadFrame(
		url: string,
		frameIndex: number | null,
		isDragFrame = false,
	): Promise<void> {
		// Fast path: cache hit — no network round-trip, no loading indicator.
		if (frameIndex !== null) {
			const cached = frameCache.get(frameIndex);
			if (cached) {
				revokeIfOrphaned(displaySrc);
				displaySrc = cached;
				loading = false;
				loadError = null;
				if (!isDragFrame) {
					liveWindowCenter = null;
					liveWindowWidth = null;
				}
				return;
			}
		}

		// Abort any stale in-flight request before starting a new one.
		pendingController?.abort();
		const controller = new AbortController();
		pendingController = controller;

		loading = true;
		loadError = null;

		try {
			const response = await fetch(url, { signal: controller.signal });
			if (!response.ok) throw new Error(`HTTP ${response.status}`);
			const blob = await response.blob();

			// Another fetch superseded this one — do not update display.
			if (controller.signal.aborted) return;

			const blobUrl = URL.createObjectURL(blob);

			revokeIfOrphaned(displaySrc);

			if (frameIndex !== null) {
				frameCache.set(frameIndex, blobUrl);
			}

			displaySrc = blobUrl;
			// loading is cleared by <img onload> once the browser has painted the image.
			loadError = null;
			if (!isDragFrame) {
				liveWindowCenter = null;
				liveWindowWidth = null;
			}
		} catch (e) {
			if ((e as Error).name === 'AbortError') return;
			loading = false;
			loadError = 'Failed to load frame';
		} finally {
			if (pendingController === controller) pendingController = null;
		}
	}

	/**
	 * Schedule a W/L preview fetch for the next animation frame.
	 * At most one fetch is dispatched per animation frame — subsequent calls within the
	 * same frame are no-ops. The fetch uses the current live W/L values and is not cached
	 * (intermediate drag values are not useful to retain).
	 */
	function scheduleWLFetch() {
		if (wlRafId !== null) return;
		wlRafId = requestAnimationFrame(() => {
			wlRafId = null;
			if (!activeFile || !activeFile.has_pixels) return;
			const url = frameUrl(
				activeFile.index, currentFrame, liveWindowCenter, liveWindowWidth, windowMode,
			);
			void loadFrame(url, null, true);
		});
	}

	// --- Effects ---

	// Initialise per-file transform state on first access.
	$effect(() => {
		if (activeFile && !transformsByFile[activeFile.index]) {
			transformsByFile = {
				...transformsByFile,
				[activeFile.index]: { scale: 1, tx: 0, ty: 0 },
			};
		}
	});

	/**
	 * Main image load effect. Runs when:
	 *   - the active file changes (activeFileIndex)
	 *   - the current frame changes
	 *   - committed W/L values change (windowCenter, windowWidth, windowMode)
	 *
	 * The cache is invalidated whenever the scope (file + committed W/L) changes.
	 * Intermediate W/L drag values do NOT flow through here — they go via scheduleWLFetch().
	 */
	$effect(() => {
		if (!activeFile || !activeFile.has_pixels) {
			displaySrc = '';
			loading = false;
			return;
		}

		const key = `${activeFileIndex}|${windowCenter ?? ''}|${windowWidth ?? ''}|${windowMode}`;
		if (key !== _frameCacheKey) {
			// File or committed W/L changed: evict all cached blobs and clear displaySrc
			// so the stale image from the previous scope is not briefly shown.
			clearFrameCache();
			_frameCacheKey = key;
			revokeIfOrphaned(displaySrc);
			displaySrc = '';
			loading = true;
		}

		const url = frameUrl(activeFile.index, currentFrame, windowCenter, windowWidth, windowMode);
		void loadFrame(url, currentFrame);
	});

	// Cleanup on component destroy: abort in-flight request, revoke all blob URLs.
	$effect(() => {
		return () => {
			if (wlRafId !== null) cancelAnimationFrame(wlRafId);
			pendingController?.abort();
			revokeIfOrphaned(displaySrc);
			clearFrameCache();
		};
	});

	// Reset transform when toolbar Reset is clicked (resetCount incremented by parent).
	$effect(() => {
		if (resetCount > 0) {
			if (activeFile) {
				updateTransform(activeFile.index, { scale: 1, tx: 0, ty: 0 });
			}
			liveWindowCenter = null;
			liveWindowWidth = null;
		}
	});

	// Reset wheel accumulator when file changes.
	$effect(() => {
		activeFileIndex; // dependency
		wheelAccum = 0;
	});

	function updateTransform(index: number, transform: TransformState) {
		transformsByFile = {
			...transformsByFile,
			[index]: transform,
		};
	}

	/**
	 * Zoom to `newScale`, keeping the point under (`clientX`, `clientY`) fixed.
	 * Coordinates are in client (page) space, not element-relative.
	 */
	function zoomAt(newScale: number, clientX: number, clientY: number) {
		if (!activeFile || !imgEl) return;
		const { scale, tx, ty } = activeTransform;
		const clamped = Math.min(8, Math.max(0.2, newScale));
		const imgRect = imgEl.getBoundingClientRect();
		const lx = (clientX - imgRect.left) / scale;
		const ly = (clientY - imgRect.top) / scale;
		const natX = imgRect.left - tx;
		const natY = imgRect.top - ty;
		updateTransform(activeFile.index, {
			scale: clamped,
			tx: clientX - natX - lx * clamped,
			ty: clientY - natY - ly * clamped,
		});
	}

	function onWheel(event: WheelEvent) {
		if (!activeFile || !activeFile.has_pixels) return;
		event.preventDefault();

		const isModifiedZoom = event.ctrlKey || event.metaKey;

		if (isModifiedZoom) {
			// Ctrl/Cmd + wheel: always zoom (includes trackpad pinch which fires as ctrlKey+pixel)
			const delta = event.deltaMode === 0
				? -event.deltaY * 0.01
				: event.deltaY < 0 ? 0.05 : -0.05;
			zoomAt(activeTransform.scale + delta, event.clientX, event.clientY);
			return;
		}

		if (activeFile.frame_count > 1) {
			// Multi-frame: scrub through stack
			if (event.deltaMode !== 0) {
				// Discrete mouse wheel: 1 frame per click
				if (event.deltaY > 0) currentFrame = Math.min(activeFile.frame_count - 1, currentFrame + 1);
				else if (event.deltaY < 0) currentFrame = Math.max(0, currentFrame - 1);
			} else {
				// Pixel mode trackpad: accumulate to avoid over-scrubbing
				wheelAccum += event.deltaY;
				const threshold = 30;
				while (wheelAccum >= threshold) {
					currentFrame = Math.min(activeFile.frame_count - 1, currentFrame + 1);
					wheelAccum -= threshold;
				}
				while (wheelAccum <= -threshold) {
					currentFrame = Math.max(0, currentFrame - 1);
					wheelAccum += threshold;
				}
			}
			return;
		}

		// Single-frame: original zoom/pan behavior
		if (event.deltaMode !== 0) {
			const delta = event.deltaY < 0 ? 0.05 : -0.05;
			zoomAt(activeTransform.scale + delta, event.clientX, event.clientY);
		} else {
			// Trackpad two-finger scroll: pan
			updateTransform(activeFile.index, {
				...activeTransform,
				tx: activeTransform.tx - event.deltaX,
				ty: activeTransform.ty - event.deltaY,
			});
		}
	}

	function onPointerDown(event: PointerEvent) {
		if (!activeFile || !activeFile.has_pixels) return;
		(event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);

		if (event.button === 1) {
			// Middle: always pan
			event.preventDefault();
			dragState = {
				mode: "pan",
				startX: event.clientX,
				startY: event.clientY,
				baseTx: activeTransform.tx,
				baseTy: activeTransform.ty,
			};
			return;
		}

		if (event.button === 2) {
			// Right: always zoom
			event.preventDefault();
			dragState = {
				mode: "zoom_drag",
				startX: event.clientX,
				startY: event.clientY,
				baseScale: activeTransform.scale,
				pivotX: event.clientX,
				pivotY: event.clientY,
			};
			return;
		}

		if (event.button === 0) {
			// Left: route by active tool
			switch (activeTool) {
				case "window_level": {
					const baseCenter = windowCenter ?? activeFile.default_window?.center ?? 0;
					const baseWidth = windowWidth ?? activeFile.default_window?.width ?? 1;
					dragState = {
						mode: "wl",
						startX: event.clientX,
						startY: event.clientY,
						baseCenter,
						baseWidth,
					};
					liveWindowCenter = baseCenter;
					liveWindowWidth = baseWidth;
					break;
				}
				case "pan":
					dragState = {
						mode: "pan",
						startX: event.clientX,
						startY: event.clientY,
						baseTx: activeTransform.tx,
						baseTy: activeTransform.ty,
					};
					break;
				case "zoom":
					dragState = {
						mode: "zoom_drag",
						startX: event.clientX,
						startY: event.clientY,
						baseScale: activeTransform.scale,
						pivotX: event.clientX,
						pivotY: event.clientY,
					};
					break;
				case "scroll":
					if (activeFile.frame_count > 1) {
						dragState = {
							mode: "scroll_drag",
							startY: event.clientY,
							baseFrame: currentFrame,
						};
					}
					break;
			}
		}
	}

	function onPointerMove(event: PointerEvent) {
		if (!activeFile || !dragState) return;

		if (dragState.mode === "pan") {
			const dx = event.clientX - dragState.startX;
			const dy = event.clientY - dragState.startY;
			updateTransform(activeFile.index, {
				...activeTransform,
				tx: dragState.baseTx + dx,
				ty: dragState.baseTy + dy,
			});
			return;
		}

		if (dragState.mode === "wl") {
			const dx = event.clientX - dragState.startX;
			const dy = event.clientY - dragState.startY;
			const nextWidth = Math.max(1, dragState.baseWidth + dx * 4);
			const nextCenter = dragState.baseCenter - dy * 2;
			liveWindowCenter = nextCenter;
			liveWindowWidth = nextWidth;
			// Throttle server requests to at most one per animation frame (~60/s).
			// Each request aborts its predecessor, preventing out-of-order renders.
			scheduleWLFetch();
			return;
		}

		if (dragState.mode === "zoom_drag") {
			const dy = event.clientY - dragState.startY;
			const newScale = Math.min(8, Math.max(0.2, dragState.baseScale * Math.exp(-dy * 0.005)));
			zoomAt(newScale, dragState.pivotX, dragState.pivotY);
			return;
		}

		if (dragState.mode === "scroll_drag" && activeFile.frame_count > 1) {
			const dy = event.clientY - dragState.startY;
			const frameDelta = Math.round(dy / 10);
			currentFrame = Math.max(0, Math.min(activeFile.frame_count - 1, dragState.baseFrame + frameDelta));
		}
	}

	function onPointerUp(event: PointerEvent) {
		(event.currentTarget as HTMLElement).releasePointerCapture(event.pointerId);

		if (dragState?.mode === 'wl') {
			// Cancel any pending rAF — the main $effect fires the final fetch.
			if (wlRafId !== null) {
				cancelAnimationFrame(wlRafId);
				wlRafId = null;
			}
			// Committing live values triggers the main $effect, which invalidates the
			// frame cache (new W/L scope) and fetches+caches the frame at this W/L.
			windowCenter = liveWindowCenter;
			windowWidth = liveWindowWidth;
		}

		dragState = null;
	}

	function onPointerCancel() {
		if (wlRafId !== null) {
			cancelAnimationFrame(wlRafId);
			wlRafId = null;
		}
		dragState = null;
	}

	function onContextMenu(event: MouseEvent) {
		event.preventDefault();
	}

	function resetViewport() {
		if (!activeFile) return;
		updateTransform(activeFile.index, { scale: 1, tx: 0, ty: 0 });
		windowCenter = activeFile.default_window?.center ?? null;
		windowWidth = activeFile.default_window?.width ?? null;
		liveWindowCenter = null;
		liveWindowWidth = null;
	}

	function zoomToLevel(level: number) {
		if (!activeFile || !activeFile.has_pixels) return;
		const rect = viewportEl?.getBoundingClientRect();
		const cx = rect ? rect.left + rect.width / 2 : 0;
		const cy = rect ? rect.top + rect.height / 2 : 0;
		zoomAt(level, cx, cy);
	}

	function stepZoom(direction: 1 | -1) {
		if (!activeFile) return;
		const current = activeTransform.scale;
		if (direction > 0) {
			const next = ZOOM_STEPS.find(s => s > current + 0.001);
			if (next !== undefined) zoomToLevel(next);
		} else {
			const prev = [...ZOOM_STEPS].reverse().find(s => s < current - 0.001);
			if (prev !== undefined) zoomToLevel(prev);
		}
	}
</script>

<section
	bind:this={viewportEl}
	class="viewport"
	class:dragging={isDragging}
	data-tool={activeTool}
	role="application"
	onwheel={onWheel}
	onpointerdown={onPointerDown}
	onpointermove={onPointerMove}
	onpointerup={onPointerUp}
	onpointercancel={onPointerCancel}
	oncontextmenu={onContextMenu}
	ondblclick={() => { if (onreset) { onreset(); } else { resetViewport(); } }}
>
	{#if !activeFile}
		<div class="placeholder">No file selected</div>
	{:else if !activeFile.has_pixels}
		<div class="placeholder">No pixel data</div>
	{:else if loadError}
		<div class="placeholder">{loadError}</div>
	{:else}
		{#if loading}
			<div class="loading">Loading frame…</div>
		{/if}
		{#if displaySrc}
			<img
				bind:this={imgEl}
				src={displaySrc}
				alt={`frame ${currentFrame + 1}`}
				draggable="false"
				style={`transform:${transformCss}`}
				onload={() => { loading = false; }}
				onerror={() => { loading = false; loadError = 'Failed to load frame'; }}
			/>
		{/if}
		<div class="overlay">
			<span>frame {currentFrame + 1} / {activeFile.frame_count}</span>
			<span>{isJpegPassthrough ? 'W/L N/A' : `W: ${Math.round(displayWindowWidth)} · C: ${Math.round(displayWindowCenter)}`}</span>
		</div>
		<div class="zoom-controls">
			<button type="button" onclick={() => stepZoom(-1)} disabled={activeTransform.scale <= ZOOM_STEPS[0]}>−</button>
			<button type="button" class="zoom-level" onclick={() => { if (activeFile) updateTransform(activeFile.index, { scale: 1, tx: 0, ty: 0 }); }}>{zoomPercent}%</button>
			<button type="button" onclick={() => stepZoom(1)} disabled={activeTransform.scale >= ZOOM_STEPS[ZOOM_STEPS.length - 1]}>+</button>
		</div>
	{/if}
</section>

<style>
	.viewport {
		position: relative;
		display: grid;
		place-items: center;
		background: #111;
		min-height: 0;
		overflow: hidden;
		user-select: none;
	}
	.viewport[data-tool="window_level"] { cursor: crosshair; }
	.viewport[data-tool="pan"] { cursor: grab; }
	.viewport[data-tool="pan"]:active { cursor: grabbing; }
	.viewport[data-tool="zoom"] { cursor: zoom-in; }
	.viewport[data-tool="scroll"] { cursor: ns-resize; }
	.viewport.dragging { cursor: grabbing; }
	img {
		max-width: 100%;
		max-height: 100%;
		object-fit: contain;
		transform-origin: 0 0;
		transition: transform 0.03s linear;
	}
	.placeholder,
	.loading {
		color: #9a9a9a;
	}
	.loading {
		position: absolute;
		top: 0.75rem;
		left: 0.75rem;
		font-size: 0.85rem;
		z-index: 2;
	}
	.overlay {
		position: absolute;
		left: 0.75rem;
		bottom: 0.75rem;
		display: flex;
		gap: 0.75rem;
		font-size: 0.82rem;
		padding: 0.3rem 0.5rem;
		background: rgba(18, 18, 18, 0.75);
		border: 1px solid #333;
		border-radius: 4px;
	}
	.zoom-controls {
		position: absolute;
		right: 0.75rem;
		bottom: 0.75rem;
		display: flex;
		align-items: center;
		gap: 0;
		background: rgba(18, 18, 18, 0.85);
		border: 1px solid #333;
		border-radius: 6px;
		overflow: hidden;
	}
	.zoom-controls button {
		background: none;
		border: none;
		color: #e0e0e0;
		padding: 0.3rem 0.55rem;
		font-size: 0.95rem;
		cursor: pointer;
		line-height: 1;
	}
	.zoom-controls button:hover:not(:disabled) {
		background: rgba(74, 158, 255, 0.15);
	}
	.zoom-controls button:disabled {
		color: #555;
		cursor: default;
	}
	.zoom-controls .zoom-level {
		padding: 0.3rem 0.4rem;
		font-size: 0.78rem;
		font-family: ui-monospace, monospace;
		color: #ccc;
		min-width: 3.2rem;
		text-align: center;
		cursor: pointer;
		border-left: 1px solid #333;
		border-right: 1px solid #333;
	}
	.zoom-controls .zoom-level:hover {
		color: #4a9eff;
	}
</style>

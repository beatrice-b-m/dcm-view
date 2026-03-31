<script lang="ts">
	import { fetchRawFrame, type RawFrame, type WindowMode, type FileSummary } from "../api";
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
	let viewportEl: HTMLElement | undefined = $state();
	let canvasEl: HTMLCanvasElement | undefined = $state();
	let wheelAccum = $state(0);
	let currentRawFrame = $state<RawFrame | null>(null);
	let fileScopeKey = $state('');

	// Non-reactive: mutations must not trigger Svelte re-renders.
	let pendingRawCtrl: AbortController | null = null;
	let prefetchController: AbortController | null = null;
	let rawFrameCache = new Map<number, RawFrame>();
	// lastHandledResetCount is non-reactive: only the effect reads/writes it,
	// so mutations here must not trigger Svelte dependency tracking.
	let lastHandledResetCount = 0;

	const ZOOM_STEPS = [0.25, 0.5, 0.75, 1, 1.25, 1.5, 2, 3, 4, 6, 8];
	const activeFile = $derived(files[activeFileIndex] ?? { frame_count: 0, default_window: null });
	const activeTransform = $derived(transformsByFile[activeFileIndex] ?? { scale: 1, tx: 0, ty: 0 });
	const transformCss = $derived(
		`translate(${activeTransform.tx}px, ${activeTransform.ty}px) scale(${activeTransform.scale})`,
	);
	const zoomPercent = $derived(Math.round(activeTransform.scale * 100));
	const isDragging = $derived(dragState !== null);

	const displayWindow = $derived(
		currentRawFrame
			? resolveDisplayWindow(
					currentRawFrame,
					liveWindowCenter,
					liveWindowWidth,
					windowCenter,
					windowWidth,
					windowMode,
				)
			: { wc: 0, ww: 1 },
	);

	// --- Windowing functions ---

	function buildLut(
		bitsAllocated: number,
		pixelRepresentation: number,
		rescaleSlope: number,
		rescaleIntercept: number,
		wc: number,
		ww: number,
		invert: boolean,
	): Uint8Array {
		const low = wc - ww / 2;
		const high = wc + ww / 2;
		const range = Math.max(high - low, 1e-10);

		let minRaw: number, size: number;
		if (bitsAllocated === 8) { minRaw = 0; size = 256; }
		else if (pixelRepresentation === 1) { minRaw = -32768; size = 65536; }
		else { minRaw = 0; size = 65536; }

		const lut = new Uint8Array(size);
		for (let i = 0; i < size; i++) {
			const raw = i + minRaw;
			const modal = raw * rescaleSlope + rescaleIntercept;
			let val = (modal - low) / range;
			val = val < 0 ? 0 : val > 1 ? 1 : val;
			if (invert) val = 1 - val;
			lut[i] = Math.round(val * 255);
		}
		return lut;
	}

	function renderRawFrame(
		canvas: HTMLCanvasElement,
		frame: RawFrame,
		wc: number,
		ww: number,
	): void {
		const { rows, columns, bitsAllocated, pixelRepresentation, rescaleSlope, rescaleIntercept, photometricInterpretation } = frame.metadata;
		canvas.width = columns;
		canvas.height = rows;
		const ctx = canvas.getContext('2d', { alpha: false })!;
		const invert = photometricInterpretation === 'MONOCHROME1';
		const lut = buildLut(bitsAllocated, pixelRepresentation, rescaleSlope, rescaleIntercept, wc, Math.max(ww, 1), invert);
		const numPixels = rows * columns;
		const imageData = ctx.createImageData(columns, rows);
		const rgba = imageData.data;

		if (bitsAllocated === 8) {
			const view = new Uint8Array(frame.buffer);
			for (let i = 0; i < numPixels; i++) {
				const g = lut[view[i]];
				const o = i * 4;
				rgba[o] = g; rgba[o+1] = g; rgba[o+2] = g; rgba[o+3] = 255;
			}
		} else if (pixelRepresentation === 1) {
			const view = new Int16Array(frame.buffer);
			for (let i = 0; i < numPixels; i++) {
				const g = lut[view[i] + 32768];
				const o = i * 4;
				rgba[o] = g; rgba[o+1] = g; rgba[o+2] = g; rgba[o+3] = 255;
			}
		} else {
			const view = new Uint16Array(frame.buffer);
			for (let i = 0; i < numPixels; i++) {
				const g = lut[view[i]];
				const o = i * 4;
				rgba[o] = g; rgba[o+1] = g; rgba[o+2] = g; rgba[o+3] = 255;
			}
		}
		ctx.putImageData(imageData, 0, 0);
	}

	function resolveDisplayWindow(
		frame: RawFrame,
		liveWc: number | null,
		liveWw: number | null,
		wc: number | null,
		ww: number | null,
		mode: WindowMode,
	): { wc: number; ww: number } {
		if (mode === 'full_dynamic') {
			return computeFullDynamicWindow(frame);
		}
		if (liveWc !== null && liveWw !== null) {
			return { wc: liveWc, ww: liveWw };
		}
		if (wc !== null && ww !== null) {
			return { wc, ww };
		}
		const { defaultWc, defaultWw } = frame.metadata;
		if (defaultWc !== null && defaultWw !== null) {
			return { wc: defaultWc!, ww: defaultWw! };
		}
		return computePercentileWindow(frame);
	}

	function computeFullDynamicWindow(frame: RawFrame): { wc: number; ww: number } {
		const { bitsAllocated, pixelRepresentation, rescaleSlope, rescaleIntercept, rows, columns } = frame.metadata;
		const numPixels = rows * columns;
		let min = Infinity, max = -Infinity;
		if (bitsAllocated === 8) {
			const view = new Uint8Array(frame.buffer);
			for (let i = 0; i < numPixels; i++) {
				const v = view[i] * rescaleSlope + rescaleIntercept;
				if (v < min) min = v; if (v > max) max = v;
			}
		} else if (pixelRepresentation === 1) {
			const view = new Int16Array(frame.buffer);
			for (let i = 0; i < numPixels; i++) {
				const v = view[i] * rescaleSlope + rescaleIntercept;
				if (v < min) min = v; if (v > max) max = v;
			}
		} else {
			const view = new Uint16Array(frame.buffer);
			for (let i = 0; i < numPixels; i++) {
				const v = view[i] * rescaleSlope + rescaleIntercept;
				if (v < min) min = v; if (v > max) max = v;
			}
		}
		if (!isFinite(min) || !isFinite(max)) return { wc: 128, ww: 256 };
		const width = Math.max(max - min, 1);
		return { wc: min + width / 2, ww: width };
	}

	function computePercentileWindow(frame: RawFrame): { wc: number; ww: number } {
		const { bitsAllocated, pixelRepresentation, rescaleSlope, rescaleIntercept, rows, columns } = frame.metadata;
		const numPixels = rows * columns;
		const values = new Float64Array(numPixels);
		if (bitsAllocated === 8) {
			const view = new Uint8Array(frame.buffer);
			for (let i = 0; i < numPixels; i++) values[i] = view[i] * rescaleSlope + rescaleIntercept;
		} else if (pixelRepresentation === 1) {
			const view = new Int16Array(frame.buffer);
			for (let i = 0; i < numPixels; i++) values[i] = view[i] * rescaleSlope + rescaleIntercept;
		} else {
			const view = new Uint16Array(frame.buffer);
			for (let i = 0; i < numPixels; i++) values[i] = view[i] * rescaleSlope + rescaleIntercept;
		}
		values.sort();
		const p1 = values[Math.floor(numPixels * 0.01)];
		const p99 = values[Math.min(Math.ceil(numPixels * 0.99), numPixels - 1)];
		const width = Math.max(p99 - p1, 1);
		return { wc: p1 + width / 2, ww: width };
	}

	// --- Prefetch ---

	function buildPriorityOrder(current: number, total: number): number[] {
		const result: number[] = [];
		for (let delta = 1; delta < total; delta++) {
			const fwd = current + delta;
			const bwd = current - delta;
			if (fwd < total) result.push(fwd);
			if (bwd >= 0) result.push(bwd);
		}
		return result;
	}

	function ensurePrefetchRunning(fileIndex: number, totalFrames: number, startFrame: number): void {
		if (prefetchController || totalFrames <= 1) return;
		prefetchController = new AbortController();
		const ctrl = prefetchController;
		runPrefetch(fileIndex, totalFrames, startFrame, ctrl.signal)
			.catch(() => {})
			.finally(() => { if (prefetchController === ctrl) prefetchController = null; });
	}

	async function runPrefetch(
		fileIndex: number,
		totalFrames: number,
		startFrame: number,
		signal: AbortSignal,
	): Promise<void> {
		const frames = buildPriorityOrder(startFrame, totalFrames);
		const CONCURRENCY = 3;
		for (let i = 0; i < frames.length && !signal.aborted; i += CONCURRENCY) {
			const batch = frames.slice(i, i + CONCURRENCY).filter(f => !rawFrameCache.has(f));
			if (batch.length === 0) continue;
			await Promise.allSettled(
				batch.map(async f => {
					if (signal.aborted || rawFrameCache.has(f)) return;
					try {
						const raw = await fetchRawFrame(fileIndex, f, signal);
						rawFrameCache.set(f, raw);
					} catch { /* network errors are non-fatal during prefetch */ }
				}),
			);
		}
	}

	// --- Frame loading ---

	async function loadRawFrameAndRender(fileIndex: number, frameIndex: number): Promise<void> {
		const cached = rawFrameCache.get(frameIndex);
		if (cached) {
			currentRawFrame = cached;
			loading = false;
			loadError = null;
			ensurePrefetchRunning(fileIndex, activeFile?.frame_count ?? 1, frameIndex);
			return;
		}
		pendingRawCtrl?.abort();
		const ctrl = new AbortController();
		pendingRawCtrl = ctrl;
		loading = true;
		loadError = null;
		try {
			const raw = await fetchRawFrame(fileIndex, frameIndex, ctrl.signal);
			if (ctrl.signal.aborted) return;
			rawFrameCache.set(frameIndex, raw);
			currentRawFrame = raw;
			loading = false;
			loadError = null;
			ensurePrefetchRunning(fileIndex, activeFile?.frame_count ?? 1, frameIndex);
		} catch (e) {
			if ((e as Error).name === 'AbortError') return;
			loading = false;
			loadError = (e as Error).message || 'Failed to load frame';
		} finally {
			if (pendingRawCtrl === ctrl) pendingRawCtrl = null;
		}
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

	// Effect 1: load raw frame when file or frame changes.
	$effect(() => {
		if (!activeFile?.has_pixels) {
			currentRawFrame = null;
			loading = false;
			return;
		}
		// Detect file change → evict cache, cancel prefetch.
		const newScopeKey = String(activeFileIndex);
		if (newScopeKey !== fileScopeKey) {
			fileScopeKey = newScopeKey;
			prefetchController?.abort();
			prefetchController = null;
			rawFrameCache.clear();
			currentRawFrame = null;
			loading = true;
		}
		void loadRawFrameAndRender(activeFile.index, currentFrame);
	});

	// Effect 2: re-render when WL changes or frame loads.
	$effect(() => {
		if (!currentRawFrame || !canvasEl) return;
		const win = resolveDisplayWindow(
			currentRawFrame,
			liveWindowCenter,
			liveWindowWidth,
			windowCenter,
			windowWidth,
			windowMode,
		);
		renderRawFrame(canvasEl, currentRawFrame, win.wc, win.ww);
	});

	// Cleanup on component destroy: abort in-flight requests, clear cache.
	$effect(() => {
		return () => {
			prefetchController?.abort();
			pendingRawCtrl?.abort();
			rawFrameCache.clear();
		};
	});

	// Reset transform/window when toolbar Reset is clicked.
	// Edge-triggered on resetCount: acts only when the counter increments,
	// never while it merely stays > 0 (which would snap every user interaction).
	$effect(() => {
		if (resetCount === lastHandledResetCount) return;
		lastHandledResetCount = resetCount;
		// Guard against the initial render (count is 0, no reset has occurred).
		if (resetCount === 0) return;
		if (activeFile) {
			updateTransform(activeFile.index, { scale: 1, tx: 0, ty: 0 });
		}
		liveWindowCenter = null;
		liveWindowWidth = null;
		// Clear transient interaction state so in-progress gestures don't
		// resurrect stale drag / scroll context after the reset.
		dragState = null;
		wheelAccum = 0;
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
		if (!activeFile || !canvasEl) return;
		const { scale, tx, ty } = activeTransform;
		const clamped = Math.min(8, Math.max(0.2, newScale));
		const rect = canvasEl.getBoundingClientRect();
		const lx = (clientX - rect.left) / scale;
		const ly = (clientY - rect.top) / scale;
		const natX = rect.left - tx;
		const natY = rect.top - ty;
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
					const baseCenter = displayWindow.wc;
					const baseWidth = displayWindow.ww;
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
			// Re-render happens automatically via Effect 2 from cached raw data.
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
			// Commit live values so they persist after drag ends.
			windowCenter = liveWindowCenter;
			windowWidth = liveWindowWidth;
		}

		dragState = null;
	}

	function onPointerCancel() {
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
		<canvas
			bind:this={canvasEl}
			class="dicom-canvas"
			style={`transform:${transformCss}`}
		></canvas>
		<div class="overlay">
			<span>frame {currentFrame + 1} / {activeFile.frame_count}</span>
			<span>W: {Math.round(displayWindow.ww)} · C: {Math.round(displayWindow.wc)}</span>
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
	.dicom-canvas {
		max-width: 100%;
		max-height: 100%;
		transform-origin: 0 0;
		transition: transform 0.03s linear;
		image-rendering: pixelated;
		display: block;
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

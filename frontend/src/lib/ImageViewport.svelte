<script lang="ts">
	import { frameUrl, type FileSummary } from "../api";

	type TransformState = { scale: number; tx: number; ty: number };
	type DragState =
		| {
				mode: "pan";
				startX: number;
				startY: number;
				baseTx: number;
				baseTy: number;
		  }
		| {
				mode: "wl";
				startX: number;
				startY: number;
				baseCenter: number;
				baseWidth: number;
		  }
		| null;

	let {
		files,
		activeFileIndex,
		currentFrame,
		windowCenter = $bindable(),
		windowWidth = $bindable(),
	}: {
		files: FileSummary[];
		activeFileIndex: number;
		currentFrame: number;
		windowCenter: number | null;
		windowWidth: number | null;
	} = $props();

	let transformsByFile = $state<Record<number, TransformState>>({});
	let dragState = $state<DragState>(null);
	let loading = $state(true);
	let loadError = $state<string | null>(null);
	let liveWindowCenter = $state<number | null>(null);
	let liveWindowWidth = $state<number | null>(null);
	let wlDebounce: ReturnType<typeof setTimeout> | null = null;

	const ZOOM_STEPS = [0.25, 0.5, 0.75, 1, 1.25, 1.5, 2, 3, 4, 6, 8];
	const activeFile = $derived(files[activeFileIndex]);
	const activeTransform = $derived(transformsByFile[activeFileIndex] ?? { scale: 1, tx: 0, ty: 0 });
	const transformCss = $derived(
		`translate(${activeTransform.tx}px, ${activeTransform.ty}px) scale(${activeTransform.scale})`,
	);
	const src = $derived(
		activeFile ? frameUrl(activeFile.index, currentFrame, windowCenter, windowWidth) : "",
	);
	const displayWindowCenter = $derived(
		liveWindowCenter ?? windowCenter ?? activeFile?.default_window?.center ?? 0,
	);
	const displayWindowWidth = $derived(
		liveWindowWidth ?? windowWidth ?? activeFile?.default_window?.width ?? 1,
	);
	const zoomPercent = $derived(Math.round(activeTransform.scale * 100));

	$effect(() => {
		if (activeFile && !transformsByFile[activeFile.index]) {
			transformsByFile = {
				...transformsByFile,
				[activeFile.index]: { scale: 1, tx: 0, ty: 0 },
			};
		}
	});

	$effect(() => {
		if (src) {
			loading = true;
			loadError = null;
		}
	});

	function updateTransform(index: number, transform: TransformState) {
		transformsByFile = {
			...transformsByFile,
			[index]: transform,
		};
	}

	function onWheel(event: WheelEvent) {
		if (!activeFile || !activeFile.has_pixels) {
			return;
		}
		event.preventDefault();
		const current = activeTransform;

		// Pinch-to-zoom on trackpads fires wheel events with ctrlKey=true.
		// Discrete mouse wheels use deltaMode=1 (line) or deltaMode=2 (page).
		// deltaMode=0 (pixel) without ctrlKey is a trackpad two-finger scroll → pan.
		const isPinchZoom = event.ctrlKey && event.deltaMode === 0;
		const isDiscreteWheel = event.deltaMode !== 0;

		if (isPinchZoom || isDiscreteWheel) {
			// Zoom: pinch gesture or mouse wheel
			const delta = isPinchZoom
				? -event.deltaY * 0.01  // pinch: continuous, scale-proportional
				: event.deltaY < 0 ? 0.05 : -0.05;  // wheel: fixed step per click
			const nextScale = Math.min(8, Math.max(0.2, current.scale + delta));
			updateTransform(activeFile.index, { ...current, scale: nextScale });
		} else {
			// Trackpad scroll: pan the image
			updateTransform(activeFile.index, {
				...current,
				tx: current.tx - event.deltaX,
				ty: current.ty - event.deltaY,
			});
		}
	}

	function onPointerDown(event: PointerEvent) {
		if (!activeFile || !activeFile.has_pixels) {
			return;
		}
		(event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);

		if (event.button === 0) {
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
			event.preventDefault();
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
		}
	}

	function onPointerMove(event: PointerEvent) {
		if (!activeFile || !dragState) {
			return;
		}

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

		const dx = event.clientX - dragState.startX;
		const dy = event.clientY - dragState.startY;
		const nextWidth = Math.max(1, dragState.baseWidth + dx * 4);
		const nextCenter = dragState.baseCenter - dy * 2;
		liveWindowCenter = nextCenter;
		liveWindowWidth = nextWidth;

		if (wlDebounce) {
			clearTimeout(wlDebounce);
		}
		wlDebounce = setTimeout(() => {
			windowCenter = nextCenter;
			windowWidth = nextWidth;
		}, 150);
	}

	function onPointerUp(event: PointerEvent) {
		(event.currentTarget as HTMLElement).releasePointerCapture(event.pointerId);
		dragState = null;
	}

	function onPointerCancel() {
		dragState = null;
	}

	function onContextMenu(event: MouseEvent) {
		event.preventDefault();
	}

	function resetViewport() {
		if (!activeFile) {
			return;
		}
		updateTransform(activeFile.index, { scale: 1, tx: 0, ty: 0 });
		windowCenter = activeFile.default_window?.center ?? null;
		windowWidth = activeFile.default_window?.width ?? null;
		liveWindowCenter = null;
		liveWindowWidth = null;
	}

	function zoomToLevel(level: number) {
		if (!activeFile || !activeFile.has_pixels) return;
		const current = activeTransform;
		updateTransform(activeFile.index, { ...current, scale: level });
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
	class="viewport"
	role="application"
	onwheel={onWheel}
	onpointerdown={onPointerDown}
	onpointermove={onPointerMove}
	onpointerup={onPointerUp}
	onpointercancel={onPointerCancel}
	oncontextmenu={onContextMenu}
	ondblclick={resetViewport}
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
		<img
			src={src}
			alt={`frame ${currentFrame + 1}`}
			draggable="false"
			style={`transform:${transformCss}`}
			onload={() => {
				loading = false;
				loadError = null;
				liveWindowCenter = null;
				liveWindowWidth = null;
			}}
			onerror={() => {
				loading = false;
				loadError = "Failed to load frame";
			}}
		/>
		<div class="overlay">
			<span>frame {currentFrame + 1} / {activeFile.frame_count}</span>
			<span>W: {Math.round(displayWindowWidth)} · C: {Math.round(displayWindowCenter)}</span>
		</div>
		<div class="zoom-controls">
			<button type="button" onclick={() => stepZoom(-1)} disabled={activeTransform.scale <= ZOOM_STEPS[0]}>−</button>
			<button type="button" class="zoom-level" onclick={() => zoomToLevel(1)}>{zoomPercent}%</button>
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
	img {
		max-width: 100%;
		max-height: 100%;
		object-fit: contain;
		transform-origin: center;
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

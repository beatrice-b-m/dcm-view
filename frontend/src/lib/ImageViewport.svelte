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
		const delta = event.deltaY < 0 ? 0.1 : -0.1;
		const nextScale = Math.min(8, Math.max(0.2, current.scale + delta));
		updateTransform(activeFile.index, { ...current, scale: nextScale });
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
</style>

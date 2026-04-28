<script lang="ts">
	import { onMount } from "svelte";
	import { fetchFiles, type FilesResponse, type WindowMode } from "./api";
	import FileTabs from "./lib/FileTabs.svelte";
	import FrameSlider from "./lib/FrameSlider.svelte";
	import ImageViewport from "./lib/ImageViewport.svelte";
	import StatusBar from "./lib/StatusBar.svelte";
	import TagPanel from "./lib/TagPanel.svelte";
	import ViewerToolbar from "./lib/ViewerToolbar.svelte";
	import { DEFAULT_ORIENTATION, WL_PRESETS, type ActiveTool, type ImageOrientation } from "./lib/viewerTools";

	const SIDEBAR_DEFAULT_WIDTH_PX = 360;
	const SIDEBAR_MIN_WIDTH_PX = 260;
	const SIDEBAR_MAX_WIDTH_PX = 720;
	const SIDEBAR_COLLAPSED_WIDTH_PX = 44;

	type SidebarResizeState = {
		pointerId: number;
		startX: number;
		startWidth: number;
	};

	let filesResponse = $state<FilesResponse | null>(null);
	let loadError = $state<string | null>(null);

	let activeFileIndex = $state(0);
	let currentFrame = $state(0);
	let windowCenter = $state<number | null>(null);
	let windowWidth = $state<number | null>(null);
	let activeTool = $state<ActiveTool>('pan');
	let windowMode = $state<WindowMode>('default');
	let selectedPresetId = $state('default');
	let resetCount = $state(0);
	let orientationByFile = $state<Record<number, ImageOrientation>>({});
	const activeOrientation = $derived(orientationByFile[activeFileIndex] ?? DEFAULT_ORIENTATION);
	let tagPanelWidthPx = $state(clampSidebarWidth(SIDEBAR_DEFAULT_WIDTH_PX));
	let tagPanelCollapsed = $state(false);
	let sidebarResizeState = $state<SidebarResizeState | null>(null);

	const sidebarWidthPx = $derived(
		tagPanelCollapsed ? SIDEBAR_COLLAPSED_WIDTH_PX : tagPanelWidthPx,
	);

	function clampSidebarWidth(width: number): number {
		return Math.min(SIDEBAR_MAX_WIDTH_PX, Math.max(SIDEBAR_MIN_WIDTH_PX, width));
	}

	function resetViewport() {
		windowCenter = null;
		windowWidth = null;
		windowMode = 'default';
		selectedPresetId = 'default';
		resetCount += 1;
		if (orientationByFile[activeFileIndex]) {
			orientationByFile = { ...orientationByFile, [activeFileIndex]: DEFAULT_ORIENTATION };
		}
	}

	function getOrientation(index: number): ImageOrientation {
		return orientationByFile[index] ?? DEFAULT_ORIENTATION;
	}

	function applyFlipH() {
		const cur = getOrientation(activeFileIndex);
		orientationByFile = { ...orientationByFile, [activeFileIndex]: { ...cur, flipH: !cur.flipH } };
	}

	function applyFlipV() {
		const cur = getOrientation(activeFileIndex);
		orientationByFile = { ...orientationByFile, [activeFileIndex]: { ...cur, flipV: !cur.flipV } };
	}

	function applyRotateCW() {
		const cur = getOrientation(activeFileIndex);
		const r = ((cur.rotation + 90) % 360) as 0 | 90 | 180 | 270;
		orientationByFile = { ...orientationByFile, [activeFileIndex]: { ...cur, rotation: r } };
	}

	function applyRotateCCW() {
		const cur = getOrientation(activeFileIndex);
		const r = ((cur.rotation + 270) % 360) as 0 | 90 | 180 | 270;
		orientationByFile = { ...orientationByFile, [activeFileIndex]: { ...cur, rotation: r } };
	}

	function toggleTagPanel() {
		tagPanelCollapsed = !tagPanelCollapsed;
	}

	function startTagPanelResize(event: PointerEvent) {
		if (tagPanelCollapsed || event.button !== 0) {
			return;
		}

		const handle = event.currentTarget as HTMLElement;
		handle.setPointerCapture(event.pointerId);
		sidebarResizeState = {
			pointerId: event.pointerId,
			startX: event.clientX,
			startWidth: tagPanelWidthPx,
		};
		event.preventDefault();
	}

	function moveTagPanelResize(event: PointerEvent) {
		if (!sidebarResizeState || sidebarResizeState.pointerId !== event.pointerId) {
			return;
		}

		const delta = sidebarResizeState.startX - event.clientX;
		tagPanelWidthPx = clampSidebarWidth(sidebarResizeState.startWidth + delta);
	}

	function endTagPanelResize(event: PointerEvent) {
		const handle = event.currentTarget as HTMLElement;
		if (handle.hasPointerCapture(event.pointerId)) {
			handle.releasePointerCapture(event.pointerId);
		}

		if (sidebarResizeState?.pointerId === event.pointerId) {
			sidebarResizeState = null;
		}
	}

	function cancelTagPanelResize() {
		sidebarResizeState = null;
	}

	$effect(() => {
		const preset = WL_PRESETS.find(p => p.id === selectedPresetId);
		if (!preset) return;
		if (preset.wc !== undefined && preset.ww !== undefined) {
			windowCenter = preset.wc;
			windowWidth = preset.ww;
			windowMode = 'default';
		} else {
			windowCenter = null;
			windowWidth = null;
			windowMode = preset.mode ?? 'default';
		}
	});

	$effect(() => {
		const handleKey = (event: KeyboardEvent) => {
			const target = event.target as HTMLElement | null;
			if (target && ['INPUT', 'TEXTAREA', 'SELECT'].includes(target.tagName)) return;
			switch (event.key.toLowerCase()) {
				case 'w': activeTool = 'window_level'; break;
				case 'p': activeTool = 'pan'; break;
				case 'z': activeTool = 'zoom'; break;
				case 's': activeTool = 'scroll'; break;
			}
		};
		window.addEventListener('keydown', handleKey);
		return () => window.removeEventListener('keydown', handleKey);
	});

	onMount(async () => {
		try {
			filesResponse = await fetchFiles();
		} catch (error) {
			loadError = error instanceof Error ? error.message : String(error);
		}
	});
</script>

{#if loadError}
	<main class="error">{loadError}</main>
{:else if !filesResponse}
	<main class="loading">Loading dcmview…</main>
{:else}
	<main class="layout">
		<FileTabs
			files={filesResponse.files}
			bind:activeFileIndex
			bind:currentFrame
			bind:windowCenter
			bind:windowWidth
			bind:windowMode
			bind:selectedPresetId
		/>
		<ViewerToolbar
			bind:activeTool
			bind:selectedPresetId
			onreset={resetViewport}
			onflipH={applyFlipH}
			onflipV={applyFlipV}
			onrotateCW={applyRotateCW}
			onrotateCCW={applyRotateCCW}
		/>
		<section class="content" style={`--tag-panel-width:${sidebarWidthPx}px;`}>
			<ImageViewport
				files={filesResponse.files}
				activeFileIndex={activeFileIndex}
				bind:currentFrame
				bind:windowCenter
				bind:windowWidth
				activeTool={activeTool}
				windowMode={windowMode}
				resetCount={resetCount}
				selectedPresetId={selectedPresetId}
				orientation={activeOrientation}
				onreset={resetViewport}
			/>
			<aside class="tag-panel-shell" class:collapsed={tagPanelCollapsed}>
				<div
					class="sidebar-handle"
					class:dragging={sidebarResizeState !== null}
					class:disabled={tagPanelCollapsed}
					role="separator"
					aria-label="Resize DICOM tag panel"
					aria-orientation="vertical"
					aria-valuemin={SIDEBAR_MIN_WIDTH_PX}
					aria-valuemax={SIDEBAR_MAX_WIDTH_PX}
					aria-valuenow={tagPanelWidthPx}
					onpointerdown={startTagPanelResize}
					onpointermove={moveTagPanelResize}
					onpointerup={endTagPanelResize}
					onpointercancel={cancelTagPanelResize}
				></div>
				<button
					type="button"
					class="panel-toggle"
					onclick={toggleTagPanel}
					aria-label={tagPanelCollapsed ? "Expand DICOM tag panel" : "Collapse DICOM tag panel"}
					aria-expanded={!tagPanelCollapsed}
				>
					{tagPanelCollapsed ? "◀" : "▶"}
				</button>
				{#if !tagPanelCollapsed}
					<TagPanel files={filesResponse.files} activeFileIndex={activeFileIndex} />
				{/if}
			</aside>
		</section>
		<FrameSlider
			files={filesResponse.files}
			activeFileIndex={activeFileIndex}
			bind:currentFrame
		/>
		<StatusBar
			serverStartMs={filesResponse.server_start_ms}
			fileCount={filesResponse.files.length}
			tunnelled={filesResponse.tunnelled}
			tunnelHost={filesResponse.tunnel_host}
		/>
	</main>
{/if}

<style>
	:global(html),
	:global(body) {
		margin: 0;
		padding: 0;
		width: 100%;
		height: 100%;
		overflow: hidden;
		font-family: system-ui, sans-serif;
		background: #1a1a1a;
		color: #e0e0e0;
	}

	.layout {
		display: grid;
		grid-template-rows: auto auto 1fr auto auto;
		height: 100vh;
		width: 100%;
		overflow: hidden;
	}

	.content {
		display: grid;
		grid-template-columns: minmax(0, 1fr) var(--tag-panel-width);
		grid-template-rows: 1fr;
		min-height: 0;
	}

	.tag-panel-shell {
		position: relative;
		background: #242424;
		min-width: 0;
		min-height: 0;
		overflow: hidden;
	}

	.tag-panel-shell.collapsed {
		background: #202020;
	}

	.sidebar-handle {
		position: absolute;
		left: 0;
		top: 0;
		bottom: 0;
		width: 10px;
		transform: translateX(-50%);
		cursor: col-resize;
		touch-action: none;
		z-index: 5;
	}

	.sidebar-handle::after {
		content: "";
		position: absolute;
		left: 50%;
		top: 0;
		bottom: 0;
		width: 1px;
		background: #3a3a3a;
		transform: translateX(-50%);
	}

	.sidebar-handle.dragging::after {
		background: #4a9eff;
	}

	.sidebar-handle.disabled {
		cursor: default;
		pointer-events: none;
	}

	.panel-toggle {
		position: absolute;
		top: 0.6rem;
		right: 0.45rem;
		display: grid;
		place-items: center;
		width: 1.5rem;
		height: 1.5rem;
		border: 1px solid #3b3b3b;
		border-radius: 4px;
		background: #1b1b1b;
		color: #e0e0e0;
		cursor: pointer;
		z-index: 6;
	}

	.panel-toggle:hover {
		border-color: #4a9eff;
		color: #4a9eff;
	}

	.loading,
	.error {
		display: grid;
		place-content: center;
		height: 100vh;
	}
</style>
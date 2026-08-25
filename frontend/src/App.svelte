<script lang="ts">
	import { onMount } from "svelte";
	import { annotationsExportUrl, fetchFiles, type FilesResponse, type WindowMode, type WindowPreset } from "./api";
	import FileNavigator from "./lib/FileNavigator.svelte";
	import FrameSlider from "./lib/FrameSlider.svelte";
	import ImageViewport from "./lib/ImageViewport.svelte";
	import OpenImageTabs from "./lib/OpenImageTabs.svelte";
	import StatusBar from "./lib/StatusBar.svelte";
	import TagPanel from "./lib/TagPanel.svelte";
	import ViewerToolbar from "./lib/ViewerToolbar.svelte";
	import type { CineDirection, CineMode } from "./lib/cinePlayback";
	import { indexFilesById, resolveFilesById } from "./lib/fileRegistry";
	import { DEFAULT_ORIENTATION, WL_PRESETS, type ActiveTool, type ImageOrientation } from "./lib/viewerTools";

	const TAG_PANEL_DEFAULT_WIDTH_PX = 360;
	const TAG_PANEL_MIN_WIDTH_PX = 260;
	const TAG_PANEL_MAX_WIDTH_PX = 720;
	const TAG_PANEL_COLLAPSED_WIDTH_PX = 44;
	const FILE_NAV_WIDTH_PX = 300;
	const FILE_NAV_COLLAPSED_WIDTH_PX = 44;

	type SidebarResizeState = {
		pointerId: number;
		startX: number;
		startWidth: number;
	};

	type OpenTabState = {
		fileIndex: number;
		currentFrame: number;
	};

	type ManualWindowAdjustment = {
		centerOffsetRatio: number;
		widthRatio: number;
	};

	let filesResponse = $state<FilesResponse | null>(null);
	let loadError = $state<string | null>(null);

	let openTabs = $state<OpenTabState[]>([]);
	let activeFileIndex = $state<number | null>(null);
	let currentFrame = $state(0);
	let cinePlaying = $state(false);
	let cineFps = $state(10);
	let cineMode = $state<CineMode>("loop");
	let cineDirection = $state<CineDirection>(1);
	let lastCineFileIndex = $state<number | null>(null);
	let windowCenter = $state<number | null>(null);
	let windowWidth = $state<number | null>(null);
	let activeTool = $state<ActiveTool>('pan');
	let windowMode = $state<WindowMode>('default');
	let selectedPresetId = $state('default');
	let lastAppliedPresetId = 'default';
	let manualWindowAdjustment = $state<ManualWindowAdjustment | null>(null);
	let lastWindowFileIndex = $state<number | null>(null);
	let resetCount = $state(0);
	let orientationByFile = $state<Record<number, ImageOrientation>>({});
	let fileNavigatorCollapsed = $state(false);
	let tagPanelWidthPx = $state(clampTagPanelWidth(TAG_PANEL_DEFAULT_WIDTH_PX));
	let tagPanelCollapsed = $state(false);
	let sidebarResizeState = $state<SidebarResizeState | null>(null);

	const filesById = $derived(indexFilesById(filesResponse?.files ?? []));
	const activeFile = $derived(
		activeFileIndex === null ? null : filesById.get(activeFileIndex) ?? null,
	);
	const activeOrientation = $derived(activeFileIndex === null ? DEFAULT_ORIENTATION : orientationByFile[activeFileIndex] ?? DEFAULT_ORIENTATION);
	const openTabFiles = $derived(resolveFilesById(filesById, openTabs.map((tab) => tab.fileIndex)));
	const fileNavigatorWidthPx = $derived(fileNavigatorCollapsed ? FILE_NAV_COLLAPSED_WIDTH_PX : FILE_NAV_WIDTH_PX);
	const tagPanelWidth = $derived(tagPanelCollapsed ? TAG_PANEL_COLLAPSED_WIDTH_PX : tagPanelWidthPx);

	function clampTagPanelWidth(width: number): number {
		return Math.min(TAG_PANEL_MAX_WIDTH_PX, Math.max(TAG_PANEL_MIN_WIDTH_PX, width));
	}

	function defaultTabState(fileIndex: number): OpenTabState {
		return {
			fileIndex,
			currentFrame: 0,
		};
	}

	function saveActiveTabState() {
		if (activeFileIndex === null) return;
		openTabs = openTabs.map((tab) => tab.fileIndex === activeFileIndex
			? {
				...tab,
				currentFrame,
			}
			: tab);
	}

	function loadTabState(tab: OpenTabState | null) {
		if (!tab) {
			activeFileIndex = null;
			currentFrame = 0;
			return;
		}

		activeFileIndex = tab.fileIndex;
		currentFrame = tab.currentFrame;
	}

	function activateOpenTab(fileIndex: number) {
		const target = openTabs.find((tab) => tab.fileIndex === fileIndex);
		if (!target) return;
		if (activeFileIndex !== fileIndex) {
			saveActiveTabState();
		}
		loadTabState(target);
	}

	function openOrActivateFile(fileIndex: number) {
		const existing = openTabs.find((tab) => tab.fileIndex === fileIndex);
		if (existing) {
			activateOpenTab(fileIndex);
			return;
		}

		saveActiveTabState();
		const next = defaultTabState(fileIndex);
		openTabs = [...openTabs, next];
		loadTabState(next);
	}

	function closeOpenTab(fileIndex: number) {
		const closingIndex = openTabs.findIndex((tab) => tab.fileIndex === fileIndex);
		if (closingIndex === -1) return;

		const wasActive = activeFileIndex === fileIndex;
		const remaining = openTabs.filter((tab) => tab.fileIndex !== fileIndex);
		openTabs = remaining;

		if (!wasActive) return;

		const replacement = remaining[Math.min(closingIndex, remaining.length - 1)] ?? null;
		loadTabState(replacement);
	}

	function resetViewport() {
		if (activeFileIndex === null) return;
		manualWindowAdjustment = null;
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
		if (activeFileIndex === null) return;
		const cur = getOrientation(activeFileIndex);
		orientationByFile = { ...orientationByFile, [activeFileIndex]: { ...cur, flipH: !cur.flipH } };
	}

	function applyFlipV() {
		if (activeFileIndex === null) return;
		const cur = getOrientation(activeFileIndex);
		orientationByFile = { ...orientationByFile, [activeFileIndex]: { ...cur, flipV: !cur.flipV } };
	}

	function applyRotateCW() {
		if (activeFileIndex === null) return;
		const cur = getOrientation(activeFileIndex);
		const r = ((cur.rotation + 90) % 360) as 0 | 90 | 180 | 270;
		orientationByFile = { ...orientationByFile, [activeFileIndex]: { ...cur, rotation: r } };
	}

	function applyRotateCCW() {
		if (activeFileIndex === null) return;
		const cur = getOrientation(activeFileIndex);
		const r = ((cur.rotation + 270) % 360) as 0 | 90 | 180 | 270;
		orientationByFile = { ...orientationByFile, [activeFileIndex]: { ...cur, rotation: r } };
	}

	function exportAnnotations() {
		const link = document.createElement('a');
		link.href = annotationsExportUrl();
		link.download = 'dcmview-annotations.csv';
		document.body.appendChild(link);
		link.click();
		link.remove();
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
		tagPanelWidthPx = clampTagPanelWidth(sidebarResizeState.startWidth + delta);
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

	function fileByIndex(fileIndex: number): FilesResponse["files"][number] | null {
		return filesById.get(fileIndex) ?? null;
	}

	function applyFilesResponse(response: FilesResponse) {
		filesResponse = response;
		if (activeFileIndex === null && openTabs.length === 0 && response.files.length > 0) {
			openOrActivateFile(response.files[0].index);
		}
	}

	function defaultWindowForFile(fileIndex: number): WindowPreset | null {
		const window = fileByIndex(fileIndex)?.default_window ?? null;
		if (!window || !Number.isFinite(window.center) || !Number.isFinite(window.width) || window.width <= 0) {
			return null;
		}
		return window;
	}

	function resolveManualWindowForFile(fileIndex: number): WindowPreset | null {
		if (!manualWindowAdjustment) return null;
		const base = defaultWindowForFile(fileIndex);
		if (!base) return null;
		return {
			center: base.center + manualWindowAdjustment.centerOffsetRatio * base.width,
			width: Math.max(1, manualWindowAdjustment.widthRatio * base.width),
		};
	}

	function recordManualWindowLevel(center: number, width: number) {
		if (activeFileIndex === null || !Number.isFinite(center) || !Number.isFinite(width) || width <= 0) {
			return;
		}
		const base = defaultWindowForFile(activeFileIndex);
		if (!base) {
			manualWindowAdjustment = null;
			return;
		}
		manualWindowAdjustment = {
			centerOffsetRatio: (center - base.center) / base.width,
			widthRatio: width / base.width,
		};
		windowMode = 'default';
		selectedPresetId = 'default';
		lastAppliedPresetId = 'default';
	}

	function applyWindowPreset(presetId: string) {
		manualWindowAdjustment = null;
		const preset = WL_PRESETS.find(p => p.id === presetId);
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
	}

	$effect(() => {
		const presetId = selectedPresetId;
		if (presetId === lastAppliedPresetId) return;
		lastAppliedPresetId = presetId;
		applyWindowPreset(presetId);
	});

	$effect(() => {
		const fileIndex = activeFileIndex;
		if (fileIndex === lastWindowFileIndex) return;
		lastWindowFileIndex = fileIndex;
		if (fileIndex === null || !manualWindowAdjustment) return;
		const resolved = resolveManualWindowForFile(fileIndex);
		if (!resolved) return;
		windowCenter = resolved.center;
		windowWidth = resolved.width;
		windowMode = 'default';
	});

	$effect(() => {
		const fileIndex = activeFileIndex;
		if (fileIndex === lastCineFileIndex) return;
		lastCineFileIndex = fileIndex;
		cinePlaying = false;
		cineDirection = 1;
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
				case 'r': activeTool = 'annotate_rect'; break;
			}
		};
		window.addEventListener('keydown', handleKey);
		return () => window.removeEventListener('keydown', handleKey);
	});

	onMount(() => {
		let cancelled = false;
		let pollTimer: number | null = null;

		const pollFiles = async () => {
			try {
				const response = await fetchFiles();
				if (cancelled) return;
				applyFilesResponse(response);
				if (!response.scan_complete) {
					pollTimer = window.setTimeout(pollFiles, 500);
				}
			} catch (error) {
				if (cancelled) return;
				if (!filesResponse) {
					loadError = error instanceof Error ? error.message : String(error);
					return;
				}
				pollTimer = window.setTimeout(pollFiles, 1000);
			}
		};

		pollFiles();
		return () => {
			cancelled = true;
			if (pollTimer !== null) {
				window.clearTimeout(pollTimer);
			}
		};
	});
</script>

{#if loadError}
	<main class="error">{loadError}</main>
{:else if !filesResponse}
	<main class="loading">Loading dcmview…</main>
{:else}
	<main
		class="layout"
		style={`--file-nav-width:${fileNavigatorWidthPx}px; --tag-panel-width:${tagPanelWidth}px;`}
	>
		<header class="topbar">
			<img
				class="brand-mark"
				src="/assets/dcmview-icon.png"
				alt="dcmview"
			/>
			<OpenImageTabs
				openFiles={openTabFiles}
				activeFileIndex={activeFileIndex}
				onactivate={activateOpenTab}
				onclose={closeOpenTab}
			/>
		</header>
		<ViewerToolbar
			bind:activeTool
			bind:selectedPresetId
			onreset={resetViewport}
			onflipH={applyFlipH}
			onflipV={applyFlipV}
			onrotateCW={applyRotateCW}
			onrotateCCW={applyRotateCCW}
			onexportAnnotations={exportAnnotations}
		/>
		<section class="workspace">
			<FileNavigator
				files={filesResponse.files}
				activeFileIndex={activeFileIndex}
				scanComplete={filesResponse.scan_complete}
				bind:collapsed={fileNavigatorCollapsed}
				onopenfile={openOrActivateFile}
			/>
			<section class="viewer-column">
				{#if activeFile === null}
					<div class="empty-viewer">Open a file from the sidebar</div>
				{:else}
					<ImageViewport
						{activeFile}
						bind:currentFrame
						bind:windowCenter
						bind:windowWidth
						activeTool={activeTool}
						windowMode={windowMode}
						resetCount={resetCount}
						selectedPresetId={selectedPresetId}
						orientation={activeOrientation}
						bind:cinePlaying
						{cineFps}
						{cineMode}
						bind:cineDirection
						onreset={resetViewport}
						onmanualwindowlevel={recordManualWindowLevel}
					/>
					<FrameSlider
						{activeFile}
						bind:currentFrame
						bind:cinePlaying
						bind:cineFps
						bind:cineMode
						bind:cineDirection
					/>
				{/if}
			</section>
			<aside class="tag-panel-shell" class:collapsed={tagPanelCollapsed}>
				<div
					class="sidebar-handle"
					class:dragging={sidebarResizeState !== null}
					class:disabled={tagPanelCollapsed}
					role="separator"
					aria-label="Resize DICOM tag panel"
					aria-orientation="vertical"
					aria-valuemin={TAG_PANEL_MIN_WIDTH_PX}
					aria-valuemax={TAG_PANEL_MAX_WIDTH_PX}
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
					{#if activeFile === null}
						<div class="tag-empty">No file selected</div>
					{:else}
						<TagPanel fileIndex={activeFile.index} />
					{/if}
				{/if}
			</aside>
		</section>
		<StatusBar
			serverStartMs={filesResponse.server_start_ms}
			fileCount={filesResponse.files.length}
			tunnelled={filesResponse.tunnelled}
			tunnelHost={filesResponse.tunnel_host}
		/>
	</main>
{/if}

<style>
	:global(:root) {
		--font-ui: -apple-system, BlinkMacSystemFont, "SF Pro Text", "Segoe UI", system-ui, sans-serif;
		--font-mono: "SF Mono", "JetBrains Mono", ui-monospace, monospace;
		--surface-root: #151516;
		--surface-viewport: #080809;
		--surface-chrome: #202124;
		--surface-panel: #252629;
		--surface-panel-alt: #2b2c30;
		--surface-control: #303136;
		--surface-control-hover: #393a40;
		--surface-control-active: #e7e7ea;
		--border-subtle: rgba(255, 255, 255, 0.08);
		--border-strong: rgba(255, 255, 255, 0.14);
		--text-primary: #f2f2f3;
		--text-secondary: #c7c7cc;
		--text-muted: #8e8e93;
		--text-inverse: #1d1d1f;
		--accent: #0a84ff;
		--accent-soft: rgba(10, 132, 255, 0.16);
		--danger: #ff6961;
		--radius-control: 7px;
		--radius-panel: 8px;
		--control-height: 1.75rem;
		--shadow-hud: 0 12px 30px rgba(0, 0, 0, 0.28);
		--focus-ring: 0 0 0 2px rgba(10, 132, 255, 0.48);
		color-scheme: dark;
	}

	:global(*) {
		box-sizing: border-box;
	}

	:global(html),
	:global(body) {
		margin: 0;
		padding: 0;
		width: 100%;
		height: 100%;
		overflow: hidden;
		font-family: var(--font-ui);
		background: var(--surface-root);
		color: var(--text-primary);
		-webkit-font-smoothing: antialiased;
		text-rendering: optimizeLegibility;
	}

	.layout {
		display: grid;
		grid-template-rows: auto auto 1fr auto;
		height: 100vh;
		width: 100%;
		overflow: hidden;
		background: var(--surface-root);
	}

	.topbar {
		display: grid;
		grid-template-columns: auto minmax(0, 1fr);
		align-items: end;
		gap: 0.8rem;
		min-height: 2.6rem;
		background: var(--surface-chrome);
		padding: 0 0.7rem;
		border-bottom: 1px solid var(--border-subtle);
	}

	.brand-mark {
		align-self: center;
		display: block;
		width: 1.55rem;
		height: 1.55rem;
		border-radius: 0.28rem;
	}

	.workspace {
		display: grid;
		grid-template-columns: var(--file-nav-width) minmax(0, 1fr) var(--tag-panel-width);
		grid-template-rows: 1fr;
		min-height: 0;
	}

	.viewer-column {
		display: grid;
		grid-template-rows: minmax(0, 1fr) auto;
		min-width: 0;
		min-height: 0;
		background: var(--surface-viewport);
	}

	.empty-viewer,
	.tag-empty {
		display: grid;
		place-content: center;
		color: var(--text-muted);
	}

	.empty-viewer {
		min-height: 0;
		background: var(--surface-viewport);
	}

	.tag-empty {
		height: 100%;
		font-size: 0.85rem;
	}

	.tag-panel-shell {
		position: relative;
		background: var(--surface-panel);
		border-left: 1px solid var(--border-subtle);
		min-width: 0;
		min-height: 0;
		overflow: hidden;
	}

	.tag-panel-shell.collapsed {
		background: var(--surface-chrome);
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
		background: var(--border-subtle);
		transform: translateX(-50%);
	}

	.sidebar-handle.dragging::after {
		background: var(--accent);
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
		border: 1px solid var(--border-subtle);
		border-radius: var(--radius-control);
		background: var(--surface-control);
		color: var(--text-secondary);
		cursor: pointer;
		z-index: 6;
	}

	.panel-toggle:hover {
		background: var(--surface-control-hover);
		color: var(--text-primary);
	}

	.panel-toggle:focus-visible {
		outline: none;
		box-shadow: var(--focus-ring);
	}

	.loading,
	.error {
		display: grid;
		place-content: center;
		height: 100vh;
		background: var(--surface-root);
		color: var(--text-secondary);
	}

	@media (max-width: 980px) {
		.workspace {
			grid-template-columns: var(--file-nav-width) minmax(0, 1fr);
		}

		.tag-panel-shell {
			display: none;
		}
	}

	@media (max-width: 520px) {
		.topbar {
			grid-template-columns: minmax(0, 1fr);
			gap: 0.25rem;
			padding-top: 0.35rem;
		}

		.brand-mark {
			display: none;
		}

		.workspace {
			grid-template-columns: minmax(0, 1fr);
		}

		.workspace :global(.navigator) {
			display: none;
		}
	}
</style>

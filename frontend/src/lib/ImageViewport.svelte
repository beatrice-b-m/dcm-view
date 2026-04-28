<script lang="ts">
	import {
		displayFrameCacheKey,
		fetchAnnotations,
		fetchDisplayFrameBlob,
		fetchRawFrame,
		type DisplayFrameWindowOptions,
		type EmbedRoiAnnotations,
		type FileSummary,
		type RawFrame,
		type WindowMode,
	} from "../api";
	import type { ActiveTool } from "./viewerTools";

	type PipelineMode = "cine" | "diagnostic_wl";
	type TransformState = { scale: number; tx: number; ty: number };
	type DragState =
		| { mode: "pan"; startX: number; startY: number; baseTx: number; baseTy: number }
		| { mode: "wl"; startX: number; startY: number; baseCenter: number; baseWidth: number }
		| { mode: "zoom_drag"; startX: number; startY: number; baseScale: number; pivotX: number; pivotY: number }
		| { mode: "scroll_drag"; startY: number; baseFrame: number }
		| null;

	interface DisplayFrameCacheEntry {
		blob: Blob;
		bytes: number;
		bitmap: ImageBitmap | null;
		decodePromise: Promise<ImageBitmap> | null;
	}

	type VisibleRoi = {
		index: number;
		ymin: number;
		xmin: number;
		ymax: number;
		xmax: number;
		frames: number[] | null;
	};

	let {
		files,
		activeFileIndex,
		currentFrame = $bindable(),
		windowCenter = $bindable(),
		windowWidth = $bindable(),
		activeTool,
		windowMode,
		selectedPresetId,
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
		selectedPresetId: string;
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
	let currentDisplayCacheKey = $state<string | null>(null);
	let annotationsByFile = $state<Record<number, EmbedRoiAnnotations | undefined>>({});
	let annotationErrorsByFile = $state<Record<number, string | null | undefined>>({});
	let annotationLoadingByFile = $state<Record<number, boolean | undefined>>({});
	let annotationRequestedByFile: Record<number, boolean> = {};

	let rawRequestCtrl: AbortController | null = null;
	let rawPrefetchCtrl: AbortController | null = null;
	let displayRequestCtrl: AbortController | null = null;
	let displayPrefetchCtrl: AbortController | null = null;
	let displayPrefetchSeedFrame: number | null = null;
	let displayPrefetchScopeKey = "";
	let rawFrameCache = new Map<number, RawFrame>();
	let rawCacheBytes = 0;
	let displayFrameCache = new Map<string, DisplayFrameCacheEntry>();
	let displayCacheBytes = 0;
	let fileScopeKey = "";
	let lastHandledResetCount = 0;
	let requestGeneration = 0;
	let lastFrameForDirection = 0;
	let frameDirection: 1 | -1 = 1;
	let wlRenderGeneration = 0;

	let wlWorker: Worker | null = null;
	let workerInitAttempted = false;
	let workerAvailable = false;
	let workerMessageId = 0;
	let pendingWorkerResponses = new Map<number, {
		resolve: (value: { width: number; height: number; rgba: ArrayBuffer }) => void;
		reject: (error: Error) => void;
	}>();

	const ZOOM_STEPS = [0.25, 0.5, 0.75, 1, 1.25, 1.5, 2, 3, 4, 6, 8];
	const MAX_RENDER_PIXELS = 20_000_000;
	const RAW_CACHE_BYTE_BUDGET = 256 * 1024 * 1024;
	const RAW_RING_RADIUS = 10;
	const DISPLAY_CACHE_BYTE_BUDGET = 320 * 1024 * 1024;
	const DISPLAY_FULL_PREFETCH_BUDGET_BYTES = 320 * 1024 * 1024;
	const DISPLAY_NEAR_PREFETCH_DISTANCE = 48;
	const WORKER_MIN_PIXEL_THRESHOLD = 300_000;
	const PREFETCH_CONCURRENCY = 3;
	const PREFETCH_RESEED_DISTANCE = 6;
	const FRAME_SCROLL_SPEED_FACTOR = 0.7;
	const WHEEL_FRAME_THRESHOLD = 30 / FRAME_SCROLL_SPEED_FACTOR;
	const DRAG_PIXELS_PER_FRAME = 10 / FRAME_SCROLL_SPEED_FACTOR;
	const activeFile = $derived(files[activeFileIndex] ?? { frame_count: 0, default_window: null });
	const activeTransform = $derived(transformsByFile[activeFileIndex] ?? { scale: 1, tx: 0, ty: 0 });
	const transformCss = $derived(
		`translate(${activeTransform.tx}px, ${activeTransform.ty}px) scale(${activeTransform.scale})`,
	);
	const zoomPercent = $derived(Math.round(activeTransform.scale * 100));
	const isDragging = $derived(dragState !== null);
	const pipelineMode = $derived<PipelineMode>(
		activeTool === "window_level" ? "diagnostic_wl" : "cine",
	);

	const displayWindow = $derived(
		pipelineMode === "diagnostic_wl" && currentRawFrame
			? resolveDisplayWindow(
				currentRawFrame,
				liveWindowCenter,
				liveWindowWidth,
				windowCenter,
				windowWidth,
				windowMode,
			)
			: windowCenter !== null && windowWidth !== null
				? { wc: windowCenter, ww: windowWidth }
				: activeFile?.default_window
					? { wc: activeFile.default_window.center, ww: activeFile.default_window.width }
					: { wc: 0, ww: 1 },
	);
	const activeAnnotations = $derived(activeFile ? annotationsByFile[activeFile.index] ?? null : null);
	const activeAnnotationError = $derived(activeFile ? annotationErrorsByFile[activeFile.index] ?? null : null);
	const activeAnnotationLoading = $derived(activeFile ? annotationLoadingByFile[activeFile.index] ?? false : false);
	const imageRows = $derived(
		pipelineMode === "diagnostic_wl" && currentRawFrame
			? currentRawFrame.metadata.rows
			: activeFile?.rows ?? 0,
	);
	const imageColumns = $derived(
		pipelineMode === "diagnostic_wl" && currentRawFrame
			? currentRawFrame.metadata.columns
			: activeFile?.columns ?? 0,
	);
	const visibleRois = $derived(deriveVisibleRois(activeAnnotations, currentFrame));
	const roiListCountLabel = $derived(
		activeAnnotations ? `${visibleRois.length} / ${activeAnnotations.num_roi}` : String(visibleRois.length),
	);

	function deriveVisibleRois(annotations: EmbedRoiAnnotations | null, frameIndex: number): VisibleRoi[] {
		if (!annotations || annotations.roi_coords.length === 0) return [];
		const appliesToAllFrames = annotations.roi_frames.length === 0;
		const visible: VisibleRoi[] = [];
		for (let idx = 0; idx < annotations.roi_coords.length; idx += 1) {
			const [ymin, xmin, ymax, xmax] = annotations.roi_coords[idx];
			const frames = appliesToAllFrames ? null : annotations.roi_frames[idx] ?? [];
			if (!appliesToAllFrames && !frames.includes(frameIndex)) continue;
			visible.push({ index: idx, ymin, xmin, ymax, xmax, frames });
		}
		return visible;
	}

	function formatRoiFrames(frames: number[] | null): string {
		if (frames === null) return "all frames";
		if (frames.length === 0) return "no frame mapping";
		const preview = frames.slice(0, 6).join(", ");
		return frames.length > 6 ? `frames ${preview}, …` : `frames ${preview}`;
	}

	function ensureWlWorker(): boolean {
		if (workerInitAttempted) {
			return workerAvailable;
		}
		workerInitAttempted = true;
		try {
			wlWorker = new Worker(new URL("./workers/wlRenderer.worker.ts", import.meta.url), { type: "module" });
			wlWorker.onmessage = (event: MessageEvent) => {
				const payload = event.data as
					| { type: "rendered"; id: number; width: number; height: number; rgba: ArrayBuffer }
					| { type: "error"; id: number; message: string };
				if (payload.type === "error") {
					const pending = pendingWorkerResponses.get(payload.id);
					if (!pending) return;
					pendingWorkerResponses.delete(payload.id);
					pending.reject(new Error(payload.message));
					return;
				}
				const pending = pendingWorkerResponses.get(payload.id);
				if (!pending) return;
				pendingWorkerResponses.delete(payload.id);
				pending.resolve({ width: payload.width, height: payload.height, rgba: payload.rgba });
			};
			wlWorker.onerror = () => {
				workerAvailable = false;
			};
			workerAvailable = true;
			return true;
		} catch {
			workerAvailable = false;
			wlWorker = null;
			return false;
		}
	}

	function shouldUseWorker(frame: RawFrame): boolean {
		const pixels = frame.metadata.rows * frame.metadata.columns;
		return pixels >= WORKER_MIN_PIXEL_THRESHOLD && ensureWlWorker();
	}

	async function renderWithWorker(frame: RawFrame, wc: number, ww: number): Promise<ImageData> {
		if (!wlWorker || !workerAvailable) {
			throw new Error("worker unavailable");
		}
		const id = ++workerMessageId;
		const copiedBuffer = frame.buffer.slice(0);
		const pending = new Promise<{ width: number; height: number; rgba: ArrayBuffer }>((resolve, reject) => {
			pendingWorkerResponses.set(id, { resolve, reject });
		});
		wlWorker.postMessage(
			{
				type: "render",
				id,
				metadata: frame.metadata,
				wc,
				ww,
				buffer: copiedBuffer,
			},
			[copiedBuffer],
		);
		const result = await pending;
		return new ImageData(new Uint8ClampedArray(result.rgba), result.width, result.height);
	}

	function clearCanvas(): void {
		if (!canvasEl) return;
		const ctx = canvasEl.getContext("2d", { alpha: false });
		if (!ctx) return;
		ctx.clearRect(0, 0, canvasEl.width, canvasEl.height);
	}

	function estimateRawFrameBytes(frame: RawFrame): number {
		return frame.buffer.byteLength;
	}

	function clearRawFrameCache(): void {
		rawFrameCache.clear();
		rawCacheBytes = 0;
	}

	function getCachedRawFrame(frameIndex: number): RawFrame | undefined {
		const cached = rawFrameCache.get(frameIndex);
		if (!cached) return undefined;
		rawFrameCache.delete(frameIndex);
		rawFrameCache.set(frameIndex, cached);
		return cached;
	}

	function deleteCachedRawFrame(frameIndex: number): void {
		const cached = rawFrameCache.get(frameIndex);
		if (!cached) return;
		rawFrameCache.delete(frameIndex);
		rawCacheBytes = Math.max(0, rawCacheBytes - estimateRawFrameBytes(cached));
	}

	function cacheRawFrame(frameIndex: number, frame: RawFrame): void {
		const incoming = estimateRawFrameBytes(frame);
		if (incoming > RAW_CACHE_BYTE_BUDGET) return;

		deleteCachedRawFrame(frameIndex);

		while (rawCacheBytes + incoming > RAW_CACHE_BYTE_BUDGET) {
			const oldestKey = rawFrameCache.keys().next().value as number | undefined;
			if (oldestKey === undefined) break;
			deleteCachedRawFrame(oldestKey);
		}

		if (rawCacheBytes + incoming > RAW_CACHE_BYTE_BUDGET) return;
		rawFrameCache.set(frameIndex, frame);
		rawCacheBytes += incoming;
	}

	function releaseDisplayEntry(entry: DisplayFrameCacheEntry): void {
		entry.bitmap?.close();
		entry.bitmap = null;
		entry.decodePromise = null;
	}

	function clearDisplayCache(): void {
		for (const entry of displayFrameCache.values()) {
			releaseDisplayEntry(entry);
		}
		displayFrameCache.clear();
		displayCacheBytes = 0;
	}

	function getCachedDisplayFrame(key: string): DisplayFrameCacheEntry | undefined {
		const cached = displayFrameCache.get(key);
		if (!cached) return undefined;
		displayFrameCache.delete(key);
		displayFrameCache.set(key, cached);
		return cached;
	}

	function deleteCachedDisplayFrame(key: string): void {
		const cached = displayFrameCache.get(key);
		if (!cached) return;
		displayFrameCache.delete(key);
		displayCacheBytes = Math.max(0, displayCacheBytes - cached.bytes);
		releaseDisplayEntry(cached);
	}

	function cacheDisplayFrame(key: string, blob: Blob): DisplayFrameCacheEntry | null {
		const incoming = blob.size;
		if (incoming > DISPLAY_CACHE_BYTE_BUDGET) return null;

		deleteCachedDisplayFrame(key);

		while (displayCacheBytes + incoming > DISPLAY_CACHE_BYTE_BUDGET) {
			const oldestKey = displayFrameCache.keys().next().value as string | undefined;
			if (!oldestKey) break;
			deleteCachedDisplayFrame(oldestKey);
		}

		if (displayCacheBytes + incoming > DISPLAY_CACHE_BYTE_BUDGET) return null;
		const entry: DisplayFrameCacheEntry = {
			blob,
			bytes: incoming,
			bitmap: null,
			decodePromise: null,
		};
		displayFrameCache.set(key, entry);
		displayCacheBytes += incoming;
		return entry;
	}

	function currentDisplayWindowOptions(): DisplayFrameWindowOptions {
		if (windowCenter !== null && windowWidth !== null) {
			return { wc: windowCenter, ww: windowWidth, windowMode: "default" };
		}
		if (windowMode === "full_dynamic") {
			return { windowMode: "full_dynamic" };
		}
		return {};
	}

	function buildDisplayKey(fileIndex: number, frameIndex: number, options: DisplayFrameWindowOptions): string {
		return `${displayFrameCacheKey(fileIndex, frameIndex, options)}:preset:${selectedPresetId}`;
	}

function displayPrefetchScope(fileIndex: number, options: DisplayFrameWindowOptions): string {
	const wc = options.wc === null || options.wc === undefined ? "none" : options.wc.toFixed(4);
	const ww = options.ww === null || options.ww === undefined ? "none" : options.ww.toFixed(4);
	const mode = options.windowMode ?? "default";
	return `${fileIndex}:${selectedPresetId}:${mode}:${wc}:${ww}`;
}

	function validateRenderableRawFrame(frame: RawFrame): string | null {
		const { rows, columns, bitsAllocated, samplesPerPixel } = frame.metadata;
		if (rows <= 0 || columns <= 0) {
			return "Invalid raw frame dimensions";
		}
		if (samplesPerPixel !== 1) {
			return `Unsupported SamplesPerPixel: ${samplesPerPixel}`;
		}
		if (bitsAllocated !== 8 && bitsAllocated !== 16) {
			return `Unsupported BitsAllocated for viewport: ${bitsAllocated}`;
		}
		const numPixels = rows * columns;
		if (!Number.isFinite(numPixels) || numPixels <= 0) {
			return "Invalid raw frame pixel count";
		}
		if (numPixels > MAX_RENDER_PIXELS) {
			return `Frame too large to render safely (${rows}×${columns})`;
		}
		const minExpectedBytes = numPixels * (bitsAllocated / 8);
		if (frame.buffer.byteLength < minExpectedBytes) {
			return "Raw frame buffer is shorter than expected for declared metadata";
		}
		return null;
	}

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

		let minRaw: number;
		let size: number;
		if (bitsAllocated === 8) {
			minRaw = 0;
			size = 256;
		} else if (pixelRepresentation === 1) {
			minRaw = -32768;
			size = 65536;
		} else {
			minRaw = 0;
			size = 65536;
		}

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

	function renderRawFrameOnMainThread(
		canvas: HTMLCanvasElement,
		frame: RawFrame,
		wc: number,
		ww: number,
	): void {
		const { rows, columns, bitsAllocated, pixelRepresentation, rescaleSlope, rescaleIntercept, photometricInterpretation } = frame.metadata;
		canvas.width = columns;
		canvas.height = rows;
		const ctx = canvas.getContext("2d", { alpha: false });
		if (!ctx) return;
		const invert = photometricInterpretation === "MONOCHROME1";
		const lut = buildLut(bitsAllocated, pixelRepresentation, rescaleSlope, rescaleIntercept, wc, Math.max(ww, 1), invert);
		const numPixels = rows * columns;
		const imageData = ctx.createImageData(columns, rows);
		const rgba = imageData.data;

		if (bitsAllocated === 8) {
			const view = new Uint8Array(frame.buffer);
			for (let i = 0; i < numPixels; i++) {
				const g = lut[view[i]];
				const o = i * 4;
				rgba[o] = g;
				rgba[o + 1] = g;
				rgba[o + 2] = g;
				rgba[o + 3] = 255;
			}
		} else if (pixelRepresentation === 1) {
			const view = new Int16Array(frame.buffer);
			for (let i = 0; i < numPixels; i++) {
				const g = lut[view[i] + 32768];
				const o = i * 4;
				rgba[o] = g;
				rgba[o + 1] = g;
				rgba[o + 2] = g;
				rgba[o + 3] = 255;
			}
		} else {
			const view = new Uint16Array(frame.buffer);
			for (let i = 0; i < numPixels; i++) {
				const g = lut[view[i]];
				const o = i * 4;
				rgba[o] = g;
				rgba[o + 1] = g;
				rgba[o + 2] = g;
				rgba[o + 3] = 255;
			}
		}
		ctx.putImageData(imageData, 0, 0);
	}

	async function renderDiagnosticFrame(frame: RawFrame, wc: number, ww: number, generation: number): Promise<void> {
		if (!canvasEl) return;
		if (shouldUseWorker(frame)) {
			try {
				const imageData = await renderWithWorker(frame, wc, ww);
				if (generation !== wlRenderGeneration || !canvasEl || pipelineMode !== "diagnostic_wl") return;
				canvasEl.width = imageData.width;
				canvasEl.height = imageData.height;
				const ctx = canvasEl.getContext("2d", { alpha: false });
				ctx?.putImageData(imageData, 0, 0);
				return;
			} catch {
				workerAvailable = false;
			}
		}
		renderRawFrameOnMainThread(canvasEl, frame, wc, ww);
	}

	async function drawDisplayEntry(entry: DisplayFrameCacheEntry, generation: number): Promise<void> {
		if (!canvasEl || pipelineMode !== "cine") return;
		const ctx = canvasEl.getContext("2d", { alpha: false });
		if (!ctx) return;

		if (typeof createImageBitmap === "function") {
			if (!entry.bitmap) {
				if (!entry.decodePromise) {
					entry.decodePromise = createImageBitmap(entry.blob).then((bitmap) => {
						entry.bitmap = bitmap;
						return bitmap;
					});
				}
				await entry.decodePromise;
				entry.decodePromise = null;
			}
			if (generation !== requestGeneration || !canvasEl || !entry.bitmap || pipelineMode !== "cine") return;
			canvasEl.width = entry.bitmap.width;
			canvasEl.height = entry.bitmap.height;
			ctx.drawImage(entry.bitmap, 0, 0);
			return;
		}

		const fallbackUrl = URL.createObjectURL(entry.blob);
		try {
			const img = new Image();
			img.decoding = "async";
			const loaded = new Promise<void>((resolve, reject) => {
				img.onload = () => resolve();
				img.onerror = () => reject(new Error("display image decode failed"));
			});
			img.src = fallbackUrl;
			await loaded;
			if (generation !== requestGeneration || !canvasEl || pipelineMode !== "cine") return;
			canvasEl.width = img.naturalWidth;
			canvasEl.height = img.naturalHeight;
			ctx.drawImage(img, 0, 0);
		} finally {
			URL.revokeObjectURL(fallbackUrl);
		}
	}

	function resolveDisplayWindow(
		frame: RawFrame,
		liveWc: number | null,
		liveWw: number | null,
		wc: number | null,
		ww: number | null,
		mode: WindowMode,
	): { wc: number; ww: number } {
		if (mode === "full_dynamic") {
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
			return { wc: defaultWc, ww: defaultWw };
		}
		return computePercentileWindow(frame);
	}

	function computeFullDynamicWindow(frame: RawFrame): { wc: number; ww: number } {
		const { bitsAllocated, pixelRepresentation, rescaleSlope, rescaleIntercept, rows, columns } = frame.metadata;
		const numPixels = rows * columns;
		let min = Infinity;
		let max = -Infinity;
		if (bitsAllocated === 8) {
			const view = new Uint8Array(frame.buffer);
			for (let i = 0; i < numPixels; i++) {
				const v = view[i] * rescaleSlope + rescaleIntercept;
				if (v < min) min = v;
				if (v > max) max = v;
			}
		} else if (pixelRepresentation === 1) {
			const view = new Int16Array(frame.buffer);
			for (let i = 0; i < numPixels; i++) {
				const v = view[i] * rescaleSlope + rescaleIntercept;
				if (v < min) min = v;
				if (v > max) max = v;
			}
		} else {
			const view = new Uint16Array(frame.buffer);
			for (let i = 0; i < numPixels; i++) {
				const v = view[i] * rescaleSlope + rescaleIntercept;
				if (v < min) min = v;
				if (v > max) max = v;
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

	function buildDirectionalFrameOrder(
		centerFrame: number,
		totalFrames: number,
		maxDistance: number,
		direction: 1 | -1,
	): number[] {
		const result: number[] = [];
		const distanceCap = Math.min(totalFrames - 1, maxDistance);
		for (let delta = 1; delta <= distanceCap; delta++) {
			const preferred = centerFrame + delta * direction;
			const secondary = centerFrame - delta * direction;
			if (preferred >= 0 && preferred < totalFrames) result.push(preferred);
			if (secondary >= 0 && secondary < totalFrames) result.push(secondary);
		}
		return result;
	}

	function trimRawCacheToRing(centerFrame: number, totalFrames: number): void {
		const minFrame = Math.max(0, centerFrame - RAW_RING_RADIUS);
		const maxFrame = Math.min(totalFrames - 1, centerFrame + RAW_RING_RADIUS);
		for (const cachedFrame of [...rawFrameCache.keys()]) {
			if (cachedFrame < minFrame || cachedFrame > maxFrame) {
				deleteCachedRawFrame(cachedFrame);
			}
		}
	}

	function shouldPrefetchWholeDisplayStack(totalFrames: number, frameBytes: number): boolean {
		if (totalFrames <= 1 || frameBytes <= 0) return false;
		return totalFrames * frameBytes <= DISPLAY_FULL_PREFETCH_BUDGET_BYTES;
	}

	async function runRawRingPrefetch(
		fileIndex: number,
		totalFrames: number,
		centerFrame: number,
		direction: 1 | -1,
		signal: AbortSignal,
	): Promise<void> {
		trimRawCacheToRing(centerFrame, totalFrames);
		const targets = buildDirectionalFrameOrder(centerFrame, totalFrames, RAW_RING_RADIUS, direction);
		for (let i = 0; i < targets.length && !signal.aborted; i += PREFETCH_CONCURRENCY) {
			const batch = targets.slice(i, i + PREFETCH_CONCURRENCY).filter((frameIndex) => !rawFrameCache.has(frameIndex));
			if (batch.length === 0) continue;
			await Promise.allSettled(
				batch.map(async (frameIndex) => {
					if (signal.aborted || rawFrameCache.has(frameIndex)) return;
					try {
						const rawFrame = await fetchRawFrame(fileIndex, frameIndex, signal);
						if (validateRenderableRawFrame(rawFrame) !== null) return;
						cacheRawFrame(frameIndex, rawFrame);
					} catch {
						// Ignore network/decode failures during prefetch.
					}
				}),
			);
		}
		trimRawCacheToRing(centerFrame, totalFrames);
	}

	async function runDisplayPrefetch(
		fileIndex: number,
		totalFrames: number,
		startFrame: number,
		direction: 1 | -1,
		windowOptions: DisplayFrameWindowOptions,
		signal: AbortSignal,
		currentBlobSize: number,
	): Promise<void> {
		const fullVolume = shouldPrefetchWholeDisplayStack(totalFrames, currentBlobSize);
		const maxDistance = fullVolume ? totalFrames - 1 : DISPLAY_NEAR_PREFETCH_DISTANCE;
		const targets = buildDirectionalFrameOrder(startFrame, totalFrames, maxDistance, direction);
		for (let i = 0; i < targets.length && !signal.aborted; i += PREFETCH_CONCURRENCY) {
			const batch = targets.slice(i, i + PREFETCH_CONCURRENCY);
			await Promise.allSettled(
				batch.map(async (frameIndex) => {
					const key = buildDisplayKey(fileIndex, frameIndex, windowOptions);
					if (signal.aborted || displayFrameCache.has(key)) return;
					try {
						const blob = await fetchDisplayFrameBlob(fileIndex, frameIndex, windowOptions, signal);
						cacheDisplayFrame(key, blob);
					} catch {
						// Ignore network/decode failures during prefetch.
					}
				}),
			);
		}
	}

function startDisplayPrefetch(
	fileIndex: number,
	totalFrames: number,
	frameIndex: number,
	direction: 1 | -1,
	windowOptions: DisplayFrameWindowOptions,
	currentBlobSize: number,
): void {
	const scopeKey = displayPrefetchScope(fileIndex, windowOptions);
	const shouldReusePrefetch =
		displayPrefetchCtrl !== null &&
		!displayPrefetchCtrl.signal.aborted &&
		displayPrefetchScopeKey === scopeKey &&
		displayPrefetchSeedFrame !== null &&
		Math.abs(frameIndex - displayPrefetchSeedFrame) <= PREFETCH_RESEED_DISTANCE;
	if (shouldReusePrefetch) return;

	displayPrefetchCtrl?.abort();
	const ctrl = new AbortController();
	displayPrefetchCtrl = ctrl;
	displayPrefetchScopeKey = scopeKey;
	displayPrefetchSeedFrame = frameIndex;

	void runDisplayPrefetch(
		fileIndex,
		totalFrames,
		frameIndex,
		direction,
		windowOptions,
		ctrl.signal,
		currentBlobSize,
	).finally(() => {
		if (displayPrefetchCtrl === ctrl) {
			displayPrefetchCtrl = null;
			displayPrefetchScopeKey = "";
			displayPrefetchSeedFrame = null;
		}
	});
}

	async function loadRawFrameAndRender(
		fileIndex: number,
		frameIndex: number,
		generation: number,
		direction: 1 | -1,
	): Promise<void> {
		const cached = getCachedRawFrame(frameIndex);
		if (cached) {
			currentRawFrame = cached;
			loading = false;
			loadError = null;
			rawPrefetchCtrl?.abort();
			rawPrefetchCtrl = new AbortController();
			void runRawRingPrefetch(fileIndex, activeFile.frame_count, frameIndex, direction, rawPrefetchCtrl.signal);
			return;
		}

		rawRequestCtrl?.abort();
		rawRequestCtrl = new AbortController();
		const ctrl = rawRequestCtrl;

		try {
			const rawFrame = await fetchRawFrame(fileIndex, frameIndex, ctrl.signal);
			if (ctrl.signal.aborted || generation !== requestGeneration || pipelineMode !== "diagnostic_wl") return;
			const validationError = validateRenderableRawFrame(rawFrame);
			if (validationError) {
				currentRawFrame = null;
				loading = false;
				loadError = validationError;
				return;
			}
			cacheRawFrame(frameIndex, rawFrame);
			trimRawCacheToRing(frameIndex, activeFile.frame_count);
			currentRawFrame = rawFrame;
			loading = false;
			loadError = null;

			rawPrefetchCtrl?.abort();
			rawPrefetchCtrl = new AbortController();
			void runRawRingPrefetch(fileIndex, activeFile.frame_count, frameIndex, direction, rawPrefetchCtrl.signal);
		} catch (error) {
			if ((error as Error).name === "AbortError") return;
			if (generation !== requestGeneration || pipelineMode !== "diagnostic_wl") return;
			loading = false;
			loadError = (error as Error).message || "Failed to load frame";
		} finally {
			if (rawRequestCtrl === ctrl) rawRequestCtrl = null;
		}
	}

	async function loadDisplayFrameAndRender(
		fileIndex: number,
		frameIndex: number,
		generation: number,
		direction: 1 | -1,
	): Promise<void> {
		const windowOptions = currentDisplayWindowOptions();
		const cacheKey = buildDisplayKey(fileIndex, frameIndex, windowOptions);
		const cached = getCachedDisplayFrame(cacheKey);
		if (cached) {
			currentDisplayCacheKey = cacheKey;
			loading = false;
			loadError = null;
			await drawDisplayEntry(cached, generation);
			startDisplayPrefetch(
				fileIndex,
				activeFile.frame_count,
				frameIndex,
				direction,
				windowOptions,
				cached.bytes,
			);
			return;
		}

		displayRequestCtrl?.abort();
		displayRequestCtrl = new AbortController();
		const ctrl = displayRequestCtrl;

		try {
			const blob = await fetchDisplayFrameBlob(fileIndex, frameIndex, windowOptions, ctrl.signal);
			if (ctrl.signal.aborted || generation !== requestGeneration || pipelineMode !== "cine") return;
			const entry = cacheDisplayFrame(cacheKey, blob);
			if (!entry) {
				loading = false;
				loadError = "Display frame exceeded cache budget";
				return;
			}
			currentDisplayCacheKey = cacheKey;
			loading = false;
			loadError = null;
			await drawDisplayEntry(entry, generation);

			startDisplayPrefetch(
				fileIndex,
				activeFile.frame_count,
				frameIndex,
				direction,
				windowOptions,
				blob.size,
			);
		} catch (error) {
			if ((error as Error).name === "AbortError") return;
			if (generation !== requestGeneration || pipelineMode !== "cine") return;
			loading = false;
			loadError = (error as Error).message || "Failed to load frame";
		} finally {
			if (displayRequestCtrl === ctrl) displayRequestCtrl = null;
		}
	}

	function updateTransform(index: number, transform: TransformState) {
		transformsByFile = {
			...transformsByFile,
			[index]: transform,
		};
	}

	$effect(() => {
		const current = currentFrame;
		if (current > lastFrameForDirection) frameDirection = 1;
		if (current < lastFrameForDirection) frameDirection = -1;
		lastFrameForDirection = current;
	});

	$effect(() => {
		if (activeFile && !transformsByFile[activeFile.index]) {
			transformsByFile = {
				...transformsByFile,
				[activeFile.index]: { scale: 1, tx: 0, ty: 0 },
			};
		}
	});

	$effect(() => {
		if (!activeFile) return;
		const fileIndex = activeFile.index;
		if (annotationsByFile[fileIndex] !== undefined || annotationRequestedByFile[fileIndex]) {
			return;
		}

		// Direct mutation — annotationRequestedByFile is not $state, so this
		// does not trigger an effect re-run and will not fire the cleanup.
		annotationRequestedByFile[fileIndex] = true;
		annotationLoadingByFile = {
			...annotationLoadingByFile,
			[fileIndex]: true,
		};
		annotationErrorsByFile = {
			...annotationErrorsByFile,
			[fileIndex]: null,
		};

		void fetchAnnotations(fileIndex)
			.then((annotations) => {
				annotationsByFile = {
					...annotationsByFile,
					[fileIndex]: annotations,
				};
			})
			.catch((error) => {
				annotationErrorsByFile = {
					...annotationErrorsByFile,
					[fileIndex]: (error as Error).message || "Failed to load annotations",
				};
			})
			.finally(() => {
				annotationLoadingByFile = {
					...annotationLoadingByFile,
					[fileIndex]: false,
				};
			});
	});

	$effect(() => {
		if (!activeFile) return;
		const nextScope = String(activeFile.index);
		if (nextScope === fileScopeKey) return;
		fileScopeKey = nextScope;
		rawRequestCtrl?.abort();
		rawPrefetchCtrl?.abort();
		displayRequestCtrl?.abort();
		displayPrefetchCtrl?.abort();
		displayPrefetchCtrl = null;
		displayPrefetchScopeKey = "";
		displayPrefetchSeedFrame = null;
		clearRawFrameCache();
		clearDisplayCache();
		currentRawFrame = null;
		currentDisplayCacheKey = null;
		liveWindowCenter = null;
		liveWindowWidth = null;
		clearCanvas();
	});

	$effect(() => {
		const mode = pipelineMode;
		requestGeneration += 1;
		if (mode === "cine") {
			rawRequestCtrl?.abort();
			rawRequestCtrl = null;
			rawPrefetchCtrl?.abort();
			rawPrefetchCtrl = null;
			liveWindowCenter = null;
			liveWindowWidth = null;
		} else {
			displayRequestCtrl?.abort();
			displayRequestCtrl = null;
			displayPrefetchCtrl?.abort();
			displayPrefetchCtrl = null;
			displayPrefetchScopeKey = "";
			displayPrefetchSeedFrame = null;
			currentDisplayCacheKey = null;
		}
	});

	$effect(() => {
		if (!activeFile?.has_pixels) {
			currentRawFrame = null;
			currentDisplayCacheKey = null;
			loading = false;
			loadError = null;
			clearCanvas();
			return;
		}

		const mode = pipelineMode;
		const fileIndex = activeFile.index;
		const frameIndex = currentFrame;
		const generation = ++requestGeneration;
		const modeWc = mode === "cine" ? windowCenter : null;
		const modeWw = mode === "cine" ? windowWidth : null;
		const modePreset = mode === "cine" ? selectedPresetId : "";
		const modeWindowMode = mode === "cine" ? windowMode : "default";
		void modeWc;
		void modeWw;
		void modePreset;
		void modeWindowMode;

		loading = true;
		loadError = null;
		if (mode === "cine") {
			void loadDisplayFrameAndRender(fileIndex, frameIndex, generation, frameDirection);
		} else {
			void loadRawFrameAndRender(fileIndex, frameIndex, generation, frameDirection);
		}
	});

	$effect(() => {
		if (pipelineMode !== "diagnostic_wl" || !currentRawFrame || !canvasEl) return;
		const window = resolveDisplayWindow(
			currentRawFrame,
			liveWindowCenter,
			liveWindowWidth,
			windowCenter,
			windowWidth,
			windowMode,
		);
		const generation = ++wlRenderGeneration;
		void renderDiagnosticFrame(currentRawFrame, window.wc, window.ww, generation);
	});

	$effect(() => {
		if (pipelineMode !== "cine" || !canvasEl || !currentDisplayCacheKey) return;
		const entry = getCachedDisplayFrame(currentDisplayCacheKey);
		if (!entry) return;
		const generation = requestGeneration;
		void drawDisplayEntry(entry, generation);
	});

	$effect(() => {
		return () => {
			rawRequestCtrl?.abort();
			rawPrefetchCtrl?.abort();
			displayRequestCtrl?.abort();
			displayPrefetchCtrl?.abort();
			displayPrefetchCtrl = null;
			displayPrefetchScopeKey = "";
			displayPrefetchSeedFrame = null;
			clearRawFrameCache();
			clearDisplayCache();
			for (const pending of pendingWorkerResponses.values()) {
				pending.reject(new Error("viewport disposed"));
			}
			pendingWorkerResponses.clear();
			wlWorker?.terminate();
			wlWorker = null;
		};
	});

	$effect(() => {
		if (resetCount === lastHandledResetCount) return;
		lastHandledResetCount = resetCount;
		if (resetCount === 0) return;
		if (activeFile) {
			updateTransform(activeFile.index, { scale: 1, tx: 0, ty: 0 });
		}
		liveWindowCenter = null;
		liveWindowWidth = null;
		dragState = null;
		wheelAccum = 0;
	});

	$effect(() => {
		activeFileIndex;
		wheelAccum = 0;
	});

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
			const delta = event.deltaMode === 0 ? -event.deltaY * 0.01 : event.deltaY < 0 ? 0.05 : -0.05;
			zoomAt(activeTransform.scale + delta, event.clientX, event.clientY);
			return;
		}

		if (activeFile.frame_count > 1) {
			if (event.deltaMode !== 0) {
				if (event.deltaY > 0) currentFrame = Math.min(activeFile.frame_count - 1, currentFrame + 1);
				else if (event.deltaY < 0) currentFrame = Math.max(0, currentFrame - 1);
			} else {
				wheelAccum += event.deltaY;
				const threshold = WHEEL_FRAME_THRESHOLD;
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

		if (event.deltaMode !== 0) {
			const delta = event.deltaY < 0 ? 0.05 : -0.05;
			zoomAt(activeTransform.scale + delta, event.clientX, event.clientY);
		} else {
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
			switch (activeTool) {
				case "window_level": {
					if (pipelineMode !== "diagnostic_wl" || !currentRawFrame) {
						break;
					}
					const baseWindow = resolveDisplayWindow(
						currentRawFrame,
						liveWindowCenter,
						liveWindowWidth,
						windowCenter,
						windowWidth,
						windowMode,
					);
					dragState = {
						mode: "wl",
						startX: event.clientX,
						startY: event.clientY,
						baseCenter: baseWindow.wc,
						baseWidth: baseWindow.ww,
					};
					liveWindowCenter = baseWindow.wc;
					liveWindowWidth = baseWindow.ww;
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
			const frameDelta = Math.round(dy / DRAG_PIXELS_PER_FRAME);
			currentFrame = Math.max(0, Math.min(activeFile.frame_count - 1, dragState.baseFrame + frameDelta));
		}
	}

	function onPointerUp(event: PointerEvent) {
		(event.currentTarget as HTMLElement).releasePointerCapture(event.pointerId);
		if (dragState?.mode === "wl" && liveWindowCenter !== null && liveWindowWidth !== null) {
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
			const next = ZOOM_STEPS.find((step) => step > current + 0.001);
			if (next !== undefined) zoomToLevel(next);
		} else {
			const previous = [...ZOOM_STEPS].reverse().find((step) => step < current - 0.001);
			if (previous !== undefined) zoomToLevel(previous);
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
		<div
			class="image-layer"
			style={`transform:${transformCss}; width:${Math.max(imageColumns, 1)}px; height:${Math.max(imageRows, 1)}px;`}
		>
			<canvas bind:this={canvasEl} class="dicom-canvas"></canvas>
			{#if imageColumns > 0 && imageRows > 0 && visibleRois.length > 0}
				<svg
					class="roi-overlay"
					viewBox={`0 0 ${imageColumns} ${imageRows}`}
					preserveAspectRatio="none"
					aria-hidden="true"
				>
					{#each visibleRois as roi (roi.index)}
						<rect
							x={Math.min(roi.xmin, roi.xmax)}
							y={Math.min(roi.ymin, roi.ymax)}
							width={Math.max(1, Math.abs(roi.xmax - roi.xmin))}
							height={Math.max(1, Math.abs(roi.ymax - roi.ymin))}
						></rect>
					{/each}
				</svg>
			{/if}
		</div>
		<div class="overlay">
			<span>frame {currentFrame + 1} / {activeFile.frame_count}</span>
			<span>W: {Math.round(displayWindow.ww)} · C: {Math.round(displayWindow.wc)}</span>
		</div>
		<div class="roi-list">
			<div class="roi-list-title">ROIs {roiListCountLabel}</div>
			{#if activeAnnotationLoading}
				<div class="roi-list-status">Loading annotations…</div>
			{:else if activeAnnotationError}
				<div class="roi-list-status error">{activeAnnotationError}</div>
			{:else if visibleRois.length === 0}
				<div class="roi-list-status">No ROIs for this frame</div>
			{:else}
				<ul>
					{#each visibleRois as roi (roi.index)}
						<li>
							<span class="roi-id">#{roi.index + 1}</span>
							<span class="roi-coords">[{roi.ymin}, {roi.xmin}, {roi.ymax}, {roi.xmax}]</span>
							<span class="roi-frames">{formatRoiFrames(roi.frames)}</span>
						</li>
					{/each}
				</ul>
			{/if}
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
	.image-layer {
		position: relative;
		max-width: 100%;
		max-height: 100%;
		transform-origin: 0 0;
		transition: transform 0.03s linear;
	}
	.dicom-canvas {
		display: block;
		width: 100%;
		height: 100%;
		image-rendering: pixelated;
	}
	.roi-overlay {
		position: absolute;
		inset: 0;
		width: 100%;
		height: 100%;
		pointer-events: none;
	}
	.roi-overlay rect {
		fill: rgba(255, 115, 115, 0.12);
		stroke: #ff7373;
		stroke-width: 1.2;
		vector-effect: non-scaling-stroke;
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
	.roi-list {
		position: absolute;
		right: 0.75rem;
		top: 0.75rem;
		max-width: min(48ch, 46%);
		max-height: 38%;
		overflow: auto;
		font-size: 0.72rem;
		padding: 0.45rem 0.5rem;
		background: rgba(18, 18, 18, 0.82);
		border: 1px solid #333;
		border-radius: 6px;
		z-index: 2;
	}
	.roi-list-title {
		font-weight: 600;
		margin-bottom: 0.25rem;
		color: #c8ddff;
	}
	.roi-list-status {
		color: #a8a8a8;
	}
	.roi-list-status.error {
		color: #ff9c9c;
	}
	.roi-list ul {
		margin: 0;
		padding: 0;
		list-style: none;
		display: grid;
		gap: 0.2rem;
	}
	.roi-list li {
		display: grid;
		gap: 0.1rem;
		padding-top: 0.1rem;
		border-top: 1px dashed rgba(110, 110, 110, 0.5);
	}
	.roi-list li:first-child {
		border-top: none;
		padding-top: 0;
	}
	.roi-id {
		font-weight: 600;
		color: #9fcbff;
	}
	.roi-coords,
	.roi-frames {
		font-family: ui-monospace, monospace;
		line-height: 1.25;
		color: #d8d8d8;
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

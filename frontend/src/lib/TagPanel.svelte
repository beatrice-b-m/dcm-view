<script lang="ts">
	import { fetchTags, type FileSummary, type TagNode, type TagValue } from "../api";

	type FlatRow = {
		key: string;
		node: TagNode;
		depth: number;
	};

	type ColumnKey = "tag" | "keyword" | "vr";

	type ColumnResizeState = {
		pointerId: number;
		column: ColumnKey;
		startX: number;
		startWidth: number;
	};

	const TAG_COLUMN_DEFAULT_PX = 128;
	const KEYWORD_COLUMN_DEFAULT_PX = 136;
	const VR_COLUMN_DEFAULT_PX = 64;

	const TAG_COLUMN_MIN_PX = 88;
	const TAG_COLUMN_MAX_PX = 260;
	const KEYWORD_COLUMN_MIN_PX = 100;
	const KEYWORD_COLUMN_MAX_PX = 320;
	const VR_COLUMN_MIN_PX = 52;
	const VR_COLUMN_MAX_PX = 140;

	let { files, activeFileIndex }: { files: FileSummary[]; activeFileIndex: number } = $props();

	let filter = $state("");
	let tagsByFile = $state<Record<number, TagNode[]>>({});
	let loading = $state(false);
	let error = $state<string | null>(null);
	let expandedSequences = $state<Set<string>>(new Set());
	let expandedLongValues = $state<Set<string>>(new Set());
	let copiedKey = $state<string | null>(null);
	let tagColumnWidthPx = $state(TAG_COLUMN_DEFAULT_PX);
	let keywordColumnWidthPx = $state(KEYWORD_COLUMN_DEFAULT_PX);
	let vrColumnWidthPx = $state(VR_COLUMN_DEFAULT_PX);
	let columnResizeState = $state<ColumnResizeState | null>(null);

	const tableColumns = $derived(
		`${tagColumnWidthPx}px ${keywordColumnWidthPx}px ${vrColumnWidthPx}px minmax(0, 1fr)`,
	);

	$effect(() => {
		void ensureTags(activeFileIndex);
	});

	async function ensureTags(index: number) {
		if (tagsByFile[index]) {
			return;
		}
		loading = true;
		error = null;
		try {
			tagsByFile[index] = await fetchTags(index);
		} catch (err) {
			error = err instanceof Error ? err.message : String(err);
		} finally {
			loading = false;
		}
	}

	function toggleSequence(key: string) {
		const next = new Set(expandedSequences);
		if (next.has(key)) {
			next.delete(key);
		} else {
			next.add(key);
		}
		expandedSequences = next;
	}

	function toggleLongValue(key: string) {
		const next = new Set(expandedLongValues);
		if (next.has(key)) {
			next.delete(key);
		} else {
			next.add(key);
		}
		expandedLongValues = next;
	}

	function getColumnWidth(column: ColumnKey): number {
		switch (column) {
			case "tag":
				return tagColumnWidthPx;
			case "keyword":
				return keywordColumnWidthPx;
			case "vr":
				return vrColumnWidthPx;
		}
	}

	function clampColumnWidth(column: ColumnKey, width: number): number {
		switch (column) {
			case "tag":
				return Math.min(TAG_COLUMN_MAX_PX, Math.max(TAG_COLUMN_MIN_PX, width));
			case "keyword":
				return Math.min(KEYWORD_COLUMN_MAX_PX, Math.max(KEYWORD_COLUMN_MIN_PX, width));
			case "vr":
				return Math.min(VR_COLUMN_MAX_PX, Math.max(VR_COLUMN_MIN_PX, width));
		}
	}

	function setColumnWidth(column: ColumnKey, width: number) {
		if (column === "tag") {
			tagColumnWidthPx = width;
			return;
		}
		if (column === "keyword") {
			keywordColumnWidthPx = width;
			return;
		}
		vrColumnWidthPx = width;
	}

	function startColumnResize(column: ColumnKey, event: PointerEvent) {
		if (event.button !== 0) {
			return;
		}

		const handle = event.currentTarget as HTMLElement;
		handle.setPointerCapture(event.pointerId);
		columnResizeState = {
			pointerId: event.pointerId,
			column,
			startX: event.clientX,
			startWidth: getColumnWidth(column),
		};
		event.preventDefault();
	}

	function moveColumnResize(event: PointerEvent) {
		if (!columnResizeState || columnResizeState.pointerId !== event.pointerId) {
			return;
		}

		const delta = event.clientX - columnResizeState.startX;
		const nextWidth = clampColumnWidth(
			columnResizeState.column,
			columnResizeState.startWidth + delta,
		);
		setColumnWidth(columnResizeState.column, nextWidth);
	}

	function endColumnResize(event: PointerEvent) {
		const handle = event.currentTarget as HTMLElement;
		if (handle.hasPointerCapture(event.pointerId)) {
			handle.releasePointerCapture(event.pointerId);
		}

		if (columnResizeState?.pointerId === event.pointerId) {
			columnResizeState = null;
		}
	}

	function cancelColumnResize() {
		columnResizeState = null;
	}

	async function copyRow(row: FlatRow) {
		const text = `${row.node.tag}  ${row.node.keyword}  =  ${valueToCopyText(row.node.value)}`;
		try {
			await navigator.clipboard.writeText(text);
			copiedKey = row.key;
			setTimeout(() => {
				if (copiedKey === row.key) {
					copiedKey = null;
				}
			}, 1500);
		} catch {
			copiedKey = null;
		}
	}

	const visibleRows = $derived.by(() => {
		const source = tagsByFile[activeFileIndex] ?? [];
		const rows: FlatRow[] = [];
		flattenRows(source, `f${activeFileIndex}`, 0, rows, filter.trim().toLowerCase());
		return rows;
	});

	function flattenRows(
		nodes: TagNode[],
		prefix: string,
		depth: number,
		out: FlatRow[],
		needle: string,
	) {
		nodes.forEach((node, index) => {
			const key = `${prefix}-${index}`;
			const nodeMatches = matchesNeedle(node, needle);
			const descendantMatches =
				node.value.type === "sequence" ? sequenceHasNeedle(node.value.items, needle) : false;

			if (!needle || nodeMatches || descendantMatches) {
				out.push({ key, node, depth });
			}

			if (node.value.type === "sequence" && expandedSequences.has(key)) {
				node.value.items.forEach((item, itemIndex) => {
					flattenRows(item, `${key}:item${itemIndex}`, depth + 1, out, needle);
				});
			}
		});
	}

	function sequenceHasNeedle(items: TagNode[][], needle: string): boolean {
		if (!needle) {
			return true;
		}
		return items.some((item) => item.some((node) => matchesNeedle(node, needle) || (node.value.type === "sequence" && sequenceHasNeedle(node.value.items, needle))));
	}

	function matchesNeedle(node: TagNode, needle: string): boolean {
		if (!needle) {
			return true;
		}
		const haystack = `${node.tag} ${node.keyword} ${node.vr} ${valuePreview(node.value)}`.toLowerCase();
		return haystack.includes(needle);
	}

	function valuePreview(value: TagValue): string {
		switch (value.type) {
			case "string":
				return value.value;
			case "number":
				return String(value.value);
			case "numbers":
				return value.value.join(", ");
			case "binary":
				return `${value.length} bytes`;
			case "sequence":
				return `${value.items.length} item(s)`;
			case "error":
				return value.message;
		}
	}

	function valueToCopyText(value: TagValue): string {
		switch (value.type) {
			case "binary":
				return `[binary: ${value.length} bytes]`;
			case "sequence":
				return `[sequence: ${value.items.length} item(s)]`;
			case "numbers":
				return value.value.join(", ");
			case "number":
				return String(value.value);
			case "string":
				return value.value;
			case "error":
				return `error: ${value.message}`;
		}
	}

	function isSequence(node: TagNode): boolean {
		return node.value.type === "sequence";
	}

	function valueDisplay(row: FlatRow): string {
		const value = row.node.value;
		switch (value.type) {
			case "string": {
				if (value.value.length > 80 && !expandedLongValues.has(row.key)) {
					return `${value.value.slice(0, 80)}…`;
				}
				return value.value;
			}
			case "number":
				return String(value.value);
			case "numbers":
				return value.value.join(", ");
			case "binary":
				return `[${row.node.vr} · ${value.length.toLocaleString()} bytes]`;
			case "sequence":
				return `[SQ · ${value.items.length} item(s)]`;
			case "error":
				return `[error] ${value.message}`;
		}
	}
</script>

<aside class="panel">
	<header>
		<h2>DICOM Tags</h2>
		<input bind:value={filter} placeholder="filter tags..." />
	</header>
	{#if error}
		<p class="error">{error}</p>
	{:else if loading}
		<p class="loading">Loading tags…</p>
	{:else}
		<div class="table" style={`--tag-grid-columns:${tableColumns};`}>
			<div class="header-row row-grid" role="row">
				<div class="header-cell resizable">
					<span>Tag</span>
					<button
						type="button"
						class="column-resizer"
						class:dragging={columnResizeState?.column === "tag"}
						aria-label="Resize tag column"
						onpointerdown={(event) => startColumnResize("tag", event)}
						onpointermove={moveColumnResize}
						onpointerup={endColumnResize}
						onpointercancel={cancelColumnResize}
					></button>
				</div>
				<div class="header-cell resizable">
					<span>Keyword</span>
					<button
						type="button"
						class="column-resizer"
						class:dragging={columnResizeState?.column === "keyword"}
						aria-label="Resize keyword column"
						onpointerdown={(event) => startColumnResize("keyword", event)}
						onpointermove={moveColumnResize}
						onpointerup={endColumnResize}
						onpointercancel={cancelColumnResize}
					></button>
				</div>
				<div class="header-cell resizable">
					<span>VR</span>
					<button
						type="button"
						class="column-resizer"
						class:dragging={columnResizeState?.column === "vr"}
						aria-label="Resize VR column"
						onpointerdown={(event) => startColumnResize("vr", event)}
						onpointermove={moveColumnResize}
						onpointerup={endColumnResize}
						onpointercancel={cancelColumnResize}
					></button>
				</div>
				<div class="header-cell">Value</div>
			</div>
			{#each visibleRows as row}
				<div
					class="row row-grid"
					role="button"
					tabindex="0"
					onclick={() => copyRow(row)}
					onkeydown={(event) => {
						if (event.key === "Enter" || event.key === " ") {
							event.preventDefault();
							void copyRow(row);
						}
					}}
				>
					<div class="tag-cell" style={`--depth:${row.depth}`}>
						{#if isSequence(row.node)}
							<button
								type="button"
								class="chevron"
								onclick={(event) => { event.stopPropagation(); toggleSequence(row.key); }}
							>
								{expandedSequences.has(row.key) ? "▼" : "▶"}
							</button>
						{/if}
						<span>{row.node.tag}</span>
					</div>
					<div class="keyword-cell">{row.node.keyword}</div>
					<div class="vr-cell">{row.node.vr}</div>
					<div class:binary={row.node.value.type === "binary"} class="value-cell">
						<button
							type="button"
							class="value-toggle"
							onclick={(event) => {
								event.stopPropagation();
								if (row.node.value.type === "string" && row.node.value.value.length > 80) {
									toggleLongValue(row.key);
								}
							}}
						>
							{valueDisplay(row)}
						</button>
						{#if copiedKey === row.key}
							<span class="copied">Copied ✓</span>
						{/if}
					</div>
				</div>
			{/each}
		</div>
	{/if}
</aside>

<style>
	.panel {
		background: #242424;
		display: grid;
		grid-template-rows: auto 1fr;
		min-height: 0;
	}

	header {
		padding: 0.75rem;
		border-bottom: 1px solid #333;
	}

	h2 {
		margin: 0 0 0.5rem 0;
		font-size: 1rem;
	}

	input {
		width: 100%;
		background: #1b1b1b;
		border: 1px solid #3a3a3a;
		color: #e0e0e0;
		padding: 0.4rem 0.6rem;
		border-radius: 6px;
	}

	.table {
		overflow: auto;
		min-width: 0;
		font-family: "JetBrains Mono", ui-monospace, monospace;
		font-size: 0.82rem;
	}

	.row-grid {
		display: grid;
		grid-template-columns: var(--tag-grid-columns);
		gap: 0.5rem;
		align-items: center;
		min-width: 0;
	}

	.header-row {
		position: sticky;
		top: 0;
		z-index: 2;
		padding: 0.35rem 0.75rem;
		background: #242424;
		border-bottom: 1px solid #333;
	}

	.header-cell {
		position: relative;
		min-width: 0;
		color: #9ca3af;
		font-size: 0.72rem;
		font-weight: 600;
		letter-spacing: 0.04em;
		text-transform: uppercase;
		user-select: none;
	}

	.header-cell.resizable {
		padding-right: 0.45rem;
	}

	.column-resizer {
		position: absolute;
		right: -0.35rem;
		top: -0.35rem;
		bottom: -0.35rem;
		width: 0.75rem;
		border: 0;
		padding: 0;
		margin: 0;
		background: transparent;
		cursor: col-resize;
		touch-action: none;
	}

	.column-resizer::after {
		content: "";
		position: absolute;
		left: 50%;
		top: 0.2rem;
		bottom: 0.2rem;
		width: 1px;
		background: #3f3f3f;
		transform: translateX(-50%);
	}

	.column-resizer.dragging::after {
		background: #4a9eff;
	}

	.row {
		padding: 0.35rem 0.75rem;
		border-bottom: 1px solid #2e2e2e;
		color: inherit;
		text-align: left;
	}

	.row:hover {
		background: #2d2d2d;
	}

	.row > div {
		min-width: 0;
	}

	.tag-cell {
		display: flex;
		gap: 0.35rem;
		align-items: center;
		padding-left: calc(var(--depth) * 0.9rem);
	}

	.tag-cell span,
	.keyword-cell,
	.vr-cell {
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}

	.chevron {
		cursor: pointer;
		color: #4a9eff;
		font-size: 0.75rem;
		border: 0;
		padding: 0;
		background: transparent;
	}

	.value-cell {
		position: relative;
		min-width: 0;
		padding-right: 4.4rem;
	}

	.value-toggle {
		display: block;
		width: 100%;
		min-width: 0;
		border: 0;
		background: transparent;
		padding: 0;
		margin: 0;
		color: inherit;
		font: inherit;
		text-align: left;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}

	.binary {
		color: #9ca3af;
	}

	.copied {
		position: absolute;
		right: 0;
		top: 50%;
		transform: translateY(-50%);
		color: #4a9eff;
		font-size: 0.72rem;
		white-space: nowrap;
		max-width: 4rem;
		overflow: hidden;
		text-overflow: ellipsis;
		pointer-events: none;
	}

	.error,
	.loading {
		padding: 0.75rem;
	}
</style>
<script lang="ts">
	import { fetchTags, type FileSummary, type TagNode, type TagValue } from "../api";

	type FlatRow = {
		key: string;
		node: TagNode;
		depth: number;
	};

	let { files, activeFileIndex }: { files: FileSummary[]; activeFileIndex: number } = $props();

	let filter = $state("");
	let tagsByFile = $state<Record<number, TagNode[]>>({});
	let loading = $state(false);
	let error = $state<string | null>(null);
	let expandedSequences = $state<Set<string>>(new Set());
	let expandedLongValues = $state<Set<string>>(new Set());
	let copiedKey = $state<string | null>(null);

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
		<div class="table">
			{#each visibleRows as row}
				<div
					class="row"
					style={`--depth:${row.depth}`}
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
					<div class="tag-cell">
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
					<div>{row.node.keyword}</div>
					<div>{row.node.vr}</div>
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
		border-left: 1px solid #333;
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
		font-family: "JetBrains Mono", ui-monospace, monospace;
		font-size: 0.82rem;
	}
	.row {
		display: grid;
		grid-template-columns: 8rem 8.5rem 4rem 1fr;
		gap: 0.5rem;
		padding: 0.35rem 0.75rem;
		border: 0;
		border-bottom: 1px solid #2e2e2e;
		background: transparent;
		color: inherit;
		text-align: left;
		padding-left: calc(0.75rem + var(--depth) * 0.9rem);
	}
	.row:hover {
		background: #2d2d2d;
	}
	.tag-cell {
		display: flex;
		gap: 0.35rem;
		align-items: center;
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
		display: flex;
		align-items: center;
		gap: 0.5rem;
		overflow: hidden;
	}
	.value-toggle {
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
		color: #4a9eff;
		font-size: 0.75rem;
		white-space: nowrap;
	}
	.error,
	.loading {
		padding: 0.75rem;
	}
</style>

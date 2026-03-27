<script lang="ts">
	import { onMount } from "svelte";
	import { fetchTags, type FileSummary, type TagNode } from "../api";

	let { files, activeFileIndex }: { files: FileSummary[]; activeFileIndex: number } = $props();
	let filter = $state("");
	let tagsByFile = $state<Record<number, TagNode[]>>({});
	let loading = $state(false);
	let error = $state<string | null>(null);

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

	const visibleTags = $derived.by(() => {
		const tags = tagsByFile[activeFileIndex] ?? [];
		if (!filter.trim()) {
			return tags;
		}
		const needle = filter.toLowerCase();
		return tags.filter((tag) => `${tag.tag} ${tag.keyword} ${tag.vr} ${JSON.stringify(tag.value)}`.toLowerCase().includes(needle));
	});
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
			{#each visibleTags as tag}
				<div class="row">
					<div>{tag.tag}</div>
					<div>{tag.keyword}</div>
					<div>{tag.vr}</div>
					<div class="value">{JSON.stringify(tag.value)}</div>
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
		font-size: 0.85rem;
	}
	.row {
		display: grid;
		grid-template-columns: 7rem 8rem 4rem 1fr;
		gap: 0.5rem;
		padding: 0.4rem 0.75rem;
		border-bottom: 1px solid #2e2e2e;
	}
	.value {
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}
	.error,
	.loading {
		padding: 0.75rem;
	}
</style>

import type { TagNode, TagValue } from "../api";

export type FlatTagRow = {
	key: string;
	node: TagNode;
	depth: number;
};

export function flattenTagRows(
	nodes: TagNode[],
	prefix: string,
	expandedSequences: ReadonlySet<string>,
	filter: string,
): FlatTagRow[] {
	const rows: FlatTagRow[] = [];
	flattenRows(nodes, prefix, 0, rows, expandedSequences, filter.trim().toLowerCase());
	return rows;
}

function flattenRows(
	nodes: TagNode[],
	prefix: string,
	depth: number,
	out: FlatTagRow[],
	expandedSequences: ReadonlySet<string>,
	needle: string,
): void {
	nodes.forEach((node, index) => {
		const key = `${prefix}-${index}`;
		const nodeMatches = matchesTagNeedle(node, needle);
		const descendantMatches =
			node.value.type === "sequence" ? sequenceHasNeedle(node.value.items, needle) : false;

		if (!needle || nodeMatches || descendantMatches) out.push({ key, node, depth });

		if (node.value.type === "sequence" && expandedSequences.has(key)) {
			node.value.items.forEach((item, itemIndex) => {
				flattenRows(
					item,
					`${key}:item${itemIndex}`,
					depth + 1,
					out,
					expandedSequences,
					needle,
				);
			});
		}
	});
}

export function matchesTagNeedle(node: TagNode, needle: string): boolean {
	if (!needle) return true;
	const haystack = `${node.tag} ${node.keyword} ${node.vr} ${tagValuePreview(node.value)}`.toLowerCase();
	return haystack.includes(needle.toLowerCase());
}

function sequenceHasNeedle(items: TagNode[][], needle: string): boolean {
	if (!needle) return true;
	return items.some((item) => item.some((node) =>
		matchesTagNeedle(node, needle)
		|| (node.value.type === "sequence" && sequenceHasNeedle(node.value.items, needle))
	));
}

export function tagValuePreview(value: TagValue): string {
	switch (value.type) {
		case "string":
			return value.value;
		case "number":
			return String(value.value);
		case "numbers":
			return `${value.value.join(", ")}${truncatedSuffix(value.value.length, value.total, value.truncated)}`;
		case "binary":
			return `${value.length} bytes`;
		case "sequence":
			return `${value.items.length} item(s)${truncatedSuffix(value.items.length, value.total, value.truncated)}`;
		case "error":
			return value.message;
	}
}

export function tagValueToCopyText(value: TagValue): string {
	switch (value.type) {
		case "binary":
			return `[binary: ${value.length} bytes]`;
		case "sequence":
			return `[sequence: ${value.items.length} item(s)${truncatedSuffix(value.items.length, value.total, value.truncated)}]`;
		case "numbers":
			return `${value.value.join(", ")}${truncatedSuffix(value.value.length, value.total, value.truncated)}`;
		case "number":
			return String(value.value);
		case "string":
			return value.value;
		case "error":
			return `error: ${value.message}`;
	}
}

export function tagValueDisplay(row: FlatTagRow, expanded: boolean): string {
	const value = row.node.value;
	switch (value.type) {
		case "string":
			return value.value.length > 80 && !expanded
				? `${value.value.slice(0, 80)}…`
				: value.value;
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

export function isSequenceTag(node: TagNode): boolean {
	return node.value.type === "sequence";
}

export function truncatedSuffix(visible: number, total?: number, truncated?: boolean): string {
	if (!truncated) return "";
	return total === undefined ? " (truncated)" : ` (first ${visible} of ${total})`;
}

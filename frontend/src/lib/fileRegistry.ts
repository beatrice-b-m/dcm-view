export type IndexedFile = {
	index: number;
};

export function indexFilesById<File extends IndexedFile>(
	files: readonly File[],
): ReadonlyMap<number, File> {
	const byId = new Map<number, File>();
	for (const file of files) {
		if (byId.has(file.index)) {
			throw new Error(`duplicate file index ${file.index}`);
		}
		byId.set(file.index, file);
	}
	return byId;
}

export function resolveFilesById<File extends IndexedFile>(
	filesById: ReadonlyMap<number, File>,
	fileIds: readonly number[],
): File[] {
	const resolved: File[] = [];
	for (const fileId of fileIds) {
		const file = filesById.get(fileId);
		if (file) resolved.push(file);
	}
	return resolved;
}

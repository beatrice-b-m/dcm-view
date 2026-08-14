import type { FileSummary } from "../api";

export type NavKind = "patient" | "study" | "series" | "image";

export type NavFile = {
	kind: "image";
	file: FileSummary;
	label: string;
	detail: string;
};

export type NavSeries = {
	kind: "series";
	key: string;
	label: string;
	detail: string;
	files: NavFile[];
};

export type NavStudy = {
	kind: "study";
	key: string;
	label: string;
	detail: string;
	series: NavSeries[];
};

export type NavPatient = {
	kind: "patient";
	key: string;
	label: string;
	detail: string;
	studies: NavStudy[];
};

export type DirectoryFile = {
	kind: "file";
	key: string;
	label: string;
	detail: string;
	file: FileSummary;
};

export type DirectoryFolder = {
	kind: "folder";
	key: string;
	label: string;
	children: DirectoryNode[];
};

export type DirectoryNode = DirectoryFolder | DirectoryFile;

function clean(value: string | null | undefined): string {
	return (value ?? "").trim();
}

function basename(path: string): string {
	return path.split(/[\\/]/).pop() || path;
}

function pathParts(path: string): string[] {
	return path.replace(/\\/g, "/").split("/").filter(Boolean);
}

function sharedDirectoryPrefix(paths: readonly string[][]): string[] {
	if (paths.length === 0) return [];
	const first = paths[0];
	let length = first.length;
	for (const path of paths.slice(1)) {
		length = Math.min(length, path.length);
		for (let index = 0; index < length; index += 1) {
			if (first[index] !== path[index]) {
				length = index;
				break;
			}
		}
	}
	return first.slice(0, length);
}

function formatPersonName(value: string): string {
	return value.replace(/\^/g, " ").replace(/\s+/g, " ").trim();
}

function formatDate(value: string): string {
	const trimmed = clean(value);
	if (!/^\d{8}$/.test(trimmed)) return trimmed;
	return `${trimmed.slice(0, 4)}-${trimmed.slice(4, 6)}-${trimmed.slice(6, 8)}`;
}

function shortUid(value: string): string {
	const trimmed = clean(value);
	if (trimmed.length <= 18) return trimmed;
	return `...${trimmed.slice(-15)}`;
}

function nodeKey(prefix: string, fallback: string, parts: string[]): string {
	const value = parts.map(clean).find(Boolean) ?? fallback;
	return `${prefix}:${value}`;
}

function numericValue(value: string): number | null {
	const parsed = Number.parseFloat(clean(value));
	return Number.isFinite(parsed) ? parsed : null;
}

function plural(count: number, singular: string): string {
	const pluralForms: Record<string, string> = {
		image: "images",
		series: "series",
		study: "studies",
	};
	const label = count === 1 ? singular : (pluralForms[singular] ?? `${singular}s`);
	return `${count} ${label}`;
}

export function tierLabel(kind: NavKind): string {
	switch (kind) {
		case "patient":
			return "Patient";
		case "study":
			return "Study";
		case "series":
			return "Series";
		case "image":
			return "Image";
	}
}

function countStudyFiles(study: NavStudy): number {
	return study.series.reduce((total, series) => total + series.files.length, 0);
}

function countPatientSeries(patient: NavPatient): number {
	return patient.studies.reduce((total, study) => total + study.series.length, 0);
}

function countPatientFiles(patient: NavPatient): number {
	return patient.studies.reduce((total, study) => total + countStudyFiles(study), 0);
}

function patientLabel(file: FileSummary): string {
	return formatPersonName(clean(file.patient_name)) || clean(file.patient_id) || "Unknown Patient";
}

function patientDetail(file: FileSummary): string {
	const id = clean(file.patient_id);
	return id && id !== patientLabel(file) ? `ID ${id}` : "";
}

function studyLabel(file: FileSummary): string {
	return clean(file.study_description) || "Study";
}

function studyDetail(file: FileSummary): string {
	const date = formatDate(file.study_date);
	const uid = shortUid(file.study_instance_uid);
	return [date, uid && uid !== studyLabel(file) ? uid : ""]
		.filter(Boolean)
		.join(" · ");
}

function seriesLabel(file: FileSummary): string {
	const description = clean(file.series_description);
	const number = clean(file.series_number);
	return description || (number ? `Series ${number}` : "")
		|| shortUid(file.series_instance_uid)
		|| "Unknown Series";
}

function seriesDetail(file: FileSummary): string {
	const modality = clean(file.modality);
	const number = clean(file.series_number);
	const uid = shortUid(file.series_instance_uid);
	return [modality, number ? `Series ${number}` : "", uid && uid !== seriesLabel(file) ? uid : ""]
		.filter(Boolean)
		.join(" · ");
}

function fileLabel(file: FileSummary): string {
	const instance = clean(file.instance_number);
	const name = basename(file.path);
	return instance ? `#${instance} ${name}` : name;
}

function fileDetail(file: FileSummary): string {
	if (!file.has_pixels) return "no pixels";
	const dimensions = file.rows > 0 && file.columns > 0 ? `${file.columns}x${file.rows}` : "";
	const frames = file.frame_count > 1 ? `${file.frame_count} frames` : "1 frame";
	return [dimensions, frames].filter(Boolean).join(" · ");
}

function withCounts(detail: string, counts: string[]): string {
	return [detail, ...counts].filter(Boolean).join(" · ");
}

function searchableValues(file: FileSummary, scope: string | null): string[] {
	switch (scope) {
		case "patient":
			return [file.patient_id, file.patient_name];
		case "study":
			return [file.study_description, file.study_date, file.study_instance_uid];
		case "series":
			return [file.series_description, file.series_number, file.series_instance_uid];
		case "modality":
			return [file.modality];
		default:
			return [
				file.patient_id,
				file.patient_name,
				file.study_description,
				file.study_date,
				file.series_description,
				file.series_number,
				file.modality,
			];
	}
}

function fileMatchesTerm(file: FileSummary, rawTerm: string): boolean {
	const trimmed = clean(rawTerm);
	if (!trimmed) return true;

	const scoped = trimmed.match(/^(patient|study|series|modality):(.*)$/i);
	const scope = scoped?.[1].toLowerCase() ?? null;
	const needle = (scoped?.[2] ?? trimmed).trim().toLowerCase();
	if (!needle) return true;
	return searchableValues(file, scope).some((value) => clean(value).toLowerCase().includes(needle));
}

export function fileMatchesFilter(file: FileSummary, query: string): boolean {
	const terms = clean(query).split(/\s+/).filter(Boolean);
	return terms.every((term) => fileMatchesTerm(file, term));
}

export function patientDetailWithCounts(patient: NavPatient): string {
	return withCounts(patient.detail, [
		plural(patient.studies.length, "study"),
		plural(countPatientSeries(patient), "series"),
		plural(countPatientFiles(patient), "image"),
	]);
}

export function studyDetailWithCounts(study: NavStudy): string {
	return withCounts(study.detail, [
		plural(study.series.length, "series"),
		plural(countStudyFiles(study), "image"),
	]);
}

export function seriesDetailWithCounts(series: NavSeries): string {
	return withCounts(series.detail, [plural(series.files.length, "image")]);
}

export function nodeAriaLabel(
	kind: Exclude<NavKind, "image">,
	label: string,
	detail: string,
	collapsed: boolean,
): string {
	const state = collapsed ? "collapsed" : "expanded";
	const kindLabel = tierLabel(kind);
	const primary = label === kindLabel ? kindLabel : `${kindLabel} ${label}`;
	return `${primary}${detail ? `, ${detail}` : ""}, ${state}`;
}

export function fileAriaLabel(item: NavFile): string {
	return `${tierLabel(item.kind)} ${item.label}${item.detail ? `, ${item.detail}` : ""}`;
}

export function buildFileTree(files: readonly FileSummary[]): NavPatient[] {
	const patients = new Map<string, NavPatient>();
	const studies = new Map<string, NavStudy>();
	const seriesByKey = new Map<string, NavSeries>();

	for (const file of files) {
		const patientKey = nodeKey("patient", `file-${file.index}`, [file.patient_id, file.patient_name]);
		let patient = patients.get(patientKey);
		if (!patient) {
			patient = {
				kind: "patient",
				key: patientKey,
				label: patientLabel(file),
				detail: patientDetail(file),
				studies: [],
			};
			patients.set(patientKey, patient);
		}

		const studyKey = `${patientKey}/${nodeKey("study", `file-${file.index}`, [
			file.study_instance_uid,
			file.study_description,
			file.study_date,
		])}`;
		let study = studies.get(studyKey);
		if (!study) {
			study = {
				kind: "study",
				key: studyKey,
				label: studyLabel(file),
				detail: studyDetail(file),
				series: [],
			};
			patient.studies.push(study);
			studies.set(studyKey, study);
		}

		const seriesKey = `${studyKey}/${nodeKey("series", `file-${file.index}`, [
			file.series_instance_uid,
			file.series_number,
			file.series_description,
		])}`;
		let series = seriesByKey.get(seriesKey);
		if (!series) {
			series = {
				kind: "series",
				key: seriesKey,
				label: seriesLabel(file),
				detail: seriesDetail(file),
				files: [],
			};
			study.series.push(series);
			seriesByKey.set(seriesKey, series);
		}

		series.files.push({
			kind: "image",
			file,
			label: fileLabel(file),
			detail: fileDetail(file),
		});
	}

	for (const series of seriesByKey.values()) {
		series.files.sort((left, right) => {
			const leftInstance = numericValue(left.file.instance_number);
			const rightInstance = numericValue(right.file.instance_number);
			if (leftInstance !== null && rightInstance !== null && leftInstance !== rightInstance) {
				return leftInstance - rightInstance;
			}
			if (leftInstance !== null && rightInstance === null) return -1;
			if (leftInstance === null && rightInstance !== null) return 1;
			return left.file.index - right.file.index;
		});
	}

	return Array.from(patients.values());
}

export function buildDirectoryTree(files: readonly FileSummary[]): DirectoryNode[] {
	const root: DirectoryFolder = { kind: "folder", key: "directory:root", label: "Files", children: [] };
	const folders = new Map<string, DirectoryFolder>([[root.key, root]]);
	const records = files.map((file) => {
		const parts = pathParts(file.path);
		return { file, directories: parts.slice(0, -1), fileName: parts[parts.length - 1] || file.path };
	});
	const common = sharedDirectoryPrefix(records.map((record) => record.directories));
	// Keep the deepest common directory as recognizable context, while hiding machine-specific prefixes.
	const trimCount = Math.max(0, common.length - 1);

	for (const { file, directories, fileName } of records) {
		let parent = root;
		let folderPath = "";
		for (const part of directories.slice(trimCount)) {
			folderPath = folderPath ? `${folderPath}/${part}` : part;
			const key = `directory:${folderPath}`;
			let folder = folders.get(key);
			if (!folder) {
				folder = { kind: "folder", key, label: part, children: [] };
				folders.set(key, folder);
				parent.children.push(folder);
			}
			parent = folder;
		}
		parent.children.push({
			kind: "file",
			key: `directory:file:${file.index}`,
			label: fileName,
			detail: fileDetail(file),
			file,
		});
	}

	const sortNodes = (nodes: DirectoryNode[]) => {
		nodes.sort((left, right) => {
			if (left.kind !== right.kind) return left.kind === "folder" ? -1 : 1;
			return left.label.localeCompare(right.label, undefined, { numeric: true });
		});
		for (const node of nodes) if (node.kind === "folder") sortNodes(node.children);
	};
	sortNodes(root.children);
	return root.children;
}

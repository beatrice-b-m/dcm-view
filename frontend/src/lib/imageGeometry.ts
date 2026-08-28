export type ImageDisplayGeometry = {
	width: number;
	height: number;
	centerX: number;
	centerY: number;
	pixelAspectRatio: number;
};

export type ImageFitTransform = {
	scale: number;
	tx: number;
	ty: number;
};

export function effectivePixelAspectRatio(value: number | null | undefined): number {
	return typeof value === "number" && Number.isFinite(value) && value > 0 ? value : 1;
}

export function imageDisplayGeometry(
	rows: number,
	columns: number,
	pixelAspectRatio: number | null | undefined,
): ImageDisplayGeometry {
	const ratio = effectivePixelAspectRatio(pixelAspectRatio);
	const width = Math.max(columns, 0);
	const height = Math.max(rows, 0) * ratio;
	return {
		width,
		height,
		centerX: width / 2,
		centerY: height / 2,
		pixelAspectRatio: ratio,
	};
}

export function fitImageToViewportHeight(
	geometry: ImageDisplayGeometry,
	viewportWidth: number,
	viewportHeight: number,
	minimumScale: number,
): ImageFitTransform | null {
	if (geometry.width <= 0 || geometry.height <= 0 || viewportHeight <= 0) return null;
	const scale = Math.max(minimumScale, viewportHeight / geometry.height);
	return {
		scale,
		tx: (viewportWidth - geometry.width * scale) / 2,
		ty: (viewportHeight - geometry.height * scale) / 2,
	};
}

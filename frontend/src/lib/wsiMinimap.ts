import type { WsiTileRectangle, WsiTotalPixelMatrix } from "../generated/api-types";

export type WsiMinimapGeometry = {
	viewWidth: number;
	viewHeight: number;
	tile: WsiTileRectangle;
};

export function wsiMinimapGeometry(
	matrix: WsiTotalPixelMatrix | null,
	tile: WsiTileRectangle | null,
	maxWidth = 240,
	maxHeight = 80,
): WsiMinimapGeometry | null {
	if (!matrix || !tile || matrix.rows <= 0 || matrix.columns <= 0) return null;
	if (tile.x < 0 || tile.y < 0 || tile.width <= 0 || tile.height <= 0) return null;
	if (tile.x >= matrix.columns || tile.y >= matrix.rows) return null;
	const scale = Math.min(maxWidth / matrix.columns, maxHeight / matrix.rows);
	return {
		viewWidth: matrix.columns * scale,
		viewHeight: matrix.rows * scale,
		tile: {
			x: tile.x * scale,
			y: tile.y * scale,
			width: Math.min(tile.width, matrix.columns - tile.x) * scale,
			height: Math.min(tile.height, matrix.rows - tile.y) * scale,
		},
	};
}

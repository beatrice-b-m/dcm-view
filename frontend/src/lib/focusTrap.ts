export type FocusTrapTarget = "first" | "last" | "container" | null;

export function focusTrapTarget(
	activeIndex: number,
	focusableCount: number,
	shiftKey: boolean,
): FocusTrapTarget {
	if (focusableCount <= 0) return "container";
	if (activeIndex < 0) return shiftKey ? "last" : "first";
	if (shiftKey && activeIndex === 0) return "last";
	if (!shiftKey && activeIndex === focusableCount - 1) return "first";
	return null;
}

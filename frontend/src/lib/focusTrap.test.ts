import { describe, expect, it } from "vitest";
import { focusTrapTarget } from "./focusTrap";

describe("modal focus trap", () => {
	it("wraps forward focus from the last control", () => {
		expect(focusTrapTarget(2, 3, false)).toBe("first");
	});

	it("wraps reverse focus from the first control", () => {
		expect(focusTrapTarget(0, 3, true)).toBe("last");
	});

	it("moves focus into the drawer from its container", () => {
		expect(focusTrapTarget(-1, 3, false)).toBe("first");
		expect(focusTrapTarget(-1, 3, true)).toBe("last");
	});

	it("keeps focus on an empty drawer container", () => {
		expect(focusTrapTarget(-1, 0, false)).toBe("container");
	});

	it("allows normal movement between interior controls", () => {
		expect(focusTrapTarget(1, 3, false)).toBeNull();
		expect(focusTrapTarget(1, 3, true)).toBeNull();
	});
});

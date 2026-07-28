import { describe, expect, it, vi } from "vitest";
import { RevisionedPersistenceController } from "./revisionedPersistence";

type Deferred<Value> = {
	promise: Promise<Value>;
	resolve: (value: Value) => void;
	reject: (error: unknown) => void;
};

function deferred<Value>(): Deferred<Value> {
	let resolve!: (value: Value) => void;
	let reject!: (error: unknown) => void;
	const promise = new Promise<Value>((res, rej) => {
		resolve = res;
		reject = rej;
	});
	return { promise, resolve, reject };
}

async function flushPromises(): Promise<void> {
	await Promise.resolve();
	await Promise.resolve();
	await Promise.resolve();
}

describe("RevisionedPersistenceController", () => {
	it("persists an edit even when initialization and the first edit have equal values", () => {
		const save = vi.fn().mockResolvedValue("first local value");
		const controller = new RevisionedPersistenceController<number, string>({ save });

		controller.initialize(5, "first local value");
		controller.edit(5, "first local value");

		expect(save).toHaveBeenCalledOnce();
		expect(save).toHaveBeenCalledWith(5, "first local value");
	});

	it("exposes a dirty draft without starting a request", () => {
		const save = vi.fn();
		const controller = new RevisionedPersistenceController<number, string>({ save });
		controller.initialize(5, "committed");

		const draft = controller.setDraft(5, "local draft");
		expect(draft).toMatchObject({
			value: "local draft",
			status: "dirty",
			saving: false,
			dirty: true,
		});
		expect(save).not.toHaveBeenCalled();
	});

	it("serializes writes and keeps a newer edit when the older response completes", async () => {
		const first = deferred<string>();
		const second = deferred<string>();
		const save = vi.fn()
			.mockReturnValueOnce(first.promise)
			.mockReturnValueOnce(second.promise);
		const controller = new RevisionedPersistenceController<number, string>({ save });
		controller.initialize(7, "committed");

		controller.edit(7, "revision one");
		controller.edit(7, "revision two");

		expect(save).toHaveBeenCalledTimes(1);
		expect(controller.get(7)).toMatchObject({
			value: "revision two",
			status: "saving",
			dirty: true,
		});

		first.resolve("canonical one");
		await flushPromises();

		expect(save).toHaveBeenCalledTimes(2);
		expect(save).toHaveBeenLastCalledWith(7, "revision two");
		expect(controller.get(7)).toMatchObject({
			value: "revision two",
			committedValue: "canonical one",
			status: "saving",
		});

		second.resolve("canonical two");
		await flushPromises();
		expect(controller.get(7)).toMatchObject({
			value: "canonical two",
			committedValue: "canonical two",
			status: "clean",
			dirty: false,
		});
	});

	it("keeps files isolated when independent requests complete in reverse order", async () => {
		const firstFile = deferred<string>();
		const secondFile = deferred<string>();
		const save = vi.fn((key: number) => key === 1 ? firstFile.promise : secondFile.promise);
		const controller = new RevisionedPersistenceController<number, string>({ save });
		controller.initialize(1, "one");
		controller.initialize(2, "two");
		controller.edit(1, "one edited");
		controller.edit(2, "two edited");

		secondFile.resolve("two canonical");
		await flushPromises();
		firstFile.resolve("one canonical");
		await flushPromises();

		expect(controller.get(1)).toMatchObject({ status: "clean", value: "one canonical" });
		expect(controller.get(2)).toMatchObject({ status: "clean", value: "two canonical" });
	});

	it("retains failed edits and retries the newest revision", async () => {
		const retry = deferred<string>();
		const save = vi.fn()
			.mockRejectedValueOnce(new Error("network unavailable"))
			.mockReturnValueOnce(retry.promise);
		const controller = new RevisionedPersistenceController<number, string>({ save });
		controller.initialize(3, "old");

		controller.edit(3, "new");
		await flushPromises();
		expect(controller.get(3)).toMatchObject({
			value: "new",
			committedValue: "old",
			status: "error",
			error: "network unavailable",
		});

		controller.retry(3);
		expect(save).toHaveBeenCalledTimes(2);
		expect(save).toHaveBeenLastCalledWith(3, "new");

		retry.resolve("canonical new");
		await flushPromises();
		expect(controller.get(3)).toMatchObject({
			value: "canonical new",
			status: "clean",
			error: null,
		});
	});

	it("rolls a failed edit back to the last committed value", async () => {
		const controller = new RevisionedPersistenceController<number, string>({
			save: vi.fn().mockRejectedValue(new Error("rejected")),
		});
		controller.initialize(11, "server value");
		controller.edit(11, "local edit");
		await flushPromises();

		const rolledBack = controller.rollback(11);
		expect(rolledBack).toMatchObject({
			value: "server value",
			status: "clean",
			dirty: false,
			error: null,
		});
	});
});

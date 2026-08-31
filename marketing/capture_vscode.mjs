import { createHash } from "node:crypto";
import { spawn } from "node:child_process";
import { mkdtemp, mkdir, readFile, rm, writeFile } from "node:fs/promises";
import net from "node:net";
import { tmpdir } from "node:os";
import path from "node:path";
import process from "node:process";
import { chromium } from "playwright";
import vscodeTest from "@vscode/test-electron";

const { downloadAndUnzipVSCode } = vscodeTest;

function parseArguments(argv) {
	const values = {};
	for (let index = 0; index < argv.length; index += 2) {
		const key = argv[index];
		const value = argv[index + 1];
		if (!key?.startsWith("--") || value === undefined) {
			throw new Error(`invalid capture argument near ${key ?? "end of arguments"}`);
		}
		values[key.slice(2)] = value;
	}
	for (const required of ["source", "scene", "output", "report", "binary", "repo"]) {
		if (!values[required]) throw new Error(`missing --${required}`);
	}
	return values;
}

function instanceOrder(file) {
	const parsed = Number.parseInt(file.instance_number, 10);
	return Number.isFinite(parsed) ? parsed : Number.MAX_SAFE_INTEGER;
}

function chooseFile(files, scene) {
	const candidates = files
		.filter((file) => file.series_instance_uid === scene.series_instance_uid && file.has_pixels)
		.sort((left, right) => instanceOrder(left) - instanceOrder(right) || left.path.localeCompare(right.path));
	if (candidates.length === 0) throw new Error(`series is unavailable: ${scene.series_instance_uid}`);
	const position = Math.min(1, Math.max(0, Number(scene.file_position ?? 0)));
	return candidates[Math.round(position * (candidates.length - 1))];
}

async function waitForViewerFrame(window) {
	const deadline = Date.now() + 90_000;
	while (Date.now() < deadline) {
		const frame = window.frames().find((candidate) => /^http:\/\/(127\.0\.0\.1|localhost):\d+\/$/.test(candidate.url()));
		if (frame) return frame;
		await window.waitForTimeout(250);
	}
	throw new Error("VS Code webview did not load the dcmview frame");
}

async function waitForRendered(frame, fileIndex) {
	await frame.waitForFunction(
		(expected) => {
			const canvas = document.querySelector("canvas.dicom-canvas");
			return canvas instanceof HTMLCanvasElement
				&& canvas.width > 1
				&& canvas.height > 1
				&& canvas.dataset.captureRendered?.startsWith(`${expected}:`)
				&& !document.querySelector(".frame-request-indicator");
		},
		fileIndex,
		{ timeout: 60_000 },
	);
	await frame.evaluate(() => document.fonts.ready);
}

async function availablePort() {
	return new Promise((resolve, reject) => {
		const server = net.createServer();
		server.once("error", reject);
		server.listen(0, "127.0.0.1", () => {
			const address = server.address();
			if (!address || typeof address === "string") {
				server.close(() => reject(new Error("failed to reserve a CDP port")));
				return;
			}
			server.close((error) => error ? reject(error) : resolve(address.port));
		});
	});
}

async function connectToCode(port, child) {
	const deadline = Date.now() + 60_000;
	let lastError;
	while (Date.now() < deadline) {
		if (child.exitCode !== null) throw new Error(`VS Code exited before CDP startup (${child.exitCode})`);
		try {
			return await chromium.connectOverCDP(`http://127.0.0.1:${port}`);
		} catch (error) {
			lastError = error;
			await new Promise((resolve) => setTimeout(resolve, 250));
		}
	}
	throw new Error(`timed out connecting to VS Code CDP: ${lastError instanceof Error ? lastError.message : lastError}`);
}

async function stopProcess(child) {
	if (child.exitCode !== null) return;
	child.kill("SIGTERM");
	await Promise.race([
		new Promise((resolve) => child.once("exit", resolve)),
		new Promise((resolve) => setTimeout(resolve, 5_000)),
	]);
	if (child.exitCode === null) child.kill("SIGKILL");
}

async function main() {
	const args = parseArguments(process.argv.slice(2));
	const scene = JSON.parse(await readFile(args.scene, "utf8"));
	const vscodeVersion = scene.vscode_version;
	if (typeof vscodeVersion !== "string" || !vscodeVersion) {
		throw new Error("VS Code capture scene must pin vscode_version");
	}
	await mkdir(path.dirname(args.output), { recursive: true });
	const executablePath = await downloadAndUnzipVSCode(vscodeVersion);
	const scratch = await mkdtemp(path.join(tmpdir(), "dcmview-vscode-capture-"));
	const cdpPort = await availablePort();
	const code = spawn(executablePath, [
			args.source,
			"--new-window",
			`--remote-debugging-port=${cdpPort}`,
			`--extensionDevelopmentPath=${path.join(args.repo, "vscode")}`,
			`--user-data-dir=${path.join(scratch, "user-data")}`,
			`--extensions-dir=${path.join(scratch, "extensions")}`,
			"--disable-workspace-trust",
			"--disable-updates",
			"--skip-release-notes",
			"--skip-welcome",
			"--window-size=1440,900",
		], {
		env: {
			...process.env,
			DCMVIEW_BINARY: args.binary,
			DCMVIEW_VSCODE_BYPASS: "0",
		},
		stdio: "ignore",
	});
	let browser;
	try {
		browser = await connectToCode(cdpPort, code);
		const context = browser.contexts()[0];
		if (!context) throw new Error("VS Code CDP exposed no browser context");
		const window = context.pages()[0] ?? await context.waitForEvent("page", { timeout: 30_000 });
		await window.waitForTimeout(1_000);
		await window.keyboard.press(process.platform === "darwin" ? "Meta+Shift+P" : "Control+Shift+P");
		await window.keyboard.type("dcmview: Open Workspace with dcmview");
		await window.keyboard.press("Enter");
		const frame = await waitForViewerFrame(window);
		await frame.addStyleTag({ content: `
			*, *::before, *::after { animation: none !important; transition: none !important; caret-color: transparent !important; }
			.status { display: none !important; }
		` });
		const catalog = await frame.evaluate(async () => {
			const response = await fetch("/api/files");
			if (!response.ok) throw new Error(`files endpoint returned ${response.status}`);
			return response.json();
		});
		const unexpectedPatientIds = [...new Set(catalog.files
			.map((file) => file.patient_id)
			.filter(Boolean)
			.filter((patientId) => !scene.allowed_patient_ids.includes(patientId)))];
		if (unexpectedPatientIds.length > 0) {
			throw new Error(`unexpected patient identifiers: ${unexpectedPatientIds.join(", ")}`);
		}
		const file = chooseFile(catalog.files, scene);
		const button = frame.locator(`[data-capture-file-index="${file.index}"]`).first();
		await button.waitFor({ state: "visible", timeout: 30_000 });
		await button.click();
		await waitForRendered(frame, file.index);
		const visibleText = await window.locator("body").innerText();
		for (const forbidden of ["/Users/", "\\Users\\", "marketing-source-data", "127.0.0.1", "localhost"]) {
			if (visibleText.includes(forbidden)) throw new Error(`visible VS Code text contains forbidden value: ${forbidden}`);
		}
		await window.screenshot({ path: args.output, animations: "disabled", caret: "hide" });
		await writeFile(args.report, `${JSON.stringify({
			scene_id: scene.id,
			file_index: file.index,
			sop_instance_uid: file.sop_instance_uid,
			series_instance_uid: file.series_instance_uid,
			patient_ids: [...new Set(catalog.files.map((candidate) => candidate.patient_id).filter(Boolean))].sort(),
			visible_text_sha256: createHash("sha256").update(visibleText).digest("hex"),
			vscode_version: vscodeVersion,
			node_version: process.version,
			viewport: scene.viewport,
		}, null, 2)}\n`, "utf8");
	} finally {
		await stopProcess(code);
		if (browser) {
			await Promise.race([
				browser.close().catch(() => {}),
				new Promise((resolve) => setTimeout(resolve, 2_000)),
			]);
		}
		await rm(scratch, { recursive: true, force: true });
	}
}

main().catch((error) => {
	console.error(error instanceof Error ? error.stack : String(error));
	process.exitCode = 1;
});

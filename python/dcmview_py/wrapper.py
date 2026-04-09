from __future__ import annotations

import os
import shutil
import signal
import subprocess
import sys
import threading
from pathlib import Path
from typing import Iterable

_STARTUP_PREFIX = "dcmview: server running at "
_URL_WAIT_SECONDS = 5.0
_STOP_TIMEOUT_SECONDS = 5.0

PathInput = str | os.PathLike[str]


class _OutputMonitor:
	def __init__(self, process: subprocess.Popen[str]) -> None:
		self._process = process
		self._url: str | None = None
		self._url_lock = threading.Lock()
		self._url_ready = threading.Event()
		self._thread = threading.Thread(target=self._run, name="dcmview-py-output", daemon=True)

	def start(self) -> None:
		self._thread.start()

	def join(self) -> None:
		self._thread.join()

	def wait_for_url(self, timeout: float) -> str | None:
		self._url_ready.wait(timeout)
		return self.url

	@property
	def url(self) -> str | None:
		with self._url_lock:
			return self._url

	def _set_url(self, url: str) -> None:
		with self._url_lock:
			if self._url is None:
				self._url = url
				self._url_ready.set()

	def _run(self) -> None:
		stdout = self._process.stdout
		if stdout is None:
			self._url_ready.set()
			return

		for line in stdout:
			sys.stdout.write(line)
			sys.stdout.flush()
			if line.startswith(_STARTUP_PREFIX):
				self._set_url(line[len(_STARTUP_PREFIX) :].strip())

		self._url_ready.set()


class ShutdownHandle:
	"""Handle for controlling a non-blocking dcmview subprocess."""

	def __init__(self, process: subprocess.Popen[str], monitor: _OutputMonitor) -> None:
		self._process = process
		self._monitor = monitor

	@property
	def url(self) -> str | None:
		return self._monitor.url

	def stop(self, timeout: float = _STOP_TIMEOUT_SECONDS) -> int:
		if self._process.poll() is not None:
			self._monitor.join()
			return int(self._process.returncode or 0)

		self._process.send_signal(signal.SIGINT)
		try:
			return_code = self._process.wait(timeout=timeout)
		except subprocess.TimeoutExpired:
			self._process.terminate()
			try:
				return_code = self._process.wait(timeout=timeout)
			except subprocess.TimeoutExpired:
				self._process.kill()
				return_code = self._process.wait(timeout=timeout)

		self._monitor.join()
		return int(return_code)

	def __enter__(self) -> ShutdownHandle:
		return self

	def __exit__(self, _exc_type, _exc, _tb) -> None:
		self.stop()


def view(
	files: PathInput | Iterable[PathInput],
	*,
	port: int = 0,
	host: str = "127.0.0.1",
	browser: bool = True,
	tunnel: bool = False,
	tunnel_host: str | None = None,
	tunnel_port: int = 0,
	block: bool = True,
	recursive: bool = True,
	timeout: int | None = None,
) -> ShutdownHandle | None:
	"""Launch dcmview for one or more filesystem paths."""

	paths = _normalize_files(files)
	command = _build_command(
		paths,
		port=port,
		host=host,
		browser=browser,
		tunnel=tunnel,
		tunnel_host=tunnel_host,
		tunnel_port=tunnel_port,
		recursive=recursive,
		timeout=timeout,
	)

	process = subprocess.Popen(
		command,
		stdout=subprocess.PIPE,
		stderr=subprocess.STDOUT,
		text=True,
		bufsize=1,
	)
	monitor = _OutputMonitor(process)
	monitor.start()

	if block:
		return_code = process.wait()
		monitor.join()
		if return_code != 0:
			raise subprocess.CalledProcessError(return_code, command)
		return None

	monitor.wait_for_url(_URL_WAIT_SECONDS)
	if process.poll() is not None and process.returncode not in (0, None):
		monitor.join()
		raise subprocess.CalledProcessError(int(process.returncode), command)

	return ShutdownHandle(process, monitor)


def _normalize_files(files: PathInput | Iterable[PathInput]) -> list[str]:
	if isinstance(files, (str, os.PathLike)):
		candidates: list[PathInput] = [files]
	else:
		candidates = list(files)

	if not candidates:
		raise ValueError("at least one file path is required")

	normalized: list[str] = []
	for candidate in candidates:
		if not isinstance(candidate, (str, os.PathLike)):
			raise TypeError("files must be path-like values")
		normalized.append(str(Path(candidate)))
	return normalized


def _build_command(
	paths: list[str],
	*,
	port: int,
	host: str,
	browser: bool,
	tunnel: bool,
	tunnel_host: str | None,
	tunnel_port: int,
	recursive: bool,
	timeout: int | None,
) -> list[str]:
	binary = shutil.which("dcmview")
	if binary is None:
		raise RuntimeError("dcmview binary not found — install with: cargo install dcmview")
	if tunnel and not tunnel_host:
		raise ValueError("tunnel_host is required when tunnel=True")

	command = [binary, "--port", str(port), "--host", host]
	if not browser:
		command.append("--no-browser")
	if tunnel:
		command.append("--tunnel")
		command.extend(["--tunnel-host", str(tunnel_host), "--tunnel-port", str(tunnel_port)])
	if timeout is not None:
		command.extend(["--timeout", str(timeout)])
	if not recursive:
		command.append("--no-recursive")
	command.extend(paths)
	return command

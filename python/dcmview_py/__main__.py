from __future__ import annotations

import argparse
import subprocess
import sys
from typing import Sequence

from .wrapper import view


def _build_parser() -> argparse.ArgumentParser:
	parser = argparse.ArgumentParser(prog="python -m dcmview_py")
	parser.add_argument("paths", nargs="+", help="One or more DICOM file or directory paths")
	parser.add_argument("-p", "--port", type=int, default=0)
	parser.add_argument("--host", default="127.0.0.1")
	parser.add_argument("--no-browser", action="store_true")
	parser.add_argument("--tunnel", action="store_true")
	parser.add_argument("--tunnel-host")
	parser.add_argument("--tunnel-port", type=int, default=0)
	parser.add_argument("--timeout", type=int)
	parser.add_argument("--no-recursive", action="store_true")
	return parser


def run_cli(argv: Sequence[str] | None = None) -> int:
	parser = _build_parser()
	args = parser.parse_args(argv)

	try:
		view(
			args.paths,
			port=args.port,
			host=args.host,
			browser=not args.no_browser,
			tunnel=args.tunnel,
			tunnel_host=args.tunnel_host,
			tunnel_port=args.tunnel_port,
			recursive=not args.no_recursive,
			timeout=args.timeout,
			block=True,
		)
	except subprocess.CalledProcessError as error:
		return int(error.returncode)
	except (RuntimeError, ValueError, TypeError) as error:
		print(str(error), file=sys.stderr)
		return 1

	return 0


def main() -> None:
	raise SystemExit(run_cli())


if __name__ == "__main__":
	main()

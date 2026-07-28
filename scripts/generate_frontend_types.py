#!/usr/bin/env python3
from __future__ import annotations

import argparse
import difflib
import pathlib
import sys

from frontend_contract import (
	read_contract_source,
	render,
	validate_source_tree_placement,
)

REPO_ROOT = pathlib.Path(__file__).resolve().parents[1]
OUTPUT = REPO_ROOT / "frontend" / "src" / "generated" / "api-types.ts"


def main() -> int:
	parser = argparse.ArgumentParser(description="Generate frontend TypeScript API types")
	parser.add_argument("--check", action="store_true", help="Fail if generated types are stale")
	args = parser.parse_args()

	try:
		validate_source_tree_placement()
		generated = render(read_contract_source())
	except ValueError as error:
		print(str(error), file=sys.stderr)
		return 1

	if args.check:
		current = OUTPUT.read_text(encoding="utf-8") if OUTPUT.exists() else ""
		if current != generated:
			diff = difflib.unified_diff(
				current.splitlines(),
				generated.splitlines(),
				fromfile=str(OUTPUT),
				tofile="generated",
				lineterm="",
			)
			print("\n".join(diff), file=sys.stderr)
			return 1
		return 0

	OUTPUT.parent.mkdir(parents=True, exist_ok=True)
	OUTPUT.write_text(generated, encoding="utf-8")
	return 0


if __name__ == "__main__":
	raise SystemExit(main())

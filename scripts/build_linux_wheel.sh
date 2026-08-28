#!/usr/bin/env bash
set -euo pipefail

workspace_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
manylinux_image="${MANYLINUX_IMAGE:-quay.io/pypa/manylinux_2_28_x86_64}"
python_bin="${MANYLINUX_PYTHON_BIN:-/opt/python/cp311-cp311/bin/python}"
wheel_plat_name="${DCMVIEW_PY_WHEEL_PLAT_NAME:-manylinux_2_28_x86_64}"
host_uid="$(id -u)"
host_gid="$(id -g)"

cd "$workspace_root"

npm --prefix frontend ci
npm --prefix frontend run build

docker run --rm \
	-v "$workspace_root:/io" \
	-w /io \
	-e DCMVIEW_SKIP_FRONTEND_BUILD=1 \
	-e DCMVIEW_PY_BUNDLE_BINARY=/io/target/release/dcmview \
	-e DCMVIEW_PY_REQUIRE_BUNDLED_BINARY=1 \
	-e DCMVIEW_PY_WHEEL_PLAT_NAME="$wheel_plat_name" \
	-e CMAKE_TOOLCHAIN_FILE=/io/scripts/manylinux-toolchain.cmake \
	-e HOST_UID="$host_uid" \
	-e HOST_GID="$host_gid" \
	"$manylinux_image" \
	/bin/bash -lc "
		set -euo pipefail
		curl https://sh.rustup.rs -sSf | sh -s -- -y --profile minimal --default-toolchain stable
		source /root/.cargo/env
		$python_bin -m pip install --upgrade pip build wheel
		cargo build --release --locked
		$python_bin -m build --wheel
		for path in /io/dist /io/build /io/target /io/python/dcmview_py.egg-info; do
			if [ -e \"\$path\" ]; then
				chown -R \"\$HOST_UID:\$HOST_GID\" \"\$path\"
			fi
		done
	"

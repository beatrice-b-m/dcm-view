from __future__ import annotations

import pathlib
import re

from .model import (
	ENUMS,
	RESPONSE_HEADER_TYPES,
	STRUCTS,
	ApiEndpoint,
	FrameQueryParameter,
	RawFrameHeader,
	validate_endpoint_registry,
	validate_endpoint_type_token,
	validate_endpoint_ownership_invariants,
	validate_frame_query_parameter_invariant,
	validate_raw_frame_header_invariant,
)

REPO_ROOT = pathlib.Path(__file__).resolve().parents[2]
CONTRACTS_FILE = REPO_ROOT / "src" / "api" / "contracts.rs"
CONTRACTS_DIR = REPO_ROOT / "src" / "api" / "contracts"

NON_HTTP_SERDE_TYPES = {
	"BridgeLaunchRequest",
	"BridgeLaunchResponse",
	"BridgeRegistryEntry",
	"BridgeWaitResponse",
	"StartupEvent",
}
SUPPORTED_RENAME_ALL = {"camelCase", "snake_case"}


def contract_source_paths() -> list[pathlib.Path]:
	file_exists = CONTRACTS_FILE.is_file()
	directory_files = (
		sorted(CONTRACTS_DIR.rglob("*.rs")) if CONTRACTS_DIR.is_dir() else []
	)
	if file_exists and directory_files:
		raise ValueError(
			"HTTP contracts must use either src/api/contracts.rs or "
			"src/api/contracts/*.rs, not both"
		)
	if file_exists:
		return [CONTRACTS_FILE]
	if directory_files:
		return directory_files
	raise ValueError(
		"HTTP contract source not found at src/api/contracts.rs or "
		"src/api/contracts/*.rs"
	)


def read_contract_source() -> str:
	return "\n\n".join(
		f"// source: {path.relative_to(REPO_ROOT)}\n{path.read_text(encoding='utf-8')}"
		for path in contract_source_paths()
	)


def snake_case(name: str) -> str:
	return re.sub(r"(?<!^)(?=[A-Z])", "_", name).lower()


def camel_case(name: str) -> str:
	parts = name.split("_")
	return parts[0] + "".join(part.capitalize() for part in parts[1:])


def apply_rename_all(name: str, rename_all: str | None, *, context: str) -> str:
	if rename_all is None:
		return name
	if rename_all not in SUPPORTED_RENAME_ALL:
		raise ValueError(
			f"unsupported serde rename_all for {context}: {rename_all}; "
			f"supported values are {sorted(SUPPORTED_RENAME_ALL)}"
		)
	normalized = snake_case(name)
	if rename_all == "snake_case":
		return normalized
	return camel_case(normalized)


def extract_braced_block(source: str, marker: str) -> str:
	match = re.search(re.escape(marker) + r"(?![A-Za-z0-9_])", source)
	if match is None:
		raise ValueError(f"missing Rust item: {marker}")
	start = match.start()
	open_brace = source.find("{", start)
	if open_brace < 0:
		raise ValueError(f"missing opening brace for {marker}")

	depth = 0
	for index in range(open_brace, len(source)):
		char = source[index]
		if char == "{":
			depth += 1
		elif char == "}":
			depth -= 1
			if depth == 0:
				return source[open_brace + 1 : index]
	raise ValueError(f"missing closing brace for {marker}")


def split_top_level(value: str, separator: str = ",") -> list[str]:
	parts: list[str] = []
	start = 0
	depth_angle = 0
	depth_bracket = 0
	for index, char in enumerate(value):
		if char == "<":
			depth_angle += 1
		elif char == ">":
			depth_angle -= 1
		elif char == "[":
			depth_bracket += 1
		elif char == "]":
			depth_bracket -= 1
		elif char == separator and depth_angle == 0 and depth_bracket == 0:
			parts.append(value[start:index].strip())
			start = index + 1
	parts.append(value[start:].strip())
	return [part for part in parts if part]


def item_attributes(source: str, kind: str, name: str) -> str:
	match = re.search(
		rf"(?P<attrs>(?:#\[[^\]]+\]\s*)*)pub {kind} {re.escape(name)}\b",
		source,
	)
	if not match:
		raise ValueError(f"missing Rust item: pub {kind} {name}")
	return match.group("attrs")


def serde_payloads(attributes: str) -> list[str]:
	return re.findall(r"#\[serde\((.*?)\)\]", attributes, re.DOTALL)


def serde_string_setting(
	attributes: str,
	setting: str,
	*,
	context: str,
) -> str | None:
	payloads = serde_payloads(attributes)
	matches: list[str] = []
	mentions = 0
	for payload in payloads:
		mentions += len(re.findall(rf"\b{re.escape(setting)}\b", payload))
		matches.extend(
			re.findall(
				rf"\b{re.escape(setting)}\s*=\s*\"([^\"]+)\"",
				payload,
			)
		)
	if not mentions:
		return None
	if mentions != len(matches) or len(matches) != 1:
		raise ValueError(
			f"unsupported or duplicate serde {setting} semantics for {context}; "
			f"only `{setting} = \"...\"` is supported"
		)
	return matches[0]


def serde_rename_all(source: str, kind: str, name: str) -> str | None:
	attributes = item_attributes(source, kind, name)
	if any(re.search(r"\brename_all_fields\b", payload) for payload in serde_payloads(attributes)):
		raise ValueError(
			f"serde rename_all_fields is unsupported for {kind} {name}; "
			"declare field-level renames explicitly"
		)
	value = serde_string_setting(
		attributes,
		"rename_all",
		context=f"{kind} {name}",
	)
	if value is not None:
		apply_rename_all("ContractName", value, context=f"{kind} {name}")
	return value


def ensure_only_container_serde_settings(
	source: str,
	kind: str,
	name: str,
	*,
	allowed: set[str],
) -> None:
	attributes = item_attributes(source, kind, name)
	for payload in serde_payloads(attributes):
		remaining = payload
		for setting in allowed:
			remaining = re.sub(
				rf'\b{re.escape(setting)}\s*=\s*"[^"]+"',
				"",
				remaining,
			)
		if re.sub(r"[\s,]", "", remaining):
			raise ValueError(
				f"unsupported top-level serde semantics for {kind} {name}: "
				f"{payload}; supported settings are {sorted(allowed)}"
			)


def explicit_serde_rename(attributes: str, *, context: str) -> str | None:
	return serde_string_setting(attributes, "rename", context=context)


def ensure_only_field_serde_settings(
	attributes: str,
	*,
	context: str,
	allow_skip_serializing_if: bool,
) -> bool:
	optional = False
	for payload in serde_payloads(attributes):
		remaining = re.sub(r'\brename\s*=\s*"[^"]+"', "", payload)
		if allow_skip_serializing_if:
			skip_matches = re.findall(
				r'\bskip_serializing_if\s*=\s*"[^"]+"',
				remaining,
			)
			optional = optional or bool(skip_matches)
			remaining = re.sub(
				r'\bskip_serializing_if\s*=\s*"[^"]+"',
				"",
				remaining,
			)
		if re.sub(r"[\s,]", "", remaining):
			raise ValueError(
				f"unsupported serde field semantics for {context}: {payload}; "
				"only a simple rename"
				+ (" and skip_serializing_if" if allow_skip_serializing_if else "")
				+ " are supported"
			)
	return optional


def field_name(
	source: str,
	struct_name: str,
	rust_name: str,
	attributes: str,
) -> str:
	context = f"{struct_name}.{rust_name}"
	explicit = explicit_serde_rename(attributes, context=context)
	ensure_only_field_serde_settings(
		attributes,
		context=context,
		allow_skip_serializing_if=False,
	)
	if explicit is not None:
		return explicit
	rename_all = serde_rename_all(source, "struct", struct_name)
	return apply_rename_all(
		rust_name,
		rename_all,
		context=f"struct {struct_name}",
	)


def parse_struct(source: str, name: str) -> list[tuple[str, str, str]]:
	body = extract_braced_block(source, f"pub struct {name}")
	fields: list[tuple[str, str, str]] = []
	pending_attributes: list[str] = []
	for line in body.splitlines():
		line = line.strip()
		if line.startswith("#["):
			if not line.endswith("]"):
				raise ValueError(
					f"multiline field attributes are unsupported in {name}; "
					"keep the attribute on one line or extend the generator"
				)
			pending_attributes.append(line)
			continue
		if not line.startswith("pub "):
			continue
		match = re.fullmatch(r"pub\s+([A-Za-z0-9_]+):\s+(.+),", line)
		if not match:
			raise ValueError(f"could not parse field in {name}: {line}")
		fields.append(
			(
				match.group(1),
				match.group(2).strip(),
				"\n".join(pending_attributes),
			)
		)
		pending_attributes.clear()
	if not fields:
		raise ValueError(f"no public fields found for {name}")
	return fields


def variant_name(
	source: str,
	enum_name: str,
	rust_name: str,
	attributes: str,
) -> str:
	context = f"{enum_name}::{rust_name}"
	explicit = explicit_serde_rename(attributes, context=context)
	for payload in serde_payloads(attributes):
		remaining = re.sub(r'\brename\s*=\s*"[^"]+"', "", payload)
		if re.sub(r"[\s,]", "", remaining):
			raise ValueError(
				f"unsupported serde variant semantics for {context}: {payload}; "
				"only a simple rename is supported"
			)
	if explicit is not None:
		return explicit
	return apply_rename_all(
		rust_name,
		serde_rename_all(source, "enum", enum_name),
		context=f"enum {enum_name}",
	)


def enum_tag(source: str, name: str, *, required: bool) -> str | None:
	attributes = item_attributes(source, "enum", name)
	for unsupported in ("content", "untagged"):
		if any(
			re.search(rf"\b{unsupported}\b", payload)
			for payload in serde_payloads(attributes)
		):
			raise ValueError(
				f"unsupported serde enum representation for {name}: {unsupported}"
			)
	tag = serde_string_setting(attributes, "tag", context=f"enum {name}")
	if required and tag is None:
		raise ValueError(f"serde tag is required for generated enum {name}")
	if not required and tag is not None:
		raise ValueError(f"serde tag is unsupported for unit enum {name}")
	return tag


def parse_window_mode_variants(source: str) -> list[str]:
	ensure_only_container_serde_settings(
		source,
		"enum",
		"WindowMode",
		allowed={"rename_all"},
	)
	enum_tag(source, "WindowMode", required=False)
	body = extract_braced_block(source, "pub enum WindowMode")
	variants = []
	pending_attributes: list[str] = []
	for line in body.splitlines():
		line = line.strip().rstrip(",")
		if not line:
			continue
		if line.startswith("#["):
			pending_attributes.append(line)
			continue
		if re.fullmatch(r"[A-Za-z][A-Za-z0-9_]*", line):
			variants.append(
				variant_name(
					source,
					"WindowMode",
					line,
					"\n".join(pending_attributes),
				)
			)
			pending_attributes.clear()
	if not variants:
		raise ValueError("WindowMode variants not found")
	return variants


def parse_variant_fields(
	raw: str,
	enum_name: str,
	variant_name_value: str,
) -> list[tuple[str, str, bool]]:
	fields: list[tuple[str, str, bool]] = []
	pending_attributes: list[str] = []
	for line in raw.splitlines():
		line = line.strip()
		if not line:
			continue
		if line.startswith("#["):
			if not line.endswith("]"):
				raise ValueError(
					f"multiline field attributes are unsupported in "
					f"{enum_name}::{variant_name_value}"
				)
			pending_attributes.append(line)
			continue
		match = re.fullmatch(r"([A-Za-z0-9_]+):\s+(.+),", line)
		if not match:
			continue
		rust_name = match.group(1)
		attributes = "\n".join(pending_attributes)
		context = f"{enum_name}::{variant_name_value}.{rust_name}"
		wire_name = explicit_serde_rename(attributes, context=context) or rust_name
		optional = ensure_only_field_serde_settings(
			attributes,
			context=context,
			allow_skip_serializing_if=True,
		)
		fields.append((wire_name, match.group(2).strip(), optional))
		pending_attributes.clear()
	return fields


def parse_tag_value_variants(
	source: str,
) -> tuple[str, list[tuple[str, list[tuple[str, str, bool]]]]]:
	ensure_only_container_serde_settings(
		source,
		"enum",
		"TagValue",
		allowed={"rename_all", "tag"},
	)
	tag = enum_tag(source, "TagValue", required=True)
	if tag is None:
		raise ValueError("serde tag is required for generated enum TagValue")
	body = extract_braced_block(source, "pub enum TagValue")
	variants: list[tuple[str, list[tuple[str, str, bool]]]] = []
	index = 0
	while index < len(body):
		match = re.search(r"\b([A-Z][A-Za-z0-9_]*)\s*\{", body[index:])
		if not match:
			break
		name = match.group(1)
		attributes = "\n".join(
			re.findall(r"#\[[^\]]+\]", body[index : index + match.start()])
		)
		open_brace = index + match.end() - 1
		depth = 0
		for end in range(open_brace, len(body)):
			if body[end] == "{":
				depth += 1
			elif body[end] == "}":
				depth -= 1
				if depth == 0:
					raw_fields = body[open_brace + 1 : end]
					break
		else:
			raise ValueError(f"unterminated TagValue variant: {name}")

		wire_variant = variant_name(source, "TagValue", name, attributes)
		variants.append(
			(
				wire_variant,
				parse_variant_fields(raw_fields, "TagValue", name),
			)
		)
		index = end + 1

	if not variants:
		raise ValueError("TagValue variants not found")
	return tag, variants


def declaration_blocks(source: str, macro_name: str) -> list[tuple[str, str]]:
	body = extract_braced_block(source, f"{macro_name}!")
	entries: list[tuple[str, str]] = []
	index = 0
	while index < len(body):
		match = re.search(r"\b([A-Z][A-Z0-9_]*)\s*=>\s*\{", body[index:])
		if not match:
			break
		name = match.group(1)
		open_brace = index + match.end() - 1
		depth = 0
		for end in range(open_brace, len(body)):
			if body[end] == "{":
				depth += 1
			elif body[end] == "}":
				depth -= 1
				if depth == 0:
					entries.append((name, body[open_brace + 1 : end]))
					index = end + 1
					break
		else:
			raise ValueError(f"unterminated {macro_name} entry: {name}")
	if not entries:
		raise ValueError(f"{macro_name} declarations not found")
	return entries


def declaration_properties(
	raw: str,
	*,
	context: str,
	expected: set[str],
) -> dict[str, str]:
	properties: dict[str, str] = {}
	for line in raw.splitlines():
		line = line.strip().rstrip(",")
		if not line:
			continue
		match = re.fullmatch(r"([a-z][a-z0-9_]*):\s*(.+)", line)
		if not match:
			raise ValueError(f"could not parse {context} property: {line}")
		key, value = match.groups()
		if key in properties:
			raise ValueError(f"duplicate {context} property: {key}")
		properties[key] = value.strip()
	missing = sorted(expected - properties.keys())
	extra = sorted(properties.keys() - expected)
	if missing or extra:
		raise ValueError(
			f"{context} properties differ from the generator contract: "
			f"missing={missing}, extra={extra}"
		)
	return properties


def rust_string(value: str, *, context: str) -> str:
	match = re.fullmatch(r'"([^"]*)"', value)
	if not match:
		raise ValueError(f"{context} must be a string literal, found: {value}")
	return match.group(1)


def rust_optional_string(value: str, *, context: str) -> str | None:
	if value == "None":
		return None
	match = re.fullmatch(r'Some\("([^"]*)"\)', value)
	if not match:
		raise ValueError(
			f"{context} must be None or Some(\"...\"), found: {value}"
		)
	return match.group(1)


def rust_string_constant(source: str, name: str) -> str:
	match = re.search(rf'pub const {name}: &str = "([^"]+)";', source)
	if not match:
		raise ValueError(f"{name} constant not found")
	return match.group(1)


def parse_raw_frame_headers(source: str) -> list[RawFrameHeader]:
	headers: list[RawFrameHeader] = []
	for constant, raw in declaration_blocks(source, "define_raw_frame_headers"):
		properties = declaration_properties(
			raw,
			context=constant,
			expected={"field", "name"},
		)
		headers.append(
			RawFrameHeader(
				constant=constant,
				field=rust_string(
					properties["field"],
					context=f"{constant}.field",
				),
				name=rust_string(
					properties["name"],
					context=f"{constant}.name",
				),
			)
		)
	return headers


def parse_frame_query_parameters(source: str) -> list[FrameQueryParameter]:
	parameters: list[FrameQueryParameter] = []
	for constant, raw in declaration_blocks(source, "define_frame_query_parameters"):
		properties = declaration_properties(
			raw,
			context=constant,
			expected={"client_key", "wire_name"},
		)
		parameters.append(
			FrameQueryParameter(
				constant=constant,
				client_key=rust_string(
					properties["client_key"],
					context=f"{constant}.client_key",
				),
				wire_name=rust_string(
					properties["wire_name"],
					context=f"{constant}.wire_name",
				),
			)
		)
	return parameters


def rust_type_token(value: str, *, context: str) -> str:
	value = value.strip()
	if not re.fullmatch(
		r"[A-Za-z_][A-Za-z0-9_]*(?:\s*<\s*[A-Za-z_][A-Za-z0-9_]*\s*>)?",
		value,
	):
		raise ValueError(f"{context} must be a Rust type token, found: {value}")
	return re.sub(r"\s+", "", value)


def parse_api_endpoints(source: str) -> list[ApiEndpoint]:
	prefix_match = re.search(r'pub const API_PREFIX: &str = "([^"]+)";', source)
	if not prefix_match:
		raise ValueError("API_PREFIX constant not found")
	prefix = prefix_match.group(1)

	expected = {
		"operation",
		"id",
		"method",
		"path",
		"query_type",
		"request_type",
		"request_media_type",
		"response_type",
		"response_media_type",
		"response_headers_type",
		"error_type",
		"success_status",
	}
	endpoints: list[ApiEndpoint] = []
	for constant, raw in declaration_blocks(source, "define_api_endpoints"):
		properties = declaration_properties(
			raw,
			context=constant,
			expected=expected,
		)
		methods = {"Get": "GET", "Put": "PUT"}
		method = properties["method"]
		if method not in methods:
			raise ValueError(f"unsupported API method for {constant}: {method}")
		path = prefix + rust_string(properties["path"], context=f"{constant}.path")
		params = re.findall(r"\{([A-Za-z0-9_]+)\}", path)
		if len(params) != len(set(params)):
			raise ValueError(f"duplicate path parameter for {constant}: {path}")
		unsupported_params = sorted(set(params) - {"frame", "index"})
		if unsupported_params:
			raise ValueError(
				f"{constant} has path parameters without declared TypeScript semantics: "
				+ ", ".join(unsupported_params)
			)
		query_type_token = rust_type_token(
			properties["query_type"],
			context=f"{constant}.query_type",
		)
		request_type_token = rust_type_token(
			properties["request_type"],
			context=f"{constant}.request_type",
		)
		response_headers_type_token = rust_type_token(
			properties["response_headers_type"],
			context=f"{constant}.response_headers_type",
		)
		response_type_token = rust_type_token(
			properties["response_type"],
			context=f"{constant}.response_type",
		)
		error_type_token = rust_type_token(
			properties["error_type"],
			context=f"{constant}.error_type",
		)
		for label, token in (
			("query_type", query_type_token),
			("request_type", request_type_token),
			("response_type", response_type_token),
			("response_headers_type", response_headers_type_token),
			("error_type", error_type_token),
		):
			validate_endpoint_type_token(token, context=f"{constant}.{label}")
		try:
			success_status = int(properties["success_status"])
		except ValueError as error:
			raise ValueError(
				f"{constant}.success_status must be an integer literal"
			) from error
		if not 200 <= success_status <= 299:
			raise ValueError(
				f"{constant}.success_status must be a successful HTTP status"
			)
		request_media_type = rust_optional_string(
			properties["request_media_type"],
			context=f"{constant}.request_media_type",
		)
		has_request = request_type_token != "NoRequest"
		if has_request != (request_media_type is not None):
			raise ValueError(
				f"{constant} must declare request_type and request_media_type together"
			)
		if method == "Get" and has_request:
			raise ValueError(
				f"{constant} declares a body-bearing GET, which the generated client "
				"does not support"
			)
		response_media_type = rust_string(
			properties["response_media_type"],
			context=f"{constant}.response_media_type",
		)
		if request_media_type is not None and request_media_type != "application/json":
			raise ValueError(
				f"{constant} declares unsupported request media type "
				f"{request_media_type}; the generated client only encodes application/json"
			)
		if response_media_type == "application/json" and query_type_token != "NoQuery":
			raise ValueError(
				f"{constant} declares query parameters on a JSON endpoint, which "
				"requestJsonEndpoint cannot encode"
			)
		endpoint_id = rust_string(properties["id"], context=f"{constant}.id")
		if query_type_token != "NoQuery" and endpoint_id != "fileFrame":
			raise ValueError(
				f"{constant} declares query parameters without a dedicated client encoder"
			)
		if response_media_type != "application/json" and has_request:
			raise ValueError(
				f"{constant} declares a request body on a non-JSON response endpoint "
				"without a dedicated client encoder"
			)
		if response_headers_type_token not in RESPONSE_HEADER_TYPES:
			raise ValueError(
				f"{constant}.response_headers_type must be a declared response-header "
				f"group, found: {response_headers_type_token}"
			)
		if error_type_token != "ErrorResponse":
			raise ValueError(
				f"{constant}.error_type must use the common ErrorResponse contract"
			)
		endpoints.append(
			ApiEndpoint(
				constant=constant,
				operation=properties["operation"],
				id=endpoint_id,
				method=methods[method],
				path=path,
				params=tuple(params),
				query_type_token=query_type_token,
				request_type_token=request_type_token,
				request_media_type=request_media_type,
				response_type_token=response_type_token,
				response_media_type=response_media_type,
				response_headers_type_token=response_headers_type_token,
				error_type_token=error_type_token,
				success_status=success_status,
			)
		)

	validate_endpoint_registry(endpoints)
	return endpoints


def serde_items(source: str, *, public_only: bool) -> set[str]:
	wire_items: set[str] = set()
	public_visibility = r"pub(?:\s*\([^)]*\))?\s+"
	visibility = public_visibility if public_only else rf"(?:{public_visibility})?"
	for match in re.finditer(
		rf"(?P<attrs>(?:#\[[^\]]+\]\s*)*){visibility}"
		r"(?P<kind>struct|enum) (?P<name>[A-Za-z0-9_]+)",
		source,
	):
		derive = re.search(r"#\[derive\(([^)]*)\)\]", match.group("attrs"))
		if derive and re.search(r"\b(?:Serialize|Deserialize)\b", derive.group(1)):
			wire_items.add(match.group("name"))
	return wire_items


def validate_non_http_serde_sources(
	sources: dict[pathlib.Path, str],
	*,
	allowed_types: set[str] = NON_HTTP_SERDE_TYPES,
) -> None:
	locations: dict[str, list[pathlib.Path]] = {}
	for path, source in sources.items():
		found = serde_items(source, public_only=False)
		unclassified = sorted(found - allowed_types)
		if unclassified:
			raise ValueError(
				f"serde items outside src/api/contracts must be classified as non-HTTP "
				f"({path}): {', '.join(unclassified)}"
			)
		for name in found:
			locations.setdefault(name, []).append(path)

	duplicates = {
		name: paths
		for name, paths in locations.items()
		if name in allowed_types and len(paths) > 1
	}
	if duplicates:
		details = "; ".join(
			f"{name}: {', '.join(str(path) for path in paths)}"
			for name, paths in sorted(duplicates.items())
		)
		raise ValueError(
			"non-HTTP serde type names must be unique across source files: "
			+ details
		)

	stale = sorted(allowed_types - locations.keys())
	if stale:
		raise ValueError("stale non-HTTP serde classifications: " + ", ".join(stale))


def validate_struct_coverage(source: str) -> None:
	wire_items = serde_items(source, public_only=False)
	generated = set(STRUCTS) | set(ENUMS)
	missing = sorted(wire_items - generated)
	if missing:
		raise ValueError(
			"serde HTTP contract items are not generated: "
			+ ", ".join(missing)
		)
	stale = sorted(generated - wire_items)
	if stale:
		raise ValueError(
			"configured generated items are not serde HTTP contracts: " + ", ".join(stale)
		)


def validate_source_tree_placement() -> None:
	contract_paths = set(contract_source_paths())
	sources = {
		path.relative_to(REPO_ROOT): path.read_text(encoding="utf-8")
		for path in sorted((REPO_ROOT / "src").rglob("*.rs"))
		if path not in contract_paths
	}
	validate_non_http_serde_sources(sources)


def validate_field_constant_coverage(source: str) -> None:
	raw_fields = {
		field_name(source, "RawFrameMetadata", field, attributes)
		for field, _, attributes in parse_struct(source, "RawFrameMetadata")
	}
	validate_raw_frame_header_invariant(
		raw_metadata_fields=raw_fields,
		raw_frame_headers=parse_raw_frame_headers(source),
	)
	frame_query_fields = {
		field_name(source, "FrameQuery", field, attributes)
		for field, _, attributes in parse_struct(source, "FrameQuery")
	}
	validate_frame_query_parameter_invariant(
		frame_query_fields=frame_query_fields,
		frame_query_parameters=parse_frame_query_parameters(source),
	)
	validate_endpoint_ownership_invariants(parse_api_endpoints(source))

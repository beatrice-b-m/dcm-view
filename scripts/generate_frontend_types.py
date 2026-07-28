#!/usr/bin/env python3
from __future__ import annotations

import argparse
import difflib
import pathlib
import re
import sys

REPO_ROOT = pathlib.Path(__file__).resolve().parents[1]
CONTRACTS_FILE = REPO_ROOT / "src" / "api" / "contracts.rs"
CONTRACTS_DIR = REPO_ROOT / "src" / "api" / "contracts"
OUTPUT = REPO_ROOT / "frontend" / "src" / "generated" / "api-types.ts"

STRUCTS = [
	"WindowPreset",
	"FileSummary",
	"FilesResponse",
	"FrameInfo",
	"HealthResponse",
	"FrameQuery",
	"EmbedRoiAnnotations",
	"RawFrameMetadata",
	"TagNode",
	"ErrorResponse",
]
ENUMS = ["WindowMode", "TagValue"]
INPUT_STRUCTS = {"FrameQuery"}
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


def ts_type(rust_type: str, *, option_as_optional: bool = False) -> str:
	rust_type = rust_type.strip()
	if rust_type.startswith("Option<") and rust_type.endswith(">"):
		inner = ts_type(rust_type[len("Option<") : -1])
		return inner if option_as_optional else f"{inner} | null"
	if rust_type.startswith("Vec<") and rust_type.endswith(">"):
		inner = ts_type(rust_type[len("Vec<") : -1])
		return f"{inner}[]"
	if rust_type.startswith("[") and rust_type.endswith("]"):
		body = rust_type[1:-1]
		inner, length = [part.strip() for part in body.split(";")]
		return "[" + ", ".join([ts_type(inner)] * int(length)) + "]"
	if rust_type in {"usize", "u64", "u32", "u16", "i32", "f64"}:
		return "number"
	if rust_type == "bool":
		return "boolean"
	if rust_type in {"String", "&str", "&'static str"}:
		return "string"
	if rust_type in {
		"WindowPreset",
		"FileSummary",
		"TagNode",
		"TagValue",
		"WindowMode",
	}:
		return rust_type
	raise ValueError(f"unsupported Rust type: {rust_type}")


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


def render_struct(source: str, name: str) -> str:
	ensure_only_container_serde_settings(
		source,
		"struct",
		name,
		allowed={"rename_all"},
	)
	lines = [f"export interface {name} {{"]
	for field, rust_type, attributes in parse_struct(source, name):
		optional = name in INPUT_STRUCTS and rust_type.startswith("Option<")
		suffix = "?" if optional else ""
		lines.append(
			f"\t{field_name(source, name, field, attributes)}{suffix}: "
			f"{ts_type(rust_type, option_as_optional=optional)};"
		)
	lines.append("}")
	return "\n".join(lines)


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


def render_window_mode(source: str) -> str:
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
	return "export type WindowMode = " + " | ".join(f'"{variant}"' for variant in variants) + ";"


def parse_variant_fields(raw: str, enum_name: str, variant_name: str) -> list[tuple[str, str, bool]]:
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
					f"{enum_name}::{variant_name}"
				)
			pending_attributes.append(line)
			continue
		match = re.fullmatch(r"([A-Za-z0-9_]+):\s+(.+),", line)
		if not match:
			continue
		rust_name = match.group(1)
		attributes = "\n".join(pending_attributes)
		context = f"{enum_name}::{variant_name}.{rust_name}"
		wire_name = explicit_serde_rename(attributes, context=context) or rust_name
		optional = ensure_only_field_serde_settings(
			attributes,
			context=context,
			allow_skip_serializing_if=True,
		)
		fields.append((wire_name, match.group(2).strip(), optional))
		pending_attributes.clear()
	return fields


def render_tag_value(source: str) -> str:
	ensure_only_container_serde_settings(
		source,
		"enum",
		"TagValue",
		allowed={"rename_all", "tag"},
	)
	tag = enum_tag(source, "TagValue", required=True)
	body = extract_braced_block(source, "pub enum TagValue")
	variants: list[str] = []
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
		fields = [f'{tag}: "{wire_variant}"']
		for field, rust_type, optional in parse_variant_fields(
			raw_fields,
			"TagValue",
			name,
		):
			suffix = "?" if optional else ""
			fields.append(f"{field}{suffix}: {ts_type(rust_type, option_as_optional=optional)}")
		variants.append("\t| { " + "; ".join(fields) + " }")
		index = end + 1

	if not variants:
		raise ValueError("TagValue variants not found")
	return "export type TagValue =\n" + "\n".join(variants) + ";"


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


def parse_raw_frame_headers(source: str) -> list[dict[str, str]]:
	headers: list[dict[str, str]] = []
	for constant, raw in declaration_blocks(source, "define_raw_frame_headers"):
		properties = declaration_properties(
			raw,
			context=constant,
			expected={"field", "name"},
		)
		headers.append(
			{
				"constant": constant,
				"field": rust_string(
					properties["field"],
					context=f"{constant}.field",
				),
				"name": rust_string(
					properties["name"],
					context=f"{constant}.name",
				),
			}
		)
	return headers


def render_raw_frame_headers(source: str) -> str:
	lines = ["export const RAW_FRAME_HEADERS = {"]
	for header in parse_raw_frame_headers(source):
		lines.append(f'\t{header["field"]}: "{header["name"]}",')
	lines.append("} as const;")
	return "\n".join(lines)


def parse_frame_query_parameters(source: str) -> list[dict[str, str]]:
	parameters: list[dict[str, str]] = []
	for constant, raw in declaration_blocks(source, "define_frame_query_parameters"):
		properties = declaration_properties(
			raw,
			context=constant,
			expected={"client_key", "wire_name"},
		)
		parameters.append(
			{
				"constant": constant,
				"client_key": rust_string(
					properties["client_key"],
					context=f"{constant}.client_key",
				),
				"wire_name": rust_string(
					properties["wire_name"],
					context=f"{constant}.wire_name",
				),
			}
		)
	return parameters


ENDPOINT_TYPE_MARKERS = {
	"NoQuery": "never",
	"NoRequest": "never",
	"BlobBody": "Blob",
	"ArrayBufferBody": "ArrayBuffer",
	"NoResponseHeaders": "never",
	"CacheResponseHeaders": "CacheResponseHeaders",
	"RawFrameResponseHeaders": "RawFrameResponseHeaders",
	"ExportResponseHeaders": "ExportResponseHeaders",
}
RESPONSE_HEADER_TYPES = {
	"NoResponseHeaders",
	"CacheResponseHeaders",
	"RawFrameResponseHeaders",
	"ExportResponseHeaders",
}


def rust_type_token(value: str, *, context: str) -> str:
	value = value.strip()
	if not re.fullmatch(
		r"[A-Za-z_][A-Za-z0-9_]*(?:\s*<\s*[A-Za-z_][A-Za-z0-9_]*\s*>)?",
		value,
	):
		raise ValueError(f"{context} must be a Rust type token, found: {value}")
	return re.sub(r"\s+", "", value)


def endpoint_ts_type(value: str, *, context: str) -> str:
	if value in ENDPOINT_TYPE_MARKERS:
		return ENDPOINT_TYPE_MARKERS[value]
	if value.startswith("Vec<") and value.endswith(">"):
		inner = endpoint_ts_type(value[4:-1], context=context)
		if inner == "never":
			raise ValueError(f"invalid vector wire type for {context}: {value}")
		return f"{inner}[]"
	if value not in set(STRUCTS) | set(ENUMS):
		raise ValueError(f"unknown TypeScript wire type for {context}: {value}")
	return value


def parse_api_endpoints(source: str) -> list[dict[str, object]]:
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
	endpoints: list[dict[str, object]] = []
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
		query_type = endpoint_ts_type(
			query_type_token,
			context=f"{constant}.query_type",
		)
		request_type = endpoint_ts_type(
			request_type_token,
			context=f"{constant}.request_type",
		)
		response_type = endpoint_ts_type(
			response_type_token,
			context=f"{constant}.response_type",
		)
		response_headers_type = endpoint_ts_type(
			response_headers_type_token,
			context=f"{constant}.response_headers_type",
		)
		error_type = endpoint_ts_type(
			error_type_token,
			context=f"{constant}.error_type",
		)
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
			{
				"constant": constant,
				"operation": properties["operation"],
				"id": endpoint_id,
				"method": methods[method],
				"path": path,
				"params": params,
				"query_type": query_type,
				"query_type_token": query_type_token,
				"request_type": request_type,
				"request_type_token": request_type_token,
				"request_media_type": request_media_type,
				"response_type": response_type,
				"response_type_token": response_type_token,
				"response_media_type": response_media_type,
				"response_headers_type": response_headers_type,
				"response_headers_type_token": response_headers_type_token,
				"error_type": error_type,
				"error_type_token": error_type_token,
				"success_status": success_status,
			}
		)

	ids = [str(endpoint["id"]) for endpoint in endpoints]
	if len(ids) != len(set(ids)):
		raise ValueError("API endpoint ids must be unique")
	operations = [str(endpoint["operation"]) for endpoint in endpoints]
	if len(operations) != len(set(operations)):
		raise ValueError("API endpoint operations must be unique")
	method_paths = [
		(str(endpoint["method"]), str(endpoint["path"]))
		for endpoint in endpoints
	]
	if len(method_paths) != len(set(method_paths)):
		raise ValueError("API endpoint method/path pairs must be unique")
	return endpoints


def render_frame_query_keys(source: str) -> str:
	lines = ["export const FRAME_QUERY_KEYS = {"]
	for parameter in parse_frame_query_parameters(source):
		lines.append(
			f'\t{parameter["client_key"]}: "{parameter["wire_name"]}",'
		)
	lines.append("} as const;")
	return "\n".join(lines)


def endpoint_params_type(endpoint: dict[str, object]) -> str:
	params = endpoint["params"]
	if not isinstance(params, list):
		raise ValueError("endpoint params must be a list")
	if not params:
		return "Record<string, never>"
	return "{ " + "; ".join(f"{param}: number" for param in params) + " }"


def render_api_endpoints(source: str) -> str:
	endpoints = parse_api_endpoints(source)
	lines = ["export interface ApiEndpointTypes {"]
	for endpoint in endpoints:
		lines.extend(
			[
				f'\t{endpoint["id"]}: {{',
				f"\t\tparams: {endpoint_params_type(endpoint)};",
				f'\t\tquery: {endpoint["query_type"]};',
				f'\t\trequest: {endpoint["request_type"]};',
				f'\t\tresponse: {endpoint["response_type"]};',
				f'\t\tresponseHeaders: {endpoint["response_headers_type"]};',
				f'\t\terror: {endpoint["error_type"]};',
				"\t};",
			]
		)
	lines.extend(
		[
			"}",
			"",
			"export type ApiEndpointId = keyof ApiEndpointTypes;",
			"export type ApiEndpointParams<Id extends ApiEndpointId> = "
			'ApiEndpointTypes[Id]["params"];',
			"export type ApiEndpointQuery<Id extends ApiEndpointId> = "
			'ApiEndpointTypes[Id]["query"];',
			"export type ApiEndpointRequest<Id extends ApiEndpointId> = "
			'ApiEndpointTypes[Id]["request"];',
			"export type ApiEndpointResponse<Id extends ApiEndpointId> = "
			'ApiEndpointTypes[Id]["response"];',
			"export type ApiEndpointResponseHeaders<Id extends ApiEndpointId> = "
			'ApiEndpointTypes[Id]["responseHeaders"];',
			"export type ApiEndpointError<Id extends ApiEndpointId> = "
			'ApiEndpointTypes[Id]["error"];',
			"",
			"export const API_ENDPOINTS = {",
		]
	)
	for endpoint in endpoints:
		request_media = endpoint["request_media_type"]
		request_media_literal = (
			f'"{request_media}"' if request_media is not None else "null"
		)
		query_type = endpoint["query_type"]
		query_type_literal = f'"{query_type}"' if query_type != "never" else "null"
		request_type = endpoint["request_type"]
		request_type_literal = (
			f'"{request_type}"' if request_type != "never" else "null"
		)
		headers_type = endpoint["response_headers_type"]
		headers_type_literal = (
			f'"{headers_type}"' if headers_type != "never" else "null"
		)
		error_type = endpoint["error_type"]
		lines.extend(
			[
				f'\t{endpoint["id"]}: {{',
				f'\t\tmethod: "{endpoint["method"]}",',
				f'\t\tpath: "{endpoint["path"]}",',
				f"\t\tqueryType: {query_type_literal},",
				f"\t\trequestType: {request_type_literal},",
				f"\t\trequestMediaType: {request_media_literal},",
				f'\t\tresponseType: "{endpoint["response_type"]}",',
				f'\t\tresponseMediaType: "{endpoint["response_media_type"]}",',
				f"\t\tresponseHeadersType: {headers_type_literal},",
				f'\t\terrorType: "{error_type}",',
				f'\t\tsuccessStatus: {endpoint["success_status"]},',
				"\t},",
			]
		)
	lines.extend(
		[
			"} as const;",
			"",
			"export type JsonApiEndpointId = {",
			"\t[Id in ApiEndpointId]: "
			'(typeof API_ENDPOINTS)[Id]["responseMediaType"] extends '
			'"application/json" ? Id : never;',
			"}[ApiEndpointId];",
			"",
			"export type GetApiEndpointId = {",
			"\t[Id in ApiEndpointId]: "
			'(typeof API_ENDPOINTS)[Id]["method"] extends "GET" ? Id : never;',
			"}[ApiEndpointId];",
			"",
			"export function apiEndpointPath<Id extends ApiEndpointId>(",
			"\tid: Id,",
			"\tparams: ApiEndpointParams<Id>,",
			"): string {",
			"\tlet path: string = API_ENDPOINTS[id].path;",
			"\tfor (const [name, value] of Object.entries(",
			"\t\tparams as Record<string, number>,",
			"\t)) {",
			"\t\tpath = path.replace(`{${name}}`, encodeURIComponent(String(value)));",
			"\t}",
			'\tif (path.includes("{")) {',
			"\t\tthrow new Error(`missing API path parameter for ${id}`);",
			"\t}",
			"\treturn path;",
			"}",
			"",
			"export function getApiEndpointPath<Id extends GetApiEndpointId>(",
			"\tid: Id,",
			"\tparams: ApiEndpointParams<Id>,",
			"): string {",
			"\tconst method: string = API_ENDPOINTS[id].method;",
			'\tif (method !== "GET") {',
			"\t\tthrow new Error(`endpoint ${id} is not a GET endpoint`);",
			"\t}",
			"\treturn apiEndpointPath(id, params);",
			"}",
		]
	)
	return "\n".join(lines)


def render_cache_contract(source: str) -> str:
	values: dict[str, str] = {}
	for name in ("CACHE_HEADER", "CACHE_HIT", "CACHE_MISS"):
		match = re.search(rf'pub const {name}: &str = "([^"]+)";', source)
		if not match:
			raise ValueError(f"{name} constant not found")
		values[name] = match.group(1)
	return "\n".join(
		[
			f'export const CACHE_HEADER = "{values["CACHE_HEADER"]}";',
			"",
			"export const CACHE_STATUS = {",
			f'\thit: "{values["CACHE_HIT"]}",',
			f'\tmiss: "{values["CACHE_MISS"]}",',
			"} as const;",
		]
	)


def render_response_header_types(source: str) -> str:
	values: dict[str, str] = {}
	for name in (
		"CACHE_HIT",
		"CACHE_MISS",
		"EXPORT_CONTENT_DISPOSITION_HEADER",
	):
		match = re.search(rf'pub const {name}: &str = "([^"]+)";', source)
		if not match:
			raise ValueError(f"{name} constant not found")
		values[name] = match.group(1)
	return "\n".join(
		[
			"export interface CacheResponseHeaders {",
			f'\tcache: "{values["CACHE_HIT"]}" | "{values["CACHE_MISS"]}";',
			"}",
			"",
			"export interface RawFrameResponseHeaders {",
			f'\tcache: "{values["CACHE_HIT"]}" | "{values["CACHE_MISS"]}";',
			"\tmetadata: RawFrameMetadata;",
			"}",
			"",
			"export interface ExportResponseHeaders {",
			"\tcontentDisposition: string;",
			"}",
		]
	)


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
	raw_header_fields = {
		header["field"] for header in parse_raw_frame_headers(source)
	}
	if raw_fields != raw_header_fields:
		raise ValueError(
			"RawFrameMetadata fields and raw header constants differ: "
			f"metadata-only={sorted(raw_fields - raw_header_fields)}, "
			f"header-only={sorted(raw_header_fields - raw_fields)}"
		)

	frame_query_fields = {
		field_name(source, "FrameQuery", field, attributes)
		for field, _, attributes in parse_struct(source, "FrameQuery")
	}
	frame_query_keys = {
		parameter["wire_name"]
		for parameter in parse_frame_query_parameters(source)
	}
	if frame_query_fields != frame_query_keys:
		raise ValueError(
			"FrameQuery fields and query constants differ: "
			f"query-only={sorted(frame_query_fields - frame_query_keys)}, "
			f"constant-only={sorted(frame_query_keys - frame_query_fields)}"
		)

	endpoints = parse_api_endpoints(source)
	frame_endpoint = next(
		(endpoint for endpoint in endpoints if endpoint["id"] == "fileFrame"),
		None,
	)
	if frame_endpoint is None or frame_endpoint["query_type"] != "FrameQuery":
		raise ValueError("fileFrame must own the FrameQuery wire contract")
	raw_endpoint = next(
		(endpoint for endpoint in endpoints if endpoint["id"] == "fileRawFrame"),
		None,
	)
	if (
		raw_endpoint is None
		or raw_endpoint["response_headers_type"] != "RawFrameResponseHeaders"
	):
		raise ValueError(
			"fileRawFrame must own the RawFrameResponseHeaders contract"
		)
	display_endpoint = next(
		(endpoint for endpoint in endpoints if endpoint["id"] == "fileFrame"),
		None,
	)
	if (
		display_endpoint is None
		or display_endpoint["response_headers_type"] != "CacheResponseHeaders"
	):
		raise ValueError("fileFrame must own the CacheResponseHeaders contract")
	export_endpoint = next(
		(endpoint for endpoint in endpoints if endpoint["id"] == "annotationsExport"),
		None,
	)
	if (
		export_endpoint is None
		or export_endpoint["response_headers_type"] != "ExportResponseHeaders"
	):
		raise ValueError(
			"annotationsExport must own the ExportResponseHeaders contract"
		)


def render(source: str) -> str:
	validate_struct_coverage(source)
	validate_field_constant_coverage(source)
	sections = [
		"// Generated by scripts/generate_frontend_types.py from src/api/contracts.\n"
		"// Do not edit this file directly.",
		render_api_endpoints(source),
		render_response_header_types(source),
		render_frame_query_keys(source),
		render_cache_contract(source),
		render_window_mode(source),
	]
	sections.extend(render_struct(source, name) for name in STRUCTS[:8])
	sections.append(render_raw_frame_headers(source))
	sections.append(render_tag_value(source))
	sections.extend(render_struct(source, name) for name in STRUCTS[8:])
	return "\n\n".join(sections) + "\n"


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

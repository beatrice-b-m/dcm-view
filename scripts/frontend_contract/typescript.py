from __future__ import annotations

from .model import (
	ENDPOINT_TYPE_MARKERS,
	ENUMS,
	INPUT_STRUCTS,
	STRUCTS,
	ApiEndpoint,
)
from .rust_parser import (
	ensure_only_container_serde_settings,
	field_name,
	parse_api_endpoints,
	parse_frame_query_parameters,
	parse_raw_frame_headers,
	parse_struct,
	parse_tag_value_variants,
	parse_unit_enum_variants,
	parse_window_mode_variants,
	rust_string_constant,
	validate_field_constant_coverage,
	validate_struct_coverage,
)


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
	if rust_type in set(STRUCTS) | set(ENUMS):
		return rust_type
	raise ValueError(f"unsupported Rust type: {rust_type}")


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


def render_window_mode(source: str) -> str:
	variants = parse_window_mode_variants(source)
	return "export type WindowMode = " + " | ".join(f'"{variant}"' for variant in variants) + ";"


def render_api_error_code(source: str) -> str:
	variants = parse_unit_enum_variants(source, "ApiErrorCode")
	return "export type ApiErrorCode = " + " | ".join(f'"{variant}"' for variant in variants) + ";"


def render_support_state(source: str) -> str:
	variants = parse_unit_enum_variants(source, "SupportState")
	return "export type SupportState = " + " | ".join(f'"{variant}"' for variant in variants) + ";"


def render_tag_value(source: str) -> str:
	tag, parsed_variants = parse_tag_value_variants(source)
	variants: list[str] = []
	for wire_variant, parsed_fields in parsed_variants:
		fields = [f'{tag}: "{wire_variant}"']
		for field, rust_type, optional in parsed_fields:
			suffix = "?" if optional else ""
			fields.append(f"{field}{suffix}: {ts_type(rust_type, option_as_optional=optional)}")
		variants.append("\t| { " + "; ".join(fields) + " }")
	return "export type TagValue =\n" + "\n".join(variants) + ";"


def render_semantic_context() -> str:
	return "\n".join(
		[
			"export type SemanticContext =",
			'\t| ({ kind: "segmentation" } & SegmentationContext)',
			'\t| ({ kind: "parametric_map" } & ParametricMapContext)',
			'\t| ({ kind: "rt_dose" } & RtDoseContext)',
			'\t| { kind: "not_applicable"; reason: string };',
		]
	)


def render_raw_frame_headers(source: str) -> str:
	lines = ["export const RAW_FRAME_HEADERS = {"]
	for header in parse_raw_frame_headers(source):
		lines.append(f'\t{header.field}: "{header.name}",')
	lines.append("} as const;")
	return "\n".join(lines)


def render_frame_query_keys(source: str) -> str:
	lines = ["export const FRAME_QUERY_KEYS = {"]
	for parameter in parse_frame_query_parameters(source):
		lines.append(
			f'\t{parameter.client_key}: "{parameter.wire_name}",'
		)
	lines.append("} as const;")
	return "\n".join(lines)


def endpoint_params_type(endpoint: ApiEndpoint) -> str:
	if not endpoint.params:
		return "Record<string, never>"
	return "{ " + "; ".join(f"{param}: number" for param in endpoint.params) + " }"


def render_api_endpoints(source: str) -> str:
	endpoints = parse_api_endpoints(source)
	lines = ["export interface ApiEndpointTypes {"]
	for endpoint in endpoints:
		query_type = endpoint_ts_type(
			endpoint.query_type_token,
			context=f"{endpoint.constant}.query_type",
		)
		request_type = endpoint_ts_type(
			endpoint.request_type_token,
			context=f"{endpoint.constant}.request_type",
		)
		response_type = endpoint_ts_type(
			endpoint.response_type_token,
			context=f"{endpoint.constant}.response_type",
		)
		response_headers_type = endpoint_ts_type(
			endpoint.response_headers_type_token,
			context=f"{endpoint.constant}.response_headers_type",
		)
		error_type = endpoint_ts_type(
			endpoint.error_type_token,
			context=f"{endpoint.constant}.error_type",
		)
		lines.extend(
			[
				f"\t{endpoint.id}: {{",
				f"\t\tparams: {endpoint_params_type(endpoint)};",
				f"\t\tquery: {query_type};",
				f"\t\trequest: {request_type};",
				f"\t\tresponse: {response_type};",
				f"\t\tresponseHeaders: {response_headers_type};",
				f"\t\terror: {error_type};",
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
		query_type = endpoint_ts_type(
			endpoint.query_type_token,
			context=f"{endpoint.constant}.query_type",
		)
		request_type = endpoint_ts_type(
			endpoint.request_type_token,
			context=f"{endpoint.constant}.request_type",
		)
		response_type = endpoint_ts_type(
			endpoint.response_type_token,
			context=f"{endpoint.constant}.response_type",
		)
		headers_type = endpoint_ts_type(
			endpoint.response_headers_type_token,
			context=f"{endpoint.constant}.response_headers_type",
		)
		error_type = endpoint_ts_type(
			endpoint.error_type_token,
			context=f"{endpoint.constant}.error_type",
		)
		request_media_literal = (
			f'"{endpoint.request_media_type}"'
			if endpoint.request_media_type is not None
			else "null"
		)
		query_type_literal = f'"{query_type}"' if query_type != "never" else "null"
		request_type_literal = (
			f'"{request_type}"' if request_type != "never" else "null"
		)
		headers_type_literal = (
			f'"{headers_type}"' if headers_type != "never" else "null"
		)
		lines.extend(
			[
				f"\t{endpoint.id}: {{",
				f'\t\tmethod: "{endpoint.method}",',
				f'\t\tpath: "{endpoint.path}",',
				f"\t\tqueryType: {query_type_literal},",
				f"\t\trequestType: {request_type_literal},",
				f"\t\trequestMediaType: {request_media_literal},",
				f'\t\tresponseType: "{response_type}",',
				f'\t\tresponseMediaType: "{endpoint.response_media_type}",',
				f"\t\tresponseHeadersType: {headers_type_literal},",
				f'\t\terrorType: "{error_type}",',
				f"\t\tsuccessStatus: {endpoint.success_status},",
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
	values = {
		name: rust_string_constant(source, name)
		for name in ("CACHE_HEADER", "CACHE_HIT", "CACHE_MISS")
	}
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
	values = {
		name: rust_string_constant(source, name)
		for name in (
			"CACHE_HIT",
			"CACHE_MISS",
			"EXPORT_CONTENT_DISPOSITION_HEADER",
		)
	}
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
		render_support_state(source),
		render_api_error_code(source),
	]
	sections.extend(render_struct(source, name) for name in STRUCTS[:8])
	sections.append(render_raw_frame_headers(source))
	sections.append(render_tag_value(source))
	sections.append(render_semantic_context())
	sections.extend(render_struct(source, name) for name in STRUCTS[8:])
	return "\n\n".join(sections) + "\n"

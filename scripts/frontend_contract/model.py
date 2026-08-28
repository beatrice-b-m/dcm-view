from __future__ import annotations

from dataclasses import dataclass
from typing import Sequence

STRUCTS = (
	"WindowPreset",
	"FileSummary",
	"FilesResponse",
	"SeriesCatalogResponse",
	"SeriesSummary",
	"SeriesStackSummary",
	"FrameRefSummary",
	"SeriesWarningSummary",
	"ReferenceCatalogResponse",
	"ReferenceSummary",
	"ReferenceTargetSummary",
	"ReferenceMatchSummary",
	"DiscoveryResult",
	"FrameInfo",
	"ViewerIdentity",
	"HealthResponse",
	"FrameQuery",
	"TagQuery",
	"EmbedRoiAnnotations",
	"RawFrameMetadata",
	"TagNode",
	"ErrorResponse",
)
ENUMS = ("WindowMode", "SupportState", "ApiErrorCode", "TagValue")
INPUT_STRUCTS = frozenset({"FrameQuery", "TagQuery"})

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
RESPONSE_HEADER_TYPES = frozenset(
	{
		"NoResponseHeaders",
		"CacheResponseHeaders",
		"RawFrameResponseHeaders",
		"ExportResponseHeaders",
	}
)


@dataclass(frozen=True)
class ApiEndpoint:
	constant: str
	operation: str
	id: str
	method: str
	path: str
	params: tuple[str, ...]
	query_type_token: str
	request_type_token: str
	request_media_type: str | None
	response_type_token: str
	response_media_type: str
	response_headers_type_token: str
	error_type_token: str
	success_status: int


@dataclass(frozen=True)
class RawFrameHeader:
	constant: str
	field: str
	name: str


@dataclass(frozen=True)
class FrameQueryParameter:
	constant: str
	client_key: str
	wire_name: str


def validate_endpoint_type_token(value: str, *, context: str) -> None:
	if value in ENDPOINT_TYPE_MARKERS:
		return
	if value.startswith("Vec<") and value.endswith(">"):
		inner = value[4:-1]
		validate_endpoint_type_token(inner, context=context)
		if ENDPOINT_TYPE_MARKERS.get(inner) == "never":
			raise ValueError(f"invalid vector wire type for {context}: {value}")
		return
	if value not in set(STRUCTS) | set(ENUMS):
		raise ValueError(f"unknown TypeScript wire type for {context}: {value}")


def validate_endpoint_registry(endpoints: Sequence[ApiEndpoint]) -> None:
	ids = [endpoint.id for endpoint in endpoints]
	if len(ids) != len(set(ids)):
		raise ValueError("API endpoint ids must be unique")
	operations = [endpoint.operation for endpoint in endpoints]
	if len(operations) != len(set(operations)):
		raise ValueError("API endpoint operations must be unique")
	method_paths = [(endpoint.method, endpoint.path) for endpoint in endpoints]
	if len(method_paths) != len(set(method_paths)):
		raise ValueError("API endpoint method/path pairs must be unique")


def validate_cross_contract_invariants(
	*,
	endpoints: Sequence[ApiEndpoint],
	raw_metadata_fields: set[str],
	raw_frame_headers: Sequence[RawFrameHeader],
	frame_query_fields: set[str],
	frame_query_parameters: Sequence[FrameQueryParameter],
) -> None:
	validate_raw_frame_header_invariant(
		raw_metadata_fields=raw_metadata_fields,
		raw_frame_headers=raw_frame_headers,
	)
	validate_frame_query_parameter_invariant(
		frame_query_fields=frame_query_fields,
		frame_query_parameters=frame_query_parameters,
	)
	validate_endpoint_ownership_invariants(endpoints)


def validate_raw_frame_header_invariant(
	*,
	raw_metadata_fields: set[str],
	raw_frame_headers: Sequence[RawFrameHeader],
) -> None:
	raw_header_fields = {header.field for header in raw_frame_headers}
	if raw_metadata_fields != raw_header_fields:
		raise ValueError(
			"RawFrameMetadata fields and raw header constants differ: "
			f"metadata-only={sorted(raw_metadata_fields - raw_header_fields)}, "
			f"header-only={sorted(raw_header_fields - raw_metadata_fields)}"
		)


def validate_frame_query_parameter_invariant(
	*,
	frame_query_fields: set[str],
	frame_query_parameters: Sequence[FrameQueryParameter],
) -> None:
	frame_query_keys = {
		parameter.wire_name
		for parameter in frame_query_parameters
	}
	if frame_query_fields != frame_query_keys:
		raise ValueError(
			"FrameQuery fields and query constants differ: "
			f"query-only={sorted(frame_query_fields - frame_query_keys)}, "
			f"constant-only={sorted(frame_query_keys - frame_query_fields)}"
		)


def validate_endpoint_ownership_invariants(
	endpoints: Sequence[ApiEndpoint],
) -> None:
	frame_endpoint = next(
		(endpoint for endpoint in endpoints if endpoint.id == "fileFrame"),
		None,
	)
	if frame_endpoint is None or frame_endpoint.query_type_token != "FrameQuery":
		raise ValueError("fileFrame must own the FrameQuery wire contract")
	raw_endpoint = next(
		(endpoint for endpoint in endpoints if endpoint.id == "fileRawFrame"),
		None,
	)
	if (
		raw_endpoint is None
		or raw_endpoint.response_headers_type_token != "RawFrameResponseHeaders"
	):
		raise ValueError(
			"fileRawFrame must own the RawFrameResponseHeaders contract"
		)
	display_endpoint = next(
		(endpoint for endpoint in endpoints if endpoint.id == "fileFrame"),
		None,
	)
	if (
		display_endpoint is None
		or display_endpoint.response_headers_type_token != "CacheResponseHeaders"
	):
		raise ValueError("fileFrame must own the CacheResponseHeaders contract")
	export_endpoint = next(
		(endpoint for endpoint in endpoints if endpoint.id == "annotationsExport"),
		None,
	)
	if (
		export_endpoint is None
		or export_endpoint.response_headers_type_token != "ExportResponseHeaders"
	):
		raise ValueError(
			"annotationsExport must own the ExportResponseHeaders contract"
		)

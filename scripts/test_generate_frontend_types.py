#!/usr/bin/env python3
from __future__ import annotations

from dataclasses import FrozenInstanceError
import pathlib
import tempfile
import unittest

import frontend_contract as generator
import frontend_contract.rust_parser as rust_parser
from frontend_contract import (
	ApiEndpoint,
	FrameQueryParameter,
	RawFrameHeader,
)


class GenerateFrontendTypesTests(unittest.TestCase):
	def setUp(self) -> None:
		self.source = generator.read_contract_source()

	def test_generates_typed_endpoints_annotation_and_raw_metadata(self) -> None:
		output = generator.render(self.source)

		self.assertIn("fileAnnotationsUpdate: {", output)
		self.assertIn('method: "PUT"', output)
		self.assertIn("request: EmbedRoiAnnotations;", output)
		self.assertIn("response: EmbedRoiAnnotations;", output)
		self.assertIn("query: FrameQuery;", output)
		self.assertIn("responseHeaders: CacheResponseHeaders;", output)
		self.assertIn("responseHeaders: RawFrameResponseHeaders;", output)
		self.assertIn("responseHeaders: ExportResponseHeaders;", output)
		self.assertEqual(
			output.count("error: ErrorResponse;"),
			len(generator.parse_api_endpoints(self.source)),
		)
		self.assertIn("export type GetApiEndpointId", output)
		self.assertIn("export function getApiEndpointPath", output)
		self.assertIn("metadata: RawFrameMetadata;", output)
		self.assertIn("successStatus: 200,", output)
		self.assertIn("export interface EmbedRoiAnnotations", output)
		self.assertIn("roi_coords: [number, number, number, number][];", output)
		self.assertIn("export interface RawFrameMetadata", output)
		self.assertIn("export interface ViewerIdentity", output)
		self.assertIn("viewer: ViewerIdentity;", output)
		self.assertIn("discovery: DiscoveryResult[];", output)
		self.assertIn(
			'export type SupportState = "renderable" | "metadata_only" | "unsupported";',
			output,
		)
		self.assertIn("bitsAllocated: number;", output)
		self.assertNotIn("bits_allocated: number;", output)

	def test_parsed_contract_records_are_frozen_models(self) -> None:
		endpoint = generator.parse_api_endpoints(self.source)[0]
		header = rust_parser.parse_raw_frame_headers(self.source)[0]
		query = rust_parser.parse_frame_query_parameters(self.source)[0]

		self.assertIsInstance(endpoint, ApiEndpoint)
		self.assertIsInstance(header, RawFrameHeader)
		self.assertIsInstance(query, FrameQueryParameter)
		for record in (endpoint, header, query):
			with self.assertRaises(FrozenInstanceError):
				setattr(record, "constant", "CHANGED")

	def test_rejects_an_unclassified_serde_wire_item(self) -> None:
		source = (
			self.source
			+ "\n#[derive(Debug, serde::Serialize)]\n"
			+ "pub struct FutureWireResponse {\n"
			+ "    pub value: String,\n"
			+ "}\n"
		)

		with self.assertRaisesRegex(
			ValueError,
			"serde HTTP contract items are not generated: FutureWireResponse",
		):
			generator.render(source)

	def test_all_serde_items_outside_contracts_are_explicitly_non_http(self) -> None:
		generator.validate_source_tree_placement()

	def test_window_mode_rename_all_mutation_changes_the_wire_union(self) -> None:
		mutated = self.source.replace(
			'#[serde(rename_all = "snake_case")]\npub enum WindowMode',
			'#[serde(rename_all = "camelCase")]\npub enum WindowMode',
			1,
		)
		self.assertNotEqual(mutated, self.source)

		output = generator.render(mutated)

		self.assertIn('export type WindowMode = "default" | "fullDynamic";', output)
		self.assertNotEqual(output, generator.render(self.source))

	def test_stable_api_error_codes_are_generated_from_the_rust_enum(self) -> None:
		output = generator.render(self.source)
		self.assertIn('export type ApiErrorCode = "invalid_path"', output)
		self.assertIn("\tcode: ApiErrorCode;", output)

	def test_field_level_rename_mutation_changes_the_generated_property(self) -> None:
		mutated = self.source.replace(
			"\tpub patient_id: String,",
			'\t#[serde(rename = "patientId")]\n\tpub patient_id: String,',
			1,
		)
		if mutated == self.source:
			mutated = self.source.replace(
				"    pub patient_id: String,",
				'    #[serde(rename = "patientId")]\n    pub patient_id: String,',
				1,
			)
		self.assertNotEqual(mutated, self.source)

		output = generator.render(mutated)

		self.assertIn("patientId: string;", output)
		self.assertNotEqual(output, generator.render(self.source))

	def test_directional_field_rename_fails_closed(self) -> None:
		mutated = self.source.replace(
			"    pub patient_id: String,",
			"    #[serde(rename(serialize = \"patientId\", "
			"deserialize = \"patient_id\"))]\n"
			"    pub patient_id: String,",
			1,
		)

		with self.assertRaisesRegex(ValueError, "unsupported or duplicate serde rename"):
			generator.render(mutated)

	def test_unsupported_top_level_serde_setting_fails_closed(self) -> None:
		mutated = self.source.replace(
			"pub struct FrameInfo {",
			"#[serde(deny_unknown_fields)]\npub struct FrameInfo {",
			1,
		)

		with self.assertRaisesRegex(
			ValueError,
			"unsupported top-level serde semantics for struct FrameInfo",
		):
			generator.render(mutated)

	def test_request_type_and_media_type_must_be_coherent(self) -> None:
		mutated = self.source.replace(
			'request_media_type: Some("application/json"),',
			"request_media_type: None,",
			1,
		)

		with self.assertRaisesRegex(
			ValueError,
			"must declare request_type and request_media_type together",
		):
			generator.render(mutated)

	def test_body_bearing_get_fails_closed(self) -> None:
		mutated = self.source.replace(
			"operation: Health,\n"
			'        id: "health",\n'
			"        method: Get,\n"
			'        path: "/health",\n'
			"        query_type: NoQuery,\n"
			"        request_type: NoRequest,\n"
			"        request_media_type: None,",
			"operation: Health,\n"
			'        id: "health",\n'
			"        method: Get,\n"
			'        path: "/health",\n'
			"        query_type: NoQuery,\n"
			"        request_type: EmbedRoiAnnotations,\n"
			'        request_media_type: Some("application/json"),',
			1,
		)
		self.assertNotEqual(mutated, self.source)

		with self.assertRaisesRegex(ValueError, "body-bearing GET"):
			generator.render(mutated)

	def test_json_endpoint_query_without_generic_encoder_fails_closed(self) -> None:
		mutated = self.source.replace(
			"operation: Health,\n"
			'        id: "health",\n'
			"        method: Get,\n"
			'        path: "/health",\n'
			"        query_type: NoQuery,",
			"operation: Health,\n"
			'        id: "health",\n'
			"        method: Get,\n"
			'        path: "/health",\n'
			"        query_type: FrameQuery,",
			1,
		)
		self.assertNotEqual(mutated, self.source)

		with self.assertRaisesRegex(
			ValueError,
			"query parameters on a JSON endpoint",
		):
			generator.render(mutated)

	def test_non_json_endpoint_body_without_dedicated_encoder_fails_closed(self) -> None:
		mutated = self.source.replace(
			"operation: AnnotationsExport,\n"
			'        id: "annotationsExport",\n'
			"        method: Get,\n"
			'        path: "/annotations/export.csv",\n'
			"        query_type: NoQuery,\n"
			"        request_type: NoRequest,\n"
			"        request_media_type: None,",
			"operation: AnnotationsExport,\n"
			'        id: "annotationsExport",\n'
			"        method: Put,\n"
			'        path: "/annotations/export.csv",\n'
			"        query_type: NoQuery,\n"
			"        request_type: EmbedRoiAnnotations,\n"
			'        request_media_type: Some("application/json"),',
			1,
		)
		self.assertNotEqual(mutated, self.source)

		with self.assertRaisesRegex(
			ValueError,
			"request body on a non-JSON response endpoint",
		):
			generator.render(mutated)

	def test_false_file_info_response_mapping_changes_generated_client_type(self) -> None:
		mutated = self.source.replace(
			"operation: FileInfo,\n"
			'        id: "fileInfo",\n'
			"        method: Get,\n"
			'        path: "/file/{index}/info",\n'
			"        query_type: NoQuery,\n"
			"        request_type: NoRequest,\n"
			"        request_media_type: None,\n"
			"        response_type: FrameInfo,",
			"operation: FileInfo,\n"
			'        id: "fileInfo",\n'
			"        method: Get,\n"
			'        path: "/file/{index}/info",\n'
			"        query_type: NoQuery,\n"
			"        request_type: NoRequest,\n"
			"        request_media_type: None,\n"
			"        response_type: FilesResponse,",
			1,
		)
		self.assertNotEqual(mutated, self.source)

		output = generator.render(mutated)
		file_info = output.split("\tfileInfo: {", 1)[1].split("\t};", 1)[0]
		self.assertIn("response: FilesResponse;", file_info)
		self.assertNotIn("response: FrameInfo;", file_info)

	def test_endpoint_error_type_must_use_the_common_error_envelope(self) -> None:
		mutated = self.source.replace(
			"error_type: ErrorResponse,",
			"error_type: FrameInfo,",
			1,
		)

		with self.assertRaisesRegex(
			ValueError,
			"must use the common ErrorResponse contract",
		):
			generator.render(mutated)

	def test_contract_discovery_supports_a_split_contract_directory(self) -> None:
		original_root = rust_parser.REPO_ROOT
		original_file = rust_parser.CONTRACTS_FILE
		original_dir = rust_parser.CONTRACTS_DIR
		try:
			with tempfile.TemporaryDirectory() as temp_dir:
				root = pathlib.Path(temp_dir)
				contract_dir = root / "src" / "api" / "contracts"
				contract_dir.mkdir(parents=True)
				(contract_dir / "dto.rs").write_text("dto", encoding="utf-8")
				(contract_dir / "routes.rs").write_text("routes", encoding="utf-8")
				rust_parser.REPO_ROOT = root
				rust_parser.CONTRACTS_FILE = root / "src" / "api" / "contracts.rs"
				rust_parser.CONTRACTS_DIR = contract_dir

				self.assertEqual(
					[path.name for path in rust_parser.contract_source_paths()],
					["dto.rs", "routes.rs"],
				)
				source = rust_parser.read_contract_source()
				self.assertIn("dto", source)
				self.assertIn("routes", source)
		finally:
			rust_parser.REPO_ROOT = original_root
			rust_parser.CONTRACTS_FILE = original_file
			rust_parser.CONTRACTS_DIR = original_dir

	def test_serde_scanner_recognizes_restricted_rust_visibilities(self) -> None:
		source = """
#[derive(serde::Serialize)]
struct PrivateDto {}
#[derive(serde::Serialize)]
pub struct PublicDto {}
#[derive(serde::Deserialize)]
pub(crate) struct CrateDto {}
#[derive(serde::Serialize)]
pub(super) struct ParentDto {}
#[derive(serde::Deserialize)]
pub(in crate::protocol) struct ScopedDto {}
"""

		self.assertEqual(
			rust_parser.serde_items(source, public_only=True),
			{"PublicDto", "CrateDto", "ParentDto", "ScopedDto"},
		)
		self.assertEqual(
			rust_parser.serde_items(source, public_only=False),
			{"PrivateDto", "PublicDto", "CrateDto", "ParentDto", "ScopedDto"},
		)

	def test_duplicate_allowed_non_http_type_names_fail_closed(self) -> None:
		source = """
#[derive(serde::Serialize)]
pub(crate) struct StartupEvent {}
"""

		with self.assertRaisesRegex(
			ValueError,
			"non-HTTP serde type names must be unique",
		):
			rust_parser.validate_non_http_serde_sources(
				{
					pathlib.Path("src/first.rs"): source,
					pathlib.Path("src/moved.rs"): source,
				},
				allowed_types={"StartupEvent"},
			)


if __name__ == "__main__":
	unittest.main()

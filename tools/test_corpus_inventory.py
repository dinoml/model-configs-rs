"""Hermetic behavior tests for the corpus inventory tool."""

from __future__ import annotations

import http.server
import hashlib
import io
import json
import stat
import tempfile
import threading
import types
import unittest
from concurrent.futures import ThreadPoolExecutor
from contextlib import contextmanager
from pathlib import Path
from typing import Iterator
from unittest import mock

import corpus_inventory


class _QuietRequestHandler(http.server.BaseHTTPRequestHandler):
    def log_message(self, format: str, *args: object) -> None:
        pass


@contextmanager
def _running_server(
    handler: type[http.server.BaseHTTPRequestHandler],
) -> Iterator[http.server.ThreadingHTTPServer]:
    server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), handler)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    try:
        yield server
    finally:
        server.shutdown()
        server.server_close()
        thread.join(timeout=5)


def _make_directory_symlink(test: unittest.TestCase, target: Path, link: Path) -> None:
    try:
        link.symlink_to(target, target_is_directory=True)
    except (NotImplementedError, OSError) as error:
        test.skipTest(f"directory symlinks are unavailable: {error}")


class SupportedKindTests(unittest.TestCase):
    def test_recognizes_exact_names_and_safetensors_indexes(self) -> None:
        self.assertEqual(corpus_inventory.supported_kind("config.json"), "config.json")
        self.assertEqual(corpus_inventory.supported_kind("1/config.json"), "config.json")
        self.assertEqual(
            corpus_inventory.supported_kind("unet/diffusion_pytorch_model.safetensors.index.json"),
            "*.safetensors.index.json",
        )
        self.assertEqual(
            corpus_inventory.supported_kind("templates/chat_template.jinja"),
            "chat_template.jinja",
        )

    def test_ignores_metadata_cache_and_unrelated_json(self) -> None:
        self.assertIsNone(corpus_inventory.supported_kind("config.json.metadata"))
        self.assertIsNone(corpus_inventory.supported_kind(".cache/config.json"))
        self.assertIsNone(corpus_inventory.supported_kind("tokenizer.json"))
        self.assertIsNone(corpus_inventory.supported_kind("pytorch_model.bin.index.json"))

    def test_allows_hidden_component_directories_but_excludes_metadata(self) -> None:
        self.assertEqual(
            corpus_inventory.supported_kind(".hidden/config.json"),
            "config.json",
        )
        for directory in [".cache", ".git", ".hg", ".svn"]:
            with self.subTest(directory=directory):
                self.assertIsNone(
                    corpus_inventory.supported_kind(f"{directory}/config.json")
                )

    def test_rejects_noncanonical_path_spellings_before_normalization(self) -> None:
        for path in [
            "",
            "/config.json",
            "C:/config.json",
            "nested\\config.json",
            "nested//config.json",
            "nested/./config.json",
            "nested/../config.json",
            "nested:bad/config.json",
            "CON/config.json",
            "con.txt/config.json",
            "COM1.txt/config.json",
            "LPT²/config.json",
            "dir/bad./config.json",
            "dir/bad /config.json",
            "dir/control\u0001/config.json",
            f"{'x' * 256}/config.json",
            f"{'a/' * 600}config.json",
        ]:
            with self.subTest(path=path):
                self.assertIsNone(corpus_inventory.supported_kind(path))


class ExtractionTests(unittest.TestCase):
    def test_extracts_only_auditable_hub_repository_evidence(self) -> None:
        with tempfile.TemporaryDirectory() as temporary_directory:
            report_root = Path(temporary_directory)
            family = report_root / "family"
            family.mkdir()
            (family / "report.md").write_text(
                """# Audit
Model id(s): acme/primary, acme/secondary.

- [URL model](https://huggingface.co/example/url-model/blob/main/config.json)
- H:/configs/local-owner/local-repo/transformer/config.json
- `_sources/snapshot-owner__snapshot-repo/config.json`
- source path: transformers/src/transformers/models/example/modeling_example.py
- docs: https://huggingface.co/docs/transformers/model_doc/example
""",
                encoding="utf-8",
            )

            repositories = corpus_inventory.extract_report_repositories(
                [("transformers", report_root)]
            )

        self.assertEqual(
            [entry["id"] for entry in repositories],
            [
                "acme/primary",
                "acme/secondary",
                "example/url-model",
                "local-owner/local-repo",
                "snapshot-owner/snapshot-repo",
            ],
        )
        self.assertTrue(
            all(
                evidence["report"] == "transformers/family/report.md"
                for entry in repositories
                for evidence in entry["evidence"]
            )
        )


class AuditTests(unittest.TestCase):
    def test_audit_is_deterministic_and_reports_invalid_json(self) -> None:
        with tempfile.TemporaryDirectory() as temporary_directory:
            corpus_root = Path(temporary_directory)
            repository = corpus_root / "owner" / "repo"
            (repository / "scheduler").mkdir(parents=True)
            (repository / ".cache" / "nested").mkdir(parents=True)
            (repository / "config.json").write_text('{"model_type":"test"}\n', encoding="utf-8")
            (repository / "scheduler" / "scheduler_config.json").write_text(
                '{"broken":}\n', encoding="utf-8"
            )
            (repository / "model.safetensors.index.json").write_text("{}\n", encoding="utf-8")
            (repository / "chat_template.jinja").write_text("{{ value }}\n", encoding="utf-8")
            (repository / "tokenizer.json").write_text("{}\n", encoding="utf-8")
            (repository / ".cache" / "nested" / "config.json").write_text(
                "{}\n", encoding="utf-8"
            )
            (corpus_root / "root-config.json").write_text("{}\n", encoding="utf-8")

            first = corpus_inventory.audit_corpus(corpus_root, ["owner/repo", "missing/repo"])
            second = corpus_inventory.audit_corpus(corpus_root, ["owner/repo", "missing/repo"])

        self.assertEqual(first, second)
        self.assertEqual(first["corpus"]["supported_files"], 4)
        self.assertEqual(first["corpus"]["valid_json_files"], 2)
        self.assertEqual(first["corpus"]["invalid_json_files"], 1)
        self.assertEqual(first["empty_json_objects"], {"*.safetensors.index.json": 1})
        self.assertEqual(
            first["json_top_level_types"],
            {
                "*.safetensors.index.json": {"object": 1},
                "config.json": {"object": 1},
            },
        )
        self.assertEqual(
            first["document_kinds"],
            {
                "*.safetensors.index.json": {"files": 1, "unique_contents": 1},
                "chat_template.jinja": {"files": 1, "unique_contents": 1},
                "config.json": {"files": 1, "unique_contents": 1},
                "scheduler_config.json": {"files": 1, "unique_contents": 1},
            },
        )
        self.assertEqual(first["invalid_json"][0]["path"], "owner/repo/scheduler/scheduler_config.json")
        self.assertEqual(first["report_repository_coverage"][0]["id"], "missing/repo")
        self.assertFalse(first["report_repository_coverage"][0]["present"])
        self.assertEqual(first["report_repository_coverage"][1]["supported_files"], 4)

    def test_reports_duplicate_json_object_keys_without_rejecting_the_document(self) -> None:
        with tempfile.TemporaryDirectory() as temporary_directory:
            corpus_root = Path(temporary_directory)
            repository = corpus_root / "owner" / "repo"
            repository.mkdir(parents=True)
            (repository / "config.json").write_text(
                '{"model_type":"first","model_type":"second"}\n', encoding="utf-8"
            )

            audit = corpus_inventory.audit_corpus(corpus_root)

        self.assertEqual(audit["corpus"]["valid_json_files"], 1)
        self.assertEqual(audit["corpus"]["json_files_with_duplicate_keys"], 1)
        self.assertEqual(
            audit["duplicate_json_keys"],
            [{"path": "owner/repo/config.json", "keys": ["model_type"], "occurrences": 1}],
        )

    def test_verifies_revision_file_hashes_and_reports_unpinned_files(self) -> None:
        with tempfile.TemporaryDirectory() as temporary_directory:
            root = Path(temporary_directory)
            corpus_root = root / "corpus"
            repository = corpus_root / "owner" / "repo"
            repository.mkdir(parents=True)
            config = b'{"model_type":"test"}\n'
            tokenizer = b'{"tokenizer_class":"Test"}\n'
            (repository / "config.json").write_bytes(config)
            (repository / "tokenizer_config.json").write_bytes(tokenizer)
            fetch_manifest = root / "fetch.json"
            fetch_manifest.write_text(
                json.dumps(
                    {
                        "repositories": [
                            {
                                "id": "OWNER/REPO",
                                "revision": "abc",
                                "documents": [
                                    {
                                        "path": "config.json",
                                        "sha256": hashlib.sha256(config).hexdigest(),
                                    }
                                ],
                            }
                        ]
                    }
                ),
                encoding="utf-8",
            )

            audit = corpus_inventory.audit_corpus(
                corpus_root, revision_manifest_path=fetch_manifest
            )

        self.assertEqual(
            audit["provenance"],
            {
                "revision_verified_files": 1,
                "files_without_revision_record": 1,
                "revision_mismatch_files": 0,
                "revision_mismatches": [],
                "expected_revision_files_missing": 0,
                "missing_revision_documents": [],
            },
        )

    def test_rejects_symlinked_repository_instead_of_traversing_it(self) -> None:
        with tempfile.TemporaryDirectory() as temporary_directory:
            temporary_root = Path(temporary_directory)
            corpus_root = temporary_root / "corpus"
            owner = corpus_root / "owner"
            outside_repository = temporary_root / "outside"
            owner.mkdir(parents=True)
            outside_repository.mkdir()
            (outside_repository / "config.json").write_text("{}\n", encoding="utf-8")
            _make_directory_symlink(self, outside_repository, owner / "repo")

            with self.assertRaisesRegex(
                corpus_inventory.UnsafeFilesystemPath,
                "symbolic link or reparse point",
            ):
                corpus_inventory.audit_corpus(corpus_root)

    def test_rejects_symlinked_files_inside_repository(self) -> None:
        with tempfile.TemporaryDirectory() as temporary_directory:
            temporary_root = Path(temporary_directory)
            corpus_root = temporary_root / "corpus"
            repository = corpus_root / "owner" / "repo"
            repository.mkdir(parents=True)
            outside_file = temporary_root / "outside-config.json"
            outside_file.write_text("{}\n", encoding="utf-8")
            try:
                (repository / "config.json").symlink_to(outside_file)
            except (NotImplementedError, OSError) as error:
                self.skipTest(f"file symlinks are unavailable: {error}")

            with self.assertRaisesRegex(
                corpus_inventory.UnsafeFilesystemPath,
                "symbolic link or reparse point",
            ):
                corpus_inventory.audit_corpus(corpus_root)

    def test_windows_reparse_attribute_is_treated_as_an_alias(self) -> None:
        metadata = types.SimpleNamespace(
            st_mode=stat.S_IFDIR,
            st_file_attributes=0x400,
        )
        with mock.patch.object(
            corpus_inventory.stat,
            "FILE_ATTRIBUTE_REPARSE_POINT",
            0x400,
            create=True,
        ), mock.patch.object(corpus_inventory, "_path_metadata", return_value=metadata):
            self.assertTrue(corpus_inventory._is_link_or_reparse(Path("junction")))

    def test_rejects_oversized_sparse_document_before_reading_it(self) -> None:
        with tempfile.TemporaryDirectory() as temporary_directory:
            corpus_root = Path(temporary_directory)
            repository = corpus_root / "owner" / "repo"
            repository.mkdir(parents=True)
            config_path = repository / "config.json"
            with config_path.open("wb") as config_file:
                config_file.truncate(corpus_inventory.MAX_SOURCE_DOCUMENT_BYTES + 1)

            with self.assertRaisesRegex(
                corpus_inventory.ResponseTooLarge,
                "owner/repo/config.json",
            ):
                corpus_inventory.audit_corpus(corpus_root)

    def test_reports_excessive_json_nesting_as_invalid(self) -> None:
        with tempfile.TemporaryDirectory() as temporary_directory:
            corpus_root = Path(temporary_directory)
            repository = corpus_root / "owner" / "repo"
            repository.mkdir(parents=True)
            (repository / "config.json").write_text(
                "[" * 10_000 + "0" + "]" * 10_000,
                encoding="utf-8",
            )

            audit = corpus_inventory.audit_corpus(corpus_root)

        self.assertEqual(audit["corpus"]["invalid_json_files"], 1)
        self.assertEqual(audit["invalid_json"][0]["path"], "owner/repo/config.json")
        self.assertIn("recursion", audit["invalid_json"][0]["message"].casefold())


class FetchSelectionTests(unittest.TestCase):
    def test_selects_supported_siblings_without_cache_paths(self) -> None:
        siblings = [
            {"rfilename": "config.json"},
            {"rfilename": "unet/config.json"},
            {"rfilename": "model.safetensors.index.json"},
            {"rfilename": "model-00001-of-00002.safetensors"},
            {"rfilename": ".cache/config.json"},
            {"rfilename": "README.md"},
        ]
        self.assertEqual(
            corpus_inventory.select_supported_files(siblings),
            ["config.json", "model.safetensors.index.json", "unet/config.json"],
        )

    def test_rejects_nonportable_sibling_path_collisions(self) -> None:
        inventories = [
            [
                {"rfilename": "A/config.json"},
                {"rfilename": "a/config.json"},
            ],
            [
                {"rfilename": "A/config.json"},
                {"rfilename": "a/tokenizer_config.json"},
            ],
            [
                {"rfilename": "caf\u00e9/config.json"},
                {"rfilename": "cafe\u0301/tokenizer_config.json"},
            ],
            [
                {"rfilename": "A/config.json"},
                {"rfilename": "a/config.json/config.json"},
            ],
        ]
        for siblings in inventories:
            with self.subTest(siblings=siblings), self.assertRaises(
                corpus_inventory.NonPortableRepositoryInventory
            ):
                corpus_inventory.select_supported_files(siblings)

    def test_repository_manifest_rejects_parent_traversal(self) -> None:
        with tempfile.TemporaryDirectory() as temporary_directory:
            manifest = Path(temporary_directory) / "repositories.json"
            manifest.write_text('["../escape"]\n', encoding="utf-8")

            with self.assertRaisesRegex(ValueError, "invalid repository"):
                corpus_inventory._load_repository_ids(manifest)

    def test_resume_replaces_only_retried_repository_entries(self) -> None:
        previous = {
            "schema_version": 1,
            "source": "huggingface.co",
            "selection": ["config.json"],
            "repositories": [
                {"id": "owner/complete", "status": "ok", "revision": "a"},
                {"id": "owner/throttled", "status": "http_429"},
            ],
        }
        retried = {
            "schema_version": 1,
            "source": "huggingface.co",
            "selection": ["config.json"],
            "repositories": [
                {"id": "owner/throttled", "status": "ok", "revision": "b"},
            ],
        }

        merged = corpus_inventory.merge_fetch_manifests(previous, retried)

        self.assertEqual(
            merged["repositories"],
            [
                {"id": "owner/complete", "status": "ok", "revision": "a"},
                {"id": "owner/throttled", "status": "ok", "revision": "b"},
            ],
        )

    def test_fetch_and_merge_deduplicate_repository_ids_by_casefold(self) -> None:
        calls: list[str] = []

        def fake_fetch(
            repository_id: str,
            corpus_root: Path,
            token: str | None,
        ) -> dict[str, object]:
            del corpus_root, token
            calls.append(repository_id)
            return {"id": repository_id, "status": "ok"}

        with tempfile.TemporaryDirectory() as temporary_directory, mock.patch.object(
            corpus_inventory,
            "_fetch_repository",
            side_effect=fake_fetch,
        ):
            fetched = corpus_inventory.fetch_repositories(
                Path(temporary_directory),
                ["dynamic/yarn", "Dynamic/YARN", "other/repo"],
                workers=2,
            )

        self.assertEqual(sorted(value.casefold() for value in calls), ["dynamic/yarn", "other/repo"])
        self.assertEqual(len(fetched["repositories"]), 2)

        merged = corpus_inventory.merge_fetch_manifests(
            {
                "repositories": [
                    {"id": "Dynamic/YARN", "status": "old"},
                    {"id": "other/repo", "status": "kept"},
                ]
            },
            {"repositories": [{"id": "dynamic/yarn", "status": "new"}]},
        )
        self.assertEqual(
            merged["repositories"],
            [
                {"id": "dynamic/yarn", "status": "new"},
                {"id": "other/repo", "status": "kept"},
            ],
        )

    def test_load_resume_and_resolution_use_casefold_repository_identity(self) -> None:
        with tempfile.TemporaryDirectory() as temporary_directory:
            manifest = Path(temporary_directory) / "repositories.json"
            manifest.write_text(
                '["Dynamic/YARN", "dynamic/yarn", "other/repo"]\n',
                encoding="utf-8",
            )
            self.assertEqual(
                corpus_inventory._load_repository_ids(manifest),
                ["Dynamic/YARN", "other/repo"],
            )

        previous = {
            "repositories": [
                {"id": "DYNAMIC/yarn", "status": "ok"},
                {"id": "OTHER/repo", "status": "http_429"},
            ]
        }
        self.assertEqual(
            corpus_inventory._pending_repository_ids(
                ["dynamic/YARN", "other/REPO"], previous
            ),
            ["other/REPO"],
        )

        candidates = {
            "repositories": [{"id": "dynamic/YARN", "evidence": []}],
        }
        fetched = {
            "repositories": [
                {
                    "id": "DYNAMIC/yarn",
                    "status": "ok",
                    "revision": "abc",
                    "files": [],
                }
            ]
        }
        resolved, unresolved = corpus_inventory.resolve_report_candidates(
            candidates, fetched
        )
        self.assertEqual(resolved["repository_count"], 1)
        self.assertEqual(unresolved["candidate_count"], 0)

        duplicate_candidates = {
            "repositories": [
                {
                    "id": "DYNAMIC/yarn",
                    "evidence": [{"report": "a/report.md", "line": 1}],
                },
                {
                    "id": "dynamic/YARN",
                    "evidence": [{"report": "b/report.md", "line": 2}],
                },
            ]
        }
        resolved, _ = corpus_inventory.resolve_report_candidates(
            duplicate_candidates, fetched
        )
        self.assertEqual(resolved["repository_count"], 1)
        self.assertEqual(
            resolved["repositories"][0]["candidate_ids"],
            ["DYNAMIC/yarn", "dynamic/YARN"],
        )
        self.assertEqual(len(resolved["repositories"][0]["evidence"]), 2)

    def test_fetch_rejects_nonportable_inventory_before_downloads(self) -> None:
        metadata = json.dumps(
            {
                "sha": "abc",
                "siblings": [
                    {"rfilename": "A/config.json"},
                    {"rfilename": "a/tokenizer_config.json"},
                ],
            }
        ).encode("utf-8")
        with tempfile.TemporaryDirectory() as temporary_directory, mock.patch.object(
            corpus_inventory,
            "_http_bytes",
            return_value=(metadata, {}),
        ) as http_bytes:
            result = corpus_inventory._fetch_repository(
                "owner/repo", Path(temporary_directory), None
            )

        self.assertEqual(result["status"], "nonportable_inventory")
        self.assertEqual(http_bytes.call_count, 1)

    def test_resume_does_not_retry_permanent_partial_download_failures(self) -> None:
        self.assertFalse(
            corpus_inventory._retryable_fetch_entry(
                {"status": "partial", "files": [{"path": "config.json", "status": "http_401"}]}
            )
        )
        self.assertTrue(
            corpus_inventory._retryable_fetch_entry(
                {"status": "partial", "files": [{"path": "config.json", "status": "http_429"}]}
            )
        )
        for status in ["too_large", "unsafe_filesystem_path"]:
            with self.subTest(status=status):
                self.assertFalse(
                    corpus_inventory._retryable_fetch_entry(
                        {
                            "status": "partial",
                            "files": [{"path": "config.json", "status": status}],
                        }
                    )
                )

    def test_resolution_separates_revision_backed_repositories_from_unresolved_candidates(self) -> None:
        candidates = {
            "schema_version": 1,
            "report_count": 1,
            "repository_count": 2,
            "repositories": [
                {"id": "owner/resolved", "evidence": [{"report": "x/report.md", "line": 1, "kind": "model_id"}]},
                {"id": "ratio/shape", "evidence": [{"report": "x/report.md", "line": 2, "kind": "contextual_id"}]},
            ],
        }
        fetched = {
            "repositories": [
                {
                    "id": "owner/resolved",
                    "status": "ok",
                    "revision": "abc",
                    "files": [
                        {"path": "config.json", "status": "downloaded", "bytes": 3, "sha256": "def"}
                    ],
                },
                {"id": "ratio/shape", "status": "unauthorized"},
            ]
        }

        resolved, unresolved = corpus_inventory.resolve_report_candidates(candidates, fetched)

        self.assertEqual(resolved["repository_count"], 1)
        self.assertEqual(resolved["repositories"][0]["revision"], "abc")
        self.assertEqual(
            resolved["repositories"][0]["documents"][0],
            {"path": "config.json", "bytes": 3, "sha256": "def"},
        )
        self.assertEqual(unresolved["candidate_count"], 1)
        self.assertEqual(unresolved["candidates"][0]["fetch_status"], "unauthorized")

    def test_bounded_reader_stops_after_limit_plus_one_byte(self) -> None:
        response = io.BytesIO(b"0123456789")

        with self.assertRaises(corpus_inventory.ResponseTooLarge):
            corpus_inventory._read_bounded_stream(response, max_bytes=4)

        self.assertEqual(response.tell(), 5)

    def test_fetch_records_oversized_document_without_writing_it(self) -> None:
        metadata = json.dumps(
            {
                "sha": "abc",
                "siblings": [{"rfilename": "config.json"}],
            }
        ).encode("utf-8")
        with tempfile.TemporaryDirectory() as temporary_directory, mock.patch.object(
            corpus_inventory,
            "_http_bytes",
            side_effect=[
                (metadata, {}),
                corpus_inventory.ResponseTooLarge(corpus_inventory.MAX_SOURCE_DOCUMENT_BYTES),
            ],
        ):
            corpus_root = Path(temporary_directory)
            result = corpus_inventory._fetch_repository("owner/repo", corpus_root, None)

            self.assertEqual(result["status"], "partial")
            self.assertEqual(
                result["files"],
                [{"path": "config.json", "status": "too_large"}],
            )
            self.assertFalse((corpus_root / "owner" / "repo" / "config.json").exists())

    def test_fetch_records_oversized_metadata_response(self) -> None:
        with tempfile.TemporaryDirectory() as temporary_directory, mock.patch.object(
            corpus_inventory,
            "_http_bytes",
            side_effect=corpus_inventory.ResponseTooLarge(
                corpus_inventory.MAX_SOURCE_DOCUMENT_BYTES
            ),
        ):
            result = corpus_inventory._fetch_repository(
                "owner/repo",
                Path(temporary_directory),
                None,
            )

        self.assertEqual(result, {"id": "owner/repo", "status": "too_large"})

    def test_fetch_does_not_read_an_oversized_existing_target(self) -> None:
        with tempfile.TemporaryDirectory() as temporary_directory:
            corpus_root = Path(temporary_directory)
            target = corpus_root / "owner" / "repo" / "config.json"
            target.parent.mkdir(parents=True)
            with target.open("wb") as target_file:
                target_file.truncate(corpus_inventory.MAX_SOURCE_DOCUMENT_BYTES + 1)

            with self.assertRaisesRegex(
                corpus_inventory.ResponseTooLarge,
                "owner/repo/config.json",
            ):
                corpus_inventory._write_corpus_bytes_if_changed(
                    corpus_root,
                    ("owner", "repo", "config.json"),
                    b"{}\n",
                )

    def test_parallel_fetch_writes_can_create_one_shared_owner(self) -> None:
        with tempfile.TemporaryDirectory() as temporary_directory:
            corpus_root = Path(temporary_directory)

            def write(repository: str) -> str:
                return corpus_inventory._write_corpus_bytes_if_changed(
                    corpus_root,
                    ("owner", repository, "config.json"),
                    b"{}\n",
                )

            with ThreadPoolExecutor(max_workers=2) as executor:
                statuses = list(executor.map(write, ["first", "second"]))

            self.assertEqual(statuses, ["downloaded", "downloaded"])
            self.assertEqual(
                (corpus_root / "owner" / "first" / "config.json").read_bytes(),
                b"{}\n",
            )
            self.assertEqual(
                (corpus_root / "owner" / "second" / "config.json").read_bytes(),
                b"{}\n",
            )

    def test_cross_origin_redirect_does_not_forward_authorization(self) -> None:
        target_authorization: list[str | None] = []
        redirect_authorization: list[str | None] = []

        class TargetHandler(_QuietRequestHandler):
            def do_GET(self) -> None:
                target_authorization.append(self.headers.get("Authorization"))
                self.send_response(200)
                self.send_header("Content-Length", "2")
                self.end_headers()
                self.wfile.write(b"ok")

        with _running_server(TargetHandler) as target_server:
            target_url = f"http://127.0.0.1:{target_server.server_port}/target"

            class RedirectHandler(_QuietRequestHandler):
                def do_GET(self) -> None:
                    redirect_authorization.append(self.headers.get("Authorization"))
                    self.send_response(302)
                    self.send_header("Location", target_url)
                    self.end_headers()

            with _running_server(RedirectHandler) as redirect_server:
                redirect_url = f"http://127.0.0.1:{redirect_server.server_port}/redirect"
                data, _ = corpus_inventory._http_bytes(
                    redirect_url,
                    "test-secret",
                    attempts=1,
                )

        self.assertEqual(data, b"ok")
        self.assertEqual(redirect_authorization, ["Bearer test-secret"])
        self.assertEqual(target_authorization, [None])

    def test_fetch_refuses_to_write_through_symlinked_owner(self) -> None:
        metadata = json.dumps(
            {
                "sha": "abc",
                "siblings": [{"rfilename": "config.json"}],
            }
        ).encode("utf-8")
        with tempfile.TemporaryDirectory() as temporary_directory:
            temporary_root = Path(temporary_directory)
            corpus_root = temporary_root / "corpus"
            outside = temporary_root / "outside"
            corpus_root.mkdir()
            outside.mkdir()
            _make_directory_symlink(self, outside, corpus_root / "owner")

            with mock.patch.object(
                corpus_inventory,
                "_http_bytes",
                side_effect=[(metadata, {}), (b"{}\n", {})],
            ):
                result = corpus_inventory._fetch_repository("owner/repo", corpus_root, None)

            self.assertEqual(result["status"], "partial")
            self.assertEqual(
                result["files"],
                [{"path": "config.json", "status": "unsafe_filesystem_path"}],
            )
            self.assertFalse((outside / "repo" / "config.json").exists())


if __name__ == "__main__":
    unittest.main()

"""Hermetic behavior tests for the corpus inventory tool."""

from __future__ import annotations

import tempfile
import unittest
from pathlib import Path

import corpus_inventory


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

    def test_rejects_noncanonical_path_spellings_before_normalization(self) -> None:
        for path in [
            "nested\\config.json",
            "nested//config.json",
            "nested/./config.json",
            "nested/../config.json",
            "nested:bad/config.json",
            "CON/config.json",
            "dir/bad./config.json",
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


if __name__ == "__main__":
    unittest.main()

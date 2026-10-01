from __future__ import annotations

import unittest
from collections import Counter

from scripts import audit_mod_workflows as audit


class ModWorkflowAuditTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        cls.rows = {row["module_id"]: row for row in audit.audit_rows()}

    def test_all_shipped_modules_have_explicit_current_coverage(self) -> None:
        self.assertEqual(set(self.rows), set(audit.module_ids()))
        self.assertEqual(len(self.rows), 32)
        self.assertEqual(Counter(row["workflow_kind"] for row in self.rows.values()), {
            "steam_workshop": 10, "community_packages": 15, "not_modelled": 7,
        })
        self.assertEqual(Counter(row["status"] for row in self.rows.values()), {
            "supported": 11, "manual_only": 14, "not_modelled": 7,
        })
        self.assertEqual(sum(row["frontend_tab"] == "True" for row in self.rows.values()), 26)
        self.assertEqual(sum(row["frontend_apply"] == "True" for row in self.rows.values()), 25)

    def test_client_dependencies_and_hidden_catalog_entries_do_not_become_install_support(self) -> None:
        rimworld = self.rows["rimworld"]
        self.assertEqual((rimworld["status"], rimworld["frontend_tab"], rimworld["frontend_apply"],
                          rimworld["frontend_guardrail"], rimworld["collections"]),
                         ("not_modelled", "True", "False", "True", "False"))
        hidden = {name for name, row in self.rows.items() if row["frontend_tab"] == "False"}
        self.assertEqual(hidden, {"nightingale", "returntomoria", "romestead", "runescapedragonwilds", "scum", "theforest"})
        self.assertEqual(self.rows["runescapedragonwilds"]["frontend_guardrail"], "False")

    def test_local_file_support_does_not_claim_enablement_or_automatic_modrinth_installation(self) -> None:
        file_only = [row for row in self.rows.values() if row["status"] == "manual_only"]
        self.assertEqual(len(file_only), 14)
        self.assertTrue(all(row["manual_staging"] and not row["enablement"] for row in file_only))
        self.assertTrue(all(row["collections"] == "False" for row in file_only))
        self.assertEqual({row["module_id"] for row in file_only if row["package_installation"] == "verified_thunderstore_and_local_files"},
                         {"corekeeper", "valheim", "vrising"})
        self.assertEqual(self.rows["minecraft"]["package_installation"], "local_files")
        self.assertEqual(self.rows["arksurvivalascended"]["package_installation"], "reference_ids_and_local_files")

    def test_apply_coverage_reads_the_domain_plan_including_grouped_cases(self) -> None:
        self.assertEqual(audit.mod_workbench_apply_ids(), {
            "dontstarve", "projectzomboid", "unturned", "arksurvivalevolved", "barotrauma",
            "conanexiles", "soulmask", "terraria",
        })
        for module_id in ("dontstarve", "projectzomboid", "barotrauma", "conanexiles", "palworld", "squad"):
            self.assertEqual(self.rows[module_id]["status"], "supported", module_id)
            self.assertEqual(self.rows[module_id]["collections"], "True", module_id)


if __name__ == "__main__":
    unittest.main()

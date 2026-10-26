import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

import yaml


ROOT = Path(__file__).resolve().parents[1]
ACTION = yaml.safe_load((ROOT / "action.yml").read_text())
STEPS = {step["name"]: step for step in ACTION["runs"]["steps"]}
NOTICE = "::notice::casemap could not comment (read-only token); see the job summary"


def comment_record(identifier, body, login="github-actions[bot]"):
    return {"id": identifier, "body": body, "user": {"login": login}}


class ActionTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        self.bin = self.root / "bin"
        self.bin.mkdir()
        self.environment = {
            **os.environ,
            "PATH": f"{self.bin}:{os.environ['PATH']}",
            "RUNNER_TEMP": str(self.root),
            "GITHUB_OUTPUT": str(self.root / "outputs"),
            "GITHUB_STEP_SUMMARY": str(self.root / "summary"),
            "GITHUB_PATH": str(self.root / "path"),
            "REPO": "owner/repo",
            "PR": "12",
            "GH_LOG": str(self.root / "gh-log"),
            "COMMENTS": "[]",
            "ERROR_AT": "",
            "STATUS": "403",
            "BODY": "<!-- casemap:pr-comment -->\nChanged \"flow\" 🗺️\n",
            "DIFF_STATUS": "0",
            "BASE": "base sha",
            "FLOWS": "my flows",
            "BIN": "",
            "BOT_LOGIN": "github-actions[bot]",
        }
        self.executable("gh", '''#!/usr/bin/env python3
import json, os, sys
arguments = sys.argv[1:]
with open(os.environ['GH_LOG'], 'a') as log:
    log.write(json.dumps(arguments) + '\\n')
listing = '--paginate' in arguments
method = arguments[arguments.index('-X') + 1] if '-X' in arguments else 'GET'
if os.environ['ERROR_AT'] == ('list' if listing else method):
    print('gh: denied (HTTP ' + os.environ['STATUS'] + ')', file=sys.stderr)
    sys.exit(1)
if listing:
    print(os.environ['COMMENTS'])
''')
        self.executable("casemap", '''#!/usr/bin/env python3
import json, os, sys
from pathlib import Path
if sys.argv[1:] == ['--version']:
    print('casemap test')
else:
    Path(os.environ['RUNNER_TEMP'], 'diff-args').write_text(json.dumps(sys.argv[1:]))
    print(os.environ['BODY'], end='')
    sys.exit(int(os.environ['DIFF_STATUS']))
''')

    def executable(self, name, source):
        path = self.bin / name
        path.write_text(source)
        path.chmod(0o755)
        return path

    def run_step(self, name, **environment):
        return self.run_script(STEPS[name]["run"], **environment)

    def run_script(self, script, **environment):
        return subprocess.run(
            ["bash", "--noprofile", "--norc", "-e", "-o", "pipefail", "-c", script],
            env={**self.environment, **environment}, capture_output=True, text=True,
        )

    def comment(self, **environment):
        (self.root / "casemap.md").write_text(environment.get("BODY", self.environment["BODY"]))
        return self.run_step("Update pull request comment", **environment)

    def calls(self):
        path = self.root / "gh-log"
        if not path.exists():
            return []
        return [json.loads(line) for line in path.read_text().splitlines()]

    def test_report_preserves_arguments_body_and_failure(self):
        binary = self.bin / "existing casemap"
        shutil.copy(self.bin / "casemap", binary)
        result = self.run_step("Report flow changes", BIN=str(binary), DIFF_STATUS="1")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(json.loads((self.root / "diff-args").read_text()), ["diff", "base sha", "--flows", "my flows", "--format", "markdown"])
        self.assertEqual((self.root / "summary").read_text(), self.environment["BODY"])
        self.assertEqual((self.root / "outputs").read_text(), "exit-code=1\n")
        self.assertEqual(self.run_step("Return check result", EXIT_CODE="1").returncode, 1)
        self.assertEqual(self.run_step("Return check result", EXIT_CODE="0").returncode, 0)

    def test_posts_json_when_no_comment_exists(self):
        result = self.comment()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("POST", self.calls()[1])
        self.assertIn("repos/owner/repo/issues/12/comments", self.calls()[1])
        self.assertEqual(json.loads((self.root / "casemap-comment.json").read_text()), {"body": self.environment["BODY"]})

    def test_updates_first_matching_comment(self):
        comments = [
            comment_record(1, "Other"),
            comment_record(3, self.environment["BODY"]),
            comment_record(5, self.environment["BODY"]),
        ]
        result = self.comment(COMMENTS=json.dumps(comments))
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("PATCH", self.calls()[1])
        self.assertIn("repos/owner/repo/issues/comments/3", self.calls()[1])
        self.assertEqual(len(self.calls()), 2)

    def test_quiet_first_report_skips_comment_but_updates_existing(self):
        self.environment["BODY"] += "<!-- casemap:quiet -->\n"
        result = self.comment()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(len(self.calls()), 1)
        result = self.comment(COMMENTS=json.dumps([comment_record(7, "<!-- casemap:pr-comment -->")]))
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("PATCH", self.calls()[-1])

    def test_read_only_tokens_emit_notice_for_listing_and_writing(self):
        for operation in ["list", "POST"]:
            for status in ["403", "404"]:
                with self.subTest(operation=operation, status=status):
                    result = self.comment(ERROR_AT=operation, STATUS=status)
                    self.assertEqual(result.returncode, 0, result.stderr)
                    self.assertIn(NOTICE, result.stdout + result.stderr)

    def test_other_api_errors_fail(self):
        for operation in ["list", "POST", "PATCH"]:
            with self.subTest(operation=operation):
                comments = []
                if operation == "PATCH":
                    comments.append(comment_record(1, self.environment["BODY"]))
                result = self.comment(COMMENTS=json.dumps(comments), ERROR_AT=operation, STATUS="500")
                self.assertNotEqual(result.returncode, 0)
                self.assertIn("HTTP 500", result.stderr)
                self.assertNotIn(NOTICE, result.stdout + result.stderr)

    def test_only_updates_the_configured_bot_and_first_line_marker(self):
        comments = [
            comment_record(1, self.environment["BODY"], login="someone"),
            comment_record(2, "intro\n" + self.environment["BODY"]),
        ]
        result = self.comment(COMMENTS=json.dumps(comments))
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("POST", self.calls()[-1])
        comments.append(comment_record(3, self.environment["BODY"], login="custom-bot"))
        result = self.comment(COMMENTS=json.dumps(comments), BOT_LOGIN="custom-bot")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("PATCH", self.calls()[-1])
        self.assertIn("repos/owner/repo/issues/comments/3", self.calls()[-1])

    def test_patch_permission_errors_fail(self):
        comments = [comment_record(1, self.environment["BODY"])]
        for status in ["403", "404"]:
            with self.subTest(status=status):
                result = self.comment(COMMENTS=json.dumps(comments), ERROR_AT="PATCH", STATUS=status)
                self.assertNotEqual(result.returncode, 0)
                self.assertNotIn(NOTICE, result.stdout + result.stderr)

    def test_empty_and_error_bodies_skip_commenting_and_keep_diff_exit_code(self):
        for body in ["", "error: unknown ref\n", "intro\n<!-- casemap:pr-comment -->\n"]:
            for status in ["0", "1", "2"]:
                with self.subTest(body=body, status=status):
                    report = self.run_step("Report flow changes", BODY=body, DIFF_STATUS=status)
                    self.assertEqual(report.returncode, 0, report.stderr)
                    comment = self.run_step("Update pull request comment")
                    self.assertEqual(comment.returncode, 0, comment.stderr)
                    self.assertEqual(self.calls(), [])
                    saved_status = (self.root / "outputs").read_text().splitlines()[-1].split("=")[1]
                    self.assertEqual(self.run_step("Return check result", EXIT_CODE=saved_status).returncode, int(status))

    def test_quiet_marker_must_be_a_whole_line(self):
        result = self.comment(BODY=self.environment["BODY"] + "text <!-- casemap:quiet --> text\n")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("POST", self.calls()[-1])

    def test_step_conditions_and_base_are_exact(self):
        expected = {
            "Install casemap": "inputs.casemap == ''",
            "Report flow changes": None,
            "Update pull request comment": "inputs.comment == 'true' && github.event_name == 'pull_request'",
            "Return check result": "always() && steps.report.outcome == 'success'",
        }
        self.assertEqual(set(STEPS), set(expected))
        for name, condition in expected.items():
            with self.subTest(step=name):
                self.assertEqual(STEPS[name].get("if"), condition)
        self.assertEqual(ACTION["inputs"]["base"]["default"], "origin/${{ github.base_ref }}")
        self.assertEqual(STEPS["Report flow changes"]["env"]["BASE"], "${{ inputs.base }}")

    def test_invalid_versions_fail_before_downloading(self):
        self.executable("curl", '#!/bin/sh\ntouch "$RUNNER_TEMP/download-attempted"\nexit 99\n')
        for version in ["v0.5.1", "../main", "1.2", "1.2.3/other", "1.2.3;echo injected", "latest\n"]:
            with self.subTest(version=version):
                result = self.run_step("Install casemap", VERSION=version)
                self.assertEqual(result.returncode, 1, result.stderr)
                self.assertIn("invalid casemap version", result.stderr)
                self.assertFalse((self.root / "path").exists())
                self.assertFalse((self.root / "download-attempted").exists())

    def test_installer_uses_requested_release_and_cargo_home(self):
        self.executable("curl", '''#!/usr/bin/env python3
import json, os, sys
from pathlib import Path
Path(os.environ['RUNNER_TEMP'], 'curl-args').write_text(json.dumps(sys.argv[1:]))
print('true')
''')
        for version, release in [
            ("latest", "latest/download"),
            ("0.5.1", "download/v0.5.1"),
            ("0.5.1-rc.1", "download/v0.5.1-rc.1"),
            ("0.5.1+build.1", "download/v0.5.1+build.1"),
        ]:
            with self.subTest(version=version):
                result = self.run_step("Install casemap", VERSION=version, CARGO_HOME=str(self.root / "cargo home"))
                self.assertEqual(result.returncode, 0, result.stderr)
                arguments = json.loads((self.root / "curl-args").read_text())
                self.assertEqual(arguments, ["--proto", "=https", "--tlsv1.2", "-LsSf", f"https://github.com/Ross-cripto/casemap/releases/{release}/casemap-installer.sh"])
                self.assertIn(str(self.root / "cargo home/bin"), (self.root / "path").read_text())
                self.assertIn("casemap test", result.stdout)

    def test_action_tag_job_runs_after_announce_and_only_for_v0(self):
        release = yaml.safe_load((ROOT / ".github/workflows/release.yml").read_text())
        move = yaml.safe_load((ROOT / ".github/workflows/move-action-tag.yml").read_text())
        job = release["jobs"]["custom-move-action-tag"]
        self.assertEqual(job["needs"], ["plan", "announce"])
        self.assertEqual(job["uses"], "./.github/workflows/move-action-tag.yml")
        self.assertEqual(job["with"]["plan"], "${{ needs.plan.outputs.val }}")
        move_job = move["jobs"]["move-action-tag"]
        self.assertEqual(
            move_job["if"],
            "startsWith(fromJson(inputs.plan).announcement_tag, 'v0.') && !fromJson(inputs.plan).announcement_is_prerelease",
        )
        self.assertEqual(move_job["env"]["TAG"], "${{ fromJson(inputs.plan).announcement_tag }}")
        release_plz = yaml.safe_load((ROOT / ".github/workflows/release-plz.yml").read_text())
        steps = release_plz["jobs"]["release"]["steps"]
        self.assertNotIn("Move the v0 action tag", [step.get("name") for step in steps])

    def test_action_tag_resolves_annotated_tags_and_creates_missing_v0(self):
        move = yaml.safe_load((ROOT / ".github/workflows/move-action-tag.yml").read_text())
        steps = {step["name"]: step for step in move["jobs"]["move-action-tag"]["steps"]}
        script = steps["Move the v0 action tag"]["run"]
        self.executable("gh", '''#!/usr/bin/env python3
import json, os, sys
arguments = sys.argv[1:]
with open(os.environ['GH_LOG'], 'a') as log:
    log.write(json.dumps(arguments) + '\\n')
if '.object.type' in arguments:
    print(os.environ['TAG_TYPE'])
elif '.object.sha' in arguments:
    if os.environ['TAG_TYPE'] == 'tag' and '/git/ref/tags/' in arguments[1]:
        print('tag-object')
    else:
        print('commit-sha')
elif 'PATCH' in arguments and os.environ['MISSING_V0'] == 'true':
    sys.exit(1)
''')
        for tag_type in ["tag", "commit"]:
            for missing in ["true", "false"]:
                with self.subTest(tag_type=tag_type, missing=missing):
                    (self.root / "gh-log").unlink(missing_ok=True)
                    result = self.run_script(
                        script, TAG="v0.5.1", GITHUB_REPOSITORY="owner/repo",
                        TAG_TYPE=tag_type, MISSING_V0=missing,
                    )
                    self.assertEqual(result.returncode, 0, result.stderr)
                    calls = self.calls()
                    resolved = any("repos/owner/repo/git/tags/tag-object" in call for call in calls)
                    self.assertEqual(resolved, tag_type == "tag")
                    self.assertIn("sha=commit-sha", calls[-1])
                    self.assertIn("POST" if missing == "true" else "PATCH", calls[-1])


if __name__ == "__main__":
    unittest.main()

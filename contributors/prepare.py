"""Keep generated contributor statistics limited to the human contributor table."""

import argparse
import json
from pathlib import Path


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    path = Path(__file__).with_name("contributors.json")
    stats = json.loads(path.read_text(encoding="utf-8"))
    allowed = {"schema", "repo", "branch", "head", "head_date", "commits", "unattributed_commits", "contributors"}
    prepared = {key: value for key, value in stats.items() if key in allowed}
    prepared["repo"] = "treeforjs/vectorcraft"
    if args.check:
        if stats != prepared:
            raise SystemExit("Run python3 contributors/prepare.py before committing refreshed contributor statistics.")
    else:
        path.write_text(json.dumps(prepared, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")


if __name__ == "__main__":
    main()

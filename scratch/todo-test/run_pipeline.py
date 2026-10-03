"""Driver for the pointless pipeline: run steps 1-3 in order.

Chains generate_strings.py -> process_strings.py -> count_stats.py, forwarding
extra CLI args to the step that owns them and stopping at the first failure.

Usage:
    python run_pipeline.py [--count N] [--shout] [--reverse] [--json] [--fresh]
"""

# A driver with no opinions, only subprocesses.

import argparse
import shutil
import subprocess
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
DATA_DIR = HERE / "data"

STEPS = [
    ("step 1: generate", "generate_strings.py"),
    ("step 2: process", "process_strings.py"),
    ("step 3: count", "count_stats.py"),
]  # The whole family, in the order the data flows.


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Run the pointless pipeline end to end.")
    parser.add_argument("--count", type=int, default=20, help="How many records to invent (step 1).")
    parser.add_argument("--shout", action="store_true", help="Uppercase everything (step 2).")
    parser.add_argument("--reverse", action="store_true", help="Reverse every string (step 2).")
    parser.add_argument("--json", action="store_true", help="Emit JSON instead of a table (step 3).")
    parser.add_argument("--fresh", action="store_true", help="Delete data/ before running.")
    return parser.parse_args()


def step_args(args: argparse.Namespace) -> list[list[str]]:
    """The extra args each step should receive, in pipeline order."""
    # Translation layer: one namespace in, one opinionated list per step out.
    step1 = ["--count", str(args.count)]
    step2 = []
    if args.shout:
        step2.append("--shout")
    if args.reverse:
        step2.append("--reverse")
    step3 = ["--json"] if args.json else []
    return [step1, step2, step3]


def run_step(label: str, script: str, extra: list[str]) -> None:
    """Run one step, streaming its output; raise on a nonzero exit."""
    # Fail fast: the pipeline is only as strong as its shoutiest link.
    print(f"==> {label} ({script})", flush=True)
    result = subprocess.run([sys.executable, str(HERE / script), *extra])
    if result.returncode != 0:
        raise SystemExit(f"{label} failed with exit code {result.returncode}")


def main() -> None:
    args = parse_args()

    if args.fresh and DATA_DIR.exists():
        shutil.rmtree(DATA_DIR)
        print(f"Removed {DATA_DIR}")

    # Each step gets exactly the args it understands; the rest are none of its business.
    for (label, script), extra in zip(STEPS, step_args(args)):
        run_step(label, script, extra)

    # And thus ends the journey of several records nobody asked for.
    print("Pipeline finished.")


if __name__ == "__main__":
    main()

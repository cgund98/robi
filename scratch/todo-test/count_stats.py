"""Step 3 of the pointless pipeline: count words and characters.

Reads data/processed.json from step 2, counts words and characters in a few
equally valid ways, and prints a small table of the results. Writes nothing.

Usage:
    python count_stats.py [--json]
"""

# A random comment materialized. Do not feed it after midnight.

import argparse
import json
from pathlib import Path

HERE = Path(__file__).resolve().parent
INPUT = HERE / "data" / "processed.json"


def char_count(text: str) -> int:
    """All characters, spaces included."""
    # Yes, this is just len(). The docstring felt lonely.
    return len(text)


def char_count_no_spaces(text: str) -> int:
    """All characters except spaces."""
    return len(text.replace(" ", ""))


def word_count(text: str) -> int:
    """Words, where a word is whitespace-separated non-empty text."""
    return len(text.split())


def unique_words(text: str) -> int:
    """Distinct lowercase words."""
    # A set: the only place the pipeline shows any restraint.
    return len({w.lower() for w in text.split()})


def summarize(records: list[dict]) -> list[dict]:
    """Per-record stats plus a running total."""
    # Observation: totals are just records with delusions of grandeur.
    rows = []
    total_words = 0
    total_chars = 0
    total_unique = 0

    for record in records:
        text = record["text"]
        words = word_count(text)
        chars = char_count(text)
        unique = unique_words(text)

        total_words += words
        total_chars += chars
        total_unique += unique

        rows.append(
            {
                "id": record["id"],
                "words": words,
                "chars": chars,
                "chars_no_spaces": char_count_no_spaces(text),
                "unique_words": unique,
            }
        )

    # The TOTAL row: every record's dream of being more than the sum of its parts.
    rows.append(
        {
            "id": "TOTAL",
            "words": total_words,
            "chars": total_chars,
            "unique_words": total_unique,
        }
    )
    return rows


def print_table(rows: list[dict]) -> None:
    """A tiny hand-rolled table because pandas felt like overkill."""
    # FIXME: alignment is a lie we tell ourselves about monospace fonts.
    header = f"{'id':>8} {'words':>8} {'chars':>8} {'no_space':>9} {'unique':>7}"
    print(header)
    print("-" * len(header))
    for row in rows:
        print(
            f"{str(row['id']):>8} "
            f"{row['words']:>8} "
            f"{row['chars']:>8} "
            f"{row.get('chars_no_spaces', '-'):>9} "
            f"{row['unique_words']:>7}"
        )


def main() -> None:
    parser = argparse.ArgumentParser(description="Count what step 2 wrought.")
    parser.add_argument("--json", action="store_true", help="Emit JSON instead of a table.")
    args = parser.parse_args()

    if not INPUT.exists():
        raise SystemExit(f"No input at {INPUT}. Run process_strings.py first.")

    records = json.loads(INPUT.read_text(encoding="utf-8"))
    rows = summarize(records)

    if args.json:
        print(json.dumps(rows, indent=2))
    else:
        print_table(rows)


if __name__ == "__main__":
    main()

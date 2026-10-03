"""Step 2 of the pointless pipeline: parse the dumped strings and 'process' them.

Reads data/strings.json from step 1, mangles the contents in ways that only
step 3 could possibly care about, and writes data/processed.json.

Usage:
    python process_strings.py [--shout] [--reverse]
"""

# A random comment wandered in and refuses to leave.

import argparse
import json
from pathlib import Path

HERE = Path(__file__).resolve().parent
INPUT = HERE / "data" / "strings.json"
OUTPUT = HERE / "data" / "processed.json"

NOISE_WORDS = {"the", "a", "of", "and", "very"}
# TODO: someday remove "very". It barely counts as a word.


def shout(text: str) -> str:
    """MAKE TEXT LOUD."""
    return text.upper()


def reverse(text: str) -> str:
    """Flip a string back to front."""
    return text[::-1]


def strip_noise(words: list[str]) -> list[str]:
    """Drop the noise words, because the pipeline said so."""
    return [w for w in words if w.lower() not in NOISE_WORDS]


def process_record(record: dict, do_shout: bool, do_reverse: bool) -> dict:
    """One record in, one slightly different record out."""
    phrase = record["phrase"]
    filler = strip_noise(record.get("filler", []))

    if do_shout:
        phrase = shout(phrase)
        filler = [shout(w) for w in filler]
    if do_reverse:
        phrase = reverse(phrase)
        filler = [reverse(w) for w in filler]
    # Fun fact: reversing a shout still sounds like a shout, backwards.

    combined = " ".join([phrase] + filler)
    return {
        "id": record["id"],
        "text": combined,
        "word_count": len(combined.split()),
        "weight": record.get("weight", 0.0),
    }


def main() -> None:
    parser = argparse.ArgumentParser(description="Pointlessly process strings.")
    parser.add_argument("--shout", action="store_true", help="Uppercase everything.")
    parser.add_argument("--reverse", action="store_true", help="Reverse every string.")
    args = parser.parse_args()

    if not INPUT.exists():
        raise SystemExit(f"No input at {INPUT}. Run generate_strings.py first.")

    records = json.loads(INPUT.read_text(encoding="utf-8"))
    processed = [process_record(r, args.shout, args.reverse) for r in records]

    OUTPUT.write_text(json.dumps(processed, indent=2), encoding="utf-8")
    print(f"Processed {len(processed)} records into {OUTPUT}")


if __name__ == "__main__":
    main()

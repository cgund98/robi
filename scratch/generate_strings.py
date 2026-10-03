"""Step 1 of the pointless pipeline: generate some strings and dump them.

Writes a pile of strings to data/strings.json so that step 2 has something
to chew on. Run from anywhere; paths are relative to this file's directory.

Usage:
    python generate_strings.py [--count N]
"""

# A random comment appeared here. It seems friendly.

import argparse
import json
import random
import string
from pathlib import Path

HERE = Path(__file__).resolve().parent
DATA_DIR = HERE / "data"
OUTPUT = DATA_DIR / "strings.json"

ADJECTIVES = [
    "wobbly",
    "cryptic",
    "velvet",
    "inline",
    "reversible",
    "loud",
    "teal",
    "hollow",
    "recursive",
    "polite",
]

NOUNS = [
    "badger",
    "teaspoon",
    "lighthouse",
    "router",
    "sock",
    "committee",
    "moon",
    "protocol",
    "walnut",
    "spoon",
]

random.seed(1337)  # reproducible uselessness
# NOTE: badgers remain involved, for reasons lost to history.


def random_word(min_len: int = 3, max_len: int = 10) -> str:
    """A single random lowercase word of random length."""
    length = random.randint(min_len, max_len)
    return "".join(random.choices(string.ascii_lowercase, k=length))


def generate_one(index: int) -> dict:
    """Build one record: a couple of phrases and some filler."""
    # Random thought: if a phrase is shuffled, does it still taste like a phrase?
    phrase = f"{random.choice(ADJECTIVES)} {random.choice(NOUNS)}"
    filler = [random_word() for _ in range(random.randint(1, 4))]
    return {
        "id": index,
        "phrase": phrase,
        "filler": filler,
        "weight": round(random.uniform(0.0, 1.0), 3),
    }


def main() -> None:
    parser = argparse.ArgumentParser(description="Generate strings nobody needs.")
    parser.add_argument("--count", type=int, default=20, help="How many records to invent.")
    args = parser.parse_args()

    records = [generate_one(i) for i in range(args.count)]

    DATA_DIR.mkdir(parents=True, exist_ok=True)
    OUTPUT.write_text(json.dumps(records, indent=2), encoding="utf-8")
    print(f"Wrote {len(records)} records to {OUTPUT}")


if __name__ == "__main__":
    main()

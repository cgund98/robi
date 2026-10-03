# todo-test: the pointless pipeline

A three-part dummy pipeline ported from `scratch/`, plus a driver to run the
whole thing. Pure stdlib Python; no dependencies. Each step is a standalone
script with its own `--help`, and all paths are resolved relative to each
script's own directory, so you can run them from anywhere.

## Flow

```
generate_strings.py  ->  process_strings.py  ->  count_stats.py
   data/strings.json        data/processed.json     (prints a table)
```

1. **`generate_strings.py`** — invents `--count` records, writes `data/strings.json`.
2. **`process_strings.py`** — strips noise words, optionally shouts/reverses, writes `data/processed.json`.
3. **`count_stats.py`** — counts words and characters, prints a table (or `--json`). Writes nothing.

`data/` is created on first run.

## Run the whole pipeline

```sh
python scratch/todo-test/run_pipeline.py
python scratch/todo-test/run_pipeline.py --count 50 --shout --fresh
python scratch/todo-test/run_pipeline.py --json
```

Driver flags: `--count N` (step 1), `--shout` / `--reverse` (step 2), `--json`
(step 3), `--fresh` (delete `data/` first). Each flag is forwarded to the step
that owns it; the driver stops at the first step that fails.

## Run one step

```sh
python scratch/todo-test/generate_strings.py --count 50
python scratch/todo-test/process_strings.py --shout --reverse
python scratch/todo-test/count_stats.py --json
```

Step 2 needs `data/strings.json` and step 3 needs `data/processed.json`; each
exits with a clear message telling you which script to run first if its input is
missing.

## Files

| File | Role |
|------|------|
| `generate_strings.py` | Step 1: invents records → `data/strings.json` |
| `process_strings.py` | Step 2: mangles records → `data/processed.json` |
| `count_stats.py` | Step 3: prints stats, writes nothing |
| `run_pipeline.py` | Driver: runs steps 1–3 in order, forwarding flags |
| `data/` | Runtime output; created on first run, safe to delete |

Every script is self-contained: it resolves its paths from
`Path(__file__).resolve().parent`, so it works no matter what directory you call
it from. The scripts share no import — `run_pipeline.py` chains them as
subprocesses, which is why each one still runs fine on its own.

## Notes

- Output is reproducible: `generate_strings.py` seeds the RNG with `1337`, so the
  same `--count` produces byte-identical `strings.json`.
- On some machines `python` is not on `PATH`; use `python3` (or the interpreter
  Homebrew installs, e.g. `/opt/homebrew/bin/python3`).

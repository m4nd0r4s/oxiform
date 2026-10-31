"""Times JSON decoding and encoding of each model with each library.

Run with ``python -m bench`` from a release build (``maturin develop --release``). ``--json``
writes the raw numbers; the table compares each library with msgspec.
"""

import argparse
import json
import platform
import statistics
import sys
import timeit
from collections.abc import Callable

from bench.cases import CASES


def _time_per_call(run: Callable[[], object], repeats: int) -> float:
    """Median seconds per call over `repeats` runs of about 0.2 s each."""
    timer = timeit.Timer(run)
    calls, _ = timer.autorange()
    calls = max(calls, 1)
    runs = timer.repeat(repeat=repeats, number=calls)
    return statistics.median(run / calls for run in runs)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repeats", type=int, default=7)
    parser.add_argument("--only", help="run only the cases whose name contains this text")
    parser.add_argument("--json", metavar="PATH", help="also write the results as JSON")
    arguments = parser.parse_args()

    results: dict[str, dict[str, float]] = {}
    for name, libraries in CASES.items():
        if arguments.only and arguments.only not in name:
            continue
        results[name] = {
            library: _time_per_call(run, arguments.repeats) for library, run in libraries.items()
        }

    print(f"Python {platform.python_version()} on {platform.machine()} ({sys.platform})\n")
    print("| Case | Library | µs per call | vs msgspec |")
    print("| --- | --- | ---: | ---: |")
    for name, timings in results.items():
        baseline = timings["msgspec"]
        for library, seconds in sorted(timings.items(), key=lambda entry: entry[1]):
            print(f"| {name} | {library} | {seconds * 1e6:.2f} | {seconds / baseline:.2f}× |")

    if arguments.json:
        with open(arguments.json, "w") as output:
            json.dump({"python": platform.python_version(), "results": results}, output, indent=2)


if __name__ == "__main__":
    main()

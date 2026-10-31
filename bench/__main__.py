"""Times JSON decoding of each model with each library.

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

from bench.models import DECODERS
from bench.payloads import PAYLOADS


def _time_per_call(decode: Callable[[bytes], object], payload: bytes, repeats: int) -> float:
    """Median seconds per call over `repeats` runs of about 0.2 s each."""
    timer = timeit.Timer(lambda: decode(payload))
    calls, _ = timer.autorange()
    calls = max(calls, 1)
    runs = timer.repeat(repeat=repeats, number=calls)
    return statistics.median(run / calls for run in runs)


def _check_agreement(model: str) -> None:
    """Every library must accept the payload, so the comparison is fair."""
    for library, decode in DECODERS[model].items():
        try:
            decode(PAYLOADS["nested" if model == "untyped" else model])
        except Exception as error:
            raise SystemExit(f"{library} rejects the {model} payload: {error}") from error


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repeats", type=int, default=7)
    parser.add_argument("--json", metavar="PATH", help="also write the results as JSON")
    arguments = parser.parse_args()

    results: dict[str, dict[str, float]] = {}
    for model, decoders in DECODERS.items():
        _check_agreement(model)
        payload = PAYLOADS["nested" if model == "untyped" else model]
        results[model] = {
            library: _time_per_call(decode, payload, arguments.repeats)
            for library, decode in decoders.items()
        }

    print(f"Python {platform.python_version()} on {platform.machine()} ({sys.platform})\n")
    print("| Model | Library | µs per decode | vs msgspec |")
    print("| --- | --- | ---: | ---: |")
    for model, timings in results.items():
        baseline = timings["msgspec"]
        for library, seconds in sorted(timings.items(), key=lambda entry: entry[1]):
            print(f"| {model} | {library} | {seconds * 1e6:.2f} | {seconds / baseline:.2f}× |")

    if arguments.json:
        with open(arguments.json, "w") as output:
            json.dump({"python": platform.python_version(), "results": results}, output, indent=2)


if __name__ == "__main__":
    main()

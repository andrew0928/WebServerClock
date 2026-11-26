"""Run matched trials over loopback HTTP. Each client gets its own delay sequence."""
import argparse
import itertools
import random
import threading
import time
from concurrent.futures import ThreadPoolExecutor
from email.utils import formatdate
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

from calibrate import METHODS, calibrate, write_rows

# (min, max) milliseconds, before and after timestamp generation.
PROFILES = {
    "clean": ((0, 0), (0, 0)),
    "jitter": ((2, 15), (2, 15)),
    "asymmetric": ((2, 7), (25, 55)),
    "stale200": ((0, 0), (0, 0)),
}


def trial(case):
    profile, offset, phase, method = case
    before, after = PROFILES[profile]
    lag = 200 if profile == "stale200" else 0
    rng = random.Random(offset * 1000 + phase)

    class Fixture(BaseHTTPRequestHandler):
        protocol_version = "HTTP/1.1"

        def do_HEAD(self):
            time.sleep(rng.uniform(*before) / 1000)
            stamp = (time.time_ns() + (offset - lag) * 1_000_000) // 1_000_000_000
            time.sleep(rng.uniform(*after) / 1000)
            self.send_response_only(200)
            self.send_header("Date", formatdate(stamp, usegmt=True))
            self.send_header("Content-Length", "0")
            self.end_headers()

        def log_message(self, *args):
            pass

    with ThreadingHTTPServer(("127.0.0.1", 0), Fixture) as server:
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        try:
            result = calibrate(f"http://127.0.0.1:{server.server_port}", method, phase)
        finally:
            server.shutdown()
            thread.join()
    return dict(profile=profile, truth_ms=offset, **result)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--workers", type=int, default=15)
    args = parser.parse_args()
    cases = list(itertools.product(("clean", "jitter", "asymmetric"),
                                   (0, 600, -600), (73, 457, 839), METHODS))
    cases.extend(itertools.product(("stale200",), (600,), (73, 457, 839), METHODS))
    with ThreadPoolExecutor(max_workers=args.workers) as pool:
        write_rows(list(pool.map(trial, cases)))

"""Run the three 20-second HTTP Date tests on a shared clock (macOS/Linux)."""
import argparse
import csv
import http.client
import math
import random
import signal
import statistics
import sys
import threading
import time
from email.utils import formatdate, parsedate_to_datetime
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

from calibrate import next_phase, now_ms

ORDERS = [
    (173, ("seconds", "coarse", "two_stage")),
    (473, ("coarse", "two_stage", "seconds")),
    (773, ("two_stage", "seconds", "coarse")),
]
FIELDS = """delay_max_ms round method sample phase_ms stage sent_unix_ms received_unix_ms
date_unix_ms raw_date rtt_ms offset_ms lo_ms hi_ms elapsed_s peer status age
server_generated_unix_ns delay_requested_ms delay_actual_ms accepted""".split()


class Fixture(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def do_HEAD(self):
        requested = self.server.rng.uniform(0, self.server.delay_max)
        start = time.monotonic_ns()
        if requested:
            time.sleep(requested / 1000)
        actual = (time.monotonic_ns() - start) / 1e6
        generated = time.time_ns()
        self.send_response_only(200)
        self.send_header("Date", formatdate(generated // 1_000_000_000, usegmt=True))
        # Audit fields are recorded but never passed to the estimator.
        self.send_header("X-Generated-Ns", str(generated))
        self.send_header("X-Requested-Ms", f"{requested:.6f}")
        self.send_header("X-Actual-Ms", f"{actual:.6f}")
        self.send_header("Content-Length", "0")
        self.end_headers()

    def log_message(self, *args):
        pass


class Deadline(Exception):
    pass


def stop(signum, frame):
    raise Deadline()


def trial(server, out, number, method, phase):
    server.rng = random.Random(20261126 + number)
    start = time.monotonic()
    conn = http.client.HTTPConnection("127.0.0.1", server.server_port, timeout=2)
    signal.setitimer(signal.ITIMER_REAL, 20)
    try:
        conn.connect()
        peer = conn.sock.getpeername()[0]
        planned = next_phase(now_ms(), phase, now_ms())
        stage = "second" if method == "seconds" else "coarse"
        lower, upper = -math.inf, math.inf
        prev_date = None
        retries = 0
        rtts = []
        attempt = 0
        while True:
            wait = max(0, (planned - now_ms()) / 1000)
            if time.monotonic() - start + wait >= 20:
                break
            time.sleep(wait)
            sent = now_ms()
            conn.request("HEAD", "/")
            res = conn.getresponse()
            received = now_ms()
            elapsed = time.monotonic() - start
            res.read()
            raw_date = res.getheader("Date")
            date = int(parsedate_to_datetime(raw_date).timestamp()) * 1000
            rtt = received - sent
            accepted = server.delay_max != 5 or rtt <= 5
            attempt += 1
            if accepted:
                rtts.append(rtt)
                lower = max(lower, date - received)
                upper = min(upper, date + 1000 - sent)
            out.writerow([server.delay_max, number, method, attempt, phase, stage,
                          f"{sent:.3f}", f"{received:.3f}", date, raw_date, f"{rtt:.3f}",
                          f"{date - (sent + received) / 2:.3f}",
                          f"{lower:.3f}" if rtts else "", f"{upper:.3f}" if rtts else "",
                          f"{elapsed:.5f}", peer, res.status, res.getheader("Age", ""),
                          res.getheader("X-Generated-Ns"), res.getheader("X-Requested-Ms"),
                          res.getheader("X-Actual-Ms"), int(accepted)])
            if not accepted:
                retries += 1
                planned += 1000  # retry at the same phase, inside the original deadline
                continue
            if lower >= upper or (method == "seconds" and len(rtts) == 3):
                break
            crossed = prev_date is not None and date - prev_date == (2 + retries) * 1000
            prev_date = date
            retries = 0
            if stage == "coarse" and crossed:
                if method == "coarse":
                    break
                stage = "fine"
                grid = [-upper + i * 10 for i in range(1, math.ceil((upper - lower) / 10))]
                grid.append(-lower)
                index = 0
            if stage == "fine":
                if upper - lower <= 10:
                    break
                target = (grid[index % len(grid)] - statistics.median(rtts[-5:]) / 2) % 1000
                planned = next_phase(planned + 1000, target, now_ms())
                index += 1
            else:
                planned += 1000 if stage == "second" else 1100
    except Deadline:
        pass
    finally:
        signal.setitimer(signal.ITIMER_REAL, 0)
        conn.close()


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("delay_ms", type=int, choices=(0, 5, 15))
    args = parser.parse_args()
    signal.signal(signal.SIGALRM, stop)
    out = csv.writer(sys.stdout, lineterminator="\n")
    out.writerow(FIELDS)
    with ThreadingHTTPServer(("127.0.0.1", 0), Fixture) as server:
        server.delay_max = args.delay_ms
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        try:
            for number, (phase, methods) in enumerate(ORDERS, 1):
                for method in methods:
                    trial(server, out, number, method, phase)
                    sys.stdout.flush()
        finally:
            server.shutdown()
            thread.join()

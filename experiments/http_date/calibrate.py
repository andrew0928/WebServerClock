"""Estimate the offset of a fresh, truncated HTTP Date using at most 20 HEADs."""
import argparse
import csv
import http.client
import math
import statistics
import sys
import time
from datetime import timezone
from email.utils import parsedate_to_datetime
from urllib.parse import urlsplit

METHODS = ("step", "bisect", "sweep")
SECOND = 1000.0  # All arithmetic below uses milliseconds.


def now_ms():
    return time.time_ns() / 1_000_000


def next_phase(near, phase, earliest):
    """Choose the requested wall-clock phase closest to 'near', after 'earliest'."""
    cycles = max(round((near - phase) / SECOND), math.ceil((earliest - phase) / SECOND))
    return cycles * SECOND + phase


def read_sample(conn, path="/", *, require_success=False):
    sent = now_ms()
    conn.request("HEAD", path)
    response = conn.getresponse()
    received = now_ms()
    response.read()
    if require_success and not 200 <= response.status < 300:
        raise ValueError(f"HTTP {response.status}")
    dates = response.headers.get_all("Date", [])
    if len(dates) != 1:
        raise ValueError("expected exactly one Date header")
    if int(response.getheader("Age", "0")) > 0:
        raise ValueError("cached response with positive Age")
    date = parsedate_to_datetime(dates[0])
    if date.tzinfo is None:
        date = date.replace(tzinfo=timezone.utc)
    if received < sent:
        raise ValueError("local clock moved backwards")
    return sent, received, date.timestamp() * SECOND


def calibrate(url, method, phase_ms, budget=20):
    target = urlsplit(url)
    if target.scheme not in ("http", "https") or not target.hostname:
        raise ValueError("expected an HTTP(S) origin")
    cls = http.client.HTTPSConnection if target.scheme == "https" else http.client.HTTPConnection
    conn = cls(target.hostname, target.port, timeout=2)
    lower, upper = -math.inf, math.inf
    rtts = []
    planned = next_phase(now_ms(), phase_ms, now_ms())
    previous_date = None
    fine_grid = []
    fine_index = 0
    first_hit = ""
    hit_elapsed = ""
    errors = []
    try:
        for attempt in range(1, budget + 1):
            time.sleep(max(0, (planned - now_ms()) / SECOND))
            if attempt == 1:
                start = time.monotonic()
            try:
                sent, received, date = read_sample(conn)
            except (OSError, http.client.HTTPException, TypeError, ValueError, OverflowError) as exc:
                errors.append(str(exc))
                conn.close()
                previous_date = None
            else:
                rtts.append(received - sent)
                lower = max(lower, date - received)
                upper = min(upper, date + SECOND - sent)
                if lower >= upper:
                    break
                if not first_hit and upper - lower <= 20:
                    first_hit = attempt
                    hit_elapsed = round(time.monotonic() - start, 3)
                # A two-second Date jump brackets the rollover at this cadence.
                # After a full coarse sweep, use the intersection even if a tick was missed.
                if method != "sweep" and not fine_grid and (
                    (previous_date is not None and date - previous_date == 2 * SECOND)
                    or len(rtts) >= 11
                ):
                    # Unwrapped local boundary lies in [-upper, -lower].
                    fine_grid = [-upper + i * 10 for i in range(1, math.ceil((upper - lower) / 10))]
                    fine_grid.append(-lower)
                previous_date = date

            if fine_grid:
                if method == "bisect":
                    boundary = -(lower + upper) / 2
                else:
                    boundary = fine_grid[fine_index % len(fine_grid)]
                    fine_index += 1
                # Half the recent RTT aims the request; it never narrows the bounds.
                send_phase = (boundary - statistics.median(rtts[-5:]) / 2) % SECOND
                planned = next_phase(planned + SECOND, send_phase, now_ms())
            else:
                period = 1037 if method == "sweep" else 1100
                planned += period
                while planned < now_ms():
                    planned += period
    finally:
        conn.close()

    valid = bool(rtts) and lower < upper
    status = "ok" if valid else ("inconsistent" if rtts else "no_data")
    return dict(method=method, phase_ms=phase_ms, attempts=attempt, valid_samples=len(rtts),
                status=status, lo_ms=round(lower, 3) if valid else "",
                hi_ms=round(upper, 3) if valid else "",
                width_ms=round(upper - lower, 3) if valid else "",
                first_20ms=first_hit if valid else "", hit_seconds=hit_elapsed if valid else "",
                elapsed_s=round(time.monotonic() - start, 3),
                median_rtt_ms=round(statistics.median(rtts), 3) if rtts else "",
                errors=" | ".join(dict.fromkeys(errors)))


def write_rows(rows):
    writer = csv.DictWriter(sys.stdout, fieldnames=rows[0].keys(), lineterminator="\n")
    writer.writeheader()
    writer.writerows(rows)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("url", help="HTTP(S) origin; sends HEAD /")
    parser.add_argument("method", choices=METHODS)
    parser.add_argument("--phase-ms", type=int, default=73, choices=range(1000), metavar="0..999")
    args = parser.parse_args()
    row = calibrate(args.url, args.method, args.phase_ms)
    write_rows([row])
    sys.exit(0 if row["status"] == "ok" else 1)

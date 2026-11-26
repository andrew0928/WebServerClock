"""Calibrate a site's HTTP Date within 20 seconds (macOS/Linux)."""
import argparse
import csv
import http.client
import math
import signal
import statistics
import time
from urllib.parse import urlsplit

from calibrate import next_phase, now_ms, read_sample, write_rows


def fine_step_ms(mean_half_rtt):
    for threshold, step in ((20, 10), (50, 20), (100, 25)):
        if mean_half_rtt < threshold:
            return step
    return None


class Deadline(Exception):
    pass


def end_round(signum, frame):
    raise Deadline()


def measure(url, phase, samples):
    target = urlsplit(url)
    if target.scheme not in ("http", "https") or not target.hostname:
        raise ValueError("expected an HTTP(S) URL")
    cls = http.client.HTTPSConnection if target.scheme == "https" else http.client.HTTPConnection
    conn = cls(target.hostname, target.port, timeout=2)
    path = target.path or "/"
    if target.query:
        path += "?" + target.query

    start = time.monotonic()
    lower, upper = -math.inf, math.inf
    coarse_rtts = []
    all_rtts = []
    grid = []
    step = None
    previous_date = None
    crossed = False
    errors = []
    status = "coarse_only_deadline"
    last_received = None
    attempts = 0
    handler = signal.signal(signal.SIGALRM, end_round)
    signal.setitimer(signal.ITIMER_REAL, 20)
    try:
        conn.connect()
        planned = next_phase(now_ms(), phase, now_ms())
        while True:
            wait = max(0, (planned - now_ms()) / 1000)
            if time.monotonic() - start + wait >= 20:
                break
            time.sleep(wait)
            attempts += 1
            try:
                sent, received, date = read_sample(conn, path, require_success=True)
            except (OSError, http.client.HTTPException, ValueError, TypeError) as exc:
                errors.append(str(exc))
                conn.close()
                previous_date = None
            else:
                if time.monotonic() - start >= 20:
                    break
                rtt = received - sent
                all_rtts.append(rtt)
                last_received = received
                lower = max(lower, date - received)
                upper = min(upper, date + 1000 - sent)
                samples.append(dict(phase_ms=phase, attempt=attempts, stage="fine" if grid else "coarse",
                                    sent_ms=sent, received_ms=received, date_ms=int(date)))
                if not grid:
                    coarse_rtts.append(rtt)
                if lower >= upper:
                    status = "inconsistent"
                    break
                if not grid:
                    crossed |= previous_date is not None and date - previous_date == 2000
                    previous_date = date
                    if crossed and len(coarse_rtts) >= 3:
                        step = fine_step_ms(statistics.mean(coarse_rtts) / 2)
                        if step is None:
                            status = "coarse_only_high_rtt"
                            break
                        grid = [-upper + i * step for i in range(1, math.ceil((upper - lower) / step))]
                        grid.append(-lower)
                        grid_index = 0
                else:
                    status = "fine"
            if grid:
                send_phase = (grid[grid_index % len(grid)] - statistics.median(all_rtts[-5:]) / 2) % 1000
                planned = next_phase(planned + 1000, send_phase, now_ms())
                grid_index += 1
            else:
                planned += 1100
                while planned < now_ms():
                    planned += 1100
                    previous_date = None
    except Deadline:
        pass
    except OSError as exc:
        errors.append(str(exc))
    finally:
        signal.setitimer(signal.ITIMER_REAL, 0)
        signal.signal(signal.SIGALRM, handler)
        conn.close()

    valid = bool(all_rtts) and lower < upper
    return dict(phase_ms=phase, status=status if all_rtts else "no_data", attempts=attempts,
                coarse_n=len(coarse_rtts), fine_n=len(all_rtts)-len(coarse_rtts),
                coarse_half_rtt_ms=round(statistics.mean(coarse_rtts)/2, 3) if coarse_rtts else "",
                fine_step_ms=step or "", mean_half_rtt_ms=round(statistics.mean(all_rtts)/2, 3) if all_rtts else "",
                lo_ms=round(lower, 3) if valid else "", hi_ms=round(upper, 3) if valid else "",
                last_received_ms=last_received, elapsed_s=round(time.monotonic()-start, 3),
                errors=" | ".join(dict.fromkeys(errors)))


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("url")
    parser.add_argument("--phases", type=int, nargs="+", default=[173], metavar="MS")
    parser.add_argument("--samples", help="write successful samples to a CSV file")
    args = parser.parse_args()
    if any(not 0 <= p < 1000 for p in args.phases):
        parser.error("phases must be in 0..999 ms")
    samples = []
    rows = []
    for number, phase in enumerate(args.phases, 1):
        trace = []
        rows.append(dict(round=number, **measure(args.url, phase, trace)))
        samples.extend(dict(round=number, **r) for r in trace)
    write_rows(rows)
    if args.samples:
        with open(args.samples, "w", newline="") as dest:
            fields = ("round", "phase_ms", "attempt", "stage", "sent_ms", "received_ms", "date_ms")
            writer = csv.DictWriter(dest, fieldnames=fields, lineterminator="\n")
            writer.writeheader()
            writer.writerows(samples)
    raise SystemExit(1 if any(r["status"] in ("no_data", "inconsistent") for r in rows) else 0)

#!/usr/bin/env python3
"""Null-control experiment: advance sim time with zero changes and record city health.

Tests whether the benchmark map collapses on its own (death spiral) without any
agent intervention. One sample per in-game day (585 ticks), default 60 days.
"""
import json
import sys
import time
import urllib.request

BASE = "http://127.0.0.1:8787"
DAY_TICKS = 585


def http(path, body=None, timeout=600):
    req = urllib.request.Request(
        BASE + path,
        data=json.dumps(body).encode() if body is not None else None,
        method="POST" if body is not None else "GET",
    )
    with urllib.request.urlopen(req, timeout=timeout) as resp:
        return json.loads(resp.read())


def congestion_summary(segment_loads):
    if not segment_loads:
        return {"segments": 0}
    densities = sorted((s["density"] for s in segment_loads), reverse=True)
    congested = [s for s in segment_loads if s["density"] >= 0.7]
    worst_decile = densities[: max(1, len(densities) // 10)]
    return {
        "segments": len(segment_loads),
        "congested_count": len(congested),
        "congested_meters": round(sum(s.get("length", 0.0) for s in congested), 1),
        "mean_density": round(sum(densities) / len(densities), 3),
        "worst_decile_mean": round(sum(worst_decile) / len(worst_decile), 3),
    }


FROZEN_KEYS = ("population", "flow_percent", "active_vehicles", "congestion")


def looks_frozen(prev_row, row, clock):
    if clock.get("forced_paused"):
        return True
    return prev_row is not None and all(prev_row.get(k) == row.get(k) for k in FROZEN_KEYS)


def sample(day, tick):
    m = http("/metrics")
    traffic = m.get("traffic", {})
    row = {
        "day": day,
        "tick": tick,
        "wall_time": time.strftime("%H:%M:%S"),
        "flow_percent": traffic.get("flow_percent"),
        "active_vehicles": traffic.get("active_vehicles"),
        "congestion": congestion_summary(traffic.get("segment_loads", [])),
        "population": m.get("population", {}),
        "economy": m.get("economy", {}),
        "services": m.get("services", {}),
    }
    return row


def main():
    days = int(sys.argv[1]) if len(sys.argv) > 1 else 60
    out_path = sys.argv[2] if len(sys.argv) > 2 else "null-control.jsonl"

    health = http("/health", timeout=10)
    if not health.get("city_loaded"):
        print("city not loaded, aborting", file=sys.stderr)
        sys.exit(1)

    http("/clock", {"op": "set-speed", "speed": 3}, timeout=30)

    with open(out_path, "w") as f:
        row = sample(0, health["tick"])
        f.write(json.dumps(row) + "\n")
        f.flush()
        print(f"day 0: pop={row['population']} flow={row['flow_percent']} veh={row['active_vehicles']}", flush=True)

        prev_row = row
        for day in range(1, days + 1):
            clock = http("/clock", {"op": "step", "ticks": DAY_TICKS})
            if not clock.get("ok"):
                print(f"day {day}: step failed: {clock}", file=sys.stderr, flush=True)
                sys.exit(1)
            row = sample(day, clock["tick"])
            if looks_frozen(prev_row, row, clock):
                print(f"WARNING: day {day} appears frozen (game dialog?)", file=sys.stderr, flush=True)
                row = {**row, "frozen": True}
            f.write(json.dumps(row) + "\n")
            f.flush()
            pop = row["population"].get("total") if isinstance(row["population"], dict) else row["population"]
            print(
                f"day {day}: pop={pop} flow={row['flow_percent']} veh={row['active_vehicles']} "
                f"congested_m={row['congestion'].get('congested_meters')}",
                flush=True,
            )
            prev_row = row


if __name__ == "__main__":
    main()

#!/usr/bin/env python3
"""Known-good fix probe: upgrade the worst congested corridor, then watch
congested_meters for N days. Validates that the scoring metric responds to a
genuinely good intervention (compare against the null-control trace).

Run from a freshly loaded save so days are comparable with the null control.
"""
import json
import sys
import time
import urllib.request

BASE = "http://127.0.0.1:8787"
DAY_TICKS = 585
UPGRADE_TO = "Medium Road"
TOP_N = 10


def http(path, body=None, timeout=600):
    req = urllib.request.Request(
        BASE + path,
        data=json.dumps(body).encode() if body is not None else None,
        method="POST" if body is not None else "GET",
    )
    with urllib.request.urlopen(req, timeout=timeout) as resp:
        return json.loads(resp.read())


def congested_meters(segment_loads):
    return round(sum(s.get("length", 0.0) for s in segment_loads if s["density"] >= 0.7), 1)


FROZEN_KEYS = ("population", "flow_percent", "active_vehicles", "congested_meters")


def looks_frozen(prev_row, row, clock):
    if clock.get("forced_paused"):
        return True
    return prev_row is not None and all(prev_row.get(k) == row.get(k) for k in FROZEN_KEYS)


def sample(day):
    m = http("/metrics")
    traffic = m.get("traffic", {})
    return {
        "day": day,
        "tick": m.get("tick"),
        "wall_time": time.strftime("%H:%M:%S"),
        "flow_percent": traffic.get("flow_percent"),
        "active_vehicles": traffic.get("active_vehicles"),
        "congested_meters": congested_meters(traffic.get("segment_loads", [])),
        "population": m.get("population", {}).get("total"),
        "abandoned_buildings": m.get("services", {}).get("abandoned_buildings"),
    }


def worst_basic_road_segments(n):
    m = http("/metrics")
    loads = {s["segment_id"]: s for s in m["traffic"]["segment_loads"]}
    net = http("/network")
    rows = [
        (loads[s["id"]]["density"], s)
        for s in net["segments"]
        if s["id"] in loads and s["prefab"] == "Basic Road"
    ]
    rows.sort(key=lambda r: r[0], reverse=True)
    return [s for _, s in rows[:n]]


def main():
    days = int(sys.argv[1]) if len(sys.argv) > 1 else 10
    out_path = sys.argv[2] if len(sys.argv) > 2 else "fix-probe.jsonl"

    health = http("/health", timeout=10)
    if not health.get("city_loaded"):
        print("city not loaded, aborting", file=sys.stderr)
        sys.exit(1)

    http("/clock", {"op": "set-speed", "speed": 3}, timeout=30)

    targets = worst_basic_road_segments(TOP_N)
    upgrades = []
    for seg in targets:
        res = http("/action/upgrade-road", {"segment_id": seg["id"], "prefab": UPGRADE_TO}, timeout=30)
        upgrades.append({"segment": seg["id"], "ok": res.get("ok"),
                        "fronting": res.get("zoned_buildings_fronting"),
                        "reason": res.get("reason")})
        print(f"upgrade {seg['id']} -> {UPGRADE_TO}: {res.get('ok')} fronting={res.get('zoned_buildings_fronting')}", flush=True)

    with open(out_path, "w") as f:
        f.write(json.dumps({"upgrades": upgrades}) + "\n")
        row = sample(0)
        f.write(json.dumps(row) + "\n")
        f.flush()
        print(f"day 0 (post-fix): congested_m={row['congested_meters']} pop={row['population']}", flush=True)

        prev_row = row
        for day in range(1, days + 1):
            clock = http("/clock", {"op": "step", "ticks": DAY_TICKS})
            if not clock.get("ok"):
                print(f"day {day}: step failed: {clock}", file=sys.stderr, flush=True)
                sys.exit(1)
            row = sample(day)
            if looks_frozen(prev_row, row, clock):
                print(f"WARNING: day {day} appears frozen (game dialog?)", file=sys.stderr, flush=True)
                row = {**row, "frozen": True}
            f.write(json.dumps(row) + "\n")
            f.flush()
            print(
                f"day {day}: congested_m={row['congested_meters']} flow={row['flow_percent']} "
                f"veh={row['active_vehicles']} pop={row['population']} abandoned={row['abandoned_buildings']}",
                flush=True,
            )
            prev_row = row


if __name__ == "__main__":
    main()

#!/usr/bin/env python3
"""
Generate a profiling report from a Hirsel profiling session.

Usage:
    scripts/profiling-report.py ~/.hirsel/profiling/2026-02-06T00-10-00
    scripts/profiling-report.py  # uses most recent session
"""

import json
import sys
from pathlib import Path
from collections import defaultdict

PROFILING_ROOT = Path(__file__).resolve().parent.parent / ".profiling"


def find_session_dir(arg: str | None) -> Path:
    if arg:
        p = Path(arg)
        if p.is_dir():
            return p
        print(f"Error: '{arg}' is not a directory", file=sys.stderr)
        sys.exit(1)

    if not PROFILING_ROOT.is_dir():
        print(f"Error: No profiling data found at {PROFILING_ROOT}", file=sys.stderr)
        sys.exit(1)

    sessions = sorted(
        [d for d in PROFILING_ROOT.iterdir() if d.is_dir()],
        key=lambda d: d.name,
    )
    if not sessions:
        print("Error: No profiling sessions found", file=sys.stderr)
        sys.exit(1)

    return sessions[-1]


def load_json(path: Path) -> dict | list | None:
    if not path.exists():
        return None
    with open(path) as f:
        content = f.read()
    try:
        return json.loads(content)
    except json.JSONDecodeError:
        # Chrome trace files can have malformed events from spans containing
        # raw JSON (e.g. serialized IPC payloads). Parse line-by-line instead.
        events = []
        for line in content.splitlines():
            line = line.strip().rstrip(",")
            if not line or line in ("[]", "[", "]"):
                continue
            try:
                events.append(json.loads(line))
            except json.JSONDecodeError:
                continue
        return events if events else None


def format_ms(ms: float) -> str:
    if ms < 1:
        return f"{ms * 1000:.0f}us"
    if ms < 1000:
        return f"{ms:.1f}ms"
    return f"{ms / 1000:.2f}s"


def format_duration(ms: float) -> str:
    secs = ms / 1000
    if secs < 60:
        return f"{secs:.0f}s"
    mins = int(secs // 60)
    secs = secs % 60
    return f"{mins}m {secs:.0f}s"


def report_backend(trace_data: list | dict) -> None:
    # Chrome trace format: either {"traceEvents": [...]} or [...]
    if isinstance(trace_data, dict):
        events = trace_data.get("traceEvents", [])
    else:
        events = trace_data

    # Complete events (ph=X) have dur; Begin/End pairs (ph=B/E) need matching
    spans = defaultdict(list)  # name -> [duration_us, ...]
    total_events = len(events)

    # Track open B spans to match with E spans (stack per thread+name for nested spans)
    open_spans = defaultdict(list)  # (name, tid) -> [timestamp_us, ...]

    for ev in events:
        ph = ev.get("ph")
        name = ev.get("name", "unknown")
        tid = ev.get("tid", 0)
        ts = ev.get("ts", 0)

        if ph == "X" and "dur" in ev:
            spans[name].append(ev["dur"])
        elif ph == "B":
            open_spans[(name, tid)].append(ts)
        elif ph == "E":
            key = (name, tid)
            if open_spans[key]:
                start_ts = open_spans[key].pop()
                dur = ts - start_ts
                if dur > 0:
                    spans[name].append(dur)

    if not spans:
        print("  No complete spans found in trace")
        return

    print(f"  Total trace events: {total_events}")
    print()

    # Sort by total time descending
    by_total = sorted(spans.items(), key=lambda kv: sum(kv[1]), reverse=True)

    print("  Top spans by total time:")
    print(f"  {'Span':<50} {'Calls':>6} {'Total':>10} {'Avg':>10} {'Max':>10}")
    print(f"  {'-'*50} {'-'*6} {'-'*10} {'-'*10} {'-'*10}")

    for name, durations in by_total[:25]:
        count = len(durations)
        total_us = sum(durations)
        avg_us = total_us / count
        max_us = max(durations)
        # Convert from microseconds to milliseconds
        print(
            f"  {name:<50} {count:>6} "
            f"{format_ms(total_us / 1000):>10} "
            f"{format_ms(avg_us / 1000):>10} "
            f"{format_ms(max_us / 1000):>10}"
        )


def report_frontend(data: dict) -> None:
    session = data.get("session", {})
    vitals = data.get("webVitals", {})
    ipc = data.get("ipc", {})

    duration = session.get("durationMs", 0)
    print(f"  Session duration: {format_duration(duration)}")
    print(f"  Total IPC calls:  {ipc.get('totalCalls', 0)}")
    print()

    # Web vitals
    lcp = vitals.get("lcp")
    cls = vitals.get("cls")
    if lcp is not None or cls is not None:
        print("  Web Vitals:")
        if lcp is not None:
            print(f"    LCP: {format_ms(lcp)}")
        if cls is not None:
            print(f"    CLS: {cls:.4f}")
        print()

    # IPC summary
    summary = ipc.get("summary", {})
    if not summary:
        print("  No IPC measurements recorded")
        return

    # Sort by total time descending
    by_total = sorted(summary.items(), key=lambda kv: kv[1]["totalMs"], reverse=True)

    print("  IPC commands by total time:")
    print(f"  {'Command':<40} {'Calls':>6} {'Total':>10} {'Avg':>10} {'Max':>10}")
    print(f"  {'-'*40} {'-'*6} {'-'*10} {'-'*10} {'-'*10}")

    for name, stats in by_total:
        print(
            f"  {name:<40} {stats['count']:>6} "
            f"{format_ms(stats['totalMs']):>10} "
            f"{format_ms(stats['avgMs']):>10} "
            f"{format_ms(stats['maxMs']):>10}"
        )

    # Error count
    measurements = ipc.get("measurements", [])
    errors = sum(1 for m in measurements if m.get("error"))
    if errors:
        print(f"\n  IPC errors: {errors}/{len(measurements)}")


def report_memory(data: dict) -> None:
    """Report memory usage over time."""
    snapshots = data.get("memory", [])
    if not snapshots:
        print("  No memory data collected")
        return

    # Backend RSS
    rss_values = [s["backend"]["rssMB"] for s in snapshots if s["backend"].get("rssMB")]
    if rss_values:
        print("  Backend (RSS):")
        print(f"    Start:  {rss_values[0]:.1f} MB")
        print(f"    End:    {rss_values[-1]:.1f} MB")
        print(f"    Peak:   {max(rss_values):.1f} MB")
        delta = rss_values[-1] - rss_values[0]
        sign = "+" if delta >= 0 else ""
        print(f"    Delta:  {sign}{delta:.1f} MB")
        print()

    # Frontend JS heap
    heap_values = [s["frontend"]["jsHeapUsedMB"] for s in snapshots if s["frontend"].get("jsHeapUsedMB")]
    if heap_values:
        print("  Frontend (JS Heap):")
        print(f"    Start:  {heap_values[0]:.1f} MB")
        print(f"    End:    {heap_values[-1]:.1f} MB")
        print(f"    Peak:   {max(heap_values):.1f} MB")
        delta = heap_values[-1] - heap_values[0]
        sign = "+" if delta >= 0 else ""
        print(f"    Delta:  {sign}{delta:.1f} MB")
        print()

    if not rss_values and not heap_values:
        print("  No memory data available")
        return

    # Timeline (sampled every ~5s, show key points)
    print(f"  Timeline ({len(snapshots)} samples):")
    print(f"  {'Elapsed':>10} {'Backend RSS':>12} {'JS Heap':>12}")
    print(f"  {'-'*10} {'-'*12} {'-'*12}")

    # Show ~10 evenly spaced samples
    step = max(1, len(snapshots) // 10)
    indices = list(range(0, len(snapshots), step))
    if indices[-1] != len(snapshots) - 1:
        indices.append(len(snapshots) - 1)

    for i in indices:
        s = snapshots[i]
        elapsed = format_duration(s["elapsedMs"])
        rss = f"{s['backend']['rssMB']:.1f} MB" if s["backend"].get("rssMB") else "-"
        heap = f"{s['frontend']['jsHeapUsedMB']:.1f} MB" if s["frontend"].get("jsHeapUsedMB") else "-"
        print(f"  {elapsed:>10} {rss:>12} {heap:>12}")


def report_cross_correlation(trace_data: list | dict, frontend_data: dict) -> None:
    """Compare backend span times with frontend IPC round-trip times."""
    if isinstance(trace_data, dict):
        events = trace_data.get("traceEvents", [])
    else:
        events = trace_data

    # Collect backend command durations (X events and B/E pairs)
    backend_spans = defaultdict(list)
    open_spans = defaultdict(list)  # (name, tid) -> [timestamp_us, ...]

    for ev in events:
        ph = ev.get("ph")
        name = ev.get("name", "unknown")
        tid = ev.get("tid", 0)
        ts = ev.get("ts", 0)

        if ph == "X" and "dur" in ev:
            backend_spans[name].append(ev["dur"] / 1000)  # us -> ms
        elif ph == "B":
            open_spans[(name, tid)].append(ts)
        elif ph == "E":
            key = (name, tid)
            if open_spans[key]:
                start_ts = open_spans[key].pop()
                dur = ts - start_ts
                if dur > 0:
                    backend_spans[name].append(dur / 1000)  # us -> ms

    frontend_summary = frontend_data.get("ipc", {}).get("summary", {})

    # Find commands present in both
    common = set(backend_spans.keys()) & set(frontend_summary.keys())
    if not common:
        return

    print("  Backend vs Frontend round-trip (IPC overhead):")
    print(f"  {'Command':<40} {'BE avg':>10} {'FE avg':>10} {'Overhead':>10}")
    print(f"  {'-'*40} {'-'*10} {'-'*10} {'-'*10}")

    # Sort by frontend total time descending
    by_fe_total = sorted(common, key=lambda c: frontend_summary[c]["totalMs"], reverse=True)

    for cmd in by_fe_total:
        be_avg = sum(backend_spans[cmd]) / len(backend_spans[cmd])
        fe_avg = frontend_summary[cmd]["avgMs"]
        overhead = fe_avg - be_avg
        print(
            f"  {cmd:<40} "
            f"{format_ms(be_avg):>10} "
            f"{format_ms(fe_avg):>10} "
            f"{format_ms(overhead):>10}"
        )


def main() -> None:
    arg = sys.argv[1] if len(sys.argv) > 1 else None
    session_dir = find_session_dir(arg)

    print(f"Profiling Report: {session_dir}")
    print("=" * 80)

    trace_path = session_dir / "trace.json"
    daemon_trace_path = session_dir / "daemon-trace.json"
    frontend_path = session_dir / "frontend.json"

    trace_data = load_json(trace_path)
    daemon_trace_data = load_json(daemon_trace_path)
    frontend_data = load_json(frontend_path)

    if trace_data is None and daemon_trace_data is None and frontend_data is None:
        print("No profiling data found in session directory")
        sys.exit(1)

    if trace_data is not None:
        print()
        print("Backend (tracing-chrome)")
        print("-" * 80)
        report_backend(trace_data)

    if daemon_trace_data is not None:
        print()
        print("Daemon (tracing-chrome)")
        print("-" * 80)
        report_backend(daemon_trace_data)

    if frontend_data is not None:
        print()
        print("Frontend (IPC + Web Vitals)")
        print("-" * 80)
        report_frontend(frontend_data)

    if frontend_data is not None and frontend_data.get("memory"):
        print()
        print("Memory")
        print("-" * 80)
        report_memory(frontend_data)

    if trace_data is not None and frontend_data is not None:
        print()
        print("Cross-Correlation")
        print("-" * 80)
        report_cross_correlation(trace_data, frontend_data)

    print()


if __name__ == "__main__":
    main()

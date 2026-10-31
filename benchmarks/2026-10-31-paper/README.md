# Horizon Server Plugins against Paper 26.3 (2026-10-31)

The same bots on this server (Horizon with the Homes and Dragao plugins loaded) and on Paper 26.3, measured
with Horizon's own server benchmark (`tools/server_bench.py`, the tool behind Horizon's
`benchmarks/server-four-way-2026-10-19`).

## Summary

- **The work was equal**: chunks sent a second and first-time chunks a second were within 2 % on both
  servers at every count. Every bot joined, none was dropped, and both held 20 TPS throughout.
- **Memory**: 0.74 to 1.09 GB against Paper's 3.42 to 3.46 GB (Paper with a fixed 3 GB heap, `-Xms3G
  -Xmx3G`, so most of its number is the heap it was given).
- **Start**: 2.1 to 2.5 s to `Done`, against Paper's 7.0 to 7.7 s.
- **Average tick (MSPT), where Paper wins**: Paper's was 8 to 15 % shorter at every count (14.71 ms against
  16.23 ms with 25 bots). Both are far below the 50 ms limit.
- **Lag spikes, where this server wins**: the worst 5-second P99 was 28 to 55 % lower (67.7 ms against
  94.0 ms with 25 bots), and the P99 was lower with 25 bots (28.4 against 30.7).
- **CPU**: about the same with 1 and 10 bots; 18 % less with 25 (57 % against 69 %).

## Results

MSPT is milliseconds per tick from each server's own `/tick query`. P95 and P99 are means over the 5-second
queries of the 300 measured seconds; "worst P99" is the highest single query. RSS and CPU cover the server's
process tree; 100 % CPU is one of the machine's 12 threads.

| Bots | Server | MSPT | P95 | P99 | Worst P99 | TPS | RSS (mean) | CPU | Start | Chunks/s |
|---:|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| 1 | Horizon + plugins | 4.91 | 7.65 | 11.06 | **25.5** | 20 | **0.74 GB** | 17 % | **2.3 s** | 9.78 |
| 1 | Paper | **4.53** | 6.81 | 10.45 | 56.6 | 20 | 3.42 GB | 15 % | 7.7 s | 9.60 |
| 10 | Horizon + plugins | 11.68 | 15.75 | 21.40 | **57.1** | 20 | **1.03 GB** | 38 % | **2.5 s** | 62.87 |
| 10 | Paper | **10.14** | 14.33 | 21.03 | 77.8 | 20 | 3.46 GB | 41 % | 7.7 s | 62.07 |
| 25 | Horizon + plugins | 16.23 | 21.32 | **28.42** | **67.7** | 20 | **1.09 GB** | **57 %** | **2.1 s** | 158.40 |
| 25 | Paper | **14.71** | 20.38 | 30.72 | 94.0 | 20 | 3.46 GB | 69 % | 7.0 s | 156.00 |

`bench.json` is the full report; `load.txt` samples the machine's load and free memory once a minute.

## Method

- Both servers: seed 12345, view and simulation distance 10, offline, creative, difficulty easy,
  `sync-chunk-writes=true`. A fresh world for every run.
- Bots: `horizon-bots --behaviour mixed` (60 % walk, 20 % explore at 0.8 blocks a second, 20 % idle), joining
  one a second, then 120 s of warm-up and 300 s measured, with `/tick query` every 5 s.
- This server: `build.sh` at 531fb6d (Horizon 396ff1e5 with `engine/horizon.patch`), the Homes and Dragao
  plugins in `plugins/`.
- Paper: 26.3 build 235 from PaperMC's download API, on OpenJDK 25.0.4.1 (Temurin), `-Xms3G -Xmx3G`, Paper's
  defaults except `connection-throttle: -1` (every bot connects from one address). No plugins.
- Machine: a 12-thread x86_64 desktop, 14 GB, Arch Linux. The bots ran on the same machine. The desktop was
  otherwise idle (load average around 1, `load.txt`).

## Limits

- One run per configuration, on one machine. Small differences (a few percent) are within run-to-run noise.
- Up to 25 bots: with about 5 GB free, 50 bots next to Paper's heap would have measured the machine's memory,
  not the servers.
- Paper's memory depends on the heap it is given; a smaller heap would use less memory, at some cost to its
  garbage collector.

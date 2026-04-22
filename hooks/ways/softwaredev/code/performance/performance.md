---
description: performance optimization, profiling, benchmarking, latency
vocabulary: optimize profile benchmark latency throughput memory cache bottleneck flamegraph allocation heap speed slow
pattern: slow|optimi|latency|profile|performance|speed.?up|benchmark|bottleneck|throughput|memory.?leak
scope: agent, subagent
refire: 0.2
---
<!-- epistemic: heuristic -->
# Performance Way

## What Claude Should Do

When asked about performance, start with static analysis — don't just say "profile it":

1. **Scan for algorithmic issues**: nested loops over collections (O(n^2)), repeated database/API calls inside loops (N+1), string concatenation in tight loops
2. **Identify the pattern, show the fix**:
   - N+1 queries → batched query or join (show the rewrite)
   - Repeated computation → memoize or cache (show before/after)
   - Unnecessary allocation → pre-allocate or reuse (show the change)
3. **Suggest profiling tools** for the detected language:
   - Python: `cProfile`, `py-spy`, `memory_profiler`
   - Node.js: `--prof`, `clinic.js`, `0x`
   - Rust: `cargo flamegraph`, `criterion` for benchmarks
   - Go: `pprof`, `benchstat`

## Generate Measurements

When the user wants to benchmark, produce before/after measurement code — don't just suggest "measure it".

## Avoid

- Suggesting "just profile it" without analyzing the code first
- Micro-optimizations that hurt readability
- Caching without specifying an invalidation strategy

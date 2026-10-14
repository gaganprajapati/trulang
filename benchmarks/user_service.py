# Python 3 implementation of UserService benchmark
import time
import os
import resource

class UserService:
    def __init__(self):
        self.cache = {}

    def handle(self, request):
        try:
            user = {
                "id": 42,
                "name": "Alice Smith",
                "email": "alice@example.com"
            }
            if user is not None:
                self.cache[request["user_id"]] = user
            return {"status": 200, "data": user}
        except Exception as err:
            return {"status": 500, "error": str(err)}
        finally:
            pass

def get_rss_kb():
    # macOS ru_maxrss is in bytes; Linux in KB
    usage = resource.getrusage(resource.RUSAGE_SELF).ru_maxrss
    if os.uname().sysname == "Darwin":
        return usage // 1024
    return usage

def run_benchmark(total_requests=50000):
    service = UserService()
    initial_rss = get_rss_kb()

    latencies = []
    start_time = time.perf_counter()

    for i in range(total_requests):
        req_start = time.perf_counter()
        req = {"id": i, "user_id": f"user_{i}"}
        service.handle(req)
        latencies.append((time.perf_counter() - req_start) * 1_000_000) # µs

    elapsed = time.perf_counter() - start_time
    final_rss = get_rss_kb()
    throughput = total_requests / elapsed

    latencies.sort()
    n = len(latencies)
    p50 = latencies[int(n * 0.50)]
    p90 = latencies[int(n * 0.90)]
    p95 = latencies[int(n * 0.95)]
    p99 = latencies[int(n * 0.99)]
    min_lat = latencies[0]
    max_lat = latencies[-1]

    print("=== Python 3.10 Benchmark Results ===")
    print(f"Throughput:          {throughput:.2f} req/sec")
    print(f"Total Elapsed Time:  {elapsed:.3f} s")
    print(f"Total Requests:      {total_requests}")
    print(f"Latencies (µs):      min: {min_lat:.1f}µs | p50: {p50:.1f}µs | p90: {p90:.1f}µs | p95: {p95:.1f}µs | p99: {p99:.1f}µs | max: {max_lat:.1f}µs")
    print(f"Initial RSS:         {initial_rss} KB")
    print(f"Final RSS:           {final_rss} KB (Drift: +{final_rss - initial_rss} KB)")

if __name__ == "__main__":
    run_benchmark(50000)

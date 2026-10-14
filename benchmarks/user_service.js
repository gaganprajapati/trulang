// Node.js implementation of UserService benchmark
const { performance } = require('perf_hooks');

class UserService {
    constructor() {
        this.cache = new Map();
    }

    handle(request) {
        try {
            // Simulated user fetch
            const user = {
                id: 42,
                name: "Alice Smith",
                email: "alice@example.com"
            };
            if (user !== null) {
                this.cache.set(request.userId, user);
            }
            return { status: 200, data: user };
        } catch (err) {
            return { status: 500, error: err.message };
        } finally {
            // defer log_request_completed
        }
    }
}

async function runBenchmark(totalRequests = 50000, concurrency = 8) {
    const service = new UserService();
    const initialRss = process.memoryUsage().rss;

    const latencies = [];
    const startTime = performance.now();

    const worker = async (count) => {
        const localLats = [];
        for (let i = 0; i < count; i++) {
            const reqStart = performance.now();
            const req = { id: i, userId: `user_${i}` };
            service.handle(req);
            localLats.push((performance.now() - reqStart) * 1000); // µs
        }
        return localLats;
    };

    const reqsPerWorker = Math.floor(totalRequests / concurrency);
    const tasks = Array.from({ length: concurrency }, () => worker(reqsPerWorker));
    const results = await Promise.all(tasks);

    for (const res of results) {
        latencies.push(...res);
    }

    const elapsedMs = performance.now() - startTime;
    const finalRss = process.memoryUsage().rss;
    const throughput = (totalRequests / (elapsedMs / 1000)).toFixed(2);

    latencies.sort((a, b) => a - b);
    const n = latencies.length;
    const p50 = latencies[Math.floor(n * 0.50)].toFixed(1);
    const p90 = latencies[Math.floor(n * 0.90)].toFixed(1);
    const p95 = latencies[Math.floor(n * 0.95)].toFixed(1);
    const p99 = latencies[Math.floor(n * 0.99)].toFixed(1);
    const min = latencies[0].toFixed(1);
    const max = latencies[n - 1].toFixed(1);

    console.log("=== Node.js (v" + process.versions.node + " V8) Benchmark Results ===");
    console.log(`Throughput:          ${throughput} req/sec`);
    console.log(`Total Elapsed Time:  ${(elapsedMs / 1000).toFixed(3)} s`);
    console.log(`Total Requests:      ${totalRequests}`);
    console.log(`Latencies (µs):      min: ${min}µs | p50: ${p50}µs | p90: ${p90}µs | p95: ${p95}µs | p99: ${p99}µs | max: ${max}µs`);
    console.log(`Initial RSS:         ${(initialRss / 1024).toFixed(0)} KB`);
    console.log(`Final RSS:           ${(finalRss / 1024).toFixed(0)} KB (Drift: +${((finalRss - initialRss) / 1024).toFixed(0)} KB)`);
    console.log(`Heap Used:           ${(process.memoryUsage().heapUsed / 1024).toFixed(0)} KB`);
}

runBenchmark(50000, 8);

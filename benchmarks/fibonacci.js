// Fibonacci benchmark in Node.js
const { performance } = require('perf_hooks');

function fib(n) {
    let a = 0;
    let b = 1;
    let i = 0;
    while (i < n) {
        let temp = a + b;
        a = b;
        b = temp;
        i = i + 1;
    }
    return a;
}

function run(total = 100000) {
    const initialRss = process.memoryUsage().rss;
    const start = performance.now();
    for (let i = 0; i < total; i++) {
        fib(10);
    }
    const elapsed = performance.now() - start;
    const finalRss = process.memoryUsage().rss;
    console.log("=== Node.js Fibonacci (100k iterations) ===");
    console.log(`Throughput: ${(total / (elapsed / 1000)).toFixed(2)} req/sec`);
    console.log(`Elapsed:    ${(elapsed / 1000).toFixed(4)} s`);
    console.log(`RSS:        ${(initialRss/1024).toFixed(0)} KB -> ${(finalRss/1024).toFixed(0)} KB`);
}

run(100000);

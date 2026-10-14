# Python Fibonacci benchmark
import time
import os
import resource

def fib(n):
    a = 0
    b = 1
    i = 0
    while i < n:
        temp = a + b
        a = b
        b = temp
        i = i + 1
    return a

def get_rss_kb():
    usage = resource.getrusage(resource.RUSAGE_SELF).ru_maxrss
    if os.uname().sysname == "Darwin":
        return usage // 1024
    return usage

def run(total=100000):
    initial_rss = get_rss_kb()
    start = time.perf_counter()
    for _ in range(total):
        fib(10)
    elapsed = time.perf_counter() - start
    final_rss = get_rss_kb()
    print("=== Python Fibonacci (100k iterations) ===")
    print(f"Throughput: {total / elapsed:.2f} req/sec")
    print(f"Elapsed:    {elapsed:.4f} s")
    print(f"RSS:        {initial_rss} KB -> {final_rss} KB")

if __name__ == "__main__":
    run(100000)

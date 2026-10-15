// Interactive TruLang In-Browser Playground Engine

const samples = {
  user_service: `service UserService {
    memory_budget request = 16 MB;
    memory_budget persistent = 512 MB;

    let mut cache: Map<string, User> = Map::new();

    fn handle(request: Request) -> Response {
        defer log_request_completed(request.id);

        try {
            let user = database.find_user(request.user_id);
            if user != null {
                // Promote user data to persistent service cache (Region B)
                cache.insert(request.user_id, persist(user));
            }
            return Response.json(user);
        } catch (err: DatabaseError) {
            return Response.error(500, err.message);
        }
    }
}`,
  while_loop: `fn main() -> i64 {
    let mut sum: i64 = 0;
    let mut i: i64 = 1;
    let limit: i64 = 10;

    println("Iterating while loop from 1 to 10:");

    while i <= limit {
        sum = sum + i;
        println(i);
        i = i + 1;
    }

    println("Sum calculated successfully:");
    return sum;
}`,
  arrays: `fn main() {
    let numbers = [10, 20, 30, 40, 50];
    let names = ["Alice", "Bob", "Charlie"];

    println("Array contents:");
    println(numbers);
    println(names);
}`,
  variables: `fn main() {
    // Immutable by default:
    let app_name = "TruLang Service";
    let port: i64 = 8080;

    // Mutable variables with 'let mut':
    let mut requests_processed = 0;
    requests_processed = requests_processed + 1;

    println(app_name);
    println(requests_processed);
}`,
  nullable: `fn main() {
    let user_id: i64? = 42;
    let empty_user: string? = null;

    if user_id != null {
        println("User ID exists and is unwrapped safely:");
        println(user_id);
    }

    if empty_user == null {
        println("empty_user is null as expected");
    }
}`,
  try_catch: `fn main() {
    defer println("Deferred cleanup ran in LIFO order upon exit");

    try {
        println("Attempting operation inside protected try block...");
        let x = 100 / 10;
        println("Success!");
    } catch (err: Exception) {
        println("Caught exception:");
        println(err);
    }
}`,
  budget_overflow: `service TestService {
    memory_budget request = 64 B;
    memory_budget persistent = 1024 B;

    fn handle() {
        // This large string exceeds the 64 byte limit of Region A
        let a = "This string exceeds the 64 byte request memory budget allocation threshold easily";
    }
}`
};

function loadSample(key) {
  const editor = document.getElementById('codeEditor');
  if (samples[key] && editor) {
    editor.value = samples[key];
  }
}

function runSimulatedCode() {
  const editor = document.getElementById('codeEditor');
  const terminal = document.getElementById('terminal');
  const status = document.getElementById('execStatus');
  if (!editor || !terminal || !status) return;

  const code = editor.value;

  status.innerText = '● EXECUTING...';
  status.style.color = '#f59e0b';
  terminal.innerText = 'Compiling AST to Bytecode...\nExecuting on TruLang VM...';

  setTimeout(() => {
    if (code.includes('exceeds the 64 byte')) {
      terminal.innerText = `[Bytecode VM Error]
Memory limit exceeded: Region A (Execution) memory budget exceeded: attempted 81 bytes, limit is 64 bytes
Supervisor post-mortem telemetry retained at: "/tmp/trulang_shm_postmortem.dat"

Status: Execution Aborted safely (worker isolated, Region A arena reset)`;
      status.innerText = '🔴 BUDGET OVERFLOW CAUGHT';
      status.style.color = '#ef4444';
      updateGauges(64, 64, 0, 1024, true);
    } else if (code.includes('while i <= limit') || code.includes('Iterating while loop')) {
      terminal.innerText = `Iterating while loop from 1 to 10:
1
2
3
4
5
6
7
8
9
10
Sum calculated successfully:

Program exited with value: 55
--- Memory Diagnostics ---
Region A (Execution):  128 bytes used (budget: 16777216 bytes)
Region B (Persistent): 0 bytes retained (0 live objects)
Status: Completed OK (Arena reclaimed in O(1))`;
      status.innerText = '🟢 SUCCESS';
      status.style.color = '#10b981';
      updateGauges(128, 16777216, 0, 536870912, false);
    } else if (code.includes('Array contents')) {
      terminal.innerText = `Array contents:
[10, 20, 30, 40, 50]
[Alice, Bob, Charlie]

Program exited with value: null
--- Memory Diagnostics ---
Region A (Execution):  184 bytes used (budget: 16777216 bytes)
Region B (Persistent): 0 bytes retained (0 live objects)`;
      status.innerText = '🟢 SUCCESS';
      status.style.color = '#10b981';
      updateGauges(184, 16777216, 0, 536870912, false);
    } else if (code.includes('requests_processed')) {
      terminal.innerText = `TruLang Service
1

Program exited with value: null
--- Memory Diagnostics ---
Region A (Execution):  72 bytes used (budget: 16777216 bytes)
Region B (Persistent): 0 bytes retained (0 live objects)`;
      status.innerText = '🟢 SUCCESS';
      status.style.color = '#10b981';
      updateGauges(72, 16777216, 0, 536870912, false);
    } else if (code.includes('user_id != null')) {
      terminal.innerText = `User ID exists and is unwrapped safely:
42
empty_user is null as expected

Program exited with value: null
--- Memory Diagnostics ---
Region A (Execution):  64 bytes used (budget: 16777216 bytes)
Region B (Persistent): 0 bytes retained (0 live objects)`;
      status.innerText = '🟢 SUCCESS';
      status.style.color = '#10b981';
      updateGauges(64, 16777216, 0, 536870912, false);
    } else if (code.includes('Deferred cleanup ran')) {
      terminal.innerText = `Attempting operation inside protected try block...
Success!
Deferred cleanup ran in LIFO order upon exit

Program exited with value: null
--- Memory Diagnostics ---
Region A (Execution):  96 bytes used (budget: 16777216 bytes)
Region B (Persistent): 0 bytes retained (0 live objects)`;
      status.innerText = '🟢 SUCCESS';
      status.style.color = '#10b981';
      updateGauges(96, 16777216, 0, 536870912, false);
    } else {
      // Default UserService
      terminal.innerText = `[Dispatching Request to UserService Actor]
Executing handle(request: Request)...
[Deferred] log_request_completed(req_1001) executed
Program exited with value: Object{status: 200, data: Object{id: 42, name: Alice Smith, email: alice@example.com}}
--- Memory Diagnostics ---
Region A (Execution):  16 bytes used (budget: 16777216 bytes)
Region B (Persistent): 16 bytes retained (1 live objects)
Supervisor telemetry segment active at: "/tmp/trulang_shm_active.dat"`;
      status.innerText = '🟢 SUCCESS';
      status.style.color = '#10b981';
      updateGauges(16, 16777216, 16, 536870912, false);
    }
  }, 300);
}

function updateGauges(usedA, budgetA, usedB, budgetB, isOverflow) {
  const pctA = Math.min(100, Math.max(isOverflow ? 100 : 0.5, (usedA / budgetA) * 100));
  const pctB = Math.min(100, Math.max(0.2, (usedB / budgetB) * 100));

  const fillA = document.getElementById('gaugeAFill');
  const fillB = document.getElementById('gaugeBFill');
  const labelA = document.getElementById('gaugeALabel');
  const labelB = document.getElementById('gaugeBLabel');

  if (fillA) {
    fillA.style.width = pctA + '%';
    fillA.style.background = isOverflow ? '#ef4444' : 'linear-gradient(90deg, #06b6d4, #3b82f6)';
  }
  if (labelA) {
    labelA.innerText = `${usedA} B / ${budgetA} B (${(usedA / budgetA * 100).toFixed(1)}%)`;
  }

  if (fillB) {
    fillB.style.width = pctB + '%';
  }
  if (labelB) {
    labelB.innerText = `${usedB} B / ${budgetB} B (${(usedB / budgetB * 100).toFixed(2)}%)`;
  }
}

// Initialize default sample on load
window.addEventListener('DOMContentLoaded', () => {
  loadSample('user_service');
});

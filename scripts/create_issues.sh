#!/usr/bin/env bash
# Automated GitHub Issue Creator for TruLang
# Usage:
#   GITHUB_TOKEN="ghp_your_token_here" bash scripts/create_issues.sh
set -euo pipefail

REPO="gaganprajapati/trulang"
API_URL="https://api.github.com/repos/$REPO/issues"

if [ -z "${GITHUB_TOKEN:-}" ]; then
  echo "❌ Error: GITHUB_TOKEN environment variable is not set."
  echo "Usage:"
  echo "  export GITHUB_TOKEN=\"your_personal_access_token\""
  echo "  bash scripts/create_issues.sh"
  exit 1
fi

create_issue() {
  local title="$1"
  local labels="$2"
  local body="$3"

  echo ">>> Creating Issue: $title..."
  
  # Format labels as JSON array
  local labels_json
  labels_json=$(python3 -c "import json, sys; print(json.dumps([l.strip() for l in sys.argv[1].split(',')]))" "$labels")

  local payload
  payload=$(python3 -c "import json, sys; print(json.dumps({'title': sys.argv[1], 'body': sys.argv[2], 'labels': json.loads(sys.argv[3])}))" "$title" "$body" "$labels_json")

  local response
  response=$(curl -s -w "\n%{http_code}" -X POST "$API_URL" \
    -H "Authorization: Bearer $GITHUB_TOKEN" \
    -H "Accept: application/vnd.github+json" \
    -H "X-GitHub-Api-Version: 2022-11-28" \
    -d "$payload")

  local status_code
  status_code=$(echo "$response" | tail -n1)
  local response_body
  response_body=$(echo "$response" | sed '$d')

  if [ "$status_code" -eq 201 ]; then
    local issue_url
    issue_url=$(python3 -c "import json, sys; print(json.loads(sys.argv[1]).get('html_url', ''))" "$response_body")
    echo "  ✅ Created: $issue_url"
  else
    echo "  ❌ Failed (HTTP $status_code): $response_body"
  fi
  sleep 1
}

# --- Issue 1 ---
create_issue \
  "Add standard String methods (.len(), .trim(), .contains(), .to_upper(), .to_lower())" \
  "good first issue,enhancement,stdlib" \
  "### Description
Currently, calling methods on \`Value::String\` in the TruLang VM only resolves module/namespace calls. We need standard string manipulation methods directly callable on strings in \`.tr\` code.

### Proposed Methods:
- \`s.len() -> int\`: Returns character / byte length
- \`s.trim() -> string\`: Trims leading and trailing whitespace
- \`s.to_uppercase() -> string\`: Converts to uppercase
- \`s.to_lowercase() -> string\`: Converts to lowercase
- \`s.contains(sub: string) -> bool\`: Checks if substring exists
- \`s.starts_with(prefix: string) -> bool\`
- \`s.ends_with(suffix: string) -> bool\`

### Files to Modify:
- \`crates/vm/src/vm.rs\` (under \`Opcode::CallMethod\` inside \`match target { Value::String(...) => ... }\`)
- Add unit tests in \`crates/vm/src/lib.rs\`"

# --- Issue 2 ---
create_issue \
  "Add standard Array methods (.len(), .push(), .pop(), .contains(), .join())" \
  "good first issue,enhancement,stdlib" \
  "### Description
Calling methods on arrays currently returns \`null\` because \`Opcode::CallMethod\` does not match on \`Value::Array\`. We want to introduce fundamental array manipulation methods.

### Proposed Methods:
- \`arr.len() -> int\`: Number of items
- \`arr.push(item) -> null\`: Appends item to the array
- \`arr.pop() -> any\`: Removes and returns last item (or null)
- \`arr.contains(item) -> bool\`: Checks for value equality
- \`arr.join(separator: string) -> string\`: Joins array elements into a string

### Files to Modify:
- \`crates/vm/src/vm.rs\` (add \`Value::Array(ref arr) => match method.as_str() { ... }\` in \`Opcode::CallMethod\`)
- Add unit tests in \`crates/vm/src/lib.rs\`"

# --- Issue 3 ---
create_issue \
  "Add Map utility methods (.len(), .keys(), .values(), .remove(), .clear())" \
  "good first issue,enhancement,stdlib" \
  "### Description
Expand the built-in \`Map\` data structure methods in the VM. Currently, only \`.insert()\`, \`.get()\`, and \`.contains()\` are implemented.

### Proposed Methods:
- \`map.len() -> int\`
- \`map.keys() -> Array<string>\`
- \`map.values() -> Array<any>\`
- \`map.remove(key: string) -> any\`
- \`map.clear() -> null\`

### Files to Modify:
- \`crates/vm/src/vm.rs\` (under \`Value::Map(m) => match method.as_str() { ... }\`)
- Add unit tests in \`crates/vm/src/lib.rs\`"

# --- Issue 4 ---
create_issue \
  "Add 'trulang completions <shell>' CLI command for Bash, Zsh, and Fish" \
  "good first issue,tooling,cli" \
  "### Description
Modern developer CLIs (like \`rustup completions\`, \`gh completion\`) allow users to generate tab-completion scripts for their shell.

### Expected Behavior:
\`\`\`bash
trulang completions zsh > ~/.zsh/completion/_trulang
trulang completions bash > /etc/bash_completion.d/trulang
trulang completions fish > ~/.config/fish/completions/trulang.fish
\`\`\`
Should output the corresponding completion script using \`clap_complete\`.

### Files to Modify:
- \`crates/cli/Cargo.toml\` (add \`clap_complete = \"4.5\"\`)
- \`crates/cli/src/main.rs\` (add \`Commands::Completions { shell: Shell }\`)"

# --- Issue 5 ---
create_issue \
  "Add Vim & Neovim syntax highlighting plugin (editors/vim)" \
  "good first issue,tooling,editor" \
  "### Description
Provide a lightweight Vim / Neovim plugin in \`editors/vim/\` that automatically detects \`.tr\` files and applies syntax highlighting for TruLang grammar.

### Requirements:
- Detect \`*.tr\` as \`filetype=trulang\`
- Syntax highlighting for keywords: \`service\`, \`actor\`, \`persist\`, \`let mut\`, \`record\`, \`receive\`, \`memory_budget\`
- Built-in type highlighting: \`int\`, \`float\`, \`string\`, \`bool\`, \`Map\`, \`Array\`, \`Response\`, \`Request\`
- Install instructions in \`editors/vim/README.md\`"

# --- Issue 6 ---
create_issue \
  "Add 'std::env' module for environment variables and .env file loading" \
  "intermediate,stdlib,feature" \
  "### Description
Backend services heavily rely on environment variables (\`PORT\`, \`DATABASE_URL\`, \`JWT_SECRET\`). We need a native \`std::env\` module in TruLang.

### Proposed API:
\`\`\`trulang
let port = std::env::get(\"PORT\") ?? \"8080\";
let is_prod = std::env::has(\"PRODUCTION\");
std::env::load_dotenv(); // Parses local .env file into environment
\`\`\`

### Files to Modify:
- \`crates/ffi/src/registry.rs\` or \`crates/vm/src/vm.rs\`
- Documentation in \`docs/index.html\`"

# --- Issue 7 ---
create_issue \
  "Implement Server-Sent Events (SSE) streaming helper in trulang_net" \
  "intermediate,networking,phase-7" \
  "### Description
As part of the TruLang 2.0 Roadmap (Phase 7: Streaming Protocols), we need built-in Server-Sent Events (SSE) support for LLM streaming and live dashboards.

### Acceptance Criteria:
- Support \`text/event-stream\` Content-Type
- Helper methods: \`HttpResponse::sse()\`, \`sse.send_event(event_name, data)\`
- Automatic flush over chunked HTTP/1.1 wire protocol
- Verified on top of \`trulang_net::poller\` loop"

# --- Issue 8 ---
create_issue \
  "Build Tree-sitter grammar for TruLang (editors/tree-sitter-trulang)" \
  "intermediate,tooling,tree-sitter" \
  "### Description
Create an official Tree-sitter grammar for TruLang. This will enable modern AST-based syntax highlighting and code navigation across Neovim, Helix, Zed, Emacs, and GitHub.

### Acceptance Criteria:
- New directory: \`editors/tree-sitter-trulang/\`
- Complete grammar matching \`crates/syntax/\` (service actors, memory budgets, records, expressions)
- Passes tree-sitter test corpus on example \`.tr\` files"

# --- Issue 9 ---
create_issue \
  "Compile TruLang VM to WebAssembly (wasm32-wasip1) for true in-browser execution" \
  "advanced,wasm,phase-8" \
  "### Description
Currently, the interactive browser playground at \`https://gagn.dev/trulang/#playground\` uses a simulated JavaScript runner. As part of Roadmap Phase 8, we want to compile \`trulang_vm\` directly to WebAssembly (\`wasm32-unknown-unknown\` or \`wasm32-wasip1\`) so that users can compile and execute real TruLang bytecode directly inside their browser without any backend server.

### Acceptance Criteria:
- Add WASM build target in \`crates/vm\` (using \`wasm-bindgen\`)
- Expose \`compile_and_run(source: &str) -> ExecutionResult\` to JS
- Update \`docs/js/playground.js\` to run the real WASM VM"

# --- Issue 10 ---
create_issue \
  "Native TLS / HTTPS termination via rustls in trulang_net" \
  "advanced,networking,security,phase-7" \
  "### Description
Phase 7 of the TruLang 2.0 Roadmap calls for zero-overhead native HTTPS support. We want to terminate TLS directly inside \`trulang_net::poller\` using \`rustls\` (pure Rust, zero OpenSSL dependencies).

### Acceptance Criteria:
- Add \`TlsAcceptor\` configuration to \`trulang_net::server\`
- Automatic SNI handshake negotiation inside non-blocking event loop
- ALPN negotiation for HTTP/1.1 and future HTTP/2
- Zero memory leaks across 10,000 TLS handshakes"

echo ""
echo "🎉 All 10 issues processed!"

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Category of memory telemetry recorded during VM execution
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MemoryEventKind {
    AllocRegionA,
    PersistToRegionB,
    FreeRegionA,
    ArenaBurst,
}

/// A single recorded memory telemetry event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEvent {
    pub kind: MemoryEventKind,
    pub bytes: usize,
    pub function: String,
    pub line: usize,
    pub is_burst: bool,
    pub stack_path: Vec<String>,
}

/// Hierarchical flamegraph node
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfileNode {
    pub name: String,
    pub region_a_bytes: usize,
    pub persist_bytes: usize,
    pub burst_bytes: usize,
    pub hit_count: usize,
    pub children: HashMap<String, ProfileNode>,
}

impl ProfileNode {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            region_a_bytes: 0,
            persist_bytes: 0,
            burst_bytes: 0,
            hit_count: 0,
            children: HashMap::new(),
        }
    }

    pub fn total_bytes(&self) -> usize {
        self.region_a_bytes + self.persist_bytes + self.burst_bytes
    }

    pub fn insert_event(&mut self, path: &[String], kind: MemoryEventKind, bytes: usize) {
        self.hit_count += 1;
        match kind {
            MemoryEventKind::AllocRegionA => self.region_a_bytes += bytes,
            MemoryEventKind::PersistToRegionB => self.persist_bytes += bytes,
            MemoryEventKind::ArenaBurst => {
                self.burst_bytes += bytes;
                self.region_a_bytes += bytes;
            }
            MemoryEventKind::FreeRegionA => {}
        }

        if let Some((first, rest)) = path.split_first() {
            let child = self
                .children
                .entry(first.clone())
                .or_insert_with(|| ProfileNode::new(first.clone()));
            child.insert_event(rest, kind, bytes);
        }
    }
}

/// Memory profiler tracking Region A allocations, Region B promotions (`persist()`),
/// and elastic budget burst expansions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryProfiler {
    pub events: Vec<MemoryEvent>,
    pub total_region_a_bytes: usize,
    pub total_region_b_promoted: usize,
    pub burst_allocations_count: usize,
    pub burst_bytes: usize,
    pub resets_count: usize,
    pub root: ProfileNode,
}

impl Default for MemoryProfiler {
    fn default() -> Self {
        Self::new()
    }
}

impl MemoryProfiler {
    pub fn new() -> Self {
        Self {
            events: Vec::new(),
            total_region_a_bytes: 0,
            total_region_b_promoted: 0,
            burst_allocations_count: 0,
            burst_bytes: 0,
            resets_count: 0,
            root: ProfileNode::new("root"),
        }
    }

    pub fn record_alloc(
        &mut self,
        bytes: usize,
        function: impl Into<String>,
        line: usize,
        is_burst: bool,
        stack_path: Vec<String>,
    ) {
        let func = function.into();
        self.total_region_a_bytes += bytes;
        if is_burst {
            self.burst_allocations_count += 1;
            self.burst_bytes += bytes;
        }

        let kind = if is_burst {
            MemoryEventKind::ArenaBurst
        } else {
            MemoryEventKind::AllocRegionA
        };

        self.root.insert_event(&stack_path, kind, bytes);

        self.events.push(MemoryEvent {
            kind,
            bytes,
            function: func,
            line,
            is_burst,
            stack_path,
        });
    }

    pub fn record_persist(
        &mut self,
        bytes: usize,
        function: impl Into<String>,
        line: usize,
        stack_path: Vec<String>,
    ) {
        let func = function.into();
        self.total_region_b_promoted += bytes;
        self.root
            .insert_event(&stack_path, MemoryEventKind::PersistToRegionB, bytes);

        self.events.push(MemoryEvent {
            kind: MemoryEventKind::PersistToRegionB,
            bytes,
            function: func,
            line,
            is_burst: false,
            stack_path,
        });
    }

    pub fn record_reset(
        &mut self,
        function: impl Into<String>,
        line: usize,
        stack_path: Vec<String>,
    ) {
        let func = function.into();
        self.resets_count += 1;
        self.events.push(MemoryEvent {
            kind: MemoryEventKind::FreeRegionA,
            bytes: 0,
            function: func,
            line,
            is_burst: false,
            stack_path,
        });
    }

    /// Renders folded stack text format compatible with standard flamegraph visualizers
    pub fn render_folded_text(&self) -> String {
        let mut lines = Vec::new();
        self.collect_folded(&self.root, &mut Vec::new(), &mut lines);
        lines.join("\n")
    }

    fn collect_folded(
        &self,
        node: &ProfileNode,
        current_stack: &mut Vec<String>,
        output: &mut Vec<String>,
    ) {
        if node.name != "root" {
            current_stack.push(node.name.clone());
        }

        if node.region_a_bytes > 0 {
            let stack_str = current_stack.join(";");
            output.push(format!("{} {}", stack_str, node.region_a_bytes));
        }

        if node.persist_bytes > 0 {
            let mut persist_stack = current_stack.clone();
            persist_stack.push("[persist->RegionB]".to_string());
            output.push(format!(
                "{} {}",
                persist_stack.join(";"),
                node.persist_bytes
            ));
        }

        if node.burst_bytes > 0 {
            let mut burst_stack = current_stack.clone();
            burst_stack.push("[elastic_burst]".to_string());
            output.push(format!("{} {}", burst_stack.join(";"), node.burst_bytes));
        }

        for child in node.children.values() {
            self.collect_folded(child, current_stack, output);
        }

        if node.name != "root" {
            current_stack.pop();
        }
    }

    /// Renders structured JSON format containing full metrics and call tree
    pub fn render_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_else(|_| "{}".to_string())
    }

    /// Renders self-contained interactive SVG flamegraph
    pub fn render_svg(&self) -> String {
        let total = (self.total_region_a_bytes + self.total_region_b_promoted).max(1);
        let width = 1200.0;
        let row_height = 28.0;
        let margin_top = 110.0;
        let margin_left = 30.0;
        let content_width = width - (margin_left * 2.0);

        let mut svg = String::with_capacity(16384);
        svg.push_str(&format!(
            r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {} 750" width="100%" height="750" style="background:#0f172a; font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace;">
  <defs>
    <linearGradient id="headerGrad" x1="0%" y1="0%" x2="100%" y2="100%">
      <stop offset="0%" stop-color="#38bdf8"/>
      <stop offset="100%" stop-color="#818cf8"/>
    </linearGradient>
    <filter id="cardShadow" x="-5%" y="-5%" width="110%" height="110%">
      <feDropShadow dx="0" dy="4" stdDeviation="6" flood-color="#000" flood-opacity="0.35"/>
    </filter>
  </defs>

  <!-- Title and Header -->
  <rect x="0" y="0" width="{}" height="90" fill="#1e293b"/>
  <text x="30" y="38" fill="url(#headerGrad)" font-size="20" font-weight="bold">TruLang 1.0 Visual Memory Flamegraph</text>
  <text x="30" y="65" fill="#94a3b8" font-size="13">Dual-Region Allocator Telemetry &amp; Elastic Budget Burst Inspector</text>

  <!-- Stat Badges -->
  <g transform="translate(680, 22)">
    <rect x="0" y="0" width="115" height="46" rx="6" fill="#0369a1" fill-opacity="0.25" stroke="#0284c7" stroke-width="1"/>
    <text x="10" y="18" fill="#38bdf8" font-size="10" font-weight="bold">REGION A (BUMP)</text>
    <text x="10" y="37" fill="#ffffff" font-size="13" font-weight="bold">{} B</text>

    <rect x="125" y="0" width="125" height="46" rx="6" fill="#b45309" fill-opacity="0.25" stroke="#d97706" stroke-width="1"/>
    <text x="135" y="18" fill="#fbbf24" font-size="10" font-weight="bold">REGION B (PERSIST)</text>
    <text x="135" y="37" fill="#ffffff" font-size="13" font-weight="bold">{} B</text>

    <rect x="260" y="0" width="115" height="46" rx="6" fill="#be123c" fill-opacity="0.25" stroke="#e11d48" stroke-width="1"/>
    <text x="270" y="18" fill="#f43f5e" font-size="10" font-weight="bold">BURST BURDEN</text>
    <text x="270" y="37" fill="#ffffff" font-size="13" font-weight="bold">{} B</text>

    <rect x="385" y="0" width="95" height="46" rx="6" fill="#334155" stroke="#475569" stroke-width="1"/>
    <text x="395" y="18" fill="#cbd5e1" font-size="10" font-weight="bold">O(1) RESETS</text>
    <text x="395" y="37" fill="#ffffff" font-size="13" font-weight="bold">{}</text>
  </g>
"##,
            width, width, self.total_region_a_bytes, self.total_region_b_promoted, self.burst_bytes, self.resets_count
        ));

        // Render root flame bar
        let root_y = margin_top;
        svg.push_str(&format!(
            r##"  <!-- Flamegraph Blocks -->
  <g class="flame-node" style="cursor:pointer;" onclick="selectNode('root', {}, {}, {})">
    <rect x="{}" y="{}" width="{}" height="{}" rx="4" fill="#0284c7" stroke="#0f172a" stroke-width="1"/>
    <text x="{}" y="{}" fill="#ffffff" font-size="12" font-weight="bold">root (Total: {} B)</text>
  </g>
"##,
            self.total_region_a_bytes,
            self.total_region_b_promoted,
            self.burst_bytes,
            margin_left,
            root_y,
            content_width,
            row_height,
            margin_left + 10.0,
            root_y + 18.0,
            total
        ));

        // Render child levels recursively
        let y_offset = root_y + row_height + 4.0;
        let mut current_x = margin_left;

        for child in self.root.children.values() {
            let child_bytes = child.total_bytes();
            let child_w = (child_bytes as f64 / total as f64) * content_width;
            if child_w < 1.0 {
                continue;
            }

            let fill = if child.burst_bytes > 0 {
                "#e11d48" // Burst = Red
            } else if child.persist_bytes > 0 {
                "#d97706" // Persist = Amber
            } else {
                "#0d9488" // Transient = Teal
            };

            let label = if child.persist_bytes > 0 {
                format!("{} [persist] ({} B)", child.name, child_bytes)
            } else if child.burst_bytes > 0 {
                format!("{} [burst] ({} B)", child.name, child_bytes)
            } else {
                format!("{} ({} B)", child.name, child_bytes)
            };

            svg.push_str(&format!(
                r##"  <g class="flame-node" style="cursor:pointer;" onclick="selectNode('{}', {}, {}, {})">
    <rect x="{}" y="{}" width="{}" height="{}" rx="4" fill="{}" stroke="#0f172a" stroke-width="1">
      <title>{}: Region A: {} B, Persist: {} B, Burst: {} B</title>
    </rect>
    <text x="{}" y="{}" fill="#ffffff" font-size="11" font-weight="500">{}</text>
  </g>
"##,
                child.name,
                child.region_a_bytes,
                child.persist_bytes,
                child.burst_bytes,
                current_x,
                y_offset,
                child_w.max(2.0),
                row_height,
                fill,
                child.name,
                child.region_a_bytes,
                child.persist_bytes,
                child.burst_bytes,
                current_x + 6.0,
                y_offset + 18.0,
                if child_w > 60.0 { &label } else { "" }
            ));

            // Render grandchildren
            let mut sub_x = current_x;
            let sub_y = y_offset + row_height + 4.0;
            for gchild in child.children.values() {
                let gchild_bytes = gchild.total_bytes();
                let gchild_w = (gchild_bytes as f64 / total as f64) * content_width;
                if gchild_w < 1.0 {
                    continue;
                }
                let gfill = if gchild.burst_bytes > 0 {
                    "#f43f5e"
                } else if gchild.persist_bytes > 0 {
                    "#f59e0b"
                } else {
                    "#14b8a6"
                };

                svg.push_str(&format!(
                    r##"  <g class="flame-node" style="cursor:pointer;">
    <rect x="{}" y="{}" width="{}" height="{}" rx="4" fill="{}" stroke="#0f172a" stroke-width="1">
      <title>{}: Region A: {} B, Persist: {} B, Burst: {} B</title>
    </rect>
    <text x="{}" y="{}" fill="#ffffff" font-size="10">{}</text>
  </g>
"##,
                    sub_x,
                    sub_y,
                    gchild_w.max(2.0),
                    row_height,
                    gfill,
                    gchild.name,
                    gchild.region_a_bytes,
                    gchild.persist_bytes,
                    gchild.burst_bytes,
                    sub_x + 4.0,
                    sub_y + 18.0,
                    if gchild_w > 40.0 { &gchild.name } else { "" }
                ));
                sub_x += gchild_w;
            }

            current_x += child_w;
        }

        // Legend at bottom
        svg.push_str(r##"  <!-- Legend -->
  <g transform="translate(30, 690)">
    <rect x="0" y="0" width="16" height="16" rx="3" fill="#0d9488"/>
    <text x="24" y="13" fill="#94a3b8" font-size="12">Region A Bump Allocations (Transient O(1))</text>

    <rect x="360" y="0" width="16" height="16" rx="3" fill="#d97706"/>
    <text x="384" y="13" fill="#94a3b8" font-size="12">Region B Promotions (persist() DAG)</text>

    <rect x="690" y="0" width="16" height="16" rx="3" fill="#e11d48"/>
    <text x="714" y="13" fill="#94a3b8" font-size="12">Elastic Budget Burst Expansion</text>
  </g>

  <script>
    function selectNode(name, regA, persist, burst) {
      console.log('Selected:', name, 'RegionA:', regA, 'Persist:', persist, 'Burst:', burst);
    }
  </script>
</svg>"##);

        svg
    }
}

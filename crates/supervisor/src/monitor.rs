use crate::shm::{SharedTelemetryRegion, TelemetryEventType};
use std::sync::atomic::Ordering;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, PartialEq)]
pub enum HealthStatus {
    Healthy,
    PressureWarning(String),
    Critical(String),
    WorkerDead(Duration),
}

#[derive(Debug, Clone)]
pub struct MemorySnapshot {
    pub worker_pid: u32,
    pub region_a_used: u64,
    pub region_a_budget: u64,
    pub region_a_allocs: u64,
    pub region_b_live: u64,
    pub region_b_budget: u64,
    pub region_b_objects: u64,
    pub region_b_allocs: u64,
    pub time_since_heartbeat: Duration,
    pub status: HealthStatus,
}

pub struct MemorySupervisor {
    shm: SharedTelemetryRegion,
}

impl MemorySupervisor {
    pub fn attach(shm: SharedTelemetryRegion) -> Self {
        Self { shm }
    }

    pub fn snapshot(&self) -> MemorySnapshot {
        let header = self.shm.header();
        let now_ns = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64;

        let last_hb = header.last_heartbeat_ns.load(Ordering::Relaxed);
        let time_since_heartbeat = if now_ns > last_hb {
            Duration::from_nanos(now_ns - last_hb)
        } else {
            Duration::from_secs(0)
        };

        let reg_a_used = header.region_a_used_bytes.load(Ordering::Relaxed);
        let reg_a_budget = header.region_a_budget_bytes.load(Ordering::Relaxed);
        let reg_a_allocs = header.region_a_alloc_count.load(Ordering::Relaxed);

        let reg_b_live = header.region_b_live_bytes.load(Ordering::Relaxed);
        let reg_b_budget = header.region_b_budget_bytes.load(Ordering::Relaxed);
        let reg_b_objects = header.region_b_live_objects.load(Ordering::Relaxed);
        let reg_b_allocs = header.region_b_alloc_count.load(Ordering::Relaxed);

        let status = if time_since_heartbeat > Duration::from_secs(5) {
            HealthStatus::WorkerDead(time_since_heartbeat)
        } else if reg_a_budget > 0 && reg_a_used as f64 / reg_a_budget as f64 > 0.9 {
            HealthStatus::Critical(format!(
                "Region A utilization at {:.1}% of budget",
                (reg_a_used as f64 / reg_a_budget as f64) * 100.0
            ))
        } else if reg_b_budget > 0 && reg_b_live as f64 / reg_b_budget as f64 > 0.9 {
            HealthStatus::Critical(format!(
                "Region B utilization at {:.1}% of budget",
                (reg_b_live as f64 / reg_b_budget as f64) * 100.0
            ))
        } else if reg_b_budget > 0 && reg_b_live as f64 / reg_b_budget as f64 > 0.75 {
            HealthStatus::PressureWarning(format!(
                "Region B sustained pressure at {:.1}%",
                (reg_b_live as f64 / reg_b_budget as f64) * 100.0
            ))
        } else {
            HealthStatus::Healthy
        };

        MemorySnapshot {
            worker_pid: header.worker_pid,
            region_a_used: reg_a_used,
            region_a_budget: reg_a_budget,
            region_a_allocs: reg_a_allocs,
            region_b_live: reg_b_live,
            region_b_budget: reg_b_budget,
            region_b_objects: reg_b_objects,
            region_b_allocs: reg_b_allocs,
            time_since_heartbeat,
            status,
        }
    }

    pub fn print_dashboard(&self) {
        let snap = self.snapshot();
        println!("\n╔══════════════════════════════════════════════════════════════════════════╗");
        println!("║                      TruLang Independent Memory Supervisor                 ║");
        println!("╠══════════════════════════════════════════════════════════════════════════╣");
        println!(
            "║ Worker Process PID: {:<6}       Heartbeat: {:<6.2}s ago                     ║",
            snap.worker_pid,
            snap.time_since_heartbeat.as_secs_f64()
        );

        let status_str = match &snap.status {
            HealthStatus::Healthy => "🟢 HEALTHY (Within configured budgets)".to_string(),
            HealthStatus::PressureWarning(msg) => format!("🟡 WARNING: {}", msg),
            HealthStatus::Critical(msg) => format!("🔴 CRITICAL: {}", msg),
            HealthStatus::WorkerDead(d) => {
                format!(
                    "💀 WORKER DEAD / OOM-KILLED (Heartbeat lost {:.1}s ago)",
                    d.as_secs_f64()
                )
            }
        };
        println!("║ Health Status: {:<58}║", status_str);
        println!("╠──────────────────────────────────────────────────────────────────────────╢");
        println!(
            "║ Region A (Execution):  {:>10} B / {:>10} B ({:>5.1}%) - {:>6} allocs ║",
            snap.region_a_used,
            snap.region_a_budget,
            if snap.region_a_budget > 0 {
                (snap.region_a_used as f64 / snap.region_a_budget as f64) * 100.0
            } else {
                0.0
            },
            snap.region_a_allocs
        );
        println!(
            "║ Region B (Persistent): {:>10} B / {:>10} B ({:>5.1}%) - {:>6} objs   ║",
            snap.region_b_live,
            snap.region_b_budget,
            if snap.region_b_budget > 0 {
                (snap.region_b_live as f64 / snap.region_b_budget as f64) * 100.0
            } else {
                0.0
            },
            snap.region_b_objects
        );
        println!("╠──────────────────────────────────────────────────────────────────────────╢");
        println!("║ Recent Telemetry Activity (Shared Memory Ring Buffer):                   ║");

        let recs = self.shm.read_recent_records(8);
        if recs.is_empty() {
            println!(
                "║  (No recent telemetry events recorded)                                   ║"
            );
        } else {
            for rec in recs.iter().rev() {
                let ev_name = match TelemetryEventType::from(rec.event_type) {
                    TelemetryEventType::AllocRegionA => "Alloc A",
                    TelemetryEventType::FreeRegionA => "Reset A",
                    TelemetryEventType::PersistToRegionB => "Persist",
                    TelemetryEventType::BudgetExceeded => "BUDGET!",
                    TelemetryEventType::Heartbeat => "Beat   ",
                    TelemetryEventType::ArenaBurst => "BURST! ",
                };
                let label = rec.get_label();
                println!(
                    "║  [{:<7}] {:>6} B @ line {:>3}:{:02} | {:<28} ║",
                    ev_name, rec.bytes, rec.line, rec.col, label
                );
            }
        }

        println!("╚══════════════════════════════════════════════════════════════════════════╝");
    }
}

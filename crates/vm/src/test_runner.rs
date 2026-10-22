use crate::vm::{VirtualMachine, VmError};
use std::time::{Duration, Instant};
use trulang_compiler::bytecode::CompiledTest;

#[derive(Debug, Clone, PartialEq)]
pub enum TestOutcome {
    Passed {
        duration: Duration,
        region_a_bytes: usize,
    },
    Failed {
        error: String,
        line: usize,
        duration: Duration,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct TestResult {
    pub name: String,
    pub line: usize,
    pub outcome: TestOutcome,
}

impl TestResult {
    pub fn is_passed(&self) -> bool {
        matches!(self.outcome, TestOutcome::Passed { .. })
    }

    pub fn is_failed(&self) -> bool {
        matches!(self.outcome, TestOutcome::Failed { .. })
    }
}

pub struct TestRunner;

impl TestRunner {
    pub fn run_test(test: &CompiledTest) -> TestResult {
        let start = Instant::now();
        let mut vm = VirtualMachine::new();
        vm.set_quiet(true);

        match vm.execute(&test.chunk) {
            Ok(_) => {
                let duration = start.elapsed();
                let region_a_bytes = vm.region_a.used_bytes;
                TestResult {
                    name: test.name.clone(),
                    line: test.line,
                    outcome: TestOutcome::Passed {
                        duration,
                        region_a_bytes,
                    },
                }
            }
            Err(err) => {
                let duration = start.elapsed();
                let line = match &err {
                    VmError::AssertionFailure { line, .. } => *line,
                    VmError::AssertionEqFailure { line, .. } => *line,
                    _ => test.line,
                };
                TestResult {
                    name: test.name.clone(),
                    line: test.line,
                    outcome: TestOutcome::Failed {
                        error: err.to_string(),
                        line,
                        duration,
                    },
                }
            }
        }
    }

    pub fn run_all(tests: &[CompiledTest], filter: Option<&str>) -> Vec<TestResult> {
        tests
            .iter()
            .filter(|t| {
                if let Some(pat) = filter {
                    t.name.contains(pat)
                } else {
                    true
                }
            })
            .map(Self::run_test)
            .collect()
    }
}

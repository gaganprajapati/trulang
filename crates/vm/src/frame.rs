#[derive(Debug, Clone)]
pub struct CallFrame {
    pub ip: usize,
    pub slots_offset: usize,
    pub defers: Vec<usize>,
    pub try_handlers: Vec<usize>,
}

impl CallFrame {
    pub fn new(ip: usize, slots_offset: usize) -> Self {
        Self {
            ip,
            slots_offset,
            defers: Vec::new(),
            try_handlers: Vec::new(),
        }
    }
}

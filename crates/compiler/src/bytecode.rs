use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, Mutex};

pub trait ExternalResource: Send + Sync + fmt::Debug + 'static {
    fn type_name(&self) -> &'static str;
    fn call_method(&self, method: &str, _args: &[Value]) -> Result<Value, String> {
        Err(format!("Method '{}' not found on opaque handle '{}'", method, self.type_name()))
    }
}

pub struct ExternalHandle {
    pub id: u64,
    pub type_name: String,
    pub resource: Arc<dyn ExternalResource>,
}

impl fmt::Debug for ExternalHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ExternalHandle {{ id: {}, type_name: {:?}, resource: {:?} }}", self.id, self.type_name, self.resource)
    }
}

impl PartialEq for ExternalHandle {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id || Arc::ptr_eq(&self.resource, &other.resource)
    }
}

#[derive(Debug, Clone)]
pub enum Value {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    String(Arc<String>),
    Array(Vec<Value>),
    Map(Arc<Mutex<HashMap<String, Value>>>),
    Object(Arc<Mutex<HashMap<String, Value>>>),
    Handle(Arc<ExternalHandle>),
}

impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Value::Null, Value::Null) => true,
            (Value::Bool(a), Value::Bool(b)) => a == b,
            (Value::Int(a), Value::Int(b)) => a == b,
            (Value::Float(a), Value::Float(b)) => a == b,
            (Value::String(a), Value::String(b)) => a == b,
            (Value::Array(a), Value::Array(b)) => a == b,
            (Value::Map(a), Value::Map(b)) => Arc::ptr_eq(a, b),
            (Value::Object(a), Value::Object(b)) => Arc::ptr_eq(a, b),
            (Value::Handle(a), Value::Handle(b)) => a == b,
            _ => false,
        }
    }
}

impl Value {
    pub fn is_truthy(&self) -> bool {
        match self {
            Value::Null => false,
            Value::Bool(b) => *b,
            Value::Int(n) => *n != 0,
            Value::Float(f) => *f != 0.0,
            Value::String(s) => !s.is_empty(),
            Value::Array(a) => !a.is_empty(),
            Value::Map(m) => !m.lock().unwrap().is_empty(),
            Value::Object(_) => true,
            Value::Handle(_) => true,
        }
    }

    pub fn new_map() -> Self {
        Value::Map(Arc::new(Mutex::new(HashMap::new())))
    }

    pub fn new_object() -> Self {
        Value::Object(Arc::new(Mutex::new(HashMap::new())))
    }

    pub fn new_handle<R: ExternalResource>(id: u64, type_name: impl Into<String>, resource: R) -> Self {
        Value::Handle(Arc::new(ExternalHandle {
            id,
            type_name: type_name.into(),
            resource: Arc::new(resource),
        }))
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Null => write!(f, "null"),
            Value::Bool(b) => write!(f, "{}", b),
            Value::Int(n) => write!(f, "{}", n),
            Value::Float(fl) => write!(f, "{}", fl),
            Value::String(s) => write!(f, "{}", s),
            Value::Array(arr) => {
                write!(f, "[")?;
                for (i, item) in arr.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", item)?;
                }
                write!(f, "]")
            }
            Value::Map(m) => {
                let guard = m.lock().unwrap();
                write!(f, "Map{{")?;
                let mut first = true;
                for (k, v) in guard.iter() {
                    if !first {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}: {}", k, v)?;
                    first = false;
                }
                write!(f, "}}")
            }
            Value::Object(obj) => {
                let guard = obj.lock().unwrap();
                write!(f, "Object{{")?;
                let mut first = true;
                for (k, v) in guard.iter() {
                    if !first {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}: {}", k, v)?;
                    first = false;
                }
                write!(f, "}}")
            }
            Value::Handle(h) => write!(f, "<Handle: {} #{}>", h.type_name, h.id),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Opcode {
    LoadConst(usize),
    LoadInt(i64),
    LoadFloat(f64),
    LoadBool(bool),
    LoadNull,
    Pop,
    Dup,

    GetLocal(usize),
    SetLocal(usize),
    GetField(usize),
    SetField(usize),

    GetFieldNamed(String),
    SetFieldNamed(String),
    NewMap,
    CallMethod(String, usize), // method name, arg count

    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Neg,
    Not,
    Eq,
    NotEq,
    Lt,
    LtEq,
    Gt,
    GtEq,

    Jump(usize),
    JumpIfFalse(usize),
    Call(usize), // arity
    Return,

    PushTry(usize), // target catch address
    PopTry,
    Throw,

    PushDefer(usize), // target defer instruction address
    RunDefers,

    AllocRegionA(usize, Option<usize>), // (request arena budget bytes, optional max ceiling)
    FreeRegionA,                        // bulk reset Region A arena
    Persist,                            // promote top of stack from Region A to Region B

    Print,
    Println,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Chunk {
    pub code: Vec<Opcode>,
    pub constants: Vec<Value>,
    pub lines: Vec<usize>,
    pub imports: HashMap<String, String>,
}

impl Chunk {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn emit(&mut self, op: Opcode, line: usize) -> usize {
        let offset = self.code.len();
        self.code.push(op);
        self.lines.push(line);
        offset
    }

    pub fn add_constant(&mut self, val: Value) -> usize {
        self.constants.push(val);
        self.constants.len() - 1
    }

    pub fn patch_jump(&mut self, offset: usize, target: usize) {
        match &mut self.code[offset] {
            Opcode::Jump(ref mut addr)
            | Opcode::JumpIfFalse(ref mut addr)
            | Opcode::PushTry(ref mut addr) => {
                *addr = target;
            }
            _ => panic!("Attempted to patch non-jump opcode at offset {}", offset),
        }
    }
}

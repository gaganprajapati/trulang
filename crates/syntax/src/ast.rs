use crate::span::Spanned;

#[derive(Debug, Clone, PartialEq)]
pub struct Program {
    pub declarations: Vec<Spanned<Declaration>>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Declaration {
    Service(ServiceDecl),
    Function(FunctionDecl),
    Struct(StructDecl),
}

#[derive(Debug, Clone, PartialEq)]
pub struct ServiceDecl {
    pub name: String,
    pub budgets: Vec<Spanned<MemoryBudgetDecl>>,
    pub fields: Vec<Spanned<FieldDecl>>,
    pub methods: Vec<Spanned<FunctionDecl>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BudgetRegion {
    Request,
    Persistent,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MemoryBudgetDecl {
    pub region: BudgetRegion,
    pub size_bytes: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FieldDecl {
    pub name: String,
    pub is_mut: bool,
    pub ty: Option<Type>,
    pub init: Option<Spanned<Expr>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FunctionDecl {
    pub name: String,
    pub params: Vec<Param>,
    pub return_type: Option<Type>,
    pub body: Block,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StructDecl {
    pub name: String,
    pub fields: Vec<Param>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Param {
    pub name: String,
    pub is_mut: bool,
    pub ty: Type,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Type {
    I64,
    F64,
    Bool,
    String,
    Void,
    Named(String),
    Nullable(Box<Type>),
    Generic(String, Vec<Type>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Block {
    pub statements: Vec<Spanned<Statement>>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Statement {
    Let {
        name: String,
        is_mut: bool,
        ty: Option<Type>,
        init: Spanned<Expr>,
    },
    Assign {
        target: Spanned<Expr>,
        value: Spanned<Expr>,
    },
    Return(Option<Spanned<Expr>>),
    Expr(Spanned<Expr>),
    If {
        condition: Spanned<Expr>,
        then_branch: Block,
        else_branch: Option<Block>,
    },
    While {
        condition: Spanned<Expr>,
        body: Block,
    },
    TryCatch {
        try_block: Block,
        error_name: String,
        error_type: Option<Type>,
        catch_block: Block,
    },
    Defer(Spanned<Expr>),
    Block(Block),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Literal(Literal),
    Ident(String),
    Binary {
        op: BinOp,
        left: Box<Spanned<Expr>>,
        right: Box<Spanned<Expr>>,
    },
    Unary {
        op: UnaryOp,
        expr: Box<Spanned<Expr>>,
    },
    Call {
        callee: Box<Spanned<Expr>>,
        args: Vec<Spanned<Expr>>,
    },
    MethodCall {
        receiver: Box<Spanned<Expr>>,
        method: String,
        args: Vec<Spanned<Expr>>,
    },
    MemberAccess {
        target: Box<Spanned<Expr>>,
        field: String,
        is_optional: bool, // ?.
    },
    Persist(Box<Spanned<Expr>>),
    Path(Vec<String>), // e.g. Response.json or Map::new
}

#[derive(Debug, Clone, PartialEq)]
pub enum Literal {
    Int(i64),
    Float(f64),
    String(String),
    Bool(bool),
    Null,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Eq,
    NotEq,
    Lt,
    LtEq,
    Gt,
    GtEq,
    And,
    Or,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    Neg,
    Not,
}

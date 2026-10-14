use std::fmt;
use vlang_syntax::ast::Type as AstType;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SemType {
    I64,
    F64,
    Bool,
    String,
    Void,
    Null,
    Nullable(Box<SemType>),
    Named(String),
    Generic(String, Vec<SemType>),
    Any,
}

impl SemType {
    pub fn from_ast(ast: &AstType) -> Self {
        match ast {
            AstType::I64 => SemType::I64,
            AstType::F64 => SemType::F64,
            AstType::Bool => SemType::Bool,
            AstType::String => SemType::String,
            AstType::Void => SemType::Void,
            AstType::Named(name) => SemType::Named(name.clone()),
            AstType::Nullable(inner) => SemType::Nullable(Box::new(Self::from_ast(inner))),
            AstType::Generic(name, args) => {
                SemType::Generic(name.clone(), args.iter().map(Self::from_ast).collect())
            }
        }
    }

    pub fn is_assignable_to(&self, target: &SemType) -> bool {
        if self == target || target == &SemType::Any || self == &SemType::Any {
            return true;
        }

        match (self, target) {
            (SemType::Null, SemType::Nullable(_)) => true,
            (val, SemType::Nullable(inner)) => val.is_assignable_to(inner),
            _ => false,
        }
    }
}

impl fmt::Display for SemType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SemType::I64 => write!(f, "i64"),
            SemType::F64 => write!(f, "f64"),
            SemType::Bool => write!(f, "bool"),
            SemType::String => write!(f, "string"),
            SemType::Void => write!(f, "void"),
            SemType::Null => write!(f, "null"),
            SemType::Nullable(inner) => write!(f, "{}?", inner),
            SemType::Named(n) => write!(f, "{}", n),
            SemType::Generic(n, args) => {
                write!(f, "{}<", n)?;
                for (i, arg) in args.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", arg)?;
                }
                write!(f, ">")
            }
            SemType::Any => write!(f, "any"),
        }
    }
}

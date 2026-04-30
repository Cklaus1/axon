#[cfg(feature = "serde-json")]
use serde::{Deserialize, Serialize};
use crate::span::Span;

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde-json", derive(Serialize, Deserialize))]
pub struct Program {
    pub items: Vec<Item>,
}

// ── Top-level items ──────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde-json", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "serde-json", serde(tag = "kind"))]
pub enum Item {
    FnDef(FnDef),
    TypeDef(TypeDef),
    EnumDef(EnumDef),
    ModDecl(ModDecl),
    UseDecl(UseDecl),
    // Phase 3
    TraitDef(TraitDef),
    ImplBlock(ImplBlock),
    /// Module-level comptime constant: `let NAME = comptime { expr }`
    LetDef { name: String, value: Box<Expr>, span: Span },
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde-json", derive(Serialize, Deserialize))]
pub struct FnDef {
    pub public: bool,
    pub name: String,
    /// Phase 3: type parameter names for generic functions (e.g. `fn id<T>` → `["T"]`).
    pub generic_params: Vec<String>,
    /// Phase 4: trait bounds per type parameter.
    /// Each entry is `(param_name, bound_trait_names)`.
    /// Example: `fn show<T: Display + Clone>` → `[("T", ["Display", "Clone"])]`.
    pub generic_bounds: Vec<(String, Vec<String>)>,
    pub params: Vec<Param>,
    pub return_type: Option<AxonType>,
    pub body: Expr,
    pub attrs: Vec<Attr>,
    pub span: Span,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde-json", derive(Serialize, Deserialize))]
pub struct Param {
    pub name: String,
    pub ty: AxonType,
    pub span: Span,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde-json", derive(Serialize, Deserialize))]
pub struct TypeDef {
    pub name: String,
    /// Phase 3: type parameters for generic structs.
    pub generic_params: Vec<String>,
    pub fields: Vec<TypeField>,
    pub span: Span,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde-json", derive(Serialize, Deserialize))]
pub struct TypeField {
    pub name: String,
    pub ty: AxonType,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde-json", derive(Serialize, Deserialize))]
pub struct EnumDef {
    pub name: String,
    /// Phase 3: type parameters for generic enums.
    pub generic_params: Vec<String>,
    pub variants: Vec<EnumVariant>,
    pub span: Span,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde-json", derive(Serialize, Deserialize))]
pub struct EnumVariant {
    pub name: String,
    pub fields: Vec<TypeField>,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde-json", derive(Serialize, Deserialize))]
pub struct ModDecl {
    pub name: String,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde-json", derive(Serialize, Deserialize))]
pub struct UseDecl {
    pub path: Vec<String>,
    pub items: Vec<String>,
}

// ── Phase 3: Traits and impl blocks ──────────────────────────────────────────

/// `trait Foo { fn method(self) -> str }`
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde-json", derive(Serialize, Deserialize))]
pub struct TraitDef {
    pub name: String,
    pub generic_params: Vec<String>,
    pub methods: Vec<TraitMethod>,
    pub span: Span,
}

/// A method signature inside a trait definition (no body).
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde-json", derive(Serialize, Deserialize))]
pub struct TraitMethod {
    pub name: String,
    pub params: Vec<Param>,
    pub return_type: Option<AxonType>,
    pub span: Span,
}

/// `impl Displayable for Point { fn display(self) -> str { ... } }`
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde-json", derive(Serialize, Deserialize))]
pub struct ImplBlock {
    /// The trait being implemented.
    pub trait_name: String,
    /// The concrete type the trait is implemented for.
    pub for_type: AxonType,
    pub methods: Vec<FnDef>,
    pub span: Span,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde-json", derive(Serialize, Deserialize))]
pub struct Attr {
    pub name: String,
    pub args: Vec<String>,
}

// ── Types ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde-json", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "serde-json", serde(tag = "kind"))]
pub enum AxonType {
    Named(String),
    Result { ok: Box<AxonType>, err: Box<AxonType> },
    Option(Box<AxonType>),
    Chan(Box<AxonType>),
    Slice(Box<AxonType>),
    Generic { base: String, args: Vec<AxonType> },
    Fn { params: Vec<AxonType>, ret: Box<AxonType> },
    Ref(Box<AxonType>),
    /// Phase 3: trait object type — `dyn Displayable`
    DynTrait(String),
    /// Phase 3: bare type parameter name inside a generic definition — `T`, `A`, `B`
    TypeParam(String),
}

// ── Expressions ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde-json", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "serde-json", serde(tag = "kind"))]
pub enum Expr {
    Block(Vec<Stmt>),
    Let { name: String, value: Box<Expr> },
    Own { name: String, value: Box<Expr> },
    RefBind { name: String, value: Box<Expr> },
    Call { callee: Box<Expr>, args: Vec<Expr> },
    MethodCall { receiver: Box<Expr>, method: String, args: Vec<Expr> },
    BinOp { op: BinOp, left: Box<Expr>, right: Box<Expr> },
    UnaryOp { op: UnaryOp, operand: Box<Expr> },
    Question(Box<Expr>),    // expr?
    Match { subject: Box<Expr>, arms: Vec<MatchArm> },
    If { cond: Box<Expr>, then: Box<Expr>, else_: Option<Box<Expr>> },
    Spawn(Box<Expr>),
    Select(Vec<SelectArm>),
    Comptime(Box<Expr>),
    /// Phase 3: lambda params carry optional type annotations and a capture list.
    /// `captures` is filled in by the resolver; source syntax is unchanged: `|x, y| body`.
    Lambda {
        params: Vec<LambdaParam>,
        body: Box<Expr>,
        /// Resolved capture list (name + type). Empty until resolver populates it.
        captures: Vec<(String, Option<crate::types::Type>)>,
    },
    Return(Option<Box<Expr>>),
    FieldAccess { receiver: Box<Expr>, field: String },
    Index { receiver: Box<Expr>, index: Box<Expr> },
    Ident(String),
    Literal(Literal),
    /// String interpolation: `"hello {name}!"` lowered to a series of
    /// `axon_concat` calls.  Parts alternate between literal text and
    /// sub-expressions that evaluate to `str`.
    FmtStr { parts: Vec<FmtPart> },
    Ok(Box<Expr>),
    Err(Box<Expr>),
    Some(Box<Expr>),
    None,
    Array(Vec<Expr>),
    StructLit { name: String, fields: Vec<(String, Expr)> },
    While {
        cond: Box<Expr>,
        body: Vec<Stmt>,
    },
    Assign { name: String, value: Box<Expr> },
    /// Exit the nearest enclosing `while` loop.
    Break,
    /// Jump to the condition-check of the nearest enclosing `while` loop.
    Continue,
    /// `for <var> in <start>..<end> { body }` — integer range loop.
    For {
        var: String,
        start: Box<Expr>,
        end: Box<Expr>,
        body: Vec<Stmt>,
    },
}

/// A parameter in a lambda expression — name plus optional type annotation.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde-json", derive(Serialize, Deserialize))]
pub struct LambdaParam {
    pub name: String,
    pub ty: Option<AxonType>,
}

impl LambdaParam {
    pub fn untyped(name: impl Into<String>) -> Self {
        LambdaParam { name: name.into(), ty: None }
    }
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde-json", derive(Serialize, Deserialize))]
pub struct Stmt {
    pub expr: Expr,
    #[cfg_attr(feature = "serde-json", serde(default))]
    pub span: Span,
}

impl Stmt {
    /// Construct a statement with no source-span information (for synthesised AST nodes).
    pub fn simple(expr: Expr) -> Self {
        Stmt { expr, span: Span::dummy() }
    }
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde-json", derive(Serialize, Deserialize))]
pub struct MatchArm {
    pub pattern: Pattern,
    pub guard: Option<Expr>,
    pub body: Expr,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde-json", derive(Serialize, Deserialize))]
pub struct SelectArm {
    pub recv: Expr,
    pub body: Expr,
}

// ── Patterns ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde-json", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "serde-json", serde(tag = "kind"))]
pub enum Pattern {
    Wildcard,
    Ident(String),
    Literal(Literal),
    Some(Box<Pattern>),
    None,
    Ok(Box<Pattern>),
    Err(Box<Pattern>),
    Struct { name: String, fields: Vec<(String, Pattern)> },
    Tuple(Vec<Pattern>),
}

// ── Format-string parts ───────────────────────────────────────────────────────

/// A single segment in a format-string expression.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde-json", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "serde-json", serde(tag = "kind"))]
pub enum FmtPart {
    /// A literal text fragment (no interpolation).
    Lit(String),
    /// An interpolated sub-expression that must evaluate to `str`.
    Expr(Box<Expr>),
}

// ── Literals ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde-json", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "serde-json", serde(tag = "kind"))]
pub enum Literal {
    Int(i64),
    Float(f64),
    Str(String),
    Bool(bool),
}

// ── Operators ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde-json", derive(Serialize, Deserialize))]
pub enum BinOp {
    Add, Sub, Mul, Div, Rem,
    Eq, NotEq, Lt, Gt, LtEq, GtEq,
    And, Or,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde-json", derive(Serialize, Deserialize))]
pub enum UnaryOp {
    Neg, Not, Ref,
}

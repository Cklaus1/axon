//! Axon semantic type system.
//!
//! These types are the output of type inference and are used by the code
//! generator to select the appropriate LLVM type representations.

use std::collections::HashMap;

/// Resolved semantic type for an Axon expression or binding.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde-json", derive(serde::Serialize, serde::Deserialize))]
pub enum Type {
    // Signed integers
    I8,
    I16,
    I32,
    I64,
    // Unsigned integers (same storage as signed; operations differ)
    U8,
    U16,
    U32,
    U64,
    // Floats
    F32,
    F64,
    // Primitives
    Bool,
    /// UTF-8 string — lowered to `{ i64, ptr }` (length + heap pointer).
    Str,
    /// The unit / void type.
    Unit,
    // Compound / generic
    /// Option<T> — lowered to `{ i1, T }`.
    Option(Box<Type>),
    /// Result<T, E> — lowered to `{ i1, [max(sizeof T, sizeof E) x i8] }`.
    Result(Box<Type>, Box<Type>),
    /// Slice<T> — lowered to `{ i64, ptr }` (length + heap data pointer).
    Slice(Box<Type>),
    /// Anonymous tuple — lowered to an anonymous LLVM struct.
    Tuple(Vec<Type>),
    /// First-class function type.
    Fn(Vec<Type>, Box<Type>),
    /// Named user-defined struct (look up in module by name).
    Struct(String),
    /// Named user-defined enum (look up in module by `{name}_enum`).
    Enum(String),
    // Type-checker meta-types (should not reach codegen)
    /// Type has not been resolved yet.
    Unknown,
    /// Unification variable (type inference in progress).
    Var(u32),
    /// Placeholder for a named type that has not been resolved yet.
    Deferred(String),
    // Phase 3
    /// A generic type parameter (e.g. `T` inside `fn id<T>(x: T) -> T`).
    TypeParam(String),
    /// A trait object type: `dyn Displayable` — fat pointer {data_ptr, vtable_ptr}.
    DynTrait(String),
    /// A channel carrying values of type T: `chan<T>` — opaque handle ptr.
    Chan(Box<Type>),
}

impl Type {
    /// Convert a source-level type name to a `Type`, if it is a known primitive.
    pub fn from_name(name: &str) -> Option<Type> {
        match name.trim() {
            "i8" => Some(Type::I8),
            "i16" => Some(Type::I16),
            "i32" => Some(Type::I32),
            "i64" => Some(Type::I64),
            "u8" => Some(Type::U8),
            "u16" => Some(Type::U16),
            "u32" => Some(Type::U32),
            "u64" => Some(Type::U64),
            "f32" => Some(Type::F32),
            "f64" => Some(Type::F64),
            "bool" => Some(Type::Bool),
            "str" | "String" => Some(Type::Str),
            "()" | "unit" | "Unit" => Some(Type::Unit),
            _ => None,
        }
    }

    pub fn is_integer(&self) -> bool {
        matches!(
            self,
            Type::I8 | Type::I16 | Type::I32 | Type::I64
                | Type::U8 | Type::U16 | Type::U32 | Type::U64
        )
    }

    pub fn is_signed_integer(&self) -> bool {
        matches!(self, Type::I8 | Type::I16 | Type::I32 | Type::I64)
    }

    pub fn is_unsigned_integer(&self) -> bool {
        matches!(self, Type::U8 | Type::U16 | Type::U32 | Type::U64)
    }

    pub fn is_float(&self) -> bool {
        matches!(self, Type::F32 | Type::F64)
    }

    pub fn is_numeric(&self) -> bool {
        self.is_integer() || self.is_float()
    }

    pub fn is_bool(&self) -> bool {
        matches!(self, Type::Bool)
    }

    pub fn is_str(&self) -> bool {
        matches!(self, Type::Str)
    }

    pub fn is_unit(&self) -> bool {
        matches!(self, Type::Unit)
    }

    pub fn is_option(&self) -> bool {
        matches!(self, Type::Option(_))
    }

    pub fn is_result(&self) -> bool {
        matches!(self, Type::Result(_, _))
    }

    pub fn is_deferred(&self) -> bool {
        matches!(self, Type::Deferred(_))
    }

    pub fn is_unknown(&self) -> bool {
        matches!(self, Type::Unknown)
    }

    pub fn is_var(&self) -> bool {
        matches!(self, Type::Var(_))
    }

    /// True for types that can be compared with `==` and `!=` natively.
    pub fn is_comparable(&self) -> bool {
        self.is_numeric() || self.is_bool() || self.is_str()
    }

    /// True for types that can be ordered with `<`, `>`, `<=`, `>=`.
    pub fn is_ordered(&self) -> bool {
        self.is_numeric()
    }

    /// Bit-width for integer types. Returns None for non-integer types.
    pub fn int_width(&self) -> Option<u32> {
        match self {
            Type::I8 | Type::U8 => Some(8),
            Type::I16 | Type::U16 => Some(16),
            Type::I32 | Type::U32 => Some(32),
            Type::I64 | Type::U64 => Some(64),
            _ => None,
        }
    }

    /// The default integer type used when inference cannot determine a more
    /// specific type (matches Axon's "integers default to i64" rule).
    pub fn default_int() -> Type {
        Type::I64
    }

    /// The default float type.
    pub fn default_float() -> Type {
        Type::F64
    }

    pub fn display(&self) -> String {
        match self {
            Type::I8 => "i8".into(),
            Type::I16 => "i16".into(),
            Type::I32 => "i32".into(),
            Type::I64 => "i64".into(),
            Type::U8 => "u8".into(),
            Type::U16 => "u16".into(),
            Type::U32 => "u32".into(),
            Type::U64 => "u64".into(),
            Type::F32 => "f32".into(),
            Type::F64 => "f64".into(),
            Type::Bool => "bool".into(),
            Type::Str => "str".into(),
            Type::Unit => "()".into(),
            Type::Option(t) => format!("Option<{}>", t.display()),
            Type::Result(ok, err) => format!("Result<{}, {}>", ok.display(), err.display()),
            Type::Slice(t) => format!("[{}]", t.display()),
            Type::Tuple(ts) => {
                let inner = ts.iter().map(|t| t.display()).collect::<Vec<_>>().join(", ");
                format!("({inner})")
            }
            Type::Fn(params, ret) => {
                let p = params.iter().map(|t| t.display()).collect::<Vec<_>>().join(", ");
                format!("fn({p}) -> {}", ret.display())
            }
            Type::Struct(n) | Type::Enum(n) | Type::Deferred(n) => n.clone(),
            Type::Unknown => "<unknown>".into(),
            Type::Var(n) => format!("?{n}"),
            Type::TypeParam(n) => n.clone(),
            Type::DynTrait(n) => format!("dyn {n}"),
            Type::Chan(t) => format!("chan<{}>", t.display()),
        }
    }
}

// ── Constraint ────────────────────────────────────────────────────────────────

/// A single unification constraint: `lhs` must equal `rhs`.
#[derive(Debug, Clone)]
pub struct Constraint {
    pub lhs: Type,
    pub rhs: Type,
    /// Human-readable origin for error messages (e.g. `"call arg"`, `"binop"`).
    pub origin: String,
    /// Source location captured when the constraint was pushed.
    pub span: crate::span::Span,
}

// ── Substitution ──────────────────────────────────────────────────────────────

/// A map from type variable indices to their resolved types.
#[derive(Debug, Clone, Default)]
pub struct Substitution(pub HashMap<u32, Type>);

impl Substitution {
    pub fn new() -> Self {
        Substitution(HashMap::new())
    }

    /// Insert a binding, but only after an occurs check.
    /// Returns `Err` if inserting `var → ty` would create a cycle.
    pub fn insert_checked(&mut self, var: u32, ty: &Type) -> Result<(), String> {
        if self.occurs(var, ty) {
            return Err(format!(
                "cyclic type: ?{var} would occur in {}",
                ty.display()
            ));
        }
        self.0.insert(var, ty.clone());
        Ok(())
    }

    pub fn insert(&mut self, var: u32, ty: Type) {
        self.0.insert(var, ty);
    }

    /// Returns true if type variable `var` appears anywhere in `ty`
    /// (after applying the current substitution). Used to prevent cyclic types.
    pub fn occurs(&self, var: u32, ty: &Type) -> bool {
        match ty {
            Type::Var(n) => {
                if *n == var {
                    return true;
                }
                if let Some(resolved) = self.0.get(n) {
                    self.occurs(var, resolved)
                } else {
                    false
                }
            }
            Type::Option(inner) | Type::Slice(inner) | Type::Chan(inner) => self.occurs(var, inner),
            Type::Result(ok, err) => self.occurs(var, ok) || self.occurs(var, err),
            Type::Tuple(ts) => ts.iter().any(|t| self.occurs(var, t)),
            Type::Fn(params, ret) => {
                params.iter().any(|p| self.occurs(var, p)) || self.occurs(var, ret)
            }
            _ => false,
        }
    }

    /// Recursively apply this substitution to `ty`, replacing all `Var(n)`
    /// that have a mapping with their resolved type.
    pub fn apply(&self, ty: &Type) -> Type {
        self.apply_inner(ty, &mut std::collections::HashSet::new())
    }

    fn apply_inner(&self, ty: &Type, visiting: &mut std::collections::HashSet<u32>) -> Type {
        match ty {
            Type::Var(n) => {
                if visiting.contains(n) {
                    // Cycle detected — return the var itself to break the loop.
                    return ty.clone();
                }
                if let Some(resolved) = self.0.get(n) {
                    visiting.insert(*n);
                    let result = self.apply_inner(resolved, visiting);
                    visiting.remove(n);
                    result
                } else {
                    ty.clone()
                }
            }
            Type::Option(inner) => Type::Option(Box::new(self.apply_inner(inner, visiting))),
            Type::Result(ok, err) => Type::Result(
                Box::new(self.apply_inner(ok, visiting)),
                Box::new(self.apply_inner(err, visiting)),
            ),
            Type::Slice(inner) => Type::Slice(Box::new(self.apply_inner(inner, visiting))),
            Type::Chan(inner) => Type::Chan(Box::new(self.apply_inner(inner, visiting))),
            Type::Tuple(ts) => {
                Type::Tuple(ts.iter().map(|t| self.apply_inner(t, visiting)).collect())
            }
            Type::Fn(params, ret) => Type::Fn(
                params.iter().map(|p| self.apply_inner(p, visiting)).collect(),
                Box::new(self.apply_inner(ret, visiting)),
            ),
            // Ground types pass through unchanged.
            _ => ty.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Type helpers ─────────────────────────────────────────────────────────

    #[test]
    fn numeric_classification() {
        assert!(Type::I64.is_numeric());
        assert!(Type::F64.is_numeric());
        assert!(Type::U32.is_numeric());
        assert!(!Type::Bool.is_numeric());
        assert!(!Type::Str.is_numeric());
        assert!(!Type::Unit.is_numeric());
    }

    #[test]
    fn signed_unsigned_classification() {
        assert!(Type::I8.is_signed_integer());
        assert!(Type::I64.is_signed_integer());
        assert!(!Type::U32.is_signed_integer());
        assert!(Type::U8.is_unsigned_integer());
        assert!(Type::U64.is_unsigned_integer());
        assert!(!Type::I32.is_unsigned_integer());
        assert!(!Type::F64.is_signed_integer());
    }

    #[test]
    fn int_width() {
        assert_eq!(Type::I8.int_width(), Some(8));
        assert_eq!(Type::U16.int_width(), Some(16));
        assert_eq!(Type::I32.int_width(), Some(32));
        assert_eq!(Type::U64.int_width(), Some(64));
        assert_eq!(Type::F64.int_width(), None);
        assert_eq!(Type::Bool.int_width(), None);
        assert_eq!(Type::Str.int_width(), None);
    }

    #[test]
    fn comparable_and_ordered() {
        assert!(Type::I64.is_comparable());
        assert!(Type::F64.is_comparable());
        assert!(Type::Bool.is_comparable());
        assert!(Type::Str.is_comparable());
        assert!(!Type::Unit.is_comparable());

        assert!(Type::I64.is_ordered());
        assert!(Type::F64.is_ordered());
        assert!(!Type::Bool.is_ordered());
        assert!(!Type::Str.is_ordered());
    }

    #[test]
    fn display_primitives() {
        assert_eq!(Type::I64.display(), "i64");
        assert_eq!(Type::F64.display(), "f64");
        assert_eq!(Type::Bool.display(), "bool");
        assert_eq!(Type::Str.display(), "str");
        assert_eq!(Type::Unit.display(), "()");
    }

    #[test]
    fn display_compound() {
        let opt = Type::Option(Box::new(Type::I64));
        assert_eq!(opt.display(), "Option<i64>");

        let res = Type::Result(Box::new(Type::I64), Box::new(Type::Str));
        assert_eq!(res.display(), "Result<i64, str>");

        let slice = Type::Slice(Box::new(Type::F64));
        assert_eq!(slice.display(), "[f64]");

        let tup = Type::Tuple(vec![Type::I64, Type::Bool]);
        assert_eq!(tup.display(), "(i64, bool)");
    }

    #[test]
    fn from_name_primitives() {
        assert_eq!(Type::from_name("i64"), Some(Type::I64));
        assert_eq!(Type::from_name("f64"), Some(Type::F64));
        assert_eq!(Type::from_name("bool"), Some(Type::Bool));
        assert_eq!(Type::from_name("str"), Some(Type::Str));
        assert_eq!(Type::from_name("()"), Some(Type::Unit));
        assert_eq!(Type::from_name("String"), Some(Type::Str));
        assert_eq!(Type::from_name("unknown_type"), None);
    }

    // ── Substitution ─────────────────────────────────────────────────────────

    #[test]
    fn apply_ground_type_unchanged() {
        let subst = Substitution::new();
        assert_eq!(subst.apply(&Type::I64), Type::I64);
        assert_eq!(subst.apply(&Type::Str), Type::Str);
    }

    #[test]
    fn apply_resolves_var() {
        let mut subst = Substitution::new();
        subst.insert(0, Type::I64);
        assert_eq!(subst.apply(&Type::Var(0)), Type::I64);
        assert_eq!(subst.apply(&Type::Var(1)), Type::Var(1)); // unresolved
    }

    #[test]
    fn apply_follows_chain() {
        let mut subst = Substitution::new();
        subst.insert(0, Type::Var(1));
        subst.insert(1, Type::F64);
        assert_eq!(subst.apply(&Type::Var(0)), Type::F64);
    }

    #[test]
    fn apply_handles_cycle_without_infinite_loop() {
        // Manually create a cycle: Var(0) → Var(1) → Var(0)
        // apply_inner must break the cycle rather than loop forever.
        let mut subst = Substitution::new();
        subst.insert(0, Type::Var(1));
        subst.insert(1, Type::Var(0));
        // Should return without hanging; result is just Var(0) or Var(1).
        let result = subst.apply(&Type::Var(0));
        assert!(matches!(result, Type::Var(_)));
    }

    #[test]
    fn apply_recurses_into_compound() {
        let mut subst = Substitution::new();
        subst.insert(0, Type::I64);
        let opt = Type::Option(Box::new(Type::Var(0)));
        assert_eq!(subst.apply(&opt), Type::Option(Box::new(Type::I64)));
    }

    #[test]
    fn occurs_check_simple() {
        let subst = Substitution::new();
        assert!(subst.occurs(0, &Type::Var(0)));
        assert!(!subst.occurs(0, &Type::Var(1)));
        assert!(!subst.occurs(0, &Type::I64));
    }

    #[test]
    fn occurs_check_nested() {
        let subst = Substitution::new();
        let inner = Type::Option(Box::new(Type::Var(0)));
        assert!(subst.occurs(0, &inner));

        let res = Type::Result(Box::new(Type::I64), Box::new(Type::Var(0)));
        assert!(subst.occurs(0, &res));
        assert!(!subst.occurs(1, &res));
    }

    #[test]
    fn insert_checked_prevents_direct_cycle() {
        let mut subst = Substitution::new();
        // Var(0) → Var(0) is a cycle
        let result = subst.insert_checked(0, &Type::Var(0));
        assert!(result.is_err());
    }

    #[test]
    fn insert_checked_prevents_indirect_cycle() {
        let mut subst = Substitution::new();
        subst.insert(1, Type::Var(0)); // Var(1) → Var(0)
        // Now inserting Var(0) → Option(Var(1)) would create a cycle
        let cyclic = Type::Option(Box::new(Type::Var(1)));
        let result = subst.insert_checked(0, &cyclic);
        assert!(result.is_err());
    }

    #[test]
    fn insert_checked_allows_non_cycle() {
        let mut subst = Substitution::new();
        let result = subst.insert_checked(0, &Type::I64);
        assert!(result.is_ok());
        assert_eq!(subst.apply(&Type::Var(0)), Type::I64);
    }

    #[test]
    fn chan_apply_recurses() {
        let mut subst = Substitution::new();
        subst.insert(0, Type::I64);
        let chan = Type::Chan(Box::new(Type::Var(0)));
        assert_eq!(subst.apply(&chan), Type::Chan(Box::new(Type::I64)));
    }

    #[test]
    fn chan_occurs_check() {
        let subst = Substitution::new();
        let chan = Type::Chan(Box::new(Type::Var(0)));
        assert!(subst.occurs(0, &chan));
        assert!(!subst.occurs(1, &chan));
    }

    #[test]
    fn chan_display() {
        assert_eq!(Type::Chan(Box::new(Type::I64)).display(), "chan<i64>");
        assert_eq!(Type::Chan(Box::new(Type::Str)).display(), "chan<str>");
    }
}

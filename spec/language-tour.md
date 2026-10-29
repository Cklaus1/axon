# Axon Language Tour

A hands-on walkthrough of every Axon feature, in order from basic to advanced.

---

## 1. Hello World

```axon
fn main() {
    println("hello, world")
}
```

Every Axon program starts at `main`. No imports required — I/O builtins are always available.

---

## 2. Variables and Types

Variables are declared with `let`. All locals are mutable by default.

```axon
fn main() {
    let x = 42
    let y = 3.14
    let greeting = "hello"
    let flag = true

    x = x + 1          // reassignment — no `let` needed
    println(to_str(x))  // 43
}
```

### Primitive types

| Type  | Example       | Notes                        |
|-------|---------------|------------------------------|
| `i64` | `42`, `-7`    | 64-bit signed integer        |
| `i32` | (implicit)    | 32-bit signed integer        |
| `f64` | `3.14`, `1e10`| 64-bit float, scientific OK  |
| `bool`| `true`/`false`| boolean                      |
| `str` | `"hello"`     | UTF-8 string, immutable      |
| `()`  | (implicit)    | unit / void                  |

---

## 3. Functions

```axon
fn add(a: i64, b: i64) -> i64 {
    a + b               // last expression is the return value
}

fn greet(name: str) -> str {
    "hello, {name}"     // string interpolation
}

fn main() {
    println(to_str(add(3, 4)))   // 7
    println(greet("Axon"))        // hello, Axon
}
```

Functions can be recursive:

```axon
fn fib(n: i64) -> i64 {
    if n <= 1 {
        n
    } else {
        fib(n - 1) + fib(n - 2)
    }
}
```

---

## 4. String Interpolation

Curly braces inside strings are evaluated at runtime:

```axon
fn main() {
    let name = "world"
    let n = 42
    println("hello {name}")              // hello world
    println("the answer is {to_str(n)}") // the answer is 42
    println("pi is {to_str_f64(3.14)}")  // pi is 3.14
}
```

Use `{{` and `}}` to emit literal braces.

---

## 5. Control Flow

### if / else

```axon
fn sign(n: i64) -> str {
    if n > 0 {
        "positive"
    } else {
        if n < 0 {
            "negative"
        } else {
            "zero"
        }
    }
}
```

`if` is an expression — it returns a value.

### while

```axon
fn sum_to(n: i64) -> i64 {
    let acc = 0
    let i = 1
    while i <= n {
        acc = acc + i
        i = i + 1
    }
    acc
}
```

### Logical operators

```axon
fn in_range(x: i64, lo: i64, hi: i64) -> bool {
    x >= lo && x <= hi
}

fn either(a: bool, b: bool) -> bool {
    a || b
}
```

---

## 6. Arithmetic and Operators

```axon
fn main() {
    let a = 17
    let b = 5

    println(to_str(a + b))   // 22
    println(to_str(a - b))   // 12
    println(to_str(a * b))   // 85
    println(to_str(a / b))   // 3  (integer division)
    println(to_str(a % b))   // 2  (modulo)

    let x = 2.5
    let y = 1.5e1            // 15.0
    println(to_str_f64(x + y)) // 17.5
}
```

---

## 7. Structs

```axon
type Point = { x: f64, y: f64 }
type Rect   = { origin: Point, width: f64, height: f64 }

fn area(r: Rect) -> f64 {
    r.width * r.height
}

fn main() {
    let p = Point { x: 1.0, y: 2.0 }
    let r = Rect { origin: p, width: 10.0, height: 5.0 }
    println(to_str_f64(area(r)))  // 50.0
    println(to_str_f64(r.origin.x))  // 1.0
}
```

---

## 8. Enums (Algebraic Data Types)

```axon
type Color = Red | Green | Blue | Custom { r: i64, g: i64, b: i64 }

fn to_hex(c: Color) -> str {
    match c {
        Color::Red             => "#ff0000"
        Color::Green           => "#00ff00"
        Color::Blue            => "#0000ff"
        Color::Custom { r, g, b } => "#{to_str(r)}{to_str(g)}{to_str(b)}"
    }
}

fn main() {
    println(to_hex(Color::Red))
    println(to_hex(Color::Custom { r: 128, g: 0, b: 255 }))
}
```

---

## 9. Match Expressions

`match` is exhaustive — all cases must be covered:

```axon
fn describe(n: i64) -> str {
    match n {
        0 => "zero"
        1 => "one"
        _ => "many"
    }
}
```

Match on structs:

```axon
type Shape = Circle { radius: f64 } | Rect { w: f64, h: f64 }

fn area(s: Shape) -> f64 {
    match s {
        Shape::Circle { radius } => 3.14159 * radius * radius
        Shape::Rect { w, h }     => w * h
    }
}
```

---

## 10. Result<T, E> and Error Handling

Functions that can fail return `Result<T, E>`:

```axon
fn divide(a: i64, b: i64) -> Result<i64, str> {
    if b == 0 {
        Err("division by zero")
    } else {
        Ok(a / b)
    }
}

fn main() {
    match divide(10, 2) {
        Ok(n)  => println(to_str(n))   // 5
        Err(e) => println(e)
    }

    match divide(10, 0) {
        Ok(n)  => println(to_str(n))
        Err(e) => println(e)           // division by zero
    }
}
```

Use `?` to propagate errors early:

```axon
fn parse_and_double(s: str) -> Result<i64, str> {
    let n = parse_int(s)?   // returns Err early if parse fails
    Ok(n * 2)
}
```

---

## 11. Option<T>

```axon
fn find_first_positive(arr: [i64]) -> Option<i64> {
    let i = 0
    while i < len(arr) {
        if arr[i] > 0 {
            return Some(arr[i])
        }
        i = i + 1
    }
    None
}

fn main() {
    let nums = [1, 2, 3]
    match find_first_positive(nums) {
        Some(n) => println(to_str(n))
        None    => println("none found")
    }
}
```

---

## 12. Slices and Arrays

```axon
fn sum(arr: [i64]) -> i64 {
    let total = 0
    let i = 0
    while i < len(arr) {
        total = total + arr[i]
        i = i + 1
    }
    total
}

fn main() {
    let nums = [10, 20, 30, 40, 50]
    println(to_str(sum(nums)))    // 150
    println(to_str(nums[0]))      // 10
}
```

Arrays are values: `let b = a` copies, a parameter `arr: [i64]` gets its own copy
(writes to it stay local), and `arr: &[i64]` is a read-only view (writing through it is
error `E0604`). To modify the caller's array in place, declare the parameter
`&mut [T]` and pass `&mut` of a local variable:

```axon
fn fill(xs: &mut [i64], v: i64) {
    for i in 0..len(xs) {
        xs[i] = v               // reaches the caller's `nums`
    }
}

fn reset(xs: &mut [i64]) {
    fill(&mut xs, 0)            // pass it on: `&mut xs` (or `&xs` to a reader)
}

fn main() {
    let nums = [1, 2, 3]
    let copy = nums
    reset(&mut nums)
    println(to_str(nums[0]))    // 0
    println(to_str(copy[0]))    // 1 - `copy` was taken before, it is its own array
}
```

`&mut` is only allowed as a free function's top-level `[T]` parameter type, and only
of a whole local variable at the call site (`E0605` otherwise). One call may borrow a
variable `&mut` only once and may not also use it in another argument (`E0606`). The
parameter itself can be reassigned (`xs = [..]`), and that also reaches the caller.

---

## 13. Lambdas

```axon
fn apply(f: fn(i64) -> i64, x: i64) -> i64 {
    f(x)
}

fn main() {
    let double = |x| x * 2
    let square = |x| x * x

    println(to_str(apply(double, 5)))  // 10
    println(to_str(apply(square, 5)))  // 25
}
```

A named, non-generic `fn` is a first-class value of type `fn(..) -> ..`: pass it,
bind it, store it in an array or struct field, and call it through any of those.
Effects and purity follow the value — calling `loud` through `let g = loud` is
still `loud`'s effect. A call through a value whose type is known (a fn or
lambda bound by `let`, including a top-level `let`, or a param typed
`fn(..) -> ..`) is checked like a direct call: a wrong argument count is E0305
and a wrong argument type E0306. A local shadows a top-level fn of the same
name, so the local's own type decides a call by that name.

```axon
fn inc(x: i64) -> i64 { x + 1 }
fn dbl(x: i64) -> i64 { x * 2 }
fn apply(f: fn(i64) -> i64, x: i64) -> i64 { f(x) }

fn main() {
    println(to_str(apply(inc, 41)))   // 42
    let ops = [inc, dbl]
    println(to_str(ops[1](21)))       // 42
    let f = ops[0]
    println(to_str(f(41)))            // 42
}
```

A generic fn (`fn id<T>`) or a builtin (`abs_i64`) has no single runtime value
and is refused with E0306; wrap it in a lambda: `|n| abs_i64(n)`. Natively, a
function value (like a lambda) returns through an i64 slot, so one returning
`str`, an array, a tuple, a struct, `Option` or `Result` builds with an E0910
refusal and runs under `axon run`.

---

## 14. Built-in Functions

### I/O
```axon fragment
print("no newline")
println("with newline")
eprint("stderr no newline")
eprintln("stderr with newline")
```

### Conversion
```axon
fn main() {
    println(to_str(42))            // 42
    println(to_str_f64(3.14))      // 3.14
    match parse_int("42") {
        Ok(n) => println(to_str(n))   // 42
        Err(e) => println(e)
    }
    match parse_int("bad") {
        Ok(n) => println(to_str(n))
        Err(e) => println(e)          // could not parse `bad` as a base-10 integer
    }
}
```

### Math
```axon fragment
println(to_str(abs_i32(-5)))          // 5
println(to_str_f64(abs_f64(-3.14)))   // 3.14
println(to_str(min_i32(3, 7)))        // 3
println(to_str(max_i32(3, 7)))        // 7
```

### String
```axon fragment
println(to_str(len("hello")))   // 5
println(to_str(len("héllo")))   // 6 — bytes, not characters
```

---

## 15. Testing

Annotate functions with `@[test]`:

```axon
fn add(a: i64, b: i64) -> i64 { a + b }

@[test]
fn test_add() {
    assert_eq(add(2, 3), 5)
    assert_eq(add(0, 0), 0)
    assert(add(1, 1) == 2)
}

@[test(should_fail)]
fn test_panics_on_bad_input() {
    assert(1 == 2)   // this must panic for the test to pass
}
```

Run with: `axon test myfile.ax`

To grade code you do not trust (for example code a model wrote) against your own suite, put
the candidate's module files in their own directory and seal it:

```sh
AXON_PATH=suite:cand AXON_PATH_EXCLUSIVE=1 AXON_ALLOWED_EFFECTS= \
    axon test suite/accept.ax --json --seal cand
```

Code under `cand` cannot name anything the suite defines, and its own `@[test]`s are not
run. See [`docs/SEALED_TESTS.md`](../docs/SEALED_TESTS.md) and the runnable
[`examples/sealed/`](../examples/sealed/).

---

## 16. AI Annotations

Axon has first-class support for AI/agent annotations. `@[adaptive]` / `@[goal]` drive the optimizer,
`@[verify]` is a runtime-checked postcondition (a breach exits 3), and `@[contained]` is enforced at check
(`spec/axon-for-llms.md` §7, "Capability sandboxing"):

```axon
// The optimizer tunes an @[adaptive] fn: its params are the search space,
// its return value is the score.
@[adaptive]
fn throughput(workers: i64) -> i64 { 1000 - (workers - 12) * (workers - 12) }

// An @[goal] fn takes no params and names the metric; calling it runs the
// search first. `goal_met` is 1 if the best score reached `target`.
@[goal(metric: throughput, target: 1000, max_evals: 100)]
fn tune_workers() -> i64 { goal_met }

// A runtime-checked postcondition: a breach exits 3.
@[verify(value >= 0.0)]
fn critical_calculation(x: f64) -> f64 { x * x }

fn main() {
    println(to_str(tune_workers()))                       // 1
    println(to_str(goal_best_input("throughput", 1000.0))) // 12
    println(to_str_f64(critical_calculation(3.0)))        // 9
}
```

---

## Quick Reference

```
Variables:    let x = 5        x = x + 1
Types:        i64  f64  bool  str  ()
              [T]  Result<T,E>  Option<T>
Functions:    fn f(x: T) -> R { expr }
Structs:      type S = { field: T }  S { field: val }
Enums:        type E = A | B { x: T }  E::A  E::B { x: v }
Control:      if c { } else { }   while c { }   match v { pat => expr }
Errors:       Ok(v)  Err(e)  Some(v)  None  expr?
Strings:      "hello {name}"  "\n\t\\"
Operators:    + - * / %   == != < > <= >=   && ||
Builtins:     println  to_str  to_str_f64  parse_int
              abs_i32  abs_f64  min_i32  max_i32  len
Testing:      @[test]  assert  assert_eq  @[test(should_fail)]
```

"""Phase-2 task class: Axon-specific constructs the model cannot have memorised.

Phase-1-style single-function bug repair is SATURATED for this model — 11 of 12
candidates had a control pass-rate of 0.88-1.00 and most produced one distinct
reply even at temperature 0.7. A saturated task class measures the task class.

These require syntax and semantics specific to Axon. Measured: the model invents
`!i64`, `effect net io`, `CapabilitySandbox::new` — none of which exist. That is
the gap grounded observation is supposed to close, so it is the honest place to
test whether it does.

DROPPED (not broken, so they could measure nothing):
  foreign_for_in  - Axon HAS `for x in xs`; the program compiles.
  mut_binding     - Axon permits reassignment without a `mut` keyword.
  slice_param     - the borrow form was already accepted.
  verify_attr / pure_attr / test_attr_shape
                  - "add this attribute" cannot be judged by a hidden test that
                    runs perfectly well without it. Verifying an attribute needs
                    a different mechanism than running the program.
"""
TASKS = [
    # Every task below is VALIDATED: the broken form must fail its hidden test
    # with a real compiler diagnostic. Five earlier candidates were dropped
    # because they were not broken at all — Axon permits reassignment without a
    # `mut` keyword, and an "add this attribute" task cannot be judged by a
    # hidden test that runs fine without it.
    ("dict_method_syntax",
     "fn lookup(k: str) -> i64 {\n    let d = dict_new()\n    d.set(k, 1)\n    d.get(k)\n}\n",
     "@[test]\nfn t_hidden() { assert_eq(lookup(\"a\"), 1) }\n",
     "`lookup` stores 1 under the key and reads it back. Axon's dictionary operations are not methods."),
    ("str_field_len",
     "fn width(s: str) -> i64 {\n    s.length\n}\n",
     "@[test]\nfn t_hidden() { assert_eq(width(\"abcd\"), 4) }\n",
     "`width` returns the length of the string."),
    ("python_colon_block",
     "fn pick(n: i64) -> i64 {\n    if n > 0:\n        1\n    else:\n        0\n}\n",
     "@[test]\nfn t_hidden() {\n    assert_eq(pick(5), 1)\n    assert_eq(pick(0 - 2), 0)\n}\n",
     "`pick` returns 1 for positive input and 0 otherwise."),
    ("enum_variant_called",
     "type Shape = Circle { r: i64 } | Square { s: i64 }\n\nfn mk(r: i64) -> Shape {\n    Shape::Circle(r)\n}\n\nfn radius(sh: Shape) -> i64 {\n    match sh { Shape::Circle { r } => r  Shape::Square { s } => s }\n}\n",
     "@[test]\nfn t_hidden() { assert_eq(radius(mk(5)), 5) }\n",
     "`mk` builds a Circle with the given radius. Axon enum variants with fields are not constructed like function calls."),

    ("result_propagate",
     "fn parse_twice(s: str) -> i64 {\n    let n = parse_int(s)\n    n * 2\n}\n",
     "@[test]\nfn t_hidden() {\n    match parse_twice(\"21\") { Ok(v) => assert_eq(v, 42)  Err(_e) => assert(false) }\n}\n",
     "`parse_int` returns a Result. `parse_twice` must return `Result<i64, str>` and propagate the error rather than using the Result as a number."),
    ("option_match",
     "fn head(xs: &[i64]) -> i64 {\n    arr_first(xs)\n}\n",
     "@[test]\nfn t_hidden() {\n    let a = [4, 5]\n    assert_eq(head(&a), 4)\n    let e: [i64] = []\n    assert_eq(head(&e), 0)\n}\n",
     "`arr_first` returns an Option. `head` must return 0 for an empty slice by handling both Option cases."),
    ("string_interp",
     "fn greet(name: str, age: i64) -> str {\n    \"hello \" + name + \", age \" + age\n}\n",
     "@[test]\nfn t_hidden() { assert(str_eq(greet(\"ada\", 36), \"hello ada, age 36\")) }\n",
     "`greet` must build the string. Axon's `+` does not join a str and a number."),
]

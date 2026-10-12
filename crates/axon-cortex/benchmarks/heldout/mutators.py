"""Mutation classes for the held-out sets, every one site-aware.

The dev corpus (../ab/corpus.json) plants six classes, each at the FIRST site
in a function body: operator, constant, boolean, argswap, drop-stmt,
comparison (../common.py, ../ab/corpus.py). The files the held-out sets may
use are few and thinly tested (a task needs two checks that notice the
defect), so the first site alone yields too few tasks. `DEV_CLASS_MUTATORS`
are those six shapes again, unchanged in what they edit, but able to plant at
the k-th site rather than only the first; string literals and comments are
never a site.

`NEW_MUTATORS` are eight shapes the dev corpus does not contain:

  swap-branches    the then/else blocks of an `if ... else` exchanged
  flip-comparison  a comparison's DIRECTION reversed (`<`->`>`, `<=`->`>=`);
                   the dev `comparison` class only slips a boundary
  wrong-var        one use of a parameter or local replaced by another name
                   in scope (argswap moves two arguments; this substitutes one)
  wrong-field      one `.field` read replaced by another field of the same
                   declared type
  float-constant   a float literal nudged (last digit + 1); the dev
                   `constant` class bumps integers only
  drop-not         a unary `!` removed
  string-typo      a string literal loses its last character
  operand-swap     the operands of a non-commutative `-`, `/` or `%` swapped

Every mutator here takes `(body, k, ctx)` and returns the k-th distinct site's
`(mutant_body, how)` or None once the sites run out, so a builder can walk
sites until one survives corpus.py's validity checks. As in ../common.py,
being liberal is safe: a mutant that does not compile, or that fewer than two
checks notice, is discarded by `make_task`.
"""

import re

IDENT = r"[A-Za-z_][A-Za-z0-9_]*"


def _code_spans(body):
    """Mask of positions outside string literals and `//` comments."""
    mask = [True] * len(body)
    i, n = 0, len(body)
    while i < n:
        if body.startswith("//", i):
            j = body.find("\n", i)
            j = n if j < 0 else j
            for x in range(i, j):
                mask[x] = False
            i = j
        elif body[i] == '"':
            j = i + 1
            while j < n and body[j] != '"':
                j += 2 if body[j] == "\\" else 1
            for x in range(i, min(j + 1, n)):
                mask[x] = False
            i = j + 1
        else:
            i += 1
    return mask


def _code_matches(pattern, body):
    mask = _code_spans(body)
    return [m for m in re.finditer(pattern, body) if mask[m.start()]]


def _matching(body, o, open_c="{", close_c="}"):
    d = 0
    mask = _code_spans(body)
    for i in range(o, len(body)):
        if not mask[i]:
            continue
        if body[i] == open_c:
            d += 1
        elif body[i] == close_c:
            d -= 1
            if d == 0:
                return i
    return None


def swap_branches(body, k, ctx):
    sites = []
    for m in _code_matches(r"\bif\b", body):
        # the condition ends at the first `{` outside parentheses
        depth, i = 0, m.end()
        mask = _code_spans(body)
        while i < len(body):
            c = body[i]
            if mask[i]:
                if c in "([":
                    depth += 1
                elif c in ")]":
                    depth -= 1
                elif c == "{" and depth == 0:
                    break
            i += 1
        if i >= len(body):
            continue
        a_close = _matching(body, i)
        if a_close is None:
            continue
        em = re.compile(r"\s*else\s*\{").match(body, a_close + 1)
        if not em:
            continue  # no else, or an `else if` chain: no plain pair to swap
        b_open = em.end() - 1
        b_close = _matching(body, b_open)
        if b_close is None:
            continue
        then_b, else_b = body[i + 1:a_close], body[b_open + 1:b_close]
        if then_b.strip() == else_b.strip():
            continue
        sites.append(body[:i + 1] + else_b + body[a_close:b_open + 1] + then_b + body[b_close:])
    return (sites[k], f"swap-branches:#{k}") if k < len(sites) else None


_FLIP = {"<": ">", ">": "<", "<=": ">=", ">=": "<="}


def flip_comparison(body, k, ctx):
    ms = _code_matches(r"(?<![<=>!\-])(<=|>=|<|>)(?![<>\-])", body)
    # `->`/`=>` are excluded by the lookbehind on `-`/`=`; `<`/`>` that open
    # a generic make the mutant fail to type-check and it is discarded
    ms = [m for m in ms if not (m.group(1) == ">" and body[m.start() - 1:m.start()] in "=-")]
    if k >= len(ms):
        return None
    m = ms[k]
    return body[:m.start()] + _FLIP[m.group(1)] + body[m.end():], f"flip:{m.group(1)}->{_FLIP[m.group(1)]}"


def wrong_var(body, k, ctx):
    names = list(dict.fromkeys(ctx["params"] + re.findall(r"\blet\s+(?:mut\s+)?(%s)" % IDENT, body)))
    if len(names) < 2:
        return None
    sites = []
    for m in _code_matches(r"(?<![\w.])(%s)\b(?!\s*[(:])" % IDENT, body):
        nm = m.group(1)
        if nm not in names:
            continue
        pre = body[max(0, m.start() - 8):m.start()]
        if re.search(r"\blet\s+(mut\s+)?$", pre):
            continue  # the binding itself
        after = body[m.end():m.end() + 3]
        if re.match(r"\s*=(?!=)", after):
            continue  # an assignment target: renaming it moves the write
        for alt in names:
            if alt != nm:
                sites.append((m, alt))
    if k >= len(sites):
        return None
    m, alt = sites[k]
    return body[:m.start()] + alt + body[m.end():], f"wrong-var:{m.group(1)}->{alt}"


def wrong_field(body, k, ctx):
    fields = ctx["fields"]  # {field: declared type text}
    sites = []
    for m in _code_matches(r"\.(%s)\b(?!\s*\()" % IDENT, body):
        f = m.group(1)
        if f not in fields:
            continue
        for alt, ty in fields.items():
            if alt != f and ty == fields[f]:
                sites.append((m, alt))
    if k >= len(sites):
        return None
    m, alt = sites[k]
    return body[:m.start() + 1] + alt + body[m.end():], f"wrong-field:{m.group(1)}->{alt}"


def float_constant(body, k, ctx):
    ms = _code_matches(r"(?<![\w.])(\d+)\.(\d+)(?![\w.])", body)
    if k >= len(ms):
        return None
    m = ms[k]
    frac = m.group(2)
    new_frac = str(int(frac) + 1).zfill(len(frac))
    whole = m.group(1)
    if len(new_frac) > len(frac):  # .9 -> 1.0
        whole, new_frac = str(int(whole) + 1), "0" * len(frac)
    new = f"{whole}.{new_frac}"
    return body[:m.start()] + new + body[m.end():], f"float:{m.group(0)}->{new}"


def drop_not(body, k, ctx):
    ms = _code_matches(r"!(?!=)", body)
    if k >= len(ms):
        return None
    m = ms[k]
    return body[:m.start()] + body[m.end():], "drop-not"


def string_typo(body, k, ctx):
    mask = _code_spans(body)
    sites = []
    i = 0
    while i < len(body):
        if body[i] == '"' and (i == 0 or mask[i - 1] or body[i - 1] != "\\"):
            j = i + 1
            while j < len(body) and body[j] != '"':
                j += 2 if body[j] == "\\" else 1
            lit = body[i + 1:j]
            # plain text only: no interpolation braces, no escapes
            if len(lit) >= 2 and not re.search(r"[{}\\]", lit) and not body[i - 2:i] == "//":
                sites.append((i, j))
            i = j + 1
        elif body.startswith("//", i):
            j = body.find("\n", i)
            i = len(body) if j < 0 else j
        else:
            i += 1
    if k >= len(sites):
        return None
    i, j = sites[k]
    return body[:j - 1] + body[j:], f"string-typo:{body[i + 1:j]!r}"


_OPERAND = r"(?:%s(?:\.%s)*|\d+(?:\.\d+)?)" % (IDENT, IDENT)


def operand_swap(body, k, ctx):
    ms = _code_matches(r"(?<![\w.)\]])(%s)\s*([-/%%])\s*(%s)(?![\w.(\[])" % (_OPERAND, _OPERAND), body)
    ms = [m for m in ms if m.group(1) != m.group(3)
          and body[max(0, m.start() - 1):m.start()] not in ("-", ">")
          and body[m.end():m.end() + 1] != ">"]
    if k >= len(ms):
        return None
    m = ms[k]
    return (body[:m.start()] + f"{m.group(3)} {m.group(2)} {m.group(1)}" + body[m.end():],
            f"operand-swap:{m.group(2)}")


NEW_MUTATORS = [
    ("swap-branches", swap_branches),
    ("flip-comparison", flip_comparison),
    ("wrong-var", wrong_var),
    ("wrong-field", wrong_field),
    ("float-constant", float_constant),
    ("drop-not", drop_not),
    ("string-typo", string_typo),
    ("operand-swap", operand_swap),
]


def context(src, name):
    """What the site-aware mutators need beyond the body."""
    m = re.search(r"\bfn %s\(([^)]*)\)" % re.escape(name), src)
    params = re.findall(r"(?:^|,)\s*(?:mut\s+)?(%s)\s*:" % IDENT, m.group(1)) if m else []
    fields = {}
    for d in re.finditer(r"(?:\btype\s+%s\s*(?:<[^>]*>)?\s*=\s*|\bstruct\s+%s\s*(?:<[^>]*>)?\s*)\{([^{}]*)\}"
                         % (IDENT, IDENT), src):
        for f in re.finditer(r"(%s)\s*:\s*([^,\n]+)" % IDENT, d.group(1)):
            fields.setdefault(f.group(1), f.group(2).strip())
    return {"params": params, "fields": fields}


# ------------------------------------------------- the dev classes, any site

_SWAPS = [("+", "-"), ("*", "+"), ("<=", "<"), (">=", ">"), ("&&", "||")]  # ../common.py SWAPS


def _nth(sites, k):
    return sites[k] if k < len(sites) else None


def operator(body, k, ctx):
    sites = []
    for old, new in _SWAPS:
        for m in _code_matches(re.escape(old), body):
            sites.append((m.start(), m.end(), new, f"op:{old}->{new}"))
    s = _nth(sites, k)
    return (body[:s[0]] + s[2] + body[s[1]:], s[3]) if s else None


def constant(body, k, ctx):
    s = _nth(_code_matches(r"(?<![\w.])(\d+)(?![\w.])", body), k)
    if not s:
        return None
    v = int(s.group(1))
    return body[:s.start()] + str(v + 1) + body[s.end():], f"const:{v}->{v + 1}"


def boolean(body, k, ctx):
    s = _nth(_code_matches(r"(?<![\w])(true|false)(?![\w])", body), k)
    if not s:
        return None
    new = "false" if s.group(1) == "true" else "true"
    return body[:s.start()] + new + body[s.end():], f"bool:{s.group(1)}->{new}"


def argswap(body, k, ctx):
    ms = [m for m in _code_matches(r"\b([a-z_][\w]*)\(\s*([a-z_][\w.]*)\s*,\s*([a-z_][\w.]*)\s*\)", body)
          if m.group(2) != m.group(3)]
    s = _nth(ms, k)
    if not s:
        return None
    return (body[:s.start()] + f"{s.group(1)}({s.group(3)}, {s.group(2)})" + body[s.end():],
            f"argswap:{s.group(1)}")


def drop_stmt(body, k, ctx):
    lines = body.split("\n")
    sites = []
    for i, l in enumerate(lines):
        t = l.strip()
        if not t or t.startswith("//") or t.startswith("let ") or t.startswith("own "):
            continue
        if any(c in t for c in "{}") or "=" not in t:
            continue
        if i == len(lines) - 2:  # the tail expression is the return value
            continue
        sites.append(i)
    i = _nth(sites, k)
    return ("\n".join(lines[:i] + lines[i + 1:]), "drop-stmt") if i is not None else None


def comparison(body, k, ctx):
    sites = []
    for pat, new, tag in ((r"==", "!=", "cmp:==->!="),
                          (r"(?<![<=>!\-])<(?![<=\-])", "<=", "cmp:<-><="),
                          (r"(?<![<=>!\-])>(?![>=])", ">=", "cmp:>->>=")):
        for m in _code_matches(pat, body):
            sites.append((m.start(), m.end(), new, tag))
    s = _nth(sites, k)
    return (body[:s[0]] + s[2] + body[s[1]:], s[3]) if s else None


DEV_CLASS_MUTATORS = [
    ("operator", operator),
    ("constant", constant),
    ("boolean", boolean),
    ("argswap", argswap),
    ("drop-stmt", drop_stmt),
    ("comparison", comparison),
]

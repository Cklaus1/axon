# Agent Language Bootstrap Pack

A bootstrap pack allows an AI with little or no pretrained knowledge of a language/toolchain to become productive from a compact local corpus.

Required contents: language/compiler identity, core syntax/type/module/memory/error/build summaries, common idioms, common failures, canonical examples, stdlib/source locations, conformance locations, migration notes, and truth-resolution policy.

Recommended layout:

```text
/language-context/{LANGUAGE,QUICK_REFERENCE,COMMON_PATTERNS,COMMON_ERRORS,MIGRATIONS}.md
/examples
/stdlib-source
/how-to
/conformance
/bootstrap.json
```

When uncertain, an agent SHOULD consult the current pack/source/compiler before inventing an API from pretrained memory.

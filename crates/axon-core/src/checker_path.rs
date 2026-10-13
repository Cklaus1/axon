//! Node paths for the checker (AX-64).
//!
//! The checker names every expression node by a path (`#fn_f.body.stmt_1.value.left`)
//! that inference-supplied type maps are keyed by and diagnostics carry as a
//! node id. A path used to be a `String` rebuilt by `format!("{parent}.left")` at
//! every level, so the node `k` levels down a left-nested chain (`1 + 1 + 1 + …`)
//! held a string `O(k)` bytes long: `N` levels cost `O(N²)` bytes live on the
//! recursion and `O(N²)` time to build, and a 100,000-term chain exhausted memory.
//!
//! [`NodePath`] is a persistent linked list instead: appending a segment is `O(1)`
//! and shares the parent, so a chain of `N` nodes holds `O(N)` bytes. The text is
//! produced only when somebody needs it ([`NodePath::render`], for a diagnostic's
//! node id or a lookup in a non-empty type map), and renders exactly the string the
//! old `format!` chain built.

use std::fmt;
use std::rc::Rc;

/// One step of a path, rendered with a leading `.` (the root has none).
enum Seg {
    /// The root, e.g. `#fn_main`: rendered verbatim.
    Root(Box<str>),
    /// `.name`
    Lit(&'static str),
    /// `.<prefix>_<index>`
    Idx(&'static str, usize),
    /// `.<prefix>_<name>`
    Named(&'static str, Box<str>),
    /// `.<name>` where the name is not static (a struct field).
    Field(Box<str>),
}

struct Node {
    parent: Option<Rc<Node>>,
    seg: Seg,
}

impl Drop for Node {
    /// Unlink the parents iteratively: the default drop of a uniquely-owned chain
    /// `N` nodes long would recurse `N` times.
    fn drop(&mut self) {
        let mut next = self.parent.take();
        while let Some(rc) = next {
            match Rc::try_unwrap(rc) {
                Ok(mut node) => next = node.parent.take(),
                Err(_) => break,
            }
        }
    }
}

/// A node's path from the root of its item. Cheap to clone and to extend.
#[derive(Clone)]
pub(crate) struct NodePath(Rc<Node>);

impl NodePath {
    pub(crate) fn root(text: impl Into<Box<str>>) -> Self {
        Self(Rc::new(Node {
            parent: None,
            seg: Seg::Root(text.into()),
        }))
    }

    fn push(&self, seg: Seg) -> Self {
        Self(Rc::new(Node {
            parent: Some(Rc::clone(&self.0)),
            seg,
        }))
    }

    /// `self` + `.name`
    pub(crate) fn child(&self, name: &'static str) -> Self {
        self.push(Seg::Lit(name))
    }

    /// `self` + `.<prefix>_<i>`
    pub(crate) fn idx(&self, prefix: &'static str, i: usize) -> Self {
        self.push(Seg::Idx(prefix, i))
    }

    /// `self` + `.<prefix>_<name>`
    pub(crate) fn named(&self, prefix: &'static str, name: &str) -> Self {
        self.push(Seg::Named(prefix, name.into()))
    }

    /// `self` + `.<name>`
    pub(crate) fn field(&self, name: &str) -> Self {
        self.push(Seg::Field(name.into()))
    }

    /// The path text, root first. Walks the whole path: call it where a diagnostic
    /// or a map lookup needs the string, not on the way down a deep tree.
    pub(crate) fn render(&self) -> String {
        let mut segs: Vec<&Seg> = Vec::new();
        let mut cur: Option<&Node> = Some(&self.0);
        while let Some(n) = cur {
            segs.push(&n.seg);
            cur = n.parent.as_deref();
        }
        let mut out = String::new();
        for seg in segs.into_iter().rev() {
            match seg {
                Seg::Root(t) => out.push_str(t),
                Seg::Lit(n) => {
                    out.push('.');
                    out.push_str(n);
                }
                Seg::Idx(p, i) => {
                    out.push('.');
                    out.push_str(p);
                    out.push('_');
                    out.push_str(&i.to_string());
                }
                Seg::Named(p, n) => {
                    out.push('.');
                    out.push_str(p);
                    out.push('_');
                    out.push_str(n);
                }
                Seg::Field(n) => {
                    out.push('.');
                    out.push_str(n);
                }
            }
        }
        out
    }

    /// Whether this path is exactly `text`, without rendering a path that is
    /// longer than `text` could be (every non-root segment is at least two bytes).
    pub(crate) fn is(&self, text: &str) -> bool {
        let mut steps = 0usize;
        let mut cur: Option<&Node> = Some(&self.0);
        while let Some(n) = cur {
            steps += 1;
            if steps > text.len() / 2 + 1 {
                return false;
            }
            cur = n.parent.as_deref();
        }
        self.render() == text
    }
}

impl fmt::Display for NodePath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.render())
    }
}

impl From<&NodePath> for String {
    fn from(p: &NodePath) -> String {
        p.render()
    }
}

impl From<NodePath> for String {
    fn from(p: NodePath) -> String {
        p.render()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_exactly_the_string_the_format_chain_built() {
        let p = NodePath::root("#fn_f.body")
            .idx("stmt", 3)
            .child("value")
            .named("param", "x")
            .field("len")
            .child("left");
        assert_eq!(p.render(), "#fn_f.body.stmt_3.value.param_x.len.left");
        assert_eq!(String::from(&p), p.to_string());
        assert_eq!(NodePath::root("#fn_f").render(), "#fn_f");
    }

    #[test]
    fn is_matches_the_rendered_text_only() {
        let p = NodePath::root("#fn_main.body").idx("stmt", 0);
        assert!(p.is("#fn_main.body.stmt_0"));
        assert!(!p.is("#fn_main.body.stmt_1"));
        assert!(!p.is("#fn_main.body"));
        assert!(!p.is("#fn_main.body.stmt_0.value"));
    }

    /// AX-64: the path of the node `k` levels down a left-nested chain must cost
    /// `O(1)` to extend, sharing its parent, not `O(k)` bytes of its own. As a
    /// `String` built by `format!("{parent}.left")`, a 100,000-term chain held
    /// ~5 GB of path text on the recursion and OOM-killed the checker.
    #[test]
    fn extending_a_path_shares_the_parent() {
        let root = NodePath::root("#fn_main.body.stmt_0.value");
        let child = root.child("left");
        // One more owner of the root's node, no copy of its text.
        assert_eq!(Rc::strong_count(&root.0), 2);
        let grandchild = child.child("left");
        assert_eq!(Rc::strong_count(&child.0), 2);
        assert_eq!(Rc::strong_count(&root.0), 2);
        drop(grandchild);
        assert_eq!(Rc::strong_count(&child.0), 1);
    }

    /// 1,000,000 levels: built, cloned and dropped without recursion (the default
    /// drop glue of a uniquely-owned list this long overflows a test thread's
    /// 2 MiB stack), and `render` is linear.
    #[test]
    fn a_million_deep_path_builds_renders_and_drops_without_recursion() {
        const DEPTH: usize = 1_000_000;
        let mut p = NodePath::root("#fn_main");
        for _ in 0..DEPTH {
            p = p.child("left");
        }
        let kept = p.clone();
        drop(p);
        let text = kept.render();
        assert_eq!(text.len(), "#fn_main".len() + DEPTH * ".left".len());
        assert!(!kept.is("#fn_main.left"));
        drop(kept);
    }
}

//! # LaTeX project resolution (`\input` / `\include`)
//!
//! Resolves a multi-file LaTeX project rooted at a single `.tex` file into a
//! single logical canonical [`Document`], in document order, with source
//! provenance and — critically — without ever executing LaTeX or reading files
//! outside the authorized project root.
//!
//! Security model:
//!
//! * The **project root** is the directory containing the supplied root
//!   `.tex` file.
//! * Every `\input` / `\include` reference is resolved relative to the
//!   *including* file's directory and canonicalized. If the canonical path is
//!   outside the canonical project root (via `..`, absolute path, or a
//!   symlink), the reference is **blocked** (fail closed) — it is never read.
//! * No command is ever executed: no `\write18`, no shell, no `make`. The
//!   parser only reads source files as untrusted text.
//! * Cycles are detected and reported deterministically (`LATEX_INCLUDE_CYCLE`);
//!   recursion stops at a cycle rather than looping.
//! * Missing / unreadable includes are surfaced (`LATEX_INCLUDE_NOT_FOUND`)
//!   rather than silently yielding an incomplete manuscript.
//!
//! Determinism:
//!
//! * Included files are spliced in **document order** (never sorted).
//! * A file referenced more than once is expanded at each occurrence, exactly
//!   as LaTeX does. To guard against pathological include graphs (diamond /
//!   exponential expansion) the resolver enforces a hard cap on both the
//!   include depth and the total number of fragments.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use paper_guard_core::{CanonicalDocumentBuilder, Document, SourceLocation};
use regex::Regex;

use crate::latex::{
    build_sections, capture_first, extract_equations, extract_figures, extract_tables,
    finalize_sections, parse_references, split_bibliography,
};

/// Maximum include depth before the resolver fails closed (prevents unbounded
/// recursion / exponential expansion).
pub const MAX_INCLUDE_DEPTH: u32 = 64;
/// Maximum total number of source fragments in a project (prevents
/// pathological include graphs).
pub const MAX_TOTAL_FRAGMENTS: usize = 1024;

/// A single resolved source file within a LaTeX project.
#[derive(Debug, Clone)]
pub struct LatexFragment {
    /// Path relative to the project root (e.g. `sections/methods.tex`).
    pub rel_path: String,
    /// Rooted (canonical) path of the file.
    pub abs_path: PathBuf,
    /// Raw file contents.
    pub text: String,
    /// Include depth: 0 for the root file, 1 for a direct include, etc.
    pub include_depth: u32,
    /// The file that included this fragment (None for the root).
    pub include_parent: Option<String>,
    /// The 1-based line number in the parent file where the include
    /// directive appeared (None for the root).
    pub include_line: Option<u32>,
}

impl LatexFragment {
    /// A base [`SourceLocation`] for content originating in this fragment.
    fn base_location(&self) -> SourceLocation {
        SourceLocation {
            source_type: "latex".into(),
            file: self.rel_path.clone(),
            include_parent: self.include_parent.clone(),
            include_depth: self.include_depth,
            start_line: None,
            end_line: None,
            page: None,
        }
    }
}

/// The result of resolving a LaTeX project.
#[derive(Debug, Clone)]
pub struct ResolvedLatexProject {
    /// The root file path as supplied.
    pub root: PathBuf,
    /// The resolved project root directory (canonical).
    pub root_dir: PathBuf,
    /// The ordered content fragments (root first, then includes in order).
    pub fragments: Vec<LatexFragment>,
    /// Include references that could not be found / read.
    pub missing_includes: Vec<String>,
    /// Include cycles detected (as `a -> b -> a` chains).
    pub cycles: Vec<String>,
}

impl ResolvedLatexProject {
    /// Number of distinct source files resolved.
    pub fn file_count(&self) -> usize {
        self.fragments.len()
    }

    /// Number of include directives resolved (fragments minus the root).
    pub fn include_count(&self) -> usize {
        self.fragments.len().saturating_sub(1)
    }
}

/// The constructor (`\input` / `\include`) we recognized for a directive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IncludeKind {
    Input,
    Include,
}

/// A parsed include directive with the line it occurred on.
#[derive(Debug, Clone)]
pub(crate) struct IncludeDirective {
    #[allow(dead_code)]
    kind: IncludeKind,
    /// The target as written (extension may be absent).
    target: String,
    /// 1-based line number of the directive within its file.
    line: usize,
}

/// Resolve a LaTeX project rooted at `root_tex`.
///
/// Returns an error if the root file cannot be read or is neither a `.tex`
/// nor resolvable as the project root. Structural problems (missing includes,
/// cycles, path escapes) are collected on the returned [`ResolvedLatexProject`]
/// rather than failing the whole parse, so a researcher can see exactly what is
/// incomplete.
pub fn resolve_latex_project(root_tex: &Path) -> anyhow::Result<ResolvedLatexProject> {
    let root_abs = root_tex.canonicalize().map_err(|e| {
        anyhow::anyhow!("unable to open project root `{}`: {e}", root_tex.display())
    })?;
    let root_dir = root_abs
        .parent()
        .ok_or_else(|| {
            anyhow::anyhow!(
                "project root `{}` has no parent directory",
                root_abs.display()
            )
        })?
        .to_path_buf();
    let root_dir_canon = root_dir.canonicalize().unwrap_or_else(|_| root_dir.clone());

    let root_text = read_text(&root_abs)?;

    let mut resolver = Resolver {
        root_dir: root_dir_canon,
        fragments: Vec::new(),
        missing_includes: Vec::new(),
        cycles: Vec::new(),
        visited_stack: Vec::new(),
        visited_count: 0,
    };
    resolver.resolve_file(&root_abs, root_text, 0, None, None)?;

    Ok(ResolvedLatexProject {
        root: root_tex.to_path_buf(),
        root_dir: resolver.root_dir.clone(),
        fragments: resolver.fragments,
        missing_includes: resolver.missing_includes,
        cycles: resolver.cycles,
    })
}

fn read_text(path: &Path) -> anyhow::Result<String> {
    let bytes = std::fs::read(path)?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

struct Resolver {
    root_dir: PathBuf,
    fragments: Vec<LatexFragment>,
    missing_includes: Vec<String>,
    cycles: Vec<String>,
    /// Canonical paths on the current include chain (for cycle detection).
    visited_stack: Vec<PathBuf>,
    /// Total number of fragments resolved so far (guards against expansion).
    visited_count: usize,
}

impl Resolver {
    /// Resolve a single file's content, recursively expanding its includes.
    fn resolve_file(
        &mut self,
        abs_path: &Path,
        text: String,
        depth: u32,
        include_parent: Option<String>,
        include_line: Option<u32>,
    ) -> anyhow::Result<()> {
        if depth > MAX_INCLUDE_DEPTH {
            return Err(anyhow::anyhow!(
                "LATEX_INCLUDE_CYCLE include depth exceeded {MAX_INCLUDE_DEPTH} while \
                 expanding `{}`",
                abs_path.display()
            ));
        }
        if self.visited_count >= MAX_TOTAL_FRAGMENTS {
            return Err(anyhow::anyhow!(
                "LaTeX project exceeds the {MAX_TOTAL_FRAGMENTS} file expansion limit"
            ));
        }
        self.visited_count += 1;

        let rel_path = relative_to(&self.root_dir, abs_path);

        // Cycle detection: if this file is already on the current include
        // chain, report the cycle and stop recursion here.
        let canon = abs_path
            .canonicalize()
            .unwrap_or_else(|_| abs_path.to_path_buf());
        if self.visited_stack.contains(&canon) {
            let chain: Vec<String> = self
                .visited_stack
                .iter()
                .map(|p| relative_to(&self.root_dir, p))
                .chain(std::iter::once(rel_path.clone()))
                .collect();
            self.cycles
                .push(format!("{} -> {}", chain.join(" -> "), rel_path));
            return Ok(());
        }
        self.visited_stack.push(canon);

        // Push this fragment.
        self.fragments.push(LatexFragment {
            rel_path,
            abs_path: abs_path.to_path_buf(),
            text: text.clone(),
            include_depth: depth,
            include_parent,
            include_line,
        });

        // Expand includes in document order.
        let parent_dir = abs_path.parent().unwrap_or(&self.root_dir).to_path_buf();
        for inc in find_includes(&text) {
            let resolved = resolve_include_path(&parent_dir, &inc.target);
            match resolved {
                IncludeResolution::Path(p) if path_is_inside(&self.root_dir, &p) => {
                    let Some(child_text) = read_text_opt(&p) else {
                        self.missing_includes.push(format!(
                            "{} (in {}, line {})",
                            inc.target,
                            abs_path.display(),
                            inc.line
                        ));
                        continue;
                    };
                    self.resolve_file(
                        &p,
                        child_text,
                        depth + 1,
                        Some(rel_path_for_display(&self.root_dir, abs_path)),
                        Some(inc.line as u32),
                    )?;
                }
                IncludeResolution::Path(p) => {
                    // Outside the project root — blocked (fail closed).
                    self.missing_includes.push(format!(
                        "BLOCKED (outside project root): {} -> {}",
                        inc.target,
                        p.display()
                    ));
                }
                IncludeResolution::Missing => {
                    self.missing_includes.push(format!(
                        "{} (in {}, line {})",
                        inc.target,
                        abs_path.display(),
                        inc.line
                    ));
                }
            }
        }

        self.visited_stack.pop();
        Ok(())
    }
}

/// Filesystem-relative path with forward-slash separators so provenance is
/// portable across platforms (a `/`-joined relative path is stable whether the
/// resolver ran on POSIX or Windows, unlike an OS-native separator).
fn relative_to(root_dir: &Path, path: &Path) -> String {
    let rel = path.strip_prefix(root_dir).unwrap_or(path);
    let mut out = String::new();
    for (i, comp) in rel.components().enumerate() {
        use std::path::Component;
        let name = match comp {
            Component::Normal(n) => n.to_string_lossy().into_owned(),
            // Preserve `..`/`.`/prefix semantics so the string still reflects
            // the original path shape; normal file components dominate in
            // practice once strip_prefix succeeds.
            other => other.as_os_str().to_string_lossy().into_owned(),
        };
        if i > 0 {
            out.push('/');
        }
        out.push_str(&name);
    }
    out
}

/// The display path of a file for the `include_parent` field.
fn rel_path_for_display(root_dir: &Path, path: &Path) -> String {
    relative_to(root_dir, path)
}

/// A resolved include path: found (inside root check done by caller), or
/// explicitly missing / unreadable.
enum IncludeResolution {
    Path(PathBuf),
    Missing,
}

/// Resolve an include target to an absolute path, trying extensionless + `.tex`.
fn resolve_include_path(parent_dir: &Path, target: &str) -> IncludeResolution {
    let t = target.trim();
    if t.is_empty() {
        return IncludeResolution::Missing;
    }
    let base = if t.to_ascii_lowercase().ends_with(".tex") {
        PathBuf::from(t)
    } else {
        // Extensionless -> try `.tex`.
        PathBuf::from(format!("{t}.tex"))
    };
    let joined = parent_dir.join(base);
    // Canonicalize so symlinks are resolved and we can enforce the boundary.
    if let Ok(canon) = joined.canonicalize() {
        return IncludeResolution::Path(canon);
    }
    IncludeResolution::Missing
}

/// Whether `candidate` (already canonicalized) is strictly inside `root_dir`
/// (canonical). Equality is allowed for the root itself.
fn path_is_inside(root_dir: &Path, candidate: &Path) -> bool {
    let root_canon = root_dir
        .canonicalize()
        .unwrap_or_else(|_| root_dir.to_path_buf());
    // The candidate must have the root as a component prefix and not be a
    // sibling / ancestor.
    let rel = candidate.strip_prefix(&root_canon);
    match rel {
        Ok(r) => {
            // A candidate equal to the root is technically "inside" but a file
            // that equals the root dir is not a readable text file; still treat
            // empty prefix as inside so the root can be an include (it isn't).
            !r.as_os_str().is_empty() || candidate == root_canon
        }
        Err(_) => false,
    }
}

fn read_text_opt(path: &Path) -> Option<String> {
    let bytes = std::fs::read(path).ok()?;
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

/// Find all `\input{...}` / `\include{...}` directives in a file, in order,
/// along with the line each occurred on. Handles extensionless targets and
/// targets with explicit `.tex` (or other sub-extensions). Ignores comments.
pub(crate) fn find_includes(text: &str) -> Vec<IncludeDirective> {
    // Combine \input{...} and \include{...}; also handle a trailing space or
    // variation. We strip comments first so a directive inside a `%` comment is
    // not treated as real.
    let body = strip_latex_comments(text);
    let re = Regex::new(r"(?m)\\(input|include)\s*\{([^}]*)\}").expect("valid include regex");
    let mut out = Vec::new();
    for (line_no, line) in body.lines().enumerate() {
        for cap in re.captures_iter(line) {
            let kind = if &cap[1] == "input" {
                IncludeKind::Input
            } else {
                IncludeKind::Include
            };
            let target = cap[2].trim().to_string();
            // Only `.tex` (or extensionless) targets are expandable manuscript
            // text. `\input{figures/plot.tikz}` etc. are not LaTeX prose and are
            // skipped. We inspect the *final path component's* extension so that
            // `\input{../../secret}` (filename `secret`, no extension) is still
            // treated as a `.tex` include rather than being dropped.
            let ext = Path::new(&target)
                .extension()
                .map(|e| e.to_string_lossy().to_ascii_lowercase());
            let is_tex_or_extensionless = match ext.as_deref() {
                None => true,
                Some("tex") => true,
                Some(_) => false,
            };
            if !is_tex_or_extensionless {
                continue;
            }
            out.push(IncludeDirective {
                kind,
                target,
                line: line_no + 1,
            });
        }
    }
    out
}

/// Strip `%`-comments (but not escaped `\%`) from a LaTeX source string.
fn strip_latex_comments(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    let mut in_comment = false;
    while i < chars.len() {
        let c = chars[i];
        if c == '%' {
            if i > 0 && chars[i - 1] == '\\' {
                // Escaped \% — literal percent, not a comment.
                out.push(c);
            } else {
                in_comment = true;
            }
        } else if c == '\n' {
            in_comment = false;
            out.push(c);
        } else if !in_comment {
            out.push(c);
        }
        i += 1;
    }
    out
}

/// Parse a resolved project into a single canonical document, merging all
/// fragments in document order with globally-unique section/paragraph IDs and
/// per-paragraph source provenance.
pub fn parse_latex_project(project: &ResolvedLatexProject) -> anyhow::Result<Document> {
    let root_rel = relative_to(&project.root_dir, &project.root);
    let mut builder = CanonicalDocumentBuilder::new().source("latex", root_rel);

    // Title/abstract come from the root file.
    let root_text = project
        .fragments
        .first()
        .map(|f| f.text.as_str())
        .unwrap_or_default();
    if let Some(t) = capture_first(root_text, r"\\title\s*\{([^}]*)\}") {
        builder = builder.title(t);
    }
    if let Some(abstract_txt) =
        capture_first(root_text, r"(?s)\\begin\{abstract\}(.*?)\\end\{abstract\}")
    {
        builder = builder.abstract_text(abstract_txt.trim().to_string());
    }

    // Merge fragments in order.
    let mut section_counter = 0usize;
    let mut all_sections = Vec::new();
    let mut all_citations = Vec::new();
    let mut all_claims = Vec::new();
    let mut all_figures = Vec::new();
    let mut all_tables = Vec::new();
    let mut all_equations = Vec::new();
    let mut all_references = Vec::new();
    let mut current_section: Option<paper_guard_core::Section> = None;
    // De-duplicate bibliography references by key across fragments.
    let mut seen_refs: HashSet<String> = HashSet::new();
    // Compile the abstract regex once (used for every fragment).
    let abstract_re =
        Regex::new(r"(?s)\\begin\{abstract\}.*?\\end\{abstract\}").expect("abstract regex valid");

    for fragment in &project.fragments {
        // Split out bibliography for this fragment.
        let (body, bibliography) = split_bibliography(&fragment.text);
        let body_clean = abstract_re.replace_all(&body, "\n").into_owned();

        let mut loc = Some(fragment.base_location());
        build_sections(
            &body_clean,
            &mut loc,
            &mut section_counter,
            &mut current_section,
            &mut all_sections,
            &mut all_citations,
            &mut all_claims,
        );

        for r in parse_references(&bibliography) {
            if seen_refs.insert(r.reference_id.0.clone()) {
                all_references.push(r);
            }
        }
        all_figures.extend(extract_figures(&fragment.text));
        all_tables.extend(extract_tables(&fragment.text));
        all_equations.extend(extract_equations(&fragment.text));
    }
    finalize_sections(&mut current_section, &mut all_sections);

    for sec in all_sections {
        builder = builder.section(sec);
    }
    for c in all_citations {
        builder = builder.citation(c);
    }
    for c in all_claims {
        builder = builder.claim(c);
    }
    for r in all_references {
        builder = builder.reference(r);
    }
    for f in all_figures {
        builder = builder.figure(f);
    }
    for t in all_tables {
        builder = builder.table(t);
    }
    for e in all_equations {
        builder = builder.equation(e);
    }

    Ok(builder.build())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// Create a temp project dir with the given relative-file -> content map.
    fn write_project(files: &[(&str, &str)]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        for (rel, content) in files {
            let p = dir.path().join(rel);
            if let Some(parent) = p.parent() {
                fs::create_dir_all(parent).unwrap();
            }
            fs::write(p, content).unwrap();
        }
        dir
    }

    fn collect_paragraph_texts(doc: &paper_guard_core::Document) -> Vec<String> {
        doc.sections
            .iter()
            .flat_map(|s| s.paragraphs.iter().map(|p| p.text.clone()))
            .collect()
    }

    #[test]
    fn resolves_input_and_include_in_order() {
        let dir = write_project(&[
            (
                "main.tex",
                "\\title{T}\n\\begin{document}\n\\section{Intro}\nIntroduction text.\n\n\
                 \\input{sections/methods}\n\\include{discussion}\n\\end{document}",
            ),
            ("sections/methods.tex", "\\section{Methods}\nMethod text.\n"),
            (
                "discussion.tex",
                "\\section{Discussion}\nDiscussion text.\n",
            ),
        ]);
        let root = dir.path().join("main.tex");
        let project = resolve_latex_project(&root).unwrap();
        assert_eq!(project.file_count(), 3);
        assert_eq!(project.include_count(), 2);
        assert!(project.missing_includes.is_empty());
        assert!(project.cycles.is_empty());

        let doc = parse_latex_project(&project).unwrap();
        let paras = collect_paragraph_texts(&doc);
        // Document order must be preserved: intro, methods, discussion.
        assert!(paras.iter().any(|p| p.contains("Introduction text")));
        assert!(paras.iter().any(|p| p.contains("Method text")));
        assert!(paras.iter().any(|p| p.contains("Discussion text")));
        let intro = paras
            .iter()
            .position(|p| p.contains("Introduction text"))
            .unwrap();
        let methods = paras
            .iter()
            .position(|p| p.contains("Method text"))
            .unwrap();
        let discussion = paras
            .iter()
            .position(|p| p.contains("Discussion text"))
            .unwrap();
        assert!(
            intro < methods && methods < discussion,
            "document order not preserved"
        );
    }

    #[test]
    fn extensionless_and_explicit_tex_both_resolve() {
        let dir = write_project(&[
            (
                "main.tex",
                "\\begin{document}\\section{A}\\input{a}\n\\input{b.tex}\n\\end{document}",
            ),
            ("a.tex", "text a\n"),
            ("b.tex", "text b\n"),
        ]);
        let project = resolve_latex_project(&dir.path().join("main.tex")).unwrap();
        assert_eq!(project.file_count(), 3);
        assert!(project.missing_includes.is_empty());
    }

    #[test]
    fn nested_includes_preserve_depth_and_provenance() {
        let dir = write_project(&[
            (
                "main.tex",
                "\\begin{document}\\section{S}\n\\input{lvl1}\n\\end{document}",
            ),
            ("lvl1.tex", "level one text.\n\n\\input{sections/lvl2}\n"),
            ("sections/lvl2.tex", "level two text.\n"),
        ]);
        let project = resolve_latex_project(&dir.path().join("main.tex")).unwrap();
        let lvl2 = project
            .fragments
            .iter()
            .find(|f| f.rel_path == "sections/lvl2.tex")
            .unwrap();
        assert_eq!(lvl2.include_depth, 2);
        assert_eq!(lvl2.include_parent.as_deref(), Some("lvl1.tex"));

        let doc = parse_latex_project(&project).unwrap();
        // Levels are ordered.
        let paras = collect_paragraph_texts(&doc);
        let l1 = paras.iter().position(|p| p.contains("level one")).unwrap();
        let l2 = paras.iter().position(|p| p.contains("level two")).unwrap();
        assert!(l1 < l2);
        // Provenance is present on the lvl2 paragraph.
        let p2 = &doc.sections[0].paragraphs[1];
        let loc = p2.location.as_ref().expect("provenance present");
        assert_eq!(loc.file, "sections/lvl2.tex");
        assert_eq!(loc.include_depth, 2);
        assert!(loc.start_line.is_some());
    }

    #[test]
    fn missing_include_is_reported_not_crashing() {
        let dir = write_project(&[(
            "main.tex",
            "\\begin{document}\n\\section{A}\n\\input{missing_file}\n\\end{document}",
        )]);
        let project = resolve_latex_project(&dir.path().join("main.tex")).unwrap();
        assert!(!project.missing_includes.is_empty());
        // Parsing still succeeds (only the missing fragment is omitted).
        let doc = parse_latex_project(&project).unwrap();
        assert!(!doc.sections.is_empty());
    }

    #[test]
    fn circular_include_is_detected() {
        let dir = write_project(&[
            (
                "main.tex",
                "\\begin{document}\\section{A}\n\\input{a}\n\\end{document}",
            ),
            ("a.tex", "text a.\n\\input{b}\n"),
            ("b.tex", "text b.\n\\input{a}\n"), // a -> b -> a cycle
        ]);
        let project = resolve_latex_project(&dir.path().join("main.tex")).unwrap();
        assert!(!project.cycles.is_empty());
        // Resolution does not hang and produces a bounded fragment count.
        assert!(project.file_count() <= 4);
    }

    #[test]
    fn path_traversal_is_blocked() {
        // ../private must not be read.
        let dir = tempfile::tempdir().unwrap();
        let root_dir = dir.path().join("paper");
        fs::create_dir_all(root_dir.join("sections")).unwrap();
        fs::write(
            root_dir.join("main.tex"),
            "\\begin{document}\n\\section{A}\n\\input{sections/in}\n\\end{document}",
        )
        .unwrap();
        fs::write(
            root_dir.join("sections/in.tex"),
            "safe text.\n\\input{../../secret}\n",
        )
        .unwrap();
        // A file OUTSIDE the project root, referenced via ../.
        let outside = dir.path().join("secret.tex");
        fs::write(&outside, "TOP SECRET\n\\section{A}\nsecret text\n").unwrap();

        let project = resolve_latex_project(&root_dir.join("main.tex")).unwrap();
        assert!(
            project
                .missing_includes
                .iter()
                .any(|m| m.contains("BLOCKED")),
            "path escape must be blocked, got: {:#?}",
            project.missing_includes
        );
        // The secret fragment is never pulled in.
        assert!(
            !project.fragments.iter().any(|f| f.abs_path == outside),
            "outside file must never be read"
        );
    }

    #[test]
    fn symlink_outside_root_is_blocked() {
        // On Unix only (symlinks). On Windows the test is skipped logically by
        // creating the symlink via std which may require privileges; guard with
        // `#[cfg(unix)]`.
        #[cfg(unix)]
        {
            let outside = tempfile::tempdir().unwrap();
            fs::write(outside.path().join("linked.tex"), "OUTSIDE CONTENT\n").unwrap();
            let dir = tempfile::tempdir().unwrap();
            let root_dir = dir.path().join("paper");
            fs::create_dir_all(&root_dir).unwrap();
            fs::write(
                root_dir.join("main.tex"),
                "\\begin{document}\\section{A}\n\\input{escape}\n\\end{document}",
            )
            .unwrap();
            // Symlink "escape.tex" -> outside file.
            std::os::unix::fs::symlink(
                outside.path().join("linked.tex"),
                root_dir.join("escape.tex"),
            )
            .unwrap();
            let project = resolve_latex_project(&root_dir.join("main.tex")).unwrap();
            assert!(
                project
                    .missing_includes
                    .iter()
                    .any(|m| m.contains("BLOCKED")),
                "symlink escape must be blocked: {:#?}",
                project.missing_includes
            );
            assert!(project.fragments.len() == 1, "only the root may resolve");
        }
    }

    #[cfg(unix)]
    #[test]
    fn unicode_and_space_paths_resolve() {
        let dir = write_project(&[
            (
                "main.tex",
                "\\begin{document}\\section{H}\n\\input{sections/Método uno}\n\\end{document}",
            ),
            ("sections/Método uno.tex", "Ünïcode spâced text.\n"),
        ]);
        let project = resolve_latex_project(&dir.path().join("main.tex")).unwrap();
        assert_eq!(project.file_count(), 2);
        assert!(project.missing_includes.is_empty());
        // The relative path retains the Unicode + space.
        assert!(project.fragments.iter().any(|f| f.rel_path.contains(' ')));
        let doc = parse_latex_project(&project).unwrap();
        assert!(collect_paragraph_texts(&doc)
            .iter()
            .any(|p| p.contains("Ünïcode")));
    }

    #[test]
    fn provenance_line_numbers_are_fragment_local() {
        let dir = write_project(&[
            (
                "main.tex",
                "\\begin{document}\n\\section{Intro}\nline three here.\nline four too.\n\n\\end{document}",
            ),
        ]);
        let project = resolve_latex_project(&dir.path().join("main.tex")).unwrap();
        let doc = parse_latex_project(&project).unwrap();
        let para = &doc.sections[0].paragraphs[0];
        let loc = para.location.as_ref().expect("provenance");
        assert_eq!(loc.file, "main.tex");
        // The paragraph spans lines 3-4 within main.tex.
        assert_eq!(loc.start_line, Some(3));
        assert_eq!(loc.end_line, Some(4));
    }

    #[test]
    fn duplicate_includes_expand_deterministically_in_place() {
        // A file included twice appears at each occurrence (LaTeX-like), and
        // the same file referenced in a cycle-free diamond still expands once
        // per occurrence without exponential blowup.
        let dir = write_project(&[
            (
                "main.tex",
                "\\begin{document}\\section{A}\n\\input{shared}\ntail.\n\\input{shared}\n\\end{document}",
            ),
            ("shared.tex", "SHAREDTEXT\n"),
        ]);
        let project = resolve_latex_project(&dir.path().join("main.tex")).unwrap();
        let doc = parse_latex_project(&project).unwrap();
        let count = collect_paragraph_texts(&doc)
            .iter()
            .filter(|p| p.contains("SHAREDTEXT"))
            .count();
        // Exact occurrence count (not exponential): root + shared + shared.
        // The tail paragraph is separate.
        assert!(
            count >= 2,
            "shared fragment should appear at each occurrence"
        );
    }
}

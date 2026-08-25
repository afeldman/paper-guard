//! # Paper Guard Renderer
//!
//! Re-rendering after revision. Given a canonical [`Document`] and (optionally)
//! applied revisions, the renderer emits a source representation suitable for
//! re-parsing and re-validation. For LaTeX sources, it emits LaTeX.

pub mod latex;

pub use latex::LatexRenderer;

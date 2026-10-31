//! Text (brief M3 §5, decision D1): fonts, shaping, layout to SkParagraph's
//! rules, the paragraph's metrics and queries, and the glyph runs it
//! paints. No dart:ui, no emblema.

pub mod font;
pub(crate) mod line;
pub mod paint;
pub mod paragraph;
mod query;
pub(crate) mod shape;
pub mod style;
pub(crate) mod unicode;
pub(crate) mod wrapper;

pub use line::{LineMetricsInfo, PositionWithAffinity, RectHeightStyle, RectWidthStyle, TextBox};
pub use paragraph::{Paragraph, ParagraphBuilder};
pub use query::GlyphInfo;

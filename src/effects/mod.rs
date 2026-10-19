//! Live effects (spec 21): each shape's effect stack, its upgrade from older files, and the one
//! geometry entry point every consumer reads.

mod cache;
mod compound;
#[cfg(test)]
mod compound_tests;
mod eval;
mod instance;
#[cfg(test)]
mod live_tests;
mod model;
mod ops;
mod rows;
#[cfg(test)]
mod test_accessors;
#[cfg(test)]
mod tests;
mod upgrade;

pub use cache::Cache;
#[cfg(test)]
pub use eval::PartLocal;
pub use eval::{Geometry, Ink, Lines, Local, Paint, Piece, PieceId, Placement, TooThin, body_contours, evaluate, image_corners};
pub(crate) use model::curve_compat;
pub use model::{ContourMask, Corners, Curve, Direction, Effect, Effects, Offset, Sticker, Style};
pub use ops::{EffectEdit, Reach};
pub use rows::{EntryRef, Row, RowKind, RowView, count, has_row, rows};
pub(crate) use upgrade::ShapeRepr;

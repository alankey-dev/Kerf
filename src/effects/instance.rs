//! Instances (spec 31) through the evaluation pipeline: an instance evaluates as an effect group over its
//! parts, laid out in its own box at the origin so that moving or turning it re-places but never
//! re-evaluates. Every piece it draws is its own, named by the part that made it.

use super::eval::{LocalPiece, LocalPieces, Paint};
use super::model::Style;
use crate::design::{Contour, Operation, Seg, Shape};

/// `instance`'s box at the origin, unturned and unflipped: the frame its parts are laid out in.
pub(super) fn at_origin(instance: &Shape) -> Shape {
    Shape { x_in: 0.0, y_in: 0.0, rotation_deg: 0.0, flip_x: false, flip_y: false, ..instance.clone() }
}

/// The pieces an instance's evaluation made over its parts, each now the instance's own and named by the
/// part that made it. Its own Offset and Sticker pieces keep their names.
pub(super) fn owned(mut local: LocalPieces) -> LocalPieces {
    for piece in &mut local.pieces {
        if let Some(part) = piece.owner.take() {
            piece.local = piece.local.of_part(part);
        }
    }
    local
}

/// What an instance of a missing component draws: its box as a Guide, so it is seen and can be removed.
pub(super) fn missing(instance: &Shape) -> LocalPieces {
    let (w, h) = (instance.width_in, instance.height_in);
    let outline = Contour { start: (0.0, 0.0), segs: vec![Seg::Line((w, 0.0)), Seg::Line((w, h)), Seg::Line((0.0, h))], closed: true };
    let style = Style { operation: Operation::Guide, ..Style::of(instance) };
    LocalPieces { pieces: vec![LocalPiece { owner: None, local: super::eval::Local::Body, style, contours: vec![outline], paint: Paint::Fill }], grab: false }
}

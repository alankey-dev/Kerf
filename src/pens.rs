//! Physical pens. A Pen layer's colour is a pen from [`PEN_COLOURS`] at one of four nib sizes, so the
//! canvas, the payload's `pathColor` and the Load step all name the pen the user has to load.

use egui::Color32;
use serde::{Deserialize, Deserializer, Serialize};

use crate::design::{Operation, Shape};

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum PenSize {
    ExtraFine,
    #[default]
    Fine,
    Medium,
    Bold,
}

impl PenSize {
    pub const ALL: [PenSize; 4] = [Self::ExtraFine, Self::Fine, Self::Medium, Self::Bold];

    /// The size dropdown's row text.
    pub fn label(self) -> &'static str {
        match self {
            Self::ExtraFine => "XF - Extra Fine Point (0.3mm)",
            Self::Fine => "F - Fine Point (0.4mm)",
            Self::Medium => "M - Medium Point (1.0mm)",
            Self::Bold => "B - Bold Point (2.0mm)",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::ExtraFine => "Extra Fine Point",
            Self::Fine => "Fine Point",
            Self::Medium => "Medium Point",
            Self::Bold => "Bold Point",
        }
    }

    pub fn nib_mm(self) -> f32 {
        match self {
            Self::ExtraFine => 0.3,
            Self::Fine => 0.4,
            Self::Medium => 1.0,
            Self::Bold => 2.0,
        }
    }
}

const MM_PER_INCH: f32 = 25.4;

const fn hex(rgb: u32) -> Color32 {
    Color32::from_rgb((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8)
}

/// Neutrals dark to light, then the hue wheel from red round to pink.
pub const PEN_COLOURS: [(&str, Color32); 24] = [
    ("Black", hex(0x000000)),
    ("Midnight", hex(0x141b2d)),
    ("Armadillo", hex(0x737373)),
    ("Moccasin", hex(0x6b4a2f)),
    ("Tawny", hex(0xa67b5b)),
    ("Wine", hex(0x8a3333)),
    ("Raspberry", hex(0x9e1f63)),
    ("Red", hex(0xd0202e)),
    ("Coral", hex(0xf26b5b)),
    ("Orange", hex(0xf28c28)),
    ("Marigold", hex(0xf5b31b)),
    ("Gold", hex(0xc9a23a)),
    ("Lemon", hex(0xf2dc3a)),
    ("Lime", hex(0xa4c639)),
    ("Green", hex(0x2e9e4f)),
    ("Forest", hex(0x1f5e3a)),
    ("Teal", hex(0x138a8a)),
    ("Aqua", hex(0x3cc3d6)),
    ("Sky", hex(0x5aa9e6)),
    ("Blue", hex(0x1f5fbf)),
    ("Navy", hex(0x1c2e6b)),
    ("Purple", hex(0x6a3ba0)),
    ("Lavender", hex(0xb49ad8)),
    ("Pink", hex(0xf28db5)),
];

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Pen {
    pub size: PenSize,
    /// Index into [`PEN_COLOURS`].
    #[serde(deserialize_with = "colour_index")]
    pub colour: usize,
}

/// A hand-edited or future project may name a pen this build lacks; it loads as Black.
fn colour_index<'de, D: Deserializer<'de>>(d: D) -> Result<usize, D::Error> {
    let i = usize::deserialize(d)?;
    Ok(if i < PEN_COLOURS.len() { i } else { 0 })
}

impl Pen {
    /// The pen of `size` whose colour is closest to `fill`.
    pub fn nearest(fill: Color32, size: PenSize) -> Self {
        let distance = |c: Color32| [(c.r(), fill.r()), (c.g(), fill.g()), (c.b(), fill.b())].iter().map(|&(a, b)| (a as i32 - b as i32).pow(2)).sum::<i32>();
        let colour = (0..PEN_COLOURS.len()).min_by_key(|&i| distance(PEN_COLOURS[i].1)).unwrap_or(0);
        Self { size, colour }
    }

    pub fn colour_name(self) -> &'static str {
        PEN_COLOURS[self.colour].0
    }

    pub fn rgb(self) -> Color32 {
        PEN_COLOURS[self.colour].1
    }

    pub fn hex(self) -> String {
        let c = self.rgb();
        format!("#{:02x}{:02x}{:02x}", c.r(), c.g(), c.b())
    }

    #[cfg(test)]
    pub fn named(name: &str, size: PenSize) -> Self {
        Self { size, colour: PEN_COLOURS.iter().position(|(n, _)| *n == name).expect("a pen colour") }
    }

    /// "Black Fine Point Pen", as the Load step names it.
    pub fn display_name(self) -> String {
        format!("{} {} Pen", self.colour_name(), self.size.name())
    }

    pub fn nib_in(self) -> f32 {
        self.size.nib_mm() / MM_PER_INCH
    }
}

/// What the pen picker sets: a size, or a colour by index into `PEN_COLOURS`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PenPick {
    Size(PenSize),
    Colour(usize),
}

impl Shape {
    /// Loads `pen` and paints the shape in its colour, so every view of a Pen layer shows the pen.
    pub fn set_pen(&mut self, pen: Pen) {
        self.pen = pen;
        self.fill = pen.rgb();
    }

    /// A shape that becomes a Pen layer takes the pen nearest its current colour.
    /// A size keeps the shape's colour; a colour keeps its size.
    pub fn pick_pen(&mut self, pick: PenPick) {
        let pen = match pick {
            PenPick::Size(size) => Pen { size, ..self.pen },
            PenPick::Colour(colour) => Pen { colour, ..self.pen },
        };
        self.set_pen(pen);
    }

    pub fn set_operation(&mut self, op: Operation) {
        if op == Operation::Pen && self.operation != Operation::Pen {
            self.set_pen(Pen::nearest(self.fill, self.pen.size));
        }
        self.operation = op;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::design::{Design, ShapeKind};

    #[test]
    fn the_table_starts_with_the_reference_pens_and_has_unique_names() {
        let names: Vec<&str> = PEN_COLOURS.iter().map(|(n, _)| *n).collect();
        assert_eq!(names[..7], ["Black", "Midnight", "Armadillo", "Moccasin", "Tawny", "Wine", "Raspberry"]);
        let mut unique = names.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), names.len());
    }

    #[test]
    fn nearest_pen_matches_exact_colours_and_snaps_near_ones() {
        let wine = Pen::named("Wine", PenSize::Medium);
        assert_eq!(Pen::nearest(wine.rgb(), PenSize::Medium), wine);
        assert_eq!(Pen::nearest(Color32::from_rgb(10, 10, 10), PenSize::Fine).colour_name(), "Black");
        assert_eq!(Pen::nearest(Color32::from_rgb(0x20, 0x60, 0xc0), PenSize::Fine).colour_name(), "Blue");
    }

    #[test]
    fn becoming_a_pen_layer_snaps_the_fill_to_the_nearest_pen() {
        let mut d = Design::default();
        d.add_shape(ShapeKind::Square);
        let s = &mut d.shapes[0];
        s.fill = Color32::from_rgb(0x88, 0x30, 0x30);
        s.set_operation(Operation::Pen);
        assert_eq!((s.pen.colour_name(), s.fill), ("Wine", Pen::named("Wine", PenSize::Fine).rgb()));

        s.set_pen(Pen::named("Raspberry", PenSize::Bold));
        s.set_operation(Operation::Pen);
        assert_eq!(s.pen.colour_name(), "Raspberry", "re-picking Pen keeps the chosen pen");
    }

    #[test]
    fn the_load_name_reads_colour_then_size() {
        assert_eq!(Pen::default().display_name(), "Black Fine Point Pen");
        assert_eq!(Pen::named("Wine", PenSize::ExtraFine).display_name(), "Wine Extra Fine Point Pen");
    }

    #[test]
    fn combining_and_offsetting_pen_layers_keep_the_pen() {
        let wine = Pen::named("Wine", PenSize::Medium);
        let mut d = Design::default();
        let ids: Vec<u64> = (0..2).map(|_| d.add_shape(ShapeKind::Square)).collect();
        for s in &mut d.shapes {
            s.set_operation(Operation::Pen);
            s.set_pen(wine);
        }
        let offset = crate::effects::Effect::Offset(crate::effects::Offset {
            distance_in: 0.1,
            direction: crate::effects::Direction::Outset,
            join: crate::geometry::Join::Round,
            style: None,
        });
        let group = d.apply(&crate::edit::Edit::AddEffect { effect: offset, reach: crate::effects::Reach::Together }, &ids).unwrap();
        let outline = d.apply(&crate::edit::Edit::Expand, &group).unwrap()[0];
        let group = d.combine(&ids, crate::design::Combine::Weld);
        let [welded] = d.apply(&crate::edit::Edit::Expand, &group).unwrap()[..] else { panic!("one welded shape") };
        for id in [outline, welded] {
            assert_eq!(d.shapes.iter().find(|s| s.id == id).unwrap().pen, wine);
        }
    }

    #[test]
    fn slice_pieces_take_the_pen_of_the_shape_they_came_from() {
        let (wine, black) = (Pen::named("Wine", PenSize::Medium), Pen::named("Black", PenSize::Bold));
        let mut d = Design::default();
        let ids: Vec<u64> = [(wine, 0.0), (black, 1.0)]
            .into_iter()
            .map(|(pen, x)| {
                let id = d.add_shape(ShapeKind::Square);
                let s = d.shape_mut(id).unwrap();
                (s.x_in, s.y_in) = (x, 0.0);
                s.set_operation(Operation::Pen);
                s.set_pen(pen);
                id
            })
            .collect();
        let group = d.combine(&ids, crate::design::Combine::Slice);
        let pieces = d.apply(&crate::edit::Edit::Expand, &group).unwrap();
        let pens: Vec<Pen> = pieces.iter().map(|id| d.shapes.iter().find(|s| s.id == *id).unwrap().pen).collect();
        assert_eq!(pens, [wine, black, black], "back piece, front piece, overlap");
    }

    #[test]
    fn an_unknown_pen_colour_loads_as_black() {
        let pen: Pen = serde_json::from_str(r#"{"size":"Bold","colour":999}"#).unwrap();
        assert_eq!(pen, Pen { size: PenSize::Bold, colour: 0 });
    }

    #[test]
    fn a_project_saved_before_pens_loads_with_the_default_pen() {
        let json = r#"{"id":"pre-pens","name":"Before pens","design":{"shapes":[{"id":1,"kind":"Square","x_in":0.5,"y_in":0.5,"width_in":2.0,"height_in":2.0,"rotation_deg":0.0,"flip_x":false,"flip_y":false,"fill":[0,0,0,255],"operation":"Pen","hidden":false,"contour_hidden":[],"link":null}]},"created":1,"modified":1}"#;
        let project: crate::project::Project = serde_json::from_str(json).expect("an old project still loads");
        assert_eq!(project.design.shapes[0].pen, Pen::default());
        assert_eq!(Pen::default(), Pen { size: PenSize::Fine, colour: 0 });
    }
}

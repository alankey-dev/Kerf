//! Pure colour helpers for the swatch picker (spec 56): parsing hex and CSS colour names, the bundled
//! palettes the picker adds from, and counting the distinct colours a design already uses so picking one
//! makes no new mat. No egui layout and no I/O; the picker in `ui::colour_picker` draws what this reads.

use egui::Color32;

use crate::design::{Design, Operation};

/// Parses `#RGB`, `#RRGGBB`, a bare `RRGGBB`, or a CSS colour name (case insensitive) into an opaque
/// colour. The error is a one line message the hex field shows in place of changing the colour.
pub fn parse_colour(text: &str) -> Result<Color32, String> {
    let t = text.trim();
    if let Some(hex) = t.strip_prefix('#') {
        return from_hex(hex).ok_or_else(|| format!("\"{t}\" is not a 3 or 6 digit hex colour."));
    }
    if t.len() == 6 && t.bytes().all(|b| b.is_ascii_hexdigit()) {
        return from_hex(t).ok_or_else(|| format!("\"{t}\" is not a 3 or 6 digit hex colour."));
    }
    css_colour(t).ok_or_else(|| format!("\"{t}\" is not a hex colour or a known colour name."))
}

/// A 3 or 6 hex-digit string as an opaque colour; `None` for any other length or a non-hex digit.
fn from_hex(h: &str) -> Option<Color32> {
    let b = h.as_bytes();
    let nibble = |c: u8| (c as char).to_digit(16).map(|v| v as u8);
    match b.len() {
        3 => {
            let dup = |i: usize| nibble(b[i]).map(|v| v * 17);
            Some(Color32::from_rgb(dup(0)?, dup(1)?, dup(2)?))
        }
        6 => {
            let byte = |i: usize| Some(nibble(b[i])? * 16 + nibble(b[i + 1])?);
            Some(Color32::from_rgb(byte(0)?, byte(2)?, byte(4)?))
        }
        _ => None,
    }
}

/// `#RRGGBB`, upper case, as the hex field and the swatch read-out show a colour.
pub fn hex_string(c: Color32) -> String {
    format!("#{:02X}{:02X}{:02X}", c.r(), c.g(), c.b())
}

const fn rgb(v: u32) -> Color32 {
    Color32::from_rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)
}

/// A CSS colour name (CSS Color Module Level 4), case insensitive, including `rebeccapurple`.
fn css_colour(name: &str) -> Option<Color32> {
    let lower = name.to_ascii_lowercase();
    CSS_COLOURS.iter().find(|(n, _)| *n == lower).map(|&(_, v)| rgb(v))
}

/// One distinct colour a design uses, with how many shapes carry it. A Pen layer's pen colour is counted
/// apart from a fill of the same RGB, so the picker lists it as its own swatch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ColourUse {
    pub colour: Color32,
    /// The colour of Pen layers, not a fill.
    pub pen: bool,
    pub count: usize,
}

/// Every distinct colour in `design`, most used first, fills and pens apart. Ties keep the order the
/// colours first appear, so the list is stable as shapes are recoloured.
pub fn project_colours(design: &Design) -> Vec<ColourUse> {
    let mut uses: Vec<ColourUse> = Vec::new();
    for shape in &design.shapes {
        let pen = shape.operation == Operation::Pen;
        let colour = if pen { shape.pen.rgb() } else { shape.fill };
        match uses.iter_mut().find(|u| u.pen == pen && u.colour == colour) {
            Some(u) => u.count += 1,
            None => uses.push(ColourUse { colour, pen, count: 1 }),
        }
    }
    uses.sort_by_key(|u| std::cmp::Reverse(u.count));
    uses
}

/// A friendly name for a swatch's hover ("Red"), from the nearest pen colour so it reads in the app's own
/// colour vocabulary.
pub fn name_of(colour: Color32) -> &'static str {
    crate::pens::Pen::nearest(colour, crate::pens::PenSize::Fine).colour_name()
}

/// A bundled set of colours the picker can add from. Colours only, no artwork.
pub struct BundledPalette {
    pub name: &'static str,
    pub colours: &'static [Color32],
}

/// The six bundled palettes the picker's dropdown offers. The names echo real material lines; the colours
/// are Kerf's own, never lifted from another product's assets.
pub const BUNDLED: [BundledPalette; 6] = [
    BundledPalette { name: "Vinyl", colours: &[rgb(0xD0202E), rgb(0xF28C28), rgb(0xF5B31B), rgb(0x2E9E4F), rgb(0x1F5FBF), rgb(0x6A3BA0), rgb(0x000000), rgb(0xFFFFFF)] },
    BundledPalette {
        name: "Iron-On",
        colours: &[rgb(0x8A1F2B), rgb(0xE8552C), rgb(0xF2C14E), rgb(0x3C8C5A), rgb(0x2C5FA8), rgb(0x3A2C6B), rgb(0xEDE6DA), rgb(0x1C1C1C)],
    },
    BundledPalette {
        name: "Cardstock basics",
        colours: &[rgb(0xFFFFFF), rgb(0xF4ECD8), rgb(0xE8C9A0), rgb(0xD94F4F), rgb(0x3F8F4F), rgb(0x2F5FA0), rgb(0x5A4A8A), rgb(0x2B2B2B)],
    },
    BundledPalette { name: "Pastel", colours: &[rgb(0xF7C6D0), rgb(0xF9D9A7), rgb(0xFBF0A8), rgb(0xC3E6C3), rgb(0xB7D9EC), rgb(0xD6C7E8), rgb(0xF4C2DC), rgb(0xEDE8E0)] },
    BundledPalette { name: "Earth", colours: &[rgb(0x6B4A2F), rgb(0xA67B5B), rgb(0xC9A23A), rgb(0x7A8450), rgb(0x4F6B3A), rgb(0x2F4A3A), rgb(0xB08968), rgb(0x3D2B1F)] },
    BundledPalette { name: "Mono", colours: &[rgb(0x000000), rgb(0x262626), rgb(0x4D4D4D), rgb(0x737373), rgb(0x999999), rgb(0xBFBFBF), rgb(0xE6E6E6), rgb(0xFFFFFF)] },
];

/// CSS Color Module Level 4 named colours, lower case, with their sRGB value. Includes `rebeccapurple`.
const CSS_COLOURS: &[(&str, u32)] = &[
    ("aliceblue", 0xF0F8FF),
    ("antiquewhite", 0xFAEBD7),
    ("aqua", 0x00FFFF),
    ("aquamarine", 0x7FFFD4),
    ("azure", 0xF0FFFF),
    ("beige", 0xF5F5DC),
    ("bisque", 0xFFE4C4),
    ("black", 0x000000),
    ("blanchedalmond", 0xFFEBCD),
    ("blue", 0x0000FF),
    ("blueviolet", 0x8A2BE2),
    ("brown", 0xA52A2A),
    ("burlywood", 0xDEB887),
    ("cadetblue", 0x5F9EA0),
    ("chartreuse", 0x7FFF00),
    ("chocolate", 0xD2691E),
    ("coral", 0xFF7F50),
    ("cornflowerblue", 0x6495ED),
    ("cornsilk", 0xFFF8DC),
    ("crimson", 0xDC143C),
    ("cyan", 0x00FFFF),
    ("darkblue", 0x00008B),
    ("darkcyan", 0x008B8B),
    ("darkgoldenrod", 0xB8860B),
    ("darkgray", 0xA9A9A9),
    ("darkgreen", 0x006400),
    ("darkgrey", 0xA9A9A9),
    ("darkkhaki", 0xBDB76B),
    ("darkmagenta", 0x8B008B),
    ("darkolivegreen", 0x556B2F),
    ("darkorange", 0xFF8C00),
    ("darkorchid", 0x9932CC),
    ("darkred", 0x8B0000),
    ("darksalmon", 0xE9967A),
    ("darkseagreen", 0x8FBC8F),
    ("darkslateblue", 0x483D8B),
    ("darkslategray", 0x2F4F4F),
    ("darkslategrey", 0x2F4F4F),
    ("darkturquoise", 0x00CED1),
    ("darkviolet", 0x9400D3),
    ("deeppink", 0xFF1493),
    ("deepskyblue", 0x00BFFF),
    ("dimgray", 0x696969),
    ("dimgrey", 0x696969),
    ("dodgerblue", 0x1E90FF),
    ("firebrick", 0xB22222),
    ("floralwhite", 0xFFFAF0),
    ("forestgreen", 0x228B22),
    ("fuchsia", 0xFF00FF),
    ("gainsboro", 0xDCDCDC),
    ("ghostwhite", 0xF8F8FF),
    ("gold", 0xFFD700),
    ("goldenrod", 0xDAA520),
    ("gray", 0x808080),
    ("green", 0x008000),
    ("greenyellow", 0xADFF2F),
    ("grey", 0x808080),
    ("honeydew", 0xF0FFF0),
    ("hotpink", 0xFF69B4),
    ("indianred", 0xCD5C5C),
    ("indigo", 0x4B0082),
    ("ivory", 0xFFFFF0),
    ("khaki", 0xF0E68C),
    ("lavender", 0xE6E6FA),
    ("lavenderblush", 0xFFF0F5),
    ("lawngreen", 0x7CFC00),
    ("lemonchiffon", 0xFFFACD),
    ("lightblue", 0xADD8E6),
    ("lightcoral", 0xF08080),
    ("lightcyan", 0xE0FFFF),
    ("lightgoldenrodyellow", 0xFAFAD2),
    ("lightgray", 0xD3D3D3),
    ("lightgreen", 0x90EE90),
    ("lightgrey", 0xD3D3D3),
    ("lightpink", 0xFFB6C1),
    ("lightsalmon", 0xFFA07A),
    ("lightseagreen", 0x20B2AA),
    ("lightskyblue", 0x87CEFA),
    ("lightslategray", 0x778899),
    ("lightslategrey", 0x778899),
    ("lightsteelblue", 0xB0C4DE),
    ("lightyellow", 0xFFFFE0),
    ("lime", 0x00FF00),
    ("limegreen", 0x32CD32),
    ("linen", 0xFAF0E6),
    ("magenta", 0xFF00FF),
    ("maroon", 0x800000),
    ("mediumaquamarine", 0x66CDAA),
    ("mediumblue", 0x0000CD),
    ("mediumorchid", 0xBA55D3),
    ("mediumpurple", 0x9370DB),
    ("mediumseagreen", 0x3CB371),
    ("mediumslateblue", 0x7B68EE),
    ("mediumspringgreen", 0x00FA9A),
    ("mediumturquoise", 0x48D1CC),
    ("mediumvioletred", 0xC71585),
    ("midnightblue", 0x191970),
    ("mintcream", 0xF5FFFA),
    ("mistyrose", 0xFFE4E1),
    ("moccasin", 0xFFE4B5),
    ("navajowhite", 0xFFDEAD),
    ("navy", 0x000080),
    ("oldlace", 0xFDF5E6),
    ("olive", 0x808000),
    ("olivedrab", 0x6B8E23),
    ("orange", 0xFFA500),
    ("orangered", 0xFF4500),
    ("orchid", 0xDA70D6),
    ("palegoldenrod", 0xEEE8AA),
    ("palegreen", 0x98FB98),
    ("paleturquoise", 0xAFEEEE),
    ("palevioletred", 0xDB7093),
    ("papayawhip", 0xFFEFD5),
    ("peachpuff", 0xFFDAB9),
    ("peru", 0xCD853F),
    ("pink", 0xFFC0CB),
    ("plum", 0xDDA0DD),
    ("powderblue", 0xB0E0E6),
    ("purple", 0x800080),
    ("rebeccapurple", 0x663399),
    ("red", 0xFF0000),
    ("rosybrown", 0xBC8F8F),
    ("royalblue", 0x4169E1),
    ("saddlebrown", 0x8B4513),
    ("salmon", 0xFA8072),
    ("sandybrown", 0xF4A460),
    ("seagreen", 0x2E8B57),
    ("seashell", 0xFFF5EE),
    ("sienna", 0xA0522D),
    ("silver", 0xC0C0C0),
    ("skyblue", 0x87CEEB),
    ("slateblue", 0x6A5ACD),
    ("slategray", 0x708090),
    ("slategrey", 0x708090),
    ("snow", 0xFFFAFA),
    ("springgreen", 0x00FF7F),
    ("steelblue", 0x4682B4),
    ("tan", 0xD2B48C),
    ("teal", 0x008080),
    ("thistle", 0xD8BFD8),
    ("tomato", 0xFF6347),
    ("turquoise", 0x40E0D0),
    ("violet", 0xEE82EE),
    ("wheat", 0xF5DEB3),
    ("white", 0xFFFFFF),
    ("whitesmoke", 0xF5F5F5),
    ("yellow", 0xFFFF00),
    ("yellowgreen", 0x9ACD32),
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::design::ShapeKind;
    use crate::pens::{Pen, PenSize};

    #[test]
    fn hex_and_css_names_parse_and_bad_input_fails_with_a_message() {
        assert_eq!(parse_colour("#d0202e"), Ok(Color32::from_rgb(0xD0, 0x20, 0x2E)));
        assert_eq!(parse_colour("D0202E"), Ok(Color32::from_rgb(0xD0, 0x20, 0x2E)));
        assert_eq!(parse_colour("#f00"), Ok(Color32::from_rgb(0xFF, 0x00, 0x00)));
        assert_eq!(parse_colour("rebeccapurple"), Ok(Color32::from_rgb(0x66, 0x33, 0x99)));
        assert_eq!(parse_colour("  #F00  "), Ok(Color32::from_rgb(0xFF, 0x00, 0x00)), "surrounding space is ignored");

        let five = parse_colour("#12345").expect_err("five digits is not a hex colour");
        assert!(five.contains("#12345") && !five.is_empty());
        let word = parse_colour("blurple").expect_err("not a CSS colour name");
        assert!(word.contains("blurple") && !word.is_empty());
    }

    #[test]
    fn project_colours_count_distinct_fills_and_pens_in_count_order() {
        let mut d = Design::default();
        for _ in 0..7 {
            let id = d.add_shape(ShapeKind::Square);
            d.shape_mut(id).unwrap().fill = Color32::from_rgb(0xD0, 0x20, 0x2E);
        }
        for _ in 0..3 {
            let id = d.add_shape(ShapeKind::Square);
            d.shape_mut(id).unwrap().fill = Color32::from_rgb(0x1F, 0x5F, 0xBF);
        }
        let pen = d.add_shape(ShapeKind::Square);
        let s = d.shape_mut(pen).unwrap();
        s.set_operation(Operation::Pen);
        s.set_pen(Pen::named("Red", PenSize::Fine));

        let uses = project_colours(&d);
        assert_eq!(uses.len(), 3, "red fill, blue fill, red pen are three swatches");
        assert_eq!((uses[0].count, uses[0].pen), (7, false));
        assert_eq!(uses[0].colour, Color32::from_rgb(0xD0, 0x20, 0x2E));
        assert_eq!((uses[1].count, uses[1].pen), (3, false));
        assert_eq!((uses[2].count, uses[2].pen), (1, true), "the red pen is listed apart from the red fill");
        assert_eq!(name_of(uses[0].colour), "Red");
    }

    #[test]
    fn bundled_palettes_are_the_six_named_sets() {
        let names: Vec<&str> = BUNDLED.iter().map(|p| p.name).collect();
        assert_eq!(names, ["Vinyl", "Iron-On", "Cardstock basics", "Pastel", "Earth", "Mono"]);
        assert!(BUNDLED.iter().all(|p| !p.colours.is_empty()));
    }
}

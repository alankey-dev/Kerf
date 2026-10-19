//! The print sheet (spec 27): a Print Then Cut page's artwork as a PDF the user prints. A plain image is
//! one filled path in its own colour at its composed offset; flattened artwork, such as a sticker, prints
//! each of its parts in its own colour. With Add Bleed the art is first dilated by the bleed, then drawn on
//! top. An uploaded image (spec 37) prints its pixels clipped to its cut line,
//! and with bleed its dilated outline first, filled with the image's edge pixels extended outward. The
//! page carries no registration marks, sensor box, text or logo, so a machine cannot cut it in register.

use crate::bridge::{self, Placement};
use crate::composition::Mat;
use crate::design::{Contour, Design, Operation, POINTS_PER_INCH, Pt, Seg};
use crate::effects::{Ink, Lines, Paint, image_corners};
use crate::export::{Layer, Picture, Sheet, file_stem, map_contour};
use crate::geometry::{self, FLATTEN_TOL_IN};
use crate::make_flow::MatSetup;

/// The page's sheet, or `None` for a cut mat. Only Print Then Cut pieces print; cut, pen and other lines
/// on the page are the machine's job. Every dilated layer comes before every undilated one, because the
/// bleed stamp covers the whole page before the art (`docs/evidence-webapp.md`, "Print Then Cut
/// composition", question 3).
pub fn sheet(design: &Design, mat: &Mat, setup: &MatSetup, bleed: bool) -> Option<Sheet> {
    let area = mat.page?;
    let unmirrored = MatSetup { mirror: false, ..*setup };
    let (mut under, mut art) = (Vec::new(), Vec::new());
    for p in bridge::mat_placements(design, mat, &unmirrored).iter().filter(|p| p.piece().style.operation == Operation::PrintThenCut) {
        let name = format!("{} - {}", Operation::PrintThenCut.label(), p.shape.kind.label());
        let fill = p.piece().style.fill;
        let layer = |colour, contours, picture| Layer { name: name.clone(), group: Vec::new(), operation: Operation::PrintThenCut, colour, contours, picture };
        let place = |(x, y): Pt| {
            let (x, y) = p.transform.apply((x * POINTS_PER_INCH, y * POINTS_PER_INCH));
            (x / POINTS_PER_INCH, y / POINTS_PER_INCH)
        };
        let local: Vec<Contour> = p.piece().contours.iter().map(|c| map_contour(c, place)).collect();
        let picture = |asset: &crate::assets::AssetId, corners, bleed_in| Some(Picture::once(asset.clone(), corners, bleed_in));
        match &p.piece().paint {
            Paint::Fill => match bleed {
                true => under.push(layer(fill, grown(&local, area.bleed_in), None)),
                false => art.push(layer(fill, local, None)),
            },
            Paint::Image(asset) => {
                let corners = image_corners((p.shape.width_in, p.shape.height_in), place);
                if bleed {
                    under.push(layer(fill, grown(&local, area.bleed_in), picture(asset, corners, area.bleed_in)));
                }
                art.push(layer(fill, local, picture(asset, corners, 0.0)));
            }
            Paint::Print(parts) => {
                let on_page = to_page(mat, p);
                for (lines, ink) in parts {
                    let contours = page_contours(lines, &on_page);
                    let (colour, image) = match ink {
                        Ink::Colour(c) => (*c, None),
                        Ink::Image { asset, corners, repeat } => (fill, Some((asset, corners.map(&on_page), *repeat))),
                    };
                    // A tile repeats over whatever it covers, so its bleed is the tiling continued, not edge pixels.
                    let picture = |contours: &[Contour], bleed_in| match image {
                        Some((asset, tile, true)) => Picture::tiled(asset.clone(), tile, contours),
                        Some((asset, corners, false)) => picture(asset, corners, bleed_in),
                        None => None,
                    };
                    if bleed {
                        let grown = grown(&contours, area.bleed_in);
                        let stamp = picture(&grown, area.bleed_in);
                        under.push(layer(colour, grown, stamp));
                    }
                    let art_picture = picture(&contours, 0.0);
                    art.push(layer(colour, contours, art_picture));
                }
            }
        }
    }
    let layers = under.into_iter().chain(art).filter(|l| !l.contours.is_empty()).collect();
    Some(Sheet { name: "Print Then Cut".into(), size_in: (area.paper.width_in, area.paper.height_in), layers })
}

/// With mirroring off a placement only translates: a world point lands at itself, less the piece's world
/// top left, plus the item's offset on the page.
fn to_page(mat: &Mat, p: &Placement) -> impl Fn(Pt) -> Pt + use<> {
    let piece = p.piece();
    let offset = mat.items.iter().find(|i| i.piece == piece.id && i.copy == p.copy).map_or((0.0, 0.0), |i| i.offset_in);
    let shift = (offset.0 - piece.bounds.min_x, offset.1 - piece.bounds.min_y);
    move |(x, y)| (x + shift.0, y + shift.1)
}

fn page_contours(lines: &Lines, on_page: impl Fn(Pt) -> Pt) -> Vec<Contour> {
    lines
        .iter()
        .filter_map(|(pts, closed)| {
            let (&start, rest) = pts.split_first()?;
            Some(Contour { start: on_page(start), segs: rest.iter().map(|&p| Seg::Line(on_page(p))).collect(), closed: *closed })
        })
        .collect()
}

/// The outline offset outward by `distance` with round joins, as straight-sided rings.
fn grown(contours: &[Contour], distance: f32) -> Vec<Contour> {
    let rings: Vec<geometry::Ring> = contours.iter().filter(|c| c.closed).map(|c| geometry::flatten(c, FLATTEN_TOL_IN)).collect();
    geometry::offset(&rings, distance, geometry::Join::Round)
        .into_iter()
        .filter_map(|ring| {
            let (&start, rest) = ring.split_first()?;
            Some(Contour { start, segs: rest.iter().map(|&p| Seg::Line(p)).collect(), closed: true })
        })
        .collect()
}

/// "Project - Print 1.pdf", where `page` counts print pages from 1.
pub fn file_name(project: &str, page: usize) -> String {
    format!("{} - Print {page}.pdf", file_stem(project))
}

#[cfg(test)]
mod tests {
    use egui::Color32;

    use super::*;
    use crate::composition::compose_on;
    use crate::design::ShapeKind;
    use crate::export::{Planned, Style};
    use crate::machine::MachineFamily;
    use crate::print_then_cut::{LETTER, print_area};
    use crate::snap::Aabb;

    const EVIDENCE: &str = "docs/evidence-webapp.md, \"Print Then Cut composition\"";

    /// Two 2 in Print Then Cut squares, red and blue, on one Letter page, with a cut square on a mat after it.
    fn two_images() -> (Design, Mat, MatSetup) {
        let mut design = Design::default();
        for fill in [Color32::RED, Color32::BLUE] {
            let id = design.add_shape(ShapeKind::Square);
            let s = design.shape_mut(id).unwrap();
            s.fill = fill;
            s.set_operation(Operation::PrintThenCut);
        }
        design.add_shape(ShapeKind::Square);
        let setup = MatSetup { mirror: true, ..MatSetup::preferred(MachineFamily::StudioPro) };
        let mats = compose_on(&design, setup.mat_size, 1, LETTER);
        assert!(sheet(&design, &mats[1], &setup, true).is_none(), "a cut mat has no print sheet");
        (design, mats[0].clone(), setup)
    }

    fn bounds(contours: &[Contour]) -> Aabb {
        contours.iter().flat_map(|c| c.flatten()).fold(Aabb::EMPTY, |b, (x, y)| b.union(Aabb { min_x: x, min_y: y, max_x: x, max_y: y }))
    }

    fn pdf_text(sheet: Sheet) -> String {
        String::from_utf8_lossy(&Planned::pdf("p.pdf".into(), vec![sheet], Style::Filled).bytes(&Default::default())).into_owned()
    }

    fn within(a: Aabb, b: Aabb, tolerance: f32) -> bool {
        [(a.min_x, b.min_x), (a.min_y, b.min_y), (a.max_x, b.max_x), (a.max_y, b.max_y)].iter().all(|(p, q)| (p - q).abs() < tolerance)
    }

    fn close(a: Aabb, b: Aabb) -> bool {
        within(a, b, 2e-3)
    }

    fn shifted(b: Aabb, (dx, dy): Pt) -> Aabb {
        Aabb { min_x: b.min_x + dx, min_y: b.min_y + dy, max_x: b.max_x + dx, max_y: b.max_y + dy }
    }

    fn grown_by(b: Aabb, d: f32) -> Aabb {
        Aabb { min_x: b.min_x - d, min_y: b.min_y - d, max_x: b.max_x + d, max_y: b.max_y + d }
    }

    #[test]
    fn a_pattern_filled_circle_prints_its_tile_clipped_to_the_circle_and_transparent_outside() {
        use crate::patterns::{PatternMode, fixture};
        let design = fixture::circle(PatternMode::Tile);
        let setup = MatSetup::preferred(MachineFamily::StudioPro);
        let mat = compose_on(&design, setup.mat_size, 1, LETTER).remove(0);
        let sheet = sheet(&design, &mat, &setup, true).unwrap();
        let [under, art] = &sheet.layers[..] else { panic!("the bleed stamp and the art: {:?}", sheet.layers) };
        let (x, y) = mat.items[0].offset_in;
        assert!(close(bounds(&art.contours), Aabb { min_x: x, min_y: y, max_x: x + 2.0, max_y: y + 2.0 }), "the circle clips the art");
        let picture = art.picture.as_ref().expect("the pattern prints");
        let tile = picture.tile.expect("as a repeating tile");
        assert!(((tile[1].0 - tile[0].0) - 0.5).abs() < 1e-4, "0.5 in wide: {tile:?}");
        let stamp = under.picture.as_ref().and_then(|p| p.tile.map(|_| p.corners)).expect("the bleed stamp continues the tiling");
        assert!(stamp[0].0 < picture.corners[0].0 - 0.01, "over the grown outline: {stamp:?}");

        let pixels = fixture::pixels();
        let prepared = crate::export::picture::prepare(picture, &pixels).unwrap();
        assert_eq!(prepared.corners, picture.corners);
        assert_eq!(prepared.image.get_pixel(2, 2).0[3], 255, "the raster fills its box, corners included");
        // What the PDF draws: the raster clipped to the art's outline, on a transparent page at 100 dpi.
        let dpi = 100.0;
        let mut page = tiny_skia::Pixmap::new((sheet.size_in.0 * dpi) as u32, (sheet.size_in.1 * dpi) as u32).unwrap();
        let mut path = tiny_skia::PathBuilder::new();
        for c in &art.contours {
            let ring = crate::geometry::flatten(c, crate::geometry::FLATTEN_TOL_IN);
            path.move_to(ring[0].0 * dpi, ring[0].1 * dpi);
            ring[1..].iter().for_each(|p| path.line_to(p.0 * dpi, p.1 * dpi));
            path.close();
        }
        let raster = crate::export::picture::premultiplied(&prepared.image).unwrap();
        let shader = crate::export::picture::image_shader(&raster, prepared.corners.map(|(x, y)| (x * dpi, y * dpi)), false);
        let paint = tiny_skia::Paint { shader, ..tiny_skia::Paint::default() };
        page.fill_path(&path.finish().unwrap(), &paint, tiny_skia::FillRule::EvenOdd, tiny_skia::Transform::identity(), None);
        let at = |px: f32, py: f32| page.pixel(((x + px) * dpi) as u32, ((y + py) * dpi) as u32).unwrap().demultiply();
        for (px, py) in [(0.05, 0.05), (1.95, 0.05), (0.05, 1.95), (1.95, 1.95)] {
            assert_eq!(at(px, py).alpha(), 0, "transparent outside the circle at {px}, {py}");
        }
        let colour = |c: tiny_skia::ColorU8| [c.red(), c.green(), c.blue(), c.alpha()];
        let row: Vec<[u8; 4]> = (0..8).map(|i| colour(at(0.5 + 0.0625 + i as f32 * 0.125, 1.0))).collect();
        assert!(row.iter().all(|c| *c == fixture::RED || *c == fixture::BLUE), "inside, only the tile's colours: {row:?}");
        assert!(row.windows(2).filter(|w| w[0] != w[1]).count() >= 3, "repeating every 0.5 in: {row:?}");

        let pdf = String::from_utf8_lossy(&Planned::pdf("p.pdf".into(), vec![sheet.clone()], Style::Filled).bytes(&pixels)).into_owned();
        assert_eq!(pdf.matches(" Do\n").count(), 2, "the stamp and the art each draw their raster");
    }

    #[test]
    fn two_images_make_one_letter_page_with_two_filled_paths_at_their_offsets() {
        let (design, mat, setup) = two_images();
        let sheet = sheet(&design, &mat, &setup, false).unwrap();
        assert_eq!(sheet.size_in, (8.5, 11.0));
        let colours: Vec<Color32> = sheet.layers.iter().map(|l| l.colour).collect();
        assert_eq!(colours, [Color32::RED, Color32::BLUE]);
        for (layer, item) in sheet.layers.iter().zip(&mat.items) {
            let (x, y) = item.offset_in;
            let at = Aabb { min_x: x, min_y: y, max_x: x + 2.0, max_y: y + 2.0 };
            assert!(close(bounds(&layer.contours), at), "{} at {:?}, not mirrored", layer.name, bounds(&layer.contours));
        }
        let pdf = pdf_text(sheet);
        assert_eq!(pdf.matches("/Type /Page\n").count() + pdf.matches("/Type /Page ").count(), 1, "one page:\n{pdf}");
        assert!(pdf.contains("/MediaBox [0 0 612 792]"), "Letter in points:\n{pdf}");
        assert_eq!(pdf.matches("\nf*").count(), 2, "two filled paths and nothing else");
        assert_eq!(pdf.matches("\nS").count(), 0, "no stroked marks");
    }

    #[test]
    fn bleed_grows_each_filled_path_by_the_bleed_width() {
        let (design, mat, setup) = two_images();
        let bleed = print_area(LETTER).bleed_in;
        let sheet = sheet(&design, &mat, &setup, true).unwrap();
        assert_eq!(sheet.layers.len(), 2);
        for (layer, item) in sheet.layers.iter().zip(&mat.items) {
            let (x, y) = item.offset_in;
            let grown = Aabb { min_x: x - bleed, min_y: y - bleed, max_x: x + 2.0 + bleed, max_y: y + 2.0 + bleed };
            assert!(close(bounds(&layer.contours), grown), "{EVIDENCE}, question 3: {:?}", bounds(&layer.contours));
        }
        assert_eq!(pdf_text(sheet).matches("\nf*").count(), 2, "one filled path per image");
    }

    #[test]
    fn a_sticker_prints_its_white_backing_and_artwork_translated_onto_the_page() {
        let mut design = Design::default();
        let heart = design.add_shape(ShapeKind::Heart);
        let s = design.shape_mut(heart).unwrap();
        (s.x_in, s.y_in, s.fill) = (3.0, 2.0, Color32::RED);
        let heart_world = design.geometry(&design.shapes[0]).bounds();
        design.expanded_sticker(&[heart], 0.125, crate::effects::Corners::Round).expect("makes a sticker");
        let setup = MatSetup::preferred(MachineFamily::StudioPro);
        let mat = compose_on(&design, setup.mat_size, 1, LETTER).remove(0);
        let [p] = &bridge::mat_placements(&design, &mat, &setup)[..] else { panic!("one placement") };
        let piece = p.piece();
        let offset = mat.items[0].offset_in;
        let shift = (offset.0 - piece.bounds.min_x, offset.1 - piece.bounds.min_y);

        let local = piece.contours[0].start;
        let (tx, ty) = p.transform.apply((local.0 * POINTS_PER_INCH, local.1 * POINTS_PER_INCH));
        let world = piece.lines[0].0[0];
        let by_shift = (world.0 + shift.0, world.1 + shift.1);
        assert!((tx / POINTS_PER_INCH - by_shift.0).abs() < 1e-3 && (ty / POINTS_PER_INCH - by_shift.1).abs() < 1e-3, "unmirrored placement is a translation");

        let plain = sheet(&design, &mat, &setup, false).unwrap();
        assert_eq!(plain.layers.iter().map(|l| l.colour).collect::<Vec<_>>(), [Color32::WHITE, Color32::RED], "the backing, then the art");
        let backing = shifted(piece.bounds, shift);
        assert!(within(bounds(&plain.layers[0].contours), backing, 0.02), "{:?} vs {backing:?}", bounds(&plain.layers[0].contours));
        assert!(within(bounds(&plain.layers[1].contours), shifted(heart_world, shift), 0.02), "the heart keeps its place in the sticker");

        let bled = sheet(&design, &mat, &setup, true).unwrap();
        assert_eq!(
            bled.layers.iter().map(|l| l.colour).collect::<Vec<_>>(),
            [Color32::WHITE, Color32::RED, Color32::WHITE, Color32::RED],
            "{EVIDENCE}, question 3: dilated art, then the art on top"
        );
        let bleed = print_area(LETTER).bleed_in;
        assert!(within(bounds(&bled.layers[0].contours), grown_by(bounds(&plain.layers[0].contours), bleed), 2e-3), "{EVIDENCE}, question 3");
        assert_eq!(bled.layers[2].contours, plain.layers[0].contours);
    }

    /// A 2 x 1 in Print Then Cut image, 40 x 20 red pixels, alone on a Letter page, turned by `rotation_deg`
    /// and mirrored by `flip_x`, with the pixels the writer draws from.
    fn one_image(rotation_deg: f32, flip_x: bool) -> (Design, Mat, MatSetup, crate::assets::Pixels) {
        let img = image::RgbaImage::from_pixel(40, 20, image::Rgba([255, 0, 0, 255]));
        let asset = crate::assets::id_of(&img);
        let mut design = Design::default();
        let id = design.add_shape(ShapeKind::Bitmap { asset: asset.clone(), px: (40, 20), clip: Vec::new() });
        let s = design.shape_mut(id).unwrap();
        (s.x_in, s.y_in, s.width_in, s.height_in, s.rotation_deg, s.flip_x) = (1.0, 1.0, 2.0, 1.0, rotation_deg, flip_x);
        let setup = MatSetup::preferred(MachineFamily::StudioPro);
        let mat = compose_on(&design, setup.mat_size, 1, LETTER).remove(0);
        (design, mat, setup, crate::assets::Pixels::from([(asset, std::sync::Arc::new(img))]))
    }

    fn image_pdf(sheet: Sheet, pixels: &crate::assets::Pixels) -> String {
        String::from_utf8_lossy(&Planned::pdf("p.pdf".into(), vec![sheet], Style::Filled).bytes(pixels)).into_owned()
    }

    /// Each image draw's matrix, the line before its `Do`, as the top-left, top-right and bottom-left corners
    /// it sends the image to, in page inches with Y down.
    fn drawn_corners(pdf: &str) -> Vec<[Pt; 3]> {
        let lines: Vec<&str> = pdf.lines().collect();
        lines
            .windows(2)
            .filter(|w| w[1].ends_with(" Do"))
            .map(|w| {
                let numbers: Vec<f32> = w[0].split(' ').take(6).map(|v| v.parse().unwrap()).collect();
                let [a, b, c, d, e, f] = numbers[..] else { panic!("six numbers in {}", w[0]) };
                let inches = |(x, y): Pt| (x / POINTS_PER_INCH, 11.0 - y / POINTS_PER_INCH);
                [inches((c + e, d + f)), inches((a + c + e, b + d + f)), inches((e, f))]
            })
            .collect()
    }

    fn near(p: Pt, q: Pt) -> bool {
        (p.0 - q.0).abs() < 0.01 && (p.1 - q.1).abs() < 0.01
    }

    #[test]
    fn an_image_prints_as_one_image_xobject_with_an_smask_at_its_placement() {
        let (design, mat, setup, pixels) = one_image(0.0, false);
        let sheet = sheet(&design, &mat, &setup, false).unwrap();
        let pdf = image_pdf(sheet, &pixels);
        assert_eq!(pdf.matches("/Subtype /Image").count(), 2, "the image and its alpha mask:\n{pdf}");
        assert_eq!(pdf.matches("/SMask").count(), 1, "the image names its mask");
        assert_eq!(pdf.matches("/FlateDecode").count(), 2, "both compressed");
        assert_eq!(pdf.matches(" Do\n").count(), 1, "drawn once");
        assert_eq!(pdf.matches("\nW*").count(), 1, "clipped to its cut line");
        let (x, y) = mat.items[0].offset_in;
        let [tl, tr, bl] = drawn_corners(&pdf)[0];
        assert!(near(tl, (x, y)) && near(tr, (x + 2.0, y)) && near(bl, (x, y + 1.0)), "{:?} at {:?}", [tl, tr, bl], (x, y));
    }

    #[test]
    fn a_turned_or_mirrored_image_prints_turned_or_mirrored() {
        for (rotation, flip, expected) in [(180.0, false, [(2.0, 1.0), (0.0, 1.0), (2.0, 0.0)]), (0.0, true, [(2.0, 0.0), (0.0, 0.0), (2.0, 1.0)])] {
            let (design, mat, setup, pixels) = one_image(rotation, flip);
            let (x, y) = mat.items[0].offset_in;
            let corners = drawn_corners(&image_pdf(sheet(&design, &mat, &setup, false).unwrap(), &pixels))[0];
            let expected = expected.map(|(dx, dy)| (x + dx, y + dy));
            assert!(corners.iter().zip(expected).all(|(&p, q)| near(p, q)), "rotation {rotation}, flip {flip}: {corners:?} vs {expected:?}");
        }
    }

    #[test]
    fn bleed_prints_the_dilated_cut_line_filled_with_edge_pixels_before_the_image() {
        let (design, mat, setup, pixels) = one_image(0.0, false);
        let bleed = print_area(LETTER).bleed_in;
        let bled = sheet(&design, &mat, &setup, true).unwrap();
        let [under, art] = &bled.layers[..] else { panic!("two layers, not {}", bled.layers.len()) };
        let (x, y) = mat.items[0].offset_in;
        let placed = Aabb { min_x: x, min_y: y, max_x: x + 2.0, max_y: y + 1.0 };
        assert!(within(bounds(&under.contours), grown_by(placed, bleed), 2e-3), "{EVIDENCE}, question 3: {:?}", bounds(&under.contours));
        assert_eq!(under.picture.as_ref().map(|p| p.bleed_in), Some(bleed), "the dilated fill extends the image's edges");
        assert!(close(bounds(&art.contours), placed) && art.picture.as_ref().is_some_and(|p| p.bleed_in == 0.0), "then the art itself");

        let pdf = image_pdf(bled, &pixels);
        let draws = drawn_corners(&pdf);
        assert_eq!(draws.len(), 2, "the bleed stamp, then the image");
        let pixel_in = 2.0 / 40.0;
        let reaches = |by: f32| (bleed..bleed + pixel_in).contains(&by);
        let [tl, tr, bl] = draws[0];
        let past = [x - tl.0, y - tl.1, tr.0 - x - 2.0, bl.1 - y - 1.0];
        assert!(past.into_iter().all(reaches), "each edge extends by the bleed, rounded up to whole pixels: {past:?}");
        assert!(near(draws[1][0], (x, y)), "{:?}", draws[1]);

        let plain = image_pdf(sheet(&design, &mat, &setup, false).unwrap(), &pixels);
        assert_eq!(drawn_corners(&plain).len(), 1, "no bleed, no stamp");
    }

    #[test]
    fn a_flattened_image_part_prints_its_pixels_beside_the_other_parts() {
        let (mut design, _, setup, pixels) = one_image(0.0, false);
        let square = design.add_shape(ShapeKind::Square);
        let s = design.shape_mut(square).unwrap();
        (s.x_in, s.y_in, s.width_in, s.height_in, s.fill) = (3.5, 1.0, 1.0, 1.0, Color32::BLUE);
        let ids: Vec<u64> = design.shapes.iter().map(|s| s.id).collect();
        design.flatten(&ids).expect("flattens");
        let mat = compose_on(&design, setup.mat_size, 1, LETTER).remove(0);
        let printed = sheet(&design, &mat, &setup, false).unwrap();
        assert_eq!(printed.layers.iter().map(|l| l.picture.is_some()).collect::<Vec<_>>(), [true, false], "the image's pixels, then the blue square");
        let pdf = image_pdf(printed, &pixels);
        let (x, y) = mat.items[0].offset_in;
        let [tl, tr, _] = drawn_corners(&pdf)[0];
        assert!(near(tl, (x, y)) && near(tr, (x + 2.0, y)), "the image keeps its place in the flattened shape: {tl:?} {tr:?}");
    }

    #[test]
    fn the_file_is_named_after_the_project_and_page() {
        assert_eq!(file_name("Stickers", 1), "Stickers - Print 1.pdf");
        assert_eq!(file_name("a/b", 2), "a-b - Print 2.pdf");
    }
}

use std::collections::BTreeSet;

use egui::{Align, Align2, Color32, Context, Frame, Id, Pos2, Rect, Response, RichText, Sense, Stroke, StrokeKind, Ui, Vec2, WidgetInfo, WidgetType, pos2, vec2};

use crate::design::ShapeKind;
use crate::shape_library::Category;

use super::chrome::{text, text_width};
use super::fonts;
use super::icons::{Icon, paint_icon};
use super::popover::{popover, text_row, titled};
use super::tokens::*;
use super::tooltip::tip;

/// Ghost tiles are 91 px; the grid has 3 columns and an 8 px gap.
const TILE_PX: f32 = 91.0;
/// Columns are `1fr` of the 305 px content box, so tiles sit 106 px apart across and 99 px down.
const GAP_X: f32 = 15.0;
const GAP_Y: f32 = 8.0;
const GUTTER: f32 = 4.0;
const GRID_W: f32 = 3.0 * TILE_PX + 2.0 * GAP_X;
const HEADER_H: f32 = 40.0;
const CONTROL_H: f32 = 32.0;

pub enum PanelEvent {
    Insert(ShapeKind),
    Collapse,
}

/// What the Shapes panel shows: names containing `search`, in any of `categories` (all when empty).
/// The app keeps one for the session, so it survives collapsing the panel.
#[derive(Default)]
pub struct ShapeFilter {
    search: String,
    categories: BTreeSet<Category>,
}

impl ShapeFilter {
    fn admits(&self, kind: &ShapeKind) -> bool {
        let named = kind.label().to_lowercase().contains(&self.search.trim().to_lowercase());
        named && (self.categories.is_empty() || Category::of(kind).is_some_and(|c| self.categories.contains(&c)))
    }

    fn visible(&self) -> Vec<ShapeKind> {
        ShapeKind::free().into_iter().filter(|k| self.admits(k)).collect()
    }

    fn toggle(&mut self, category: Category) {
        if !self.categories.remove(&category) {
            self.categories.insert(category);
        }
    }
}

/// `app-static-shapes-panel`, plus the split area's collapse tab on its right edge.
pub fn show(ctx: &Context, filter: &mut ShapeFilter) -> Option<PanelEvent> {
    let frame = Frame::NONE.fill(Color32::WHITE).inner_margin(egui::Margin { left: 15, right: 16, top: 0, bottom: 0 });
    let panel = egui::SidePanel::left("shapes-panel").exact_width(SHAPES_PANEL_W + GUTTER).resizable(false).frame(frame).show_separator_line(false);
    panel
        .show(ctx, |ui| {
            let full = ui.max_rect();
            super::anchors::mark(ui, super::anchors::Anchor::ShapesPanel, full.expand2(vec2(16.0, 0.0)));
            ui.painter().vline(full.right() + 16.0 - GUTTER / 2.0, full.y_range(), Stroke::new(1.0_f32, BORDER_SUBTLE));
            header(ui, filter);
            let shown = filter.visible();
            egui::ScrollArea::vertical()
                .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing = vec2(GAP_X, GAP_Y);
                    ui.add_space(8.0);
                    if shown.is_empty() {
                        no_match(ui, filter);
                        return None;
                    }
                    let clicked = tile_grid(ui, &shown, |ui, kind| {
                        let category = Category::of(kind).map_or("", Category::label);
                        let resp = tip(tile(ui, kind.label(), true), kind.label(), category, None);
                        paint_glyph(ui, resp.rect, kind);
                        resp.clicked().then_some(kind.clone())
                    });
                    ui.add_space(16.0);
                    clicked
                })
                .inner
                .map(PanelEvent::Insert)
                .or_else(|| collapse_tab(ctx, full.right() + 16.0, full.center().y).then_some(PanelEvent::Collapse))
        })
        .inner
}

/// The Filter button's right edge is the grid's, so it lines up with the third column.
fn header(ui: &mut Ui, filter: &mut ShapeFilter) {
    let (row, _) = ui.allocate_exact_size(vec2(GRID_W, HEADER_H), Sense::hover());
    let title = fonts::semibold(14.0);
    let title_w = text_width(ui, "Shapes", title.clone());
    text(ui.painter(), row.left_center(), Align2::LEFT_CENTER, "Shapes", title, TEXT_PRIMARY);
    let button = Rect::from_min_max(pos2(row.right() - CONTROL_H, row.center().y - CONTROL_H / 2.0), pos2(row.right(), row.center().y + CONTROL_H / 2.0));
    let field = Rect::from_min_max(pos2(row.left() + title_w + 12.0, button.top()), pos2(button.left() - 8.0, button.bottom()));
    search_field(ui, field, &mut filter.search);
    filter_button(ui, button, filter);
    super::anchors::mark(ui, super::anchors::Anchor::ShapesSearch, field.union(button));
}

fn search_field(ui: &mut Ui, rect: Rect, search: &mut String) {
    let id = Id::new("shapes-search");
    let focused = ui.memory(|m| m.has_focus(id));
    ui.painter().rect(rect, 8, Color32::WHITE, Stroke::new(1.0_f32, if focused { ACCENT_PRIMARY } else { BORDER_SUBTLE }), StrokeKind::Inside);
    paint_magnifier(ui, pos2(rect.left() + 15.0, rect.center().y));
    let clear = Rect::from_center_size(pos2(rect.right() - 16.0, rect.center().y), Vec2::splat(20.0));
    let right = if search.is_empty() { rect.right() - 8.0 } else { clear.left() - 2.0 };
    let inner = Rect::from_min_max(pos2(rect.left() + 28.0, rect.top() + 1.0), pos2(right, rect.bottom() - 1.0));
    let edit = egui::TextEdit::singleline(search)
        .id(id)
        .frame(false)
        .hint_text(RichText::new("Search shapes").color(TEXT_MUTED))
        .font(fonts::regular(13.0))
        .text_color(TEXT_PRIMARY)
        .vertical_align(Align::Center);
    ui.put(inner, edit).widget_info(|| WidgetInfo::labeled(WidgetType::TextEdit, true, "Search shapes"));
    if search.is_empty() {
        return;
    }
    let resp = tip(ui.interact(clear, id.with("clear"), Sense::click()), "Clear search", "", None);
    resp.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, "Clear search"));
    if resp.hovered() {
        ui.painter().rect_filled(clear, 4.0, ACCENT_HOVER);
    }
    paint_icon(ui, Icon::Close, clear.shrink(3.0), TEXT_MUTED);
    if resp.clicked() {
        search.clear();
    }
}

fn paint_magnifier(ui: &Ui, centre: Pos2) {
    let stroke = Stroke::new(1.5_f32, TEXT_MUTED);
    let lens = centre - vec2(1.5, 1.5);
    ui.painter().circle_stroke(lens, 5.0, stroke);
    ui.painter().line_segment([lens + vec2(3.6, 3.6), lens + vec2(7.5, 7.5)], stroke);
}

fn filter_button(ui: &mut Ui, rect: Rect, filter: &mut ShapeFilter) {
    let resp = ui.interact(rect, Id::new("shapes-filter"), Sense::click());
    let resp = tip(resp, "Filter shapes", "Show only the categories you pick.", None);
    resp.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, "Filter shapes"));
    let active = !filter.categories.is_empty();
    if resp.hovered() || active {
        ui.painter().rect_filled(rect, 8.0, ACCENT_HOVER);
    }
    let ink = if active { ACCENT_PRIMARY } else { TEXT_PRIMARY };
    paint_icon(ui, Icon::Filter, Rect::from_center_size(rect.center(), Vec2::splat(24.0)), ink);
    if active {
        ui.painter().circle_filled(rect.right_top() + vec2(-6.0, 6.0), 3.5, ACCENT_PRIMARY);
    }
    // Opened from the panel's outer edge rather than the button, so it never hides the tiles it filters.
    let mut beside = resp.clone();
    let edge = ui.max_rect().right() + 16.0 + GUTTER;
    beside.rect = Rect::from_x_y_ranges(edge..=edge, rect.y_range());
    popover(ui, &beside, "filter", 240.0, |ui| {
        titled(ui, "Filter");
        let free = ShapeKind::free();
        for category in Category::ALL {
            let count = free.iter().filter(|k| Category::of(k) == Some(category)).count();
            if toggle_row(ui, category.label(), count, filter.categories.contains(&category)).clicked() {
                filter.toggle(category);
            }
        }
        ui.add_space(8.0);
        if text_row(ui, "Clear filters").clicked() {
            filter.categories.clear();
        }
        None::<()>
    });
}

/// A 32 px row: a check box, the category name and its shape count.
fn toggle_row(ui: &mut Ui, label: &str, count: usize, on: bool) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 32.0), Sense::click());
    resp.widget_info(|| WidgetInfo::selected(WidgetType::Checkbox, true, on, label));
    if resp.hovered() {
        ui.painter().rect_filled(rect, 4.0, ACCENT_HOVER);
    }
    let check = Rect::from_center_size(pos2(rect.left() + 16.0, rect.center().y), Vec2::splat(16.0));
    if on {
        ui.painter().rect_filled(check, 4.0, ACCENT_PRIMARY);
        paint_icon(ui, Icon::Tick, check.shrink(1.0), ON_PRIMARY);
    } else {
        ui.painter().rect_stroke(check, 4.0, Stroke::new(1.0_f32, TEXT_MUTED), StrokeKind::Inside);
    }
    text(ui.painter(), pos2(rect.left() + 32.0, rect.center().y), Align2::LEFT_CENTER, label, fonts::regular(14.0), TEXT_PRIMARY);
    text(ui.painter(), pos2(rect.right() - 8.0, rect.center().y), Align2::RIGHT_CENTER, &count.to_string(), fonts::regular(13.0), TEXT_MUTED);
    resp
}

fn no_match(ui: &mut Ui, filter: &mut ShapeFilter) {
    ui.set_width(GRID_W);
    ui.vertical_centered(|ui| {
        ui.add_space(24.0);
        ui.label(RichText::new("No shapes match").font(fonts::regular(14.0)).color(TEXT_MUTED));
        let clear = egui::Button::new(RichText::new("Clear").font(fonts::semibold(14.0)).color(ACCENT_PRIMARY)).frame(false);
        if ui.add(clear).clicked() {
            *filter = ShapeFilter::default();
        }
    });
}

/// 12 x 90 px, `border-radius: 0 8px 8px 0`, `box-shadow: 3px 0 5px #12121226`.
fn collapse_tab(ctx: &Context, x: f32, centre_y: f32) -> bool {
    let size = vec2(12.0, 90.0);
    egui::Area::new(egui::Id::new("shapes-collapse"))
        .order(egui::Order::Middle)
        .fixed_pos(pos2(x, centre_y - size.y / 2.0))
        .show(ctx, |ui| {
            let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
            let resp = super::tooltip::tip(resp, "Collapse Shapes", "", None);
            resp.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, "Collapse Shapes"));
            let radius = egui::CornerRadius { nw: 0, ne: 8, sw: 0, se: 8 };
            for (dx, alpha) in [(3.0, 8u8), (1.5, 14)] {
                ui.painter().rect_filled(rect.translate(vec2(dx, 0.0)), radius, Color32::from_black_alpha(alpha));
            }
            ui.painter().rect_filled(rect, radius, Color32::WHITE);
            paint_icon(ui, Icon::CollapseLeft, Rect::from_center_size(rect.center(), Vec2::splat(12.0)), TEXT_PRIMARY);
            resp.clicked()
        })
        .inner
}

fn tile_grid<T, R>(ui: &mut Ui, items: &[T], mut add: impl FnMut(&mut Ui, &T) -> Option<R>) -> Option<R> {
    let mut hit = None;
    for row in items.chunks(3) {
        ui.horizontal(|ui| {
            for item in row {
                hit = add(ui, item).or(hit.take());
            }
        });
    }
    hit
}

/// `content-tile`: radius 8 over `#f5f5f5`, with the hover shadow `0 1px 3px #12121233`.
fn tile(ui: &mut Ui, label: &str, enabled: bool) -> Response {
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(TILE_PX), sense);
    resp.widget_info(|| WidgetInfo::labeled(WidgetType::Button, enabled, label));
    // Flat: no card, just an accent-hover wash on hover.
    if resp.hovered() {
        ui.painter().rect_filled(rect, 8.0, ACCENT_HOVER);
    }
    resp
}

/// Draws a shape's silhouette (holes included) as a flat text_primary glyph centred in the tile.
fn paint_glyph(ui: &Ui, tile: Rect, kind: &ShapeKind) {
    let (w, h) = kind.default_size_in();
    let contours = kind.outline(w, h);
    let image = Rect::from_min_size(tile.min + vec2(0.18, 0.18) * TILE_PX, Vec2::splat(0.64 * TILE_PX));
    let scale = image.width() / w.max(h).max(0.01);
    let origin = image.center() - vec2(w, h) * scale / 2.0;
    let map = |(x, y): (f32, f32)| origin + vec2(x, y) * scale;
    let rings: Vec<Vec<(f32, f32)>> = contours.iter().filter(|c| c.closed).map(|c| c.flatten().into_iter().map(|p| (map(p).x, map(p).y)).collect()).collect();
    let (points, tris) = crate::geometry::triangulate(&rings);
    let mut mesh = egui::Mesh::default();
    for p in &points {
        mesh.colored_vertex(pos2(p.0, p.1), TEXT_PRIMARY);
    }
    for [a, b, c] in tris {
        mesh.add_triangle(a as u32, b as u32, c as u32);
    }
    ui.painter().add(mesh);
    for contour in contours.iter().filter(|c| !c.closed) {
        let pts: Vec<Pos2> = contour.flatten().into_iter().map(map).collect();
        ui.painter().add(egui::Shape::line(pts, Stroke::new(1.5_f32, TEXT_PRIMARY)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(filter: &ShapeFilter) -> Vec<&'static str> {
        filter.visible().iter().map(ShapeKind::label).collect()
    }

    #[test]
    fn search_matches_any_case_inside_the_name() {
        let filter = ShapeFilter { search: "ARROW".into(), ..ShapeFilter::default() };
        let shown = names(&filter);
        assert_eq!(shown, ["Arrow", "Double Arrow", "Block Arrow", "Notched Arrow", "U-Turn Arrow"]);
        assert!(shown.iter().all(|n| n.to_lowercase().contains("arrow")));
    }

    #[test]
    fn search_and_category_both_have_to_match() {
        let mut filter = ShapeFilter { search: "block".into(), ..ShapeFilter::default() };
        filter.toggle(Category::Arrows);
        assert_eq!(names(&filter), ["Block Arrow"]);
    }

    #[test]
    fn categories_add_up_and_none_means_all() {
        let mut filter = ShapeFilter::default();
        assert_eq!(filter.visible(), ShapeKind::free());
        filter.toggle(Category::Lines);
        assert_eq!(names(&filter), ["Line"]);
        filter.toggle(Category::Arrows);
        assert_eq!(names(&filter), ["Line", "Chevron", "Arrow", "Double Arrow", "Block Arrow", "Notched Arrow", "U-Turn Arrow", "Double Chevron"]);
        filter.toggle(Category::Lines);
        filter.toggle(Category::Arrows);
        assert_eq!(filter.visible(), ShapeKind::free());
    }
}
